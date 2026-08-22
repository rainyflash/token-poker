import { spawn } from "node:child_process";
import { access } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { loadCommunityDirectory } from "./community-network.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(scriptDirectory, "..");
const options = parseArguments(process.argv.slice(2));
const sidecarPath = resolve(
  options.sidecar ||
    join(
      projectRoot,
      "target",
      "debug",
      process.platform === "win32" ? "token-holdem-sidecar.exe" : "token-holdem-sidecar",
    ),
);
const directoryPath = resolve(projectRoot, options.directory);

async function main() {
  await access(sidecarPath);
  const directory = await loadCommunityDirectory(directoryPath);
  const selectedNodes = options.nodeName
    ? directory.nodes.filter((node) => node.name === options.nodeName)
    : directory.nodes;

  assert(selectedNodes.length > 0, `社区目录中没有节点：${options.nodeName || "全部节点"}`);

  const targets = selectedNodes.flatMap((node) =>
    node.addresses.map((address) => ({
      node: node.name,
      peerId: node.peerId,
      address,
      transport: address.includes("/quic-v1") ? "quic-v1" : "tcp",
    })),
  );
  const results = [];

  for (const target of targets) {
    results.push(await verifyTarget(target));
  }

  process.stdout.write(
    `${JSON.stringify({
      ok: true,
      directory: directoryPath,
      verified: results,
    })}\n`,
  );
}

async function verifyTarget(target) {
  const probe = new SidecarProbe(`${target.node}/${target.transport}`, sidecarPath);
  try {
    await waitFor(probe, "ready", options.timeoutMs);
    probe.send({ type: "use_relay", address: target.address });
    const reservation = await waitFor(
      probe,
      "relay_reservation_accepted",
      options.timeoutMs,
      (event) => event.peer_id === target.peerId,
    );

    probe.send({
      type: "configure_discovery",
      addresses: [target.address],
      namespace: "token-holdem/deployment-verification",
    });
    const registration = await waitFor(
      probe,
      "rendezvous_registered",
      options.timeoutMs,
      (event) => event.node === target.peerId,
    );

    return {
      node: target.node,
      transport: target.transport,
      address: target.address,
      relayReservationAccepted: true,
      rendezvousRegistered: true,
      relayDurationSeconds: reservation.duration_seconds,
      relayDataBytes: reservation.data_bytes,
      rendezvousTtlSeconds: registration.ttl_seconds,
    };
  } catch (error) {
    throw new Error(
      `${target.node} 的 ${target.transport} 公网验收失败：${error instanceof Error ? error.message : String(error)}\n\n${probe.diagnostics()}`,
    );
  } finally {
    await probe.stop();
  }
}

class SidecarProbe {
  #buffer = "";
  #errors = "";
  #events = [];

  constructor(label, executablePath) {
    this.label = label;
    this.process = spawn(executablePath, [], {
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

  latest(type, predicate = () => true) {
    return this.#events.findLast((event) => event?.type === type && predicate(event)) ?? null;
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

async function waitFor(probe, type, timeoutMs, predicate = () => true) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const event = probe.latest(type, predicate);
    if (event !== null) return event;
    if (probe.process.exitCode !== null) {
      throw new Error(`sidecar 提前退出，退出码 ${String(probe.process.exitCode)}`);
    }
    await delay(50);
  }
  throw new Error(`等待 ${type} 超时`);
}

function parseArguments(argumentsList) {
  const result = {
    directory: "config/community-nodes.json",
    nodeName: "",
    sidecar: "",
    timeoutMs: 45_000,
  };
  for (let index = 0; index < argumentsList.length; index += 1) {
    const argument = argumentsList[index];
    const value = argumentsList[index + 1];
    if (argument === "--directory" && value !== undefined) result.directory = value;
    else if (argument === "--node" && value !== undefined) result.nodeName = value;
    else if (argument === "--sidecar" && value !== undefined) result.sidecar = value;
    else if (argument === "--timeout-ms" && value !== undefined) {
      result.timeoutMs = Number.parseInt(value, 10);
    } else {
      throw new Error(`未知或缺值参数：${String(argument)}`);
    }
    index += 1;
  }
  if (!Number.isInteger(result.timeoutMs) || result.timeoutMs < 1_000 || result.timeoutMs > 120_000) {
    throw new Error("--timeout-ms 必须在 1000 到 120000 之间");
  }
  return result;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function delay(milliseconds) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

await main();
