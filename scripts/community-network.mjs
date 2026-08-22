import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdir, open, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const DIRECTORY_SCHEMA_VERSION = 1;
const SETTINGS_SCHEMA_VERSION = 1;
const CACHE_SCHEMA_VERSION = 1;
const MAX_JSON_BYTES = 256 * 1_024;
const MAX_DIRECTORY_NODES = 64;
const MAX_CACHE_NODES = 32;
const MAX_ADDRESSES_PER_NODE = 8;
const MAX_CACHED_ADDRESSES_PER_NODE = 4;
const CACHE_TTL_MS = 30 * 24 * 60 * 60 * 1_000;
const FILE_LOCK_WAIT_MS = 5_000;
const FILE_LOCK_STALE_MS = 15_000;
const FILE_LOCK_RETRY_MS = 20;
const ALLOWED_ROLES = new Set(["rendezvous", "relay", "archive"]);
const ALLOWED_CONSENT = new Set(["undecided", "granted", "declined"]);

export function defaultStatePaths(environment = process.env, platform = process.platform) {
  const baseDirectory =
    platform === "win32" && isNonEmptyString(environment.LOCALAPPDATA)
      ? join(environment.LOCALAPPDATA, "TokenHoldem")
      : join(homedir(), ".token-holdem");
  return Object.freeze({
    directory: baseDirectory,
    settings: join(baseDirectory, "settings.json"),
    cache: join(baseDirectory, "network-cache.json"),
    nodeKey: join(baseDirectory, "libp2p-identity-key"),
  });
}

export async function loadVolunteerSettings(path) {
  const value = await readBoundedJson(path);
  if (value === null) return Object.freeze({ consent: "undecided" });
  if (
    !isRecord(value) ||
    value.schema_version !== SETTINGS_SCHEMA_VERSION ||
    !ALLOWED_CONSENT.has(value.volunteer_consent) ||
    Object.keys(value).some(
      (key) => !["schema_version", "volunteer_consent", "updated_at_unix_ms"].includes(key),
    )
  ) {
    throw new Error(`志愿设置文件格式无效：${path}`);
  }
  return Object.freeze({ consent: value.volunteer_consent });
}

export async function saveVolunteerConsent(path, consent, nowUnixMs = Date.now()) {
  if (!ALLOWED_CONSENT.has(consent) || consent === "undecided") {
    throw new Error("只能持久化 granted 或 declined 志愿授权");
  }
  await atomicWriteJson(path, {
    schema_version: SETTINGS_SCHEMA_VERSION,
    volunteer_consent: consent,
    updated_at_unix_ms: boundedTimestamp(nowUnixMs),
  });
}

export async function loadCommunityDirectory(path) {
  const value = await readBoundedJson(path);
  if (value === null) {
    return Object.freeze({ namespace: "token-holdem/v1", nodes: Object.freeze([]) });
  }
  return parseCommunityDirectory(value, path);
}

export function parseCommunityDirectory(value, source = "社区目录") {
  if (
    !isRecord(value) ||
    value.schema_version !== DIRECTORY_SCHEMA_VERSION ||
    !isDiscoveryNamespace(value.namespace) ||
    !Array.isArray(value.nodes) ||
    value.nodes.length > MAX_DIRECTORY_NODES
  ) {
    throw new Error(`${source} 顶层结构无效`);
  }
  const names = new Set();
  const nodes = value.nodes.map((node, index) => {
    if (
      !isRecord(node) ||
      !isBoundedString(node.name, 80) ||
      names.has(node.name) ||
      !Array.isArray(node.roles) ||
      node.roles.length < 1 ||
      node.roles.length > ALLOWED_ROLES.size ||
      !node.roles.every((role) => ALLOWED_ROLES.has(role)) ||
      new Set(node.roles).size !== node.roles.length ||
      !Array.isArray(node.addresses) ||
      node.addresses.length < 1 ||
      node.addresses.length > MAX_ADDRESSES_PER_NODE ||
      !node.addresses.every(isPublicPeerMultiaddr) ||
      new Set(node.addresses).size !== node.addresses.length
    ) {
      throw new Error(`${source} 的第 ${String(index + 1)} 个节点无效`);
    }
    names.add(node.name);
    const peerIds = new Set(node.addresses.map(peerIdFromMultiaddr));
    if (peerIds.size !== 1) {
      throw new Error(`${source} 的节点 ${node.name} 混用了多个 PeerId`);
    }
    return Object.freeze({
      name: node.name,
      peerId: [...peerIds][0],
      roles: Object.freeze([...node.roles]),
      addresses: Object.freeze([...node.addresses]),
    });
  });
  return Object.freeze({
    namespace: value.namespace,
    nodes: Object.freeze(nodes),
  });
}

