import { spawn } from "node:child_process";
import { access, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { integrationSidecarPath } from "./integration-runtime.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(scriptDirectory, "..");
const sidecarPath = integrationSidecarPath(projectRoot);

class SidecarProbe {
  #buffer = "";
  #errors = "";
  #events = [];

  constructor(label, argumentsList = []) {
    this.label = label;
    this.process = spawn(sidecarPath, argumentsList, {
      cwd: projectRoot,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    this.process.stdout.setEncoding("utf8");
    this.process.stderr.setEncoding("utf8");
    this.process.stdout.on("data", (chunk) => this.#consume(String(chunk)));
    this.process.stderr.on("data", (chunk) => {
      this.#errors += String(chunk);
    });
  }

  send(command) {
    this.process.stdin.write(`${JSON.stringify(command)}\n`);
  }

  latest(type) {
    return this.#events.findLast((event) => event?.type === type) ?? null;
  }

  events(type) {
    return this.#events.filter((event) => event?.type === type);
  }

  diagnostics() {
    return `${this.label} stderr:\n${this.#errors}\n${this.label} events:\n${this.#events
      .map((event) => JSON.stringify(event))
      .join("\n")}`;
  }

  async stop() {
    if (this.process.exitCode !== null) return;
    this.send({ type: "shutdown" });
    await Promise.race([
      new Promise((resolveExit) => this.process.once("exit", resolveExit)),
      delay(2_000),
    ]);
    if (this.process.exitCode === null) this.process.kill();
  }

  #consume(chunk) {
    this.#buffer += chunk;
    const lines = this.#buffer.split(/\r?\n/u);
    this.#buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (line.trim().length === 0) continue;
      try {
        this.#events.push(JSON.parse(line));
      } catch {
        this.#errors += `无法解析输出：${line}\n`;
      }
    }
  }
}

async function main() {
  await access(sidecarPath);
  const identityDirectory = await mkdtemp(join(tmpdir(), "token-holdem-node-"));
  const nodeKeyPath = join(identityDirectory, "libp2p-identity-key");
  const serverArguments = [
    "--rendezvous-server",
    "--relay-server",
    "--public-node",
    "--volunteer-consent=granted",
    "--network-cost=unmetered",
    "--power-source=ac",
    `--node-key-file=${nodeKeyPath}`,
    "--relay-max-reservations=3",
    "--relay-max-circuits=2",
    "--relay-circuit-seconds=180",
    "--relay-circuit-bytes=1048576",
  ];
  const server = new SidecarProbe("志愿服务端", serverArguments);
  const client = new SidecarProbe("Relay 客户端");
  let restartedServer = null;

  try {
    await Promise.all([
      waitFor(server, "listen_address", 15_000),
      waitFor(client, "listen_address", 15_000),
    ]);
    const initialStatus = await waitFor(server, "volunteer_status", 10_000);
    assert(initialStatus.role === "active_discovery_relay", "专用节点没有进入发现 + Relay 活跃角色");
    assert(initialStatus.max_reservations === 3, "Relay 预约上限未传播");
    assert(initialStatus.max_circuits === 2, "Relay Circuit 上限未传播");
    assert(initialStatus.max_circuit_duration_seconds === 180, "Relay 时长上限未传播");
    assert(initialStatus.max_circuit_bytes === 1_048_576, "Relay 字节上限未传播");

    const serverAddress = dialableAddress(server);
    server.send({ type: "add_external_address", address: publishableLocalAddress(server) });
    await waitFor(server, "advertised_address_added", 10_000);
    client.send({ type: "use_relay", address: serverAddress });
    const [accepted, serverAccepted] = await Promise.all([
      waitFor(client, "relay_reservation_accepted", 30_000),
      waitUntil(
        () => server.events("relay_server_reservation").find((event) => event.action === "accepted"),
        30_000,
        "志愿服务端没有接受 Relay 预约",
      ),
    ]);
    assert(accepted.peer_id === server.latest("ready").peer_id, "客户端接受了错误 Relay 的预约");
    assert(accepted.duration_seconds === 180, "客户端收到的 Circuit 时长上限不符");
    assert(accepted.data_bytes === 1_048_576, "客户端收到的 Circuit 字节上限不符");

    client.send({
      type: "configure_discovery",
      addresses: [serverAddress],
      namespace: "token-holdem/volunteer-verification",
    });
    await waitFor(client, "rendezvous_registered", 20_000);

    const originalPeerId = server.latest("ready").peer_id;
    await server.stop();
    restartedServer = new SidecarProbe("重启后的志愿服务端", serverArguments);
    const restartedReady = await waitFor(restartedServer, "ready", 15_000);
    assert(restartedReady.peer_id === originalPeerId, "持久节点密钥没有保持稳定 PeerId");

    process.stdout.write(
      `${JSON.stringify({
        ok: true,
        relayPeerId: originalPeerId,
        reservationAccepted: true,
        rendezvousRegistered: true,
        stablePeerId: true,
        limits: {
          reservations: initialStatus.max_reservations,
          circuits: initialStatus.max_circuits,
          seconds: accepted.duration_seconds,
          bytes: accepted.data_bytes,
        },
      })}\n`,
    );
  } catch (error) {
    throw new Error(
      `${error instanceof Error ? error.message : String(error)}\n\n${server.diagnostics()}\n\n${client.diagnostics()}${restartedServer === null ? "" : `\n\n${restartedServer.diagnostics()}`}`,
    );
  } finally {
    await Promise.all([server.stop(), client.stop(), restartedServer?.stop()]);
    await rm(identityDirectory, { recursive: true, force: true });
  }
}

function dialableAddress(probe) {
  const listenAddresses = probe.events("listen_address");
  const preferred = listenAddresses.findLast(
    (candidate) =>
      typeof candidate.address === "string" && candidate.address.includes("/ip4/127.0.0.1/tcp/"),
  );
  const fallback = listenAddresses.findLast(
    (candidate) => typeof candidate.address === "string" && candidate.address.includes("/tcp/"),
  );
  const event = preferred ?? fallback;
  const ready = probe.latest("ready");
  assert(event !== null && typeof ready?.peer_id === "string", `${probe.label} 缺少 TCP 地址或 PeerId`);
  let address = event.address
    .replace("/ip4/0.0.0.0/", "/ip4/127.0.0.1/")
    .replace("/ip6/::/", "/ip6/::1/");
  if (!address.includes("/p2p/")) address = `${address}/p2p/${ready.peer_id}`;
  return address;
}

function publishableLocalAddress(probe) {
  const address = dialableAddress(probe);
  const tcpPort = address.match(/\/tcp\/(\d+)/u)?.[1];
  const peerId = probe.latest("ready")?.peer_id;
  assert(typeof tcpPort === "string" && typeof peerId === "string", `${probe.label} 缺少本地 TCP 端点`);
  // The production runtime rejects loopback IPs as public addresses. This
  // DNS form exercises explicit advertisement without exposing the test port.
  return `/dns4/localhost/tcp/${tcpPort}/p2p/${peerId}`;
}

async function waitFor(probe, type, timeoutMs) {
  return waitUntil(
    () => probe.latest(type),
    timeoutMs,
    `${probe.label} 等待 ${type} 超时`,
  );
}

async function waitUntil(predicate, timeoutMs, message) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = predicate();
    if (value) return value;
    await delay(40);
  }
  throw new Error(message);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function delay(milliseconds) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

await main();
