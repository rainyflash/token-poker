import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { access } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { integrationSidecarPath } from "./integration-runtime.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(scriptDirectory, "..");
const sidecarPath = integrationSidecarPath(projectRoot);
const relayVerification = process.argv.includes("--relay");
const gossipFailureVerification = process.argv.includes("--without-gossip");

class SidecarProbe {
  #buffer = "";
  #errors = "";
  #events = [];

  constructor(label, argumentsList = [], environment = {}) {
    this.label = label;
    this.process = spawn(sidecarPath, argumentsList, {
      cwd: projectRoot,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
      env: { ...process.env, ...environment },
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

  latestWhere(type, predicate) {
    return this.#events.findLast((event) => event?.type === type && predicate(event)) ?? null;
  }

  count(type) {
    return this.#events.filter((event) => event?.type === type).length;
  }

  diagnostics() {
    const noisyEvents = new Set(["gossip_message", "pool_directory_updated", "pool_ticket_published"]);
    return `${this.label} stderr:\n${this.#errors}\n${this.label} events:\n${this.#events
      .filter((event) => !noisyEvents.has(event?.type))
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
  const discoveryArguments = relayVerification
    ? [
        "--rendezvous-server",
        "--relay-server",
        "--public-node",
        "--volunteer-consent=granted",
        "--network-cost=unmetered",
        "--power-source=ac",
        "--relay-max-circuits=8",
        "--relay-max-circuits-per-peer=4",
      ]
    : ["--rendezvous-server"];
  const discovery = new SidecarProbe("社区发现端", discoveryArguments);
  const host = new SidecarProbe("主端", [], gossipFailureVerification ? {
    TOKEN_POKER_TEST_DROP_POOL_GOSSIP: "1",
  } : {});
  const guest = new SidecarProbe("访客端", [], gossipFailureVerification ? {
    TOKEN_POKER_TEST_DROP_POOL_GOSSIP: "1",
    TOKEN_POKER_TEST_DROP_ROOM_GOSSIP: "1",
  } : {});

  try {
    await Promise.all([
      waitFor(discovery, "listen_address", 15_000),
      waitFor(host, "listen_address", 15_000),
      waitFor(guest, "listen_address", 15_000),
    ]);
    host.send({
      type: "token_snapshot",
      lifetime_tokens: 3_550_000_000,
      username: "p2p-host",
      display_name: "P2P 验证主端",
      observed_at_unix_ms: Date.now(),
    });
    guest.send({
      type: "token_snapshot",
      lifetime_tokens: 3_550_000_000,
      username: "p2p-guest",
      display_name: "P2P 验证访客端",
      observed_at_unix_ms: Date.now(),
    });
    await waitForBoth(host, guest, "token_snapshot_accepted", 10_000);
    host.send({
      type: "create_identity",
      expected_account_fingerprint: host.latest("token_snapshot_accepted").account_fingerprint,
      recovery_secret: "token-holdem-host-p2p-verification",
      device_label: "P2P 验证主端",
    });
    guest.send({
      type: "create_identity",
      expected_account_fingerprint: guest.latest("token_snapshot_accepted").account_fingerprint,
      recovery_secret: "token-holdem-guest-p2p-verification",
      device_label: "P2P 验证访客端",
    });
    await waitForBoth(host, guest, "identity_ready", 20_000);

    const discoveryAddress = dialableAddress(discovery);
    if (relayVerification) {
      discovery.send({
        type: "add_external_address",
        address: publishableLocalAddress(discovery),
      });
      await waitFor(discovery, "advertised_address_added", 10_000);
    }
    for (const player of [host, guest]) {
      if (relayVerification) {
        player.send({ type: "use_relay", address: discoveryAddress });
      } else {
        player.send({ type: "add_external_address", address: publishableLocalAddress(player) });
      }
    }
    if (relayVerification) {
      await waitForBoth(host, guest, "relay_reservation_accepted", 30_000);
    }
    for (const player of [host, guest]) {
      player.send({
        type: "configure_discovery",
        addresses: [discoveryAddress],
        namespace: "token-holdem/verification",
      });
    }
    await waitForBoth(host, guest, "discovery_configured", 20_000);
    await waitForBoth(host, guest, "rendezvous_registered", 20_000);

    guest.send({
      type: "join_public_pool",
      level_id: "1m-2m",
      buy_in: 90_000_000,
    });
    host.send({
      type: "join_public_pool",
      level_id: "1m-2m",
      buy_in: 80_000_000,
    });
    await waitUntil(
      () => host.latest("peers_discovered")?.peers > 0 || guest.latest("peers_discovered")?.peers > 0,
      30_000,
      "双方没有通过社区 Rendezvous 自动发现对手",
    );
    await waitForBothWhere(
      host,
      guest,
      "room_snapshot",
      (event) => event.seats.length === 2,
      45_000,
    );

    const hostRoom = host.latestWhere("room_snapshot", (event) => event.seats.length === 2);
    const guestRoom = guest.latestWhere("room_snapshot", (event) => event.seats.length === 2);
    assert(hostRoom.table_id === guestRoom.table_id, "双端进入了不同的动态牌桌");
    assert(
      new Set(hostRoom.seats.map((seat) => seat.physical_seat)).size === 2,
      "动态牌桌给两名玩家分配了重复席位",
    );
    if (gossipFailureVerification) {
      assert(
        host.latest("pool_ticket_published")?.published_to_mesh === false &&
          guest.latest("pool_ticket_published")?.published_to_mesh === false,
        "测试没有真正关闭匹配池 Gossip，请使用调试版验证故障恢复",
      );
    }
    assert(
      host.count("pool_directory_updated") < 20 && guest.count("pool_directory_updated") < 20,
      "匹配池重复发送了没有变化的目录状态",
    );

    if (relayVerification) {
      assert(
        [host, guest].some(
          (player) =>
            player.latestWhere(
              "relay_circuit_established",
              (event) => event.direction === "inbound",
            ) !== null,
        ),
        "双端均未接受 Circuit Relay 连接",
      );
      assert(
        [host, guest].some(
          (player) =>
            player.latestWhere(
              "relay_circuit_established",
              (event) => event.direction === "outbound",
            ) !== null,
        ),
        "双端均未建立出站 Circuit Relay 连接",
      );
    }

    if (!relayVerification) await discovery.stop();

    await waitForBoth(host, guest, "hand_ready", 75_000);
    await waitForBoth(host, guest, "hand_state", 20_000);
    await waitForBothWhere(
      host,
      guest,
      "hand_protocol_progress",
      (event) => event.phase === "dealing" && event.completed === 2,
      20_000,
    );
    const hostReady = host.latest("hand_ready");
    const guestReady = guest.latest("hand_ready");
    const initialHostState = host.latest("hand_state");
    const initialGuestState = guest.latest("hand_state");
    assert(hostReady.table_id === guestReady.table_id, "双端私牌属于不同牌桌");
    assert(hostReady.hand_number === guestReady.hand_number, "双端手牌编号不同");
    assert(hostReady.hole_cards.length === 2, "主端没有收到两张私牌");
    assert(guestReady.hole_cards.length === 2, "访客端没有收到两张私牌");
    assert(hostReady.transcript_hash === guestReady.transcript_hash, "双端初始协议摘要不一致");
    assert(initialHostState.action_timeout_ms === 30_000, "主端没有公布 30 秒行动时限");
    assert(initialGuestState.action_timeout_ms === 30_000, "访客端没有公布 30 秒行动时限");
    assert(
      Number.isSafeInteger(initialHostState.turn_deadline_unix_ms) &&
        initialHostState.turn_deadline_unix_ms > Date.now(),
      "主端没有公布当前行动截止时间",
    );
    assert(
      initialHostState.seats.filter((seat) => seat.committed > 0).length === 2,
      "主端初始状态没有同时包含大小盲下注",
    );

    let actionCount = 0;
    while (host.latest("hand_settled") === null || guest.latest("hand_settled") === null) {
      assert(actionCount < 12, "完整摊牌需要的动作数异常");
      await waitUntil(
        () => {
          if (host.latest("hand_settled") !== null && guest.latest("hand_settled") !== null) {
            return true;
          }
          return host.latest("hand_state")?.can_act === true || guest.latest("hand_state")?.can_act === true;
        },
        30_000,
        "等待下一位玩家行动超时",
      );
      if (host.latest("hand_settled") !== null && guest.latest("hand_settled") !== null) break;
      const hostState = host.latest("hand_state");
      const guestState = guest.latest("hand_state");
      const actor = hostState.can_act ? host : guestState.can_act ? guest : null;
      const actorState = actor === host ? hostState : guestState;
      assert(actor !== null && actorState !== null, "当前没有唯一可行动玩家");
      if (actionCount === 0) {
        for (const mismatch of [{ table_id: "ff".repeat(32) }, { hand_number: actorState.hand_number + 1 },
          { sequence: actorState.sequence + 1 }, { public_state_hash: "ff".repeat(32) }]) {
          const requestId = randomUUID();
          actor.send({ type: "submit_action", request_id: requestId, action: "call",
            expected: { table_id: actorState.table_id, hand_number: actorState.hand_number,
              sequence: actorState.sequence, public_state_hash: actorState.public_state_hash, ...mismatch } });
          await waitUntil(() => actor.latestWhere("command_failed", (event) => event.request_id === requestId) !== null, 5000, "过期动作没有被拒绝");
          assert(actor.latest("hand_state").sequence === actorState.sequence, "过期动作错误推进了手牌序号");
        }
      }
      actor.send({
        type: "submit_action",
        expected: { table_id: actorState.table_id, hand_number: actorState.hand_number,
          sequence: actorState.sequence, public_state_hash: actorState.public_state_hash },
        action: actorState.to_call === 0 ? "check" : "call",
      });
      actionCount += 1;
      const expectedSequence = actionCount;
      await waitUntil(
        () => {
          if (host.latest("hand_settled") !== null && guest.latest("hand_settled") !== null) {
            return true;
          }
          const nextHost = host.latest("hand_state");
          const nextGuest = guest.latest("hand_state");
          return (
            nextHost?.sequence >= expectedSequence &&
            nextGuest?.sequence >= expectedSequence &&
            (nextHost.can_act === true || nextGuest.can_act === true)
          );
        },
        30_000,
        `双端没有确认动作 #${String(expectedSequence)}`,
      );
      if (expectedSequence === 1) {
        assert(
          host.latest("hand_state").seats.some((seat) => seat.last_action !== null),
          "主端没有投影首个玩家动作",
        );
        assert(
          guest.latest("hand_state").seats.some((seat) => seat.last_action !== null),
          "访客端没有投影首个玩家动作",
        );
      }
    }

    const hostSettlement = host.latest("hand_settled");
    const guestSettlement = guest.latest("hand_settled");
    const hostFinalState = host.latest("hand_state");
    const guestFinalState = guest.latest("hand_state");
    assert(hostFinalState.sequence === actionCount, "主端动作序列不完整");
    assert(guestFinalState.sequence === actionCount, "访客端动作序列不完整");
    assert(hostFinalState.transcript_hash === guestFinalState.transcript_hash, "双端动作日志摘要不一致");
    assert(hostFinalState.board.length === 5, "主端没有完成五张公共牌的可验证公开");
    assert(guestFinalState.board.length === 5, "访客端没有完成五张公共牌的可验证公开");
    assert(JSON.stringify(hostFinalState.board) === JSON.stringify(guestFinalState.board), "双端公共牌不一致");
    assert(
      JSON.stringify(hostSettlement.outcomes) === JSON.stringify(guestSettlement.outcomes),
      "双端结算结果不一致",
    );
    assert(hostSettlement.transcript_hash === guestSettlement.transcript_hash, "双端最终协议摘要不一致");
    const totalDelta = hostSettlement.outcomes.reduce((sum, outcome) => sum + outcome.delta, 0);
    assert(totalDelta === 0, "结算不是零和结果");

    process.stdout.write(
      `${JSON.stringify({
        ok: true,
        tableId: hostRoom.table_id,
        handNumber: hostReady.hand_number,
        actions: actionCount,
        transcriptHash: hostSettlement.transcript_hash,
        outcomes: hostSettlement.outcomes,
        checkpoints: {
          matched: true,
          directPoolSyncWithoutGossip: gossipFailureVerification,
          rendezvousDiscovery: true,
          rendezvousOfflineDuringHand: !relayVerification,
          circuitRelayEstablished: relayVerification,
          keyExchange: host.count("hand_protocol_progress") > 0,
          privateDeal: true,
          dealBarrier: true,
          signedAction: true,
          settled: true,
        },
      })}\n`,
    );
  } catch (error) {
    throw new Error(
      `${error instanceof Error ? error.message : String(error)}\n\n${discovery.diagnostics()}\n\n${host.diagnostics()}\n\n${guest.diagnostics()}`,
    );
  } finally {
    await Promise.all([discovery.stop(), host.stop(), guest.stop()]);
  }
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
  const ready = probe.latest("ready");
  assert(event !== null && typeof ready?.peer_id === "string", `${probe.label} 缺少地址或 PeerId`);
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
  // Production refuses to publish loopback IPs as public addresses. Local
  // Rendezvous verification uses a DNS form that passes the same validation,
  // while the direct-dial helper remains pinned to IPv4 loopback.
  return `/dns4/localhost/tcp/${tcpPort}/p2p/${peerId}`;
}

async function waitForBoth(left, right, type, timeoutMs) {
  await Promise.all([waitFor(left, type, timeoutMs), waitFor(right, type, timeoutMs)]);
}

async function waitForBothWhere(left, right, type, predicate, timeoutMs) {
  await Promise.all([
    waitForWhere(left, type, predicate, timeoutMs),
    waitForWhere(right, type, predicate, timeoutMs),
  ]);
}

async function waitFor(probe, type, timeoutMs) {
  await waitForWhere(probe, type, () => true, timeoutMs);
}

async function waitForWhere(probe, type, predicate, timeoutMs) {
  await waitUntil(
    () => {
      if (probe.process.exitCode !== null) {
        throw new Error(`${probe.label} 在等待 ${type} 时提前退出`);
      }
      return probe.latestWhere(type, predicate) !== null;
    },
    timeoutMs,
    `${probe.label} 等待 ${type} 超时`,
  );
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