export async function loadVerifiedNodeCache(path, nowUnixMs = Date.now()) {
  const value = await readBoundedJson(path);
  if (value === null) return Object.freeze([]);
  if (
    !isRecord(value) ||
    value.schema_version !== CACHE_SCHEMA_VERSION ||
    !Array.isArray(value.nodes) ||
    value.nodes.length > MAX_CACHE_NODES
  ) {
    throw new Error(`社区节点缓存格式无效：${path}`);
  }
  const now = boundedTimestamp(nowUnixMs);
  const nodes = value.nodes.map((node, index) => parseCachedNode(node, index, path));
  return Object.freeze(
    nodes
      .filter((node) => now - node.lastVerifiedUnixMs <= CACHE_TTL_MS)
      .sort((left, right) => right.lastVerifiedUnixMs - left.lastVerifiedUnixMs),
  );
}

export function isCacheableVerifiedNode({ peerId, address, role }) {
  return (
    isBoundedString(peerId, 128) &&
    isPublicPeerMultiaddr(address) &&
    peerIdFromMultiaddr(address) === peerId &&
    ALLOWED_ROLES.has(role)
  );
}

export async function recordVerifiedNode(
  path,
  { peerId, address, role },
  nowUnixMs = Date.now(),
) {
  if (!isCacheableVerifiedNode({ peerId, address, role })) {
    throw new Error("拒绝缓存无效或非公网社区节点地址");
  }
  const now = boundedTimestamp(nowUnixMs);
  await withExclusiveFileLock(path, async () => {
    const existing = [...(await loadVerifiedNodeCache(path, now))];
    const found = existing.find((node) => node.peerId === peerId);
    if (found === undefined) {
      existing.push({
        peerId,
        roles: [role],
        addresses: [address],
        lastVerifiedUnixMs: now,
      });
    } else {
      found.roles = uniqueBounded([role, ...found.roles], ALLOWED_ROLES.size);
      found.addresses = uniqueBounded(
        [address, ...found.addresses],
        MAX_CACHED_ADDRESSES_PER_NODE,
      );
      found.lastVerifiedUnixMs = now;
    }
    const nodes = existing
      .sort((left, right) => right.lastVerifiedUnixMs - left.lastVerifiedUnixMs)
      .slice(0, MAX_CACHE_NODES)
      .map((node) => ({
        peer_id: node.peerId,
        roles: node.roles,
        addresses: node.addresses,
        last_verified_unix_ms: node.lastVerifiedUnixMs,
      }));
    await atomicWriteJson(path, { schema_version: CACHE_SCHEMA_VERSION, nodes });
  });
}

export function mergeNetworkSources({
  explicitRendezvous = [],
  explicitRelays = [],
  explicitArchives = [],
  directory,
  cache,
}) {
  const rendezvous = [];
  const relays = [];
  const archives = [];
  appendUnique(rendezvous, explicitRendezvous, 8);
  appendUnique(relays, explicitRelays, 4);
  appendUnique(archives, explicitArchives, 16);
  for (const node of directory.nodes) {
    for (const role of node.roles) {
      appendRoleAddresses({ rendezvous, relays, archives }, role, node.addresses);
    }
  }
  for (const node of cache) {
    for (const role of node.roles) {
      appendRoleAddresses({ rendezvous, relays, archives }, role, node.addresses);
    }
  }
  return Object.freeze({
    namespace: directory.namespace,
    rendezvous: Object.freeze(rendezvous),
    relays: Object.freeze(relays),
    archives: Object.freeze(archives),
  });
}

