import { access } from "node:fs/promises";
import { join, resolve } from "node:path";
import {
  defaultStatePaths,
  detectWindowsHostConditions,
  loadCommunityDirectory,
  loadVerifiedNodeCache,
  loadVolunteerSettings,
  mergeNetworkSources,
  saveVolunteerConsent,
} from "../../../../scripts/community-network.mjs";
import { runtimePipeName } from "./runtime-protocol.mjs";

const DEFAULT_IDLE_TIMEOUT_SECONDS = 30 * 60;

export async function createRuntimeLaunchPlan(pluginRoot, releaseVersion) {
  const resolvedPluginRoot = resolve(pluginRoot);
  if (typeof releaseVersion !== "string" || releaseVersion.length === 0) {
    throw new Error("共享运行时缺少插件版本");
  }
  const projectRoot = resolve(resolvedPluginRoot, "..", "..");
  const statePaths = defaultStatePaths();
  const communityDirectoryPath = await firstExisting(
    [
      join(resolvedPluginRoot, "config", "community-nodes.json"),
      join(projectRoot, "config", "community-nodes.json"),
    ],
    "插件包缺少社区节点目录 config/community-nodes.json",
  );
  const [runtimePath, workerPath, settings, directory, cache, hostConditions] = await Promise.all([
    resolveExecutable(
      process.env.TOKEN_HOLDEM_RUNTIME_PATH,
      "token-holdem-runtime",
      resolvedPluginRoot,
      projectRoot,
    ),
    resolveExecutable(
      process.env.TOKEN_HOLDEM_SIDECAR_PATH,
      "token-holdem-sidecar",
      resolvedPluginRoot,
      projectRoot,
    ),
    loadVolunteerSettings(statePaths.settings),
    loadCommunityDirectory(communityDirectoryPath),
    loadVerifiedNodeCache(statePaths.cache),
    detectWindowsHostConditions(),
  ]);
  const networkPlan = mergeNetworkSources({ directory, cache });
  const workerArgs = Object.freeze([
    `--node-key-file=${statePaths.nodeKey}`,
    `--volunteer-consent=${settings.consent}`,
    `--network-cost=${hostConditions.networkCost}`,
    `--power-source=${hostConditions.powerSource}`,
  ]);
  const bootstrapCommands = Object.freeze(createBootstrapCommands(networkPlan));
  const pipeName = runtimePipeName(`${resolvedPluginRoot}\0${releaseVersion}`);
  const idleTimeoutSeconds = readIdleTimeoutSeconds();
  return Object.freeze({
    runtimePath,
    workerPath,
    pipeName,
    idleTimeoutSeconds,
    workerArgs,
    bootstrapCommands,
    settings,
    statePaths,
    networkPlan,
    hostWarning: hostConditions.warning,
    supervisorArgs: Object.freeze([
      `--pipe-name=${pipeName}`,
      `--worker-executable=${workerPath}`,
      `--idle-timeout-seconds=${String(idleTimeoutSeconds)}`,
      "--",
      ...workerArgs,
    ]),
  });
}

export async function saveRuntimeVolunteerConsent(enabled) {
  const consent = enabled ? "granted" : "declined";
  await saveVolunteerConsent(defaultStatePaths().settings, consent);
  return consent;
}

function createBootstrapCommands(networkPlan) {
  const commands = [];
  // Relay transport owns the initial dial. Discovery and archive behavior use
  // DisconnectedAndNotDialing and therefore reuse the same Windows connection.
  for (const address of networkPlan.relays) {
    commands.push({ type: "use_relay", address });
  }
  if (networkPlan.rendezvous.length > 0) {
    commands.push({
      type: "configure_discovery",
      addresses: [...networkPlan.rendezvous],
      namespace: networkPlan.namespace,
    });
  }
  if (networkPlan.archives.length > 0) {
    commands.push({
      type: "configure_archive_nodes",
      addresses: [...networkPlan.archives],
      minimum_confirmed_replicas: 1,
    });
  }
  return commands;
}

async function resolveExecutable(override, baseName, pluginRoot, projectRoot) {
  const executableName = process.platform === "win32" ? `${baseName}.exe` : baseName;
  return firstExisting(
    [
      override,
      join(pluginRoot, "bin", executableName),
      join(projectRoot, "target", "release", executableName),
      join(projectRoot, "target", "debug", executableName),
    ].filter((value) => typeof value === "string" && value.length > 0),
    `插件包缺少 ${executableName}；请重新构建完整插件包`,
  );
}

async function firstExisting(candidates, errorMessage) {
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {
      continue;
    }
  }
  throw new Error(errorMessage);
}

function readIdleTimeoutSeconds() {
  const raw = process.env.TOKEN_HOLDEM_RUNTIME_IDLE_TIMEOUT_SECONDS;
  if (raw === undefined) return DEFAULT_IDLE_TIMEOUT_SECONDS;
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value < 1 || value > 86_400) {
    throw new Error("TOKEN_HOLDEM_RUNTIME_IDLE_TIMEOUT_SECONDS 必须在 1 到 86400 之间");
  }
  return value;
}
