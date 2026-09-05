import { spawn } from "node:child_process";
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
const joinWaitersDuringHand = process.env.TOKEN_HOLDEM_JOIN_WAITERS_DURING_HAND !== "0";

class SidecarProbe {
  #buffer = "";
  #errors = "";
  #events = [];
  #exit = null;

  constructor(label) {
    this.label = label;
    // Multi-process integration tests must isolate TUN, WSL, and WLAN routing
    // differences. Production listens on all interfaces and lets the sidecar
    // address policy choose relay, public, or private paths.
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
    this.process.on("exit", (code, signal) => {
      this.#exit = { code, signal };
    });
  }

  send(command) {
    this.process.stdin.write(`${JSON.stringify(command)}\n`);
  }

  latest(type) {
    return this.#events.findLast((event) => event?.type === type) ?? null;
  }

  latestWhere(type, predicate) {
    return this.#events.findLast((event) => event?.type === type && predicate(event)) ?? null;
  }

  diagnostics() {
    const warningCounts = Object.fromEntries(
      Object.entries(
        this.#events
          .filter((event) => event?.type === "warning")
          .reduce((counts, event) => {
            const key = String(event.message);
            return { ...counts, [key]: (counts[key] ?? 0) + 1 };
          }, {}),
      ).sort(([left], [right]) => left.localeCompare(right)),
    );
    const milestoneTypes = new Set([
      "peer_connected",
      "peer_disconnected",
      "pool_joining_table",
      "pool_creating_table",
      "room_entered",
      "room_closed",
      "room_snapshot",
      "next_hand_ready",
      "hand_protocol_started",
      "hand_protocol_progress",
      "hand_ready",
      "hand_settled",
      "hand_session_interrupted",
      "hand_session_resumed",
      "receipt_consensus_progress",
      "receipt_finalized",
    ]);
    const latestConsensus = ["membership_confirmation", "hand_roster_confirmation"].flatMap((type) => {
      const event = this.latest(type);
      return event === null ? [] : [event];
    });
    const recentHandStates = this.#events.filter((event) => event?.type === "hand_state").slice(-4);
    const recentDisconnects = this.#events
      .filter((event) => event?.type === "peer_disconnected")
      .slice(-12);
    const milestones = this.#events
      .filter((event) => milestoneTypes.has(event?.type))
      .slice(-16)
      .concat(recentDisconnects)
      .concat(latestConsensus)
      .concat(recentHandStates);
    return `${this.label} PeerId: ${String(this.latest("ready")?.peer_id ?? "未知")}\n${this.label} 进程状态: ${JSON.stringify(this.#exit ?? { code: this.process.exitCode, signal: this.process.signalCode })}\n${this.label} stderr:\n${this.#errors}\n${this.label} 警告计数:\n${JSON.stringify(warningCounts)}\n${this.label} 协议里程碑:\n${milestones
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
  const players = Array.from({ length: 7 }, (_, index) => new SidecarProbe(`玩家 ${String(index + 1)}`));
  const [host, second, third, fourth, fifth, sixth, seventh] = players;

  try {
    await Promise.all(players.map((player) => waitFor(player, "listen_address", 20_000)));
    const observedAt = Date.now();
    players.forEach((player, index) => {
      player.send({
        type: "token_snapshot",
        lifetime_tokens: 5_000_000_000 + index * 100_000_000,
        username: `dynamic-player-${String(index + 1)}`,
        display_name: `动态牌桌玩家 ${String(index + 1)}`,
        observed_at_unix_ms: observedAt,
      });
    });
    await Promise.all(players.map((player) => waitFor(player, "token_snapshot_accepted", 15_000)));

    players.forEach((player, index) => {
      player.send({
        type: "create_identity",
        expected_account_fingerprint: player.latest("token_snapshot_accepted").account_fingerprint,
        recovery_secret: `token-holdem-dynamic-table-player-${String(index + 1)}`,
        device_label: `动态牌桌玩家 ${String(index + 1)}`,
      });
    });
    await Promise.all(players.map((player) => waitFor(player, "identity_ready", 25_000)));

    for (const player of players.slice(1)) {
      player.send({ type: "dial", address: dialableAddress(host) });
    }
    await Promise.all(
      players.slice(1).map((player) => {
        const peerId = player.latest("ready")?.peer_id;
        return waitForWhere(
          host,
          "peer_connected",
          (event) => event.peer_id === peerId,
          20_000,
        );
      }),
    );

    joinPool(host, 80_000_000);
    const hostSingle = await waitForWhere(
      host,
      "room_snapshot",
      (event) => event.seats.length === 1 && event.local_role === "seated",
      20_000,
    );
    const mainTableId = hostSingle.table_id;

    joinPool(second, 90_000_000);
    await waitForPlayersAtTable([host, second], mainTableId, 2, 45_000);
    await waitForHand([host, second], 1, 75_000);

    joinPool(third, 100_000_000);
    const thirdWaiting = await waitForWhere(
      third,
      "room_snapshot",
      (event) => event.table_id === mainTableId && event.local_role === "waiting",
      45_000,
    );
    assert(thirdWaiting.seats.length === 2, "第三名玩家加入时破坏了已冻结的首手名单");
    assert(third.latestWhere("hand_ready", (event) => event.hand_number === 1) === null, "候补玩家收到首手私牌");

    const firstHand = await playHand([host, second], 1);
    await waitForHand([host, second, third], 2, 90_000);

    if (joinWaitersDuringHand) {
      await joinAdditionalPlayers([fourth, fifth, sixth], mainTableId, "waiting");
    }

    const secondHand = await playHand([host, second, third], 2);
    if (!joinWaitersDuringHand) {
      await joinAdditionalPlayers([fourth, fifth, sixth], mainTableId, "seated");
    }
    await waitForPlayersAtTable(players.slice(0, 6), mainTableId, 6, 90_000);
    await waitForHand(players.slice(0, 6), 3, 120_000);

    joinPool(seventh, 140_000_000);
    const overflowRoom = await waitForWhere(
      seventh,
      "room_snapshot",
      (event) => event.seats.length === 1 && event.local_role === "seated",
      45_000,
    );
    assert(overflowRoom.table_id !== mainTableId, "第七名玩家错误挤入已满六人桌");

    process.stdout.write(
      `${JSON.stringify({
        ok: true,
        mainTableId,
        overflowTableId: overflowRoom.table_id,
        expansion: [2, 3, 6],
        waitingIsolation: true,
        firstHand,
        secondHand,
        seventhPlayerCreatedNewTable: true,
      })}\n`,
    );
  } catch (error) {
    throw new Error(
      `${error instanceof Error ? error.message : String(error)}\n\n${players
        .map((player) => player.diagnostics())
        .join("\n\n")}`,
    );
  } finally {
    await Promise.all(players.map((player) => player.stop()));
  }
}

function joinPool(player, buyIn) {
  player.send({ type: "join_public_pool", level_id: LEVEL_ID, buy_in: buyIn });
}

async function joinAdditionalPlayers(players, tableId, expectedRole) {
  players.forEach((player, index) => joinPool(player, 110_000_000 + index * 10_000_000));
  await Promise.all(
    players.map((player) =>
      waitForWhere(
        player,
        "room_snapshot",
        (event) => event.table_id === tableId && event.local_role === expectedRole,
        60_000,
      ),
    ),
  );
}

async function waitForPlayersAtTable(players, tableId, seats, timeoutMs) {
  return Promise.all(
    players.map((player) =>
      waitForWhere(
        player,
        "room_snapshot",
        (event) => event.table_id === tableId && event.seats.length === seats,
        timeoutMs,
      ),
    ),
  );
}

async function waitForHand(players, handNumber, timeoutMs) {
  await Promise.all(
    players.map((player) =>
      waitForWhere(
        player,
        "hand_ready",
        (event) => event.hand_number === handNumber,
        timeoutMs,
        `第 ${String(handNumber)} 手`,
      ),
    ),
  );
  const readyEvents = players.map((player) =>
    player.latestWhere("hand_ready", (event) => event.hand_number === handNumber),
  );
  const tableIds = new Set(readyEvents.map((event) => event.table_id));
  const transcripts = new Set(readyEvents.map((event) => event.transcript_hash));
  assert(tableIds.size === 1, `第 ${String(handNumber)} 手参与者桌号不一致`);
  assert(transcripts.size === 1, `第 ${String(handNumber)} 手初始协议摘要不一致`);
}

async function playHand(players, handNumber) {
  await waitForHand(players, handNumber, 90_000);
  await Promise.all(
    players.map((player) =>
      waitForWhere(player, "hand_state", (event) => event.hand_number === handNumber, 30_000),
    ),
  );
  let actionCount = 0;
  while (!players.every((player) => settled(player, handNumber))) {
    assert(actionCount < 48, `第 ${String(handNumber)} 手牌动作数异常`);
    await waitUntil(
      () =>
        players.every((player) => settled(player, handNumber)) ||
        players.some((player) => handState(player, handNumber)?.can_act === true),
      45_000,
      `第 ${String(handNumber)} 手等待行动者超时`,
    );
    if (players.every((player) => settled(player, handNumber))) break;
    const actor = players.find((player) => handState(player, handNumber)?.can_act === true);
    const actorState = actor === undefined ? null : handState(actor, handNumber);
    assert(actor !== undefined && actorState !== null, `第 ${String(handNumber)} 手没有唯一行动者`);
    const expectedSequence = actorState.sequence + 1;
    actor.send({
      type: "submit_action",
      expected: { table_id: actorState.table_id, hand_number: actorState.hand_number,
        sequence: actorState.sequence, public_state_hash: actorState.public_state_hash },
      action: actorState.to_call === 0 ? "check" : "call",
    });
    actionCount += 1;
    await waitUntil(
      () =>
        players.every((player) => settled(player, handNumber)) ||
        players.every((player) => {
          const state = handState(player, handNumber);
          return state !== null && state.sequence >= expectedSequence;
        }),
      45_000,
      `第 ${String(handNumber)} 手动作 #${String(actionCount)} 未获全员确认`,
    );
  }
  const settlements = players.map((player) =>
    player.latestWhere("hand_settled", (event) => event.hand_number === handNumber),
  );
  assert(settlements.every((event) => event !== null), `第 ${String(handNumber)} 手没有全员结算`);
  assert(
    new Set(settlements.map((event) => event.transcript_hash)).size === 1,
    `第 ${String(handNumber)} 手最终协议摘要不一致`,
  );
  await Promise.all(
    players.map((player) =>
      waitForWhere(
        player,
        "receipt_finalized",
        (event) => event.hand_number === handNumber,
        30_000,
      ),
    ),
  );
  return { handNumber, actions: actionCount, transcriptHash: settlements[0].transcript_hash };
}