export async function detectWindowsHostConditions(platform = process.platform) {
  if (platform !== "win32") {
    return Object.freeze({
      networkCost: "unknown",
      powerSource: "unknown",
      source: "unsupported_platform",
      warning: null,
    });
  }
  const script = String.raw`
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
Add-Type -AssemblyName System.Windows.Forms
$profile = [Windows.Networking.Connectivity.NetworkInformation,Windows,ContentType=WindowsRuntime]::GetInternetConnectionProfile()
$networkCost = 'unknown'
if ($null -ne $profile) {
    $connectionCost = $profile.GetConnectionCost()
    if ($connectionCost.Roaming -or $connectionCost.OverDataLimit) {
        $networkCost = 'metered'
    }
    elseif ($connectionCost.NetworkCostType.ToString() -eq 'Unrestricted') {
        $networkCost = 'unmetered'
    }
    elseif ($connectionCost.NetworkCostType.ToString() -in @('Fixed', 'Variable')) {
        $networkCost = 'metered'
    }
}
$powerLine = [System.Windows.Forms.SystemInformation]::PowerStatus.PowerLineStatus.ToString()
$powerSource = switch ($powerLine) {
    'Online' { 'ac' }
    'Offline' { 'battery' }
    default { 'unknown' }
}
[ordered]@{ network_cost = $networkCost; power_source = $powerSource } | ConvertTo-Json -Compress
`;
  try {
    const { stdout } = await execFileAsync(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-Command", script],
      { timeout: 5_000, windowsHide: true, maxBuffer: 16 * 1_024 },
    );
    const value = JSON.parse(stdout.trim());
    if (
      !isRecord(value) ||
      !["unmetered", "metered", "unknown"].includes(value.network_cost) ||
      !["ac", "battery", "unknown"].includes(value.power_source)
    ) {
      throw new Error("Windows 探针返回了无效结构");
    }
    return Object.freeze({
      networkCost: value.network_cost,
      powerSource: value.power_source,
      source: "windows_api",
      warning: null,
    });
  } catch (error) {
    return Object.freeze({
      networkCost: "unknown",
      powerSource: "unknown",
      source: "probe_failed",
      warning: error instanceof Error ? error.message : String(error),
    });
  }
}

async function readBoundedJson(path) {
  let metadata;
  try {
    metadata = await stat(path);
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") return null;
    throw error;
  }
  if (!metadata.isFile() || metadata.size > MAX_JSON_BYTES) {
    throw new Error(`JSON 文件不存在或超过 ${String(MAX_JSON_BYTES)} 字节：${path}`);
  }
  const raw = await readFile(path, "utf8");
  try {
    return JSON.parse(raw);
  } catch (error) {
    throw new Error(`JSON 文件无法解析：${path}`, { cause: error });
  }
}

async function atomicWriteJson(path, value) {
  await mkdir(dirname(path), { recursive: true });
  const temporaryPath = `${path}.${String(process.pid)}.${randomUUID()}.tmp`;
  await writeFile(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, {
    encoding: "utf8",
    flag: "wx",
  });
  try {
    await rename(temporaryPath, path);
  } catch (error) {
    await rm(temporaryPath, { force: true });
    throw error;
  }
}

async function withExclusiveFileLock(targetPath, operation) {
  const lockPath = `${targetPath}.lock`;
  const deadline = Date.now() + FILE_LOCK_WAIT_MS;
  await mkdir(dirname(lockPath), { recursive: true });
  let lockHandle;
  while (lockHandle === undefined) {
    try {
      lockHandle = await open(lockPath, "wx");
    } catch (error) {
      if (!isNodeError(error) || error.code !== "EEXIST") throw error;
      await removeStaleFileLock(lockPath);
      if (Date.now() >= deadline) {
        throw new Error(`等待社区节点缓存锁超时：${targetPath}`);
      }
      await delay(FILE_LOCK_RETRY_MS);
    }
  }
  try {
    return await operation();
  } finally {
    await lockHandle.close();
    await rm(lockPath, { force: true });
  }
}

