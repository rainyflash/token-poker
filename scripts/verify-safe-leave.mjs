import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { access } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(scriptDirectory, "..");
const sidecarPath = resolve(
  process.env.TOKEN_HOLDEM_SIDECAR_PATH
    ?? join(projectRoot, "target", "debug", "token-holdem-sidecar.exe"),
);
const LEVEL_ID = "1m-2m";

class SidecarProbe {
  #buffer = "";
  #errors = "";
  #events = [];

  constructor(label) {
    this.label = label;
    this.process = spawn(sidecarPath, ["--listen=/ip4/127.0.0.1/tcp/0"], {
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
    assert(this.process.exitCode === null, `${this.label} 已退出，无法发送命令`);
    this.process.stdin.write(`${JSON.stringify(command)}\n`);
  }

  latest(type) {
    return this.#events.findLast((event) => event?.type === type) ?? null;
  }

  latestWhere(type, predicate) {
    return this.#events.findLast((event) => event?.type === type && predicate(event)) ?? null;
  }

  countWhere(type, predicate) {
    return this.#events.filter((event) => event?.type === type && predicate(event)).length;
  }

  diagnostics() {
    const ignored = new Set(["gossip_message", "pool_directory_updated", "pool_ticket_published"]);
    return `${this.label} stderr:\n${this.#errors}\n${this.label} 最近事件:\n${this.#events
      .filter((event) => !ignored.has(event?.type))
      .slice(-120)
      .map((event) => JSON.stringify(event))
      .join("\n")}`;
  }

  async crash() {
    if (this.process.exitCode !== null) return;
    this.process.kill();
    await Promise.race([
      new Promise((resolveExit) => this.process.once("exit", resolveExit)),
      delay(3_000),
    ]);
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
  const normal = await verifyConfirmedLeaveAtHandBoundary();
  const abandoned = await verifySignedDisconnectConvergence();
  process.stdout.write(`${JSON.stringify({ ok: true, normal, abandoned })}\n`);
}

async function verifyConfirmedLeaveAtHandBoundary() {
  const players = [new SidecarProbe("正常离桌 A"), new SidecarProbe("正常离桌 B")];
  try {
    await preparePlayers(players, "normal");
    await formPublicTable(players);
    await waitForHand(players, 1, 90_000);
    const leaver = players.find((player) => handState(player, 1)?.can_act === true);
    assert(leaver !== undefined, "首手没有唯一行动者可用于验证自动弃牌");
    const survivor = players.find((player) => player !== leaver);
    assert(survivor !== undefined, "正常离桌缺少留桌玩家");
    const requestId = randomUUID();
    leaver.send({ type: "leave_table", request_id: requestId });

    await waitForWhere(
      leaver,
      "command_confirmed",
      (event) => event.request_id === requestId && event.command_type === "leave_table",
      10_000,
    );
    const requested = await waitFor(leaver, "safe_leave_requested", 10_000);
    const [leaverReceipt, survivorReceipt] = await Promise.all([
      waitForWhere(leaver, "receipt_finalized", (event) => event.hand_number === 1, 45_000),
      waitForWhere(survivor, "receipt_finalized", (event) => event.hand_number === 1, 45_000),
    ]);
    await waitFor(leaver, "safe_leave_completed", 30_000);
    await waitFor(leaver, "room_closed", 30_000);
    const survivorRoom = await waitForWhere(
      survivor,
      "room_snapshot",
      (event) => event.seats.length === 1,
      45_000,
    );

    assert(requested.after_hand_number === 1, "安全离桌没有绑定当前手牌边界");
    assert(leaverReceipt.receipt_id === survivorReceipt.receipt_id, "离桌前最终凭证未在双方收敛");
    assert(survivorRoom.local_role === "seated", "留桌玩家未回到等待下一手状态");
    return {
      requestConfirmed: true,
      boundaryHand: requested.after_hand_number,
      receiptId: leaverReceipt.receipt_id,
      remainingSeats: survivorRoom.seats.length,
    };
  } catch (error) {
    throw new Error(formatFailure(error, players));
  } finally {
    await Promise.all(players.map((player) => player.stop()));
  }
}

async function verifySignedDisconnectConvergence() {
  const players = [
    new SidecarProbe("断线收敛 A"),
    new SidecarProbe("断线收敛 B"),
    new SidecarProbe("断线收敛 C"),
  ];
  try {
    await preparePlayers(players, "abandoned");
    await formPublicTable(players);
    await waitForHand(players, 1, 120_000);
    const leaver = players.find((player) => handState(player, 1)?.can_act === false);
    assert(leaver !== undefined, "三人桌缺少非行动者可用于断线验证");
    const leaverPlayerId = leaver.latest("identity_ready")?.player_id;
    assert(typeof leaverPlayerId === "string", "断线玩家缺少持久身份");
    const remaining = players.filter((player) => player !== leaver);
    const requestId = randomUUID();
    leaver.send({ type: "leave_table", request_id: requestId });
    await waitForWhere(
      leaver,
      "command_confirmed",
      (event) => event.request_id === requestId,
      10_000,
    );
    await waitFor(leaver, "safe_leave_requested", 10_000);
    await delay(2_500);
    await leaver.crash();

    const aborted = await Promise.all(
      remaining.map((player) =>
        waitForWhere(
          player,
          "hand_aborted_for_leave",
          (event) => event.hand_number === 1 && event.player_id === leaverPlayerId,
          45_000,
        ),
      ),
    );
    assert(
      new Set(aborted.map((event) => event.evidence_hash)).size === 1,
      "剩余玩家没有从签名离桌证据推导出同一作废摘要",
    );
    await Promise.all(
      remaining.map((player) =>
        waitForWhere(
          player,
          "room_snapshot",
          (event) =>
            event.seats.length === 2 &&
            event.seats.every((seat) => seat.player_id !== leaverPlayerId),
          75_000,
        ),
      ),
    );
    await waitForHand(remaining, 2, 120_000);
    for (const player of remaining) {
      assert(
        player.countWhere("receipt_finalized", (event) => event.hand_number === 1) === 0,
        "作废手牌错误生成了可计入战绩的凭证",
      );
      assert(
        player.countWhere("hand_settled", (event) => event.hand_number === 1) === 0,
        "作废手牌错误生成了结算结果",
      );
    }
    return {
      evidenceHash: aborted[0].evidence_hash,
      incompleteHandRecorded: false,
      remainingPlayersStartedHand: 2,
    };
  } catch (error) {
    throw new Error(formatFailure(error, players));
  } finally {
    await Promise.all(players.map((player) => player.stop()));
  }
}

async function preparePlayers(players, namespace) {
  await Promise.all(players.map((player) => waitFor(player, "listen_address", 20_000)));
  const observedAt = Date.now();
  players.forEach((player, index) => {
    player.send({
      type: "token_snapshot",
      lifetime_tokens: 5_000_000_000 + index * 100_000_000,
      username: `safe-leave-${namespace}-${String(index + 1)}`,
      display_name: `安全离桌 ${namespace} ${String(index + 1)}`,
      observed_at_unix_ms: observedAt + index,
    });
  });
  await Promise.all(players.map((player) => waitFor(player, "token_snapshot_accepted", 15_000)));
  players.forEach((player, index) => {
    player.send({
      type: "create_identity",
      recovery_secret: `token-poker-${namespace}-safe-leave-player-${String(index + 1)}`,
      device_label: `安全离桌设备 ${String(index + 1)}`,
    });
  });
  await Promise.all(players.map((player) => waitFor(player, "identity_ready", 25_000)));
  const [host, ...guests] = players;
  for (const guest of guests) guest.send({ type: "dial", address: dialableAddress(host) });
  await Promise.all(
    guests.map((guest) => {
      const peerId = guest.latest("ready")?.peer_id;
      return waitForWhere(host, "peer_connected", (event) => event.peer_id === peerId, 20_000);
    }),
  );
}

async function formPublicTable(players) {
  const [host, ...guests] = players;
  joinPool(host, 80_000_000);
  const initial = await waitForWhere(
    host,
    "room_snapshot",
    (event) => event.seats.length === 1 && event.local_role === "seated",
    25_000,
  );
  guests.forEach((guest, index) => joinPool(guest, 90_000_000 + index * 10_000_000));
  await Promise.all(
    players.map((player) =>
      waitForWhere(
        player,
        "room_snapshot",
        (event) => event.table_id === initial.table_id && event.seats.length === players.length,
        60_000,
      ),
    ),
  );
}

function joinPool(player, buyIn) {
  player.send({ type: "join_public_pool", level_id: LEVEL_ID, buy_in: buyIn });
}

async function waitForHand(players, handNumber, timeoutMs) {
  await Promise.all(
    players.map((player) =>
      waitForWhere(player, "hand_ready", (event) => event.hand_number === handNumber, timeoutMs),
    ),
  );
  await Promise.all(
    players.map((player) =>
      waitForWhere(player, "hand_state", (event) => event.hand_number === handNumber, 30_000),
    ),
  );
  await waitUntil(
    () => players.some((player) => handState(player, handNumber)?.can_act === true),
    10_000,
    `第 ${String(handNumber)} 手没有进入可行动状态`,
  );
  const ready = players.map((player) =>
    player.latestWhere("hand_ready", (event) => event.hand_number === handNumber),
  );
  assert(new Set(ready.map((event) => event.table_id)).size === 1, "手牌桌号未在参与者间收敛");
  assert(new Set(ready.map((event) => event.transcript_hash)).size === 1, "手牌摘要未在参与者间收敛");
}

function handState(player, handNumber) {
  return player.latestWhere("hand_state", (event) => event.hand_number === handNumber);
}

function dialableAddress(probe) {
  const event = probe.latestWhere(
    "listen_address",
    (candidate) => typeof candidate.address === "string" && candidate.address.includes("/tcp/"),
  );
  const peerId = probe.latest("ready")?.peer_id;
  assert(event !== null && typeof peerId === "string", `${probe.label} 缺少 TCP 地址或 PeerId`);
  let address = event.address
    .replace("/ip4/0.0.0.0/", "/ip4/127.0.0.1/")
    .replace("/ip6/::/", "/ip6/::1/");
  if (!address.includes("/p2p/")) address = `${address}/p2p/${peerId}`;
  return address;
}

async function waitFor(probe, type, timeoutMs) {
  await waitForWhere(probe, type, () => true, timeoutMs);
  return probe.latest(type);
}

async function waitForWhere(probe, type, predicate, timeoutMs) {
  await waitUntil(
    () => {
      if (probe.process.exitCode !== null) throw new Error(`${probe.label} 在等待 ${type} 时提前退出`);
      return probe.latestWhere(type, predicate) !== null;
    },
    timeoutMs,
    `${probe.label} 等待 ${type} 超时`,
  );
  return probe.latestWhere(type, predicate);
}

async function waitUntil(predicate, timeoutMs, message) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await delay(100);
  }
  throw new Error(message);
}

function delay(milliseconds) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

function formatFailure(error, players) {
  return `${error instanceof Error ? error.message : String(error)}\n\n${players
    .map((player) => player.diagnostics())
    .join("\n\n")}`;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

await main();