function handState(player, handNumber) {
  return player.latestWhere("hand_state", (event) => event.hand_number === handNumber);
}

function settled(player, handNumber) {
  return player.latestWhere("hand_settled", (event) => event.hand_number === handNumber) !== null;
}

function dialableAddress(probe) {
  const preferred = probe.latestWhere(
    "listen_address",
    (candidate) =>
      typeof candidate.address === "string" && candidate.address.includes("/ip4/127.0.0.1/tcp/"),
  );
  const fallback = probe.latestWhere(
    "listen_address",
    (candidate) => typeof candidate.address === "string" && candidate.address.includes("/tcp/"),
  );
  const event = preferred ?? fallback;
  const peerId = probe.latest("ready")?.peer_id;
  assert(event !== null && typeof peerId === "string", `${probe.label} 缺少 TCP 地址或 PeerId`);
  let address = event.address
    .replace("/ip4/0.0.0.0/", "/ip4/127.0.0.1/")
    .replace("/ip6/::/", "/ip6/::1/");
  if (!address.includes("/p2p/")) address = `${address}/p2p/${peerId}`;
  return address;
}

async function waitFor(probe, type, timeoutMs) {
  return waitForWhere(probe, type, () => true, timeoutMs);
}

async function waitForWhere(probe, type, predicate, timeoutMs, context = "") {
  await waitUntil(
    () => {
      if (probe.process.exitCode !== null) {
        throw new Error(`${probe.label} 在等待 ${type} 时提前退出`);
      }
      return probe.latestWhere(type, predicate) !== null;
    },
    timeoutMs,
    `${probe.label} 等待 ${context.length === 0 ? "" : `${context} `}${type} 超时`,
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

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

await main();