async function removeStaleFileLock(lockPath) {
  try {
    const metadata = await stat(lockPath);
    if (Date.now() - metadata.mtimeMs > FILE_LOCK_STALE_MS) {
      await rm(lockPath, { force: true });
    }
  } catch (error) {
    if (!isNodeError(error) || error.code !== "ENOENT") throw error;
  }
}

function parseCachedNode(node, index, source) {
  if (
    !isRecord(node) ||
    !isBoundedString(node.peer_id, 128) ||
    !Array.isArray(node.roles) ||
    node.roles.length < 1 ||
    node.roles.length > ALLOWED_ROLES.size ||
    !node.roles.every((role) => ALLOWED_ROLES.has(role)) ||
    new Set(node.roles).size !== node.roles.length ||
    !Array.isArray(node.addresses) ||
    node.addresses.length < 1 ||
    node.addresses.length > MAX_CACHED_ADDRESSES_PER_NODE ||
    !node.addresses.every(isPublicPeerMultiaddr) ||
    !Number.isSafeInteger(node.last_verified_unix_ms) ||
    node.last_verified_unix_ms < 0 ||
    node.addresses.some((address) => peerIdFromMultiaddr(address) !== node.peer_id)
  ) {
    throw new Error(`${source} 的第 ${String(index + 1)} 个缓存节点无效`);
  }
  return {
    peerId: node.peer_id,
    roles: [...node.roles],
    addresses: [...node.addresses],
    lastVerifiedUnixMs: node.last_verified_unix_ms,
  };
}

function appendRoleAddresses(targets, role, addresses) {
  const targetByRole = {
    rendezvous: [targets.rendezvous, 8],
    relay: [targets.relays, 4],
    archive: [targets.archives, 16],
  };
  const [target, limit] = targetByRole[role];
  appendUnique(target, addresses, limit);
}

function appendUnique(target, candidates, limit) {
  for (const address of candidates) {
    if (target.length >= limit) return;
    if (!target.includes(address)) target.push(address);
  }
}

function uniqueBounded(values, limit) {
  return [...new Set(values)].slice(0, limit);
}

function peerIdFromMultiaddr(value) {
  const match = /\/p2p\/([^/]+)$/u.exec(value);
  return match?.[1] ?? null;
}

function isPublicPeerMultiaddr(value) {
  if (
    typeof value !== "string" ||
    Buffer.byteLength(value, "utf8") > 2_048 ||
    value.includes("/p2p-circuit") ||
    peerIdFromMultiaddr(value) === null ||
    /\s/u.test(value)
  ) {
    return false;
  }
  if (/^\/(dns|dns4|dns6|dnsaddr)\//u.test(value)) return true;
  const ipv4 = /^\/ip4\/(\d{1,3}(?:\.\d{1,3}){3})\//u.exec(value)?.[1];
  if (ipv4 !== undefined) return isPublicIpv4(ipv4);
  if (/^\/ip6\/(?!::1(?:\/|$)|fe[89ab][0-9a-f]:|f[cd][0-9a-f]{2}:)/iu.test(value)) {
    return true;
  }
  return false;
}

function isPublicIpv4(value) {
  const octets = value.split(".").map((part) => Number.parseInt(part, 10));
  if (octets.length !== 4 || octets.some((part) => !Number.isInteger(part) || part > 255)) {
    return false;
  }
  const [first, second] = octets;
  return !(
    first === 0 ||
    first === 10 ||
    first === 127 ||
    first >= 224 ||
    (first === 100 && second >= 64 && second <= 127) ||
    (first === 169 && second === 254) ||
    (first === 172 && second >= 16 && second <= 31) ||
    (first === 192 && second === 168)
  );
}

function isDiscoveryNamespace(value) {
  return typeof value === "string" && /^[a-z0-9_/-]{1,64}$/u.test(value);
}

function isBoundedString(value, maximum) {
  return typeof value === "string" && value.length >= 1 && value.length <= maximum;
}

function isNonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function boundedTimestamp(value) {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error("时间戳无效");
  return value;
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNodeError(value) {
  return value instanceof Error && "code" in value;
}
