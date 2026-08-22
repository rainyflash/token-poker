import { spawn } from "node:child_process";
import { access, mkdir, readdir, rm } from "node:fs/promises";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(scriptDirectory, "..");
const sidecarPath = join(projectRoot, "target", "debug", "token-holdem-sidecar.exe");
const integrationRoot = resolve(projectRoot, "target", "integration");
const archiveDirectory = resolve(
  integrationRoot,
  `archive-${String(process.pid)}-${String(Date.now())}`,
);

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

  latestWhere(type, predicate) {
    return (
      this.#events.findLast((event) => event?.type === type && predicate(event)) ?? null
    );
  }

  count(type) {
    return this.#events.filter((event) => event?.type === type).length;
  }

  countWhere(type, predicate) {
    return this.#events.filter((event) => event?.type === type && predicate(event)).length;
  }

  diagnostics() {
    const noisyEvents = new Set(["gossip_message", "pool_directory_updated", "pool_ticket_published"]);
    return `${this.label} stderr:\n${this.#errors}\n${this.label} 最近事件:\n${this.#events
      .filter((event) => !noisyEvents.has(event?.type))
      .slice(-180)
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
  await mkdir(integrationRoot, { recursive: true });
  assert(
    archiveDirectory.startsWith(`${integrationRoot}${sep}`),
    "临时归档目录逃逸出集成测试目录",
  );
  await mkdir(archiveDirectory, { recursive: false });

  let archive = new SidecarProbe("归档端", ["--archive-dir", archiveDirectory]);
  let host = new SidecarProbe("主端");
  let guest = new SidecarProbe("访客端");
  let restored = null;

  try {
    await waitForAll([archive, host, guest], "listen_address", 20_000);
    await waitFor(archive, "archive_node_ready", 10_000);
    const firstArchiveKey = archive.latest("archive_node_ready")?.public_key;
    const firstArchivePeerId = archive.latest("ready")?.peer_id;
    const archiveAddress = dialableAddress(archive);

    host.send({
      type: "token_snapshot",
      lifetime_tokens: 3_550_000_000,
      username: "complete-host",
      display_name: "完整验收主端",
      observed_at_unix_ms: Date.now(),
    });
    guest.send({
      type: "token_snapshot",
      lifetime_tokens: 3_550_000_000,
      username: "complete-guest",
      display_name: "完整验收访客端",
      observed_at_unix_ms: Date.now(),
    });
    await waitForBoth(host, guest, "token_snapshot_accepted", 10_000);

    for (const player of [host, guest]) {
      player.send({
        type: "configure_archive_nodes",
        addresses: [archiveAddress],
        minimum_confirmed_replicas: 1,
      });
    }
    await waitForBoth(host, guest, "archive_peers_configured", 20_000);
    host.send({
      type: "create_identity",
      recovery_secret: "token-holdem-host-complete-verification",
      device_label: "完整验收主端",
    });
    guest.send({
      type: "create_identity",
      recovery_secret: "token-holdem-guest-complete-verification",
      device_label: "完整验收访客端",
    });
    await waitForBoth(host, guest, "identity_ready", 20_000);
    await waitForBoth(host, guest, "recovery_backup_stored", 30_000);
    for (const player of [host, guest]) {
      player.send({ type: "add_external_address", address: publishableLocalAddress(player) });
    }
    await waitForBoth(host, guest, "advertised_address_added", 10_000);
    const hostPlayerId = host.latest("identity_ready")?.player_id;
    assert(typeof hostPlayerId === "string", "主端没有输出玩家身份编号");

    host.send({
      type: "create_friend_room",
      level_id: "1m-2m",
      buy_in: 80_000_000,
    });
    await waitFor(host, "friend_room_created", 10_000);
    const invite = host.latest("friend_room_created")?.invite_code;
    assert(typeof invite === "string", "主端没有生成签名好友房邀请");
    guest.send({ type: "join_friend_room", invite_code: invite, buy_in: 120_000_000 });
    await waitFor(guest, "friend_room_joined", 20_000);
    await waitForBothWhere(
      host,
      guest,
      "room_snapshot",
      (event) => event.seats.length === 2,
      45_000,
    );

    const hostRoom = host.latestWhere("room_snapshot", (event) => event.seats.length === 2);
    const guestRoom = guest.latestWhere("room_snapshot", (event) => event.seats.length === 2);
    assert(hostRoom.table_id === guestRoom.table_id, "好友房双端进入了不同牌桌");
    assert(
      new Set(hostRoom.seats.map((seat) => seat.physical_seat)).size === 2,
      "好友房双端被分配到同一物理席位",
    );
    assert(
      JSON.stringify(hostRoom.seats.map((seat) => seat.buy_in).sort((left, right) => left - right)) ===
        JSON.stringify([80_000_000, 120_000_000]),
      "好友房没有保留双方各自在级别范围内选择的买入额",
    );

    const handSummaries = [];
    for (let handNumber = 1; handNumber <= 3; handNumber += 1) {
      const summary = await playHand(host, guest, handNumber);
      handSummaries.push(summary);
      await waitForBothWhere(
        host,
        guest,
        "receipt_finalized",
        (event) => event.hand_number === handNumber,
        30_000,
      );
      const hostReceipt = host.latestWhere(
        "receipt_finalized",
        (event) => event.hand_number === handNumber,
      );
      const guestReceipt = guest.latestWhere(
        "receipt_finalized",
        (event) => event.hand_number === handNumber,
      );
      assert(hostReceipt.receipt_id === guestReceipt.receipt_id, `第 ${String(handNumber)} 手凭证编号不一致`);
      assert(hostReceipt.signatures === 2 && guestReceipt.signatures === 2, "联合签名数量不完整");

      await waitUntil(
        () =>
          host.count("receipt_archived") >= handNumber &&
          guest.count("receipt_archived") >= handNumber,
        30_000,
        `第 ${String(handNumber)} 手没有获得远端归档确认`,
      );
      await waitUntil(
        () =>
          host.latest("statistics_updated")?.completed_hands >= handNumber &&
          guest.latest("statistics_updated")?.completed_hands >= handNumber,
        20_000,
        `第 ${String(handNumber)} 手没有进入凭证统计投影`,
      );

      if (handNumber < 3) {
        await waitForBothWhere(
          host,
          guest,
          "hand_protocol_started",
          (event) => event.hand_number === handNumber + 1,
          45_000,
        );
      }
    }

    const hostStarts = [1, 2, 3].map((handNumber) =>
      host.latestWhere("hand_protocol_started", (event) => event.hand_number === handNumber),
    );
    assert(
      JSON.stringify(hostStarts.map((event) => event?.dealer_seat)) === JSON.stringify([1, 2, 1]),
      "连续三手没有按座位轮换庄家",
    );
    assert(
      hostStarts.every(
        (event) =>
          JSON.stringify([...event.buy_ins].sort((left, right) => left - right)) ===
          JSON.stringify([80_000_000, 120_000_000]),
      ),
      "下一手没有重置为入桌时双方签名买入额",
    );

    await waitUntil(
      () =>
        [host, guest].every((player) => {
          const statistics = player.latest("statistics_updated");
          return statistics?.completed_hands === 3 &&
            statistics.recent_hands.every((hand) => hand.archived === true);
        }),
      20_000,
      "归档成功后统计投影没有收敛为已归档",
    );
    const hostStatistics = host.latest("statistics_updated");
    const guestStatistics = guest.latest("statistics_updated");
    assert(hostStatistics.completed_hands === 3 && guestStatistics.completed_hands === 3, "三手战绩计数错误");
    assert(hostStatistics.net_chips + guestStatistics.net_chips === 0, "双方累计净输赢不是零和");
    assert(
      hostStatistics.recent_hands.every((hand) => hand.archived === true) &&
        guestStatistics.recent_hands.every((hand) => hand.archived === true),
      "归档成功后统计投影仍标记为未归档",
    );

    const archivedObjects = await readdir(join(archiveDirectory, "objects"));
    assert(archivedObjects.length === 3, "志愿归档节点没有按内容地址保存恰好三份凭证");
    const recoveryObjects = await readdir(join(archiveDirectory, "recovery"));
    assert(recoveryObjects.length === 2, "志愿归档节点没有保存双方的加密身份恢复包");

    await Promise.all([host.stop(), guest.stop(), archive.stop()]);
    host = null;
    guest = null;
    archive = new SidecarProbe("重启归档端", ["--archive-dir", archiveDirectory]);
    restored = new SidecarProbe("恢复玩家端");
    await waitForBoth(archive, restored, "listen_address", 20_000);
    await waitFor(archive, "archive_node_ready", 10_000);
    assert(
      archive.latest("archive_node_ready")?.public_key === firstArchiveKey,
      "归档节点重启后没有恢复同一签名密钥",
    );
    assert(
      archive.latest("ready")?.peer_id === firstArchivePeerId,
      "志愿节点重启后没有恢复同一 libp2p PeerId",
    );

    restored.send({
      type: "token_snapshot",
      lifetime_tokens: 3_550_000_000,
      username: "complete-host",
      display_name: "完整验收主端的新设备",
      observed_at_unix_ms: Date.now(),
    });
    await waitFor(restored, "token_snapshot_accepted", 10_000);
    restored.send({
      type: "configure_archive_nodes",
      addresses: [dialableAddress(archive)],
      minimum_confirmed_replicas: 1,
    });
    await waitFor(restored, "archive_peers_configured", 20_000);
    restored.send({
      type: "restore_remote_identity",
      recovery_secret: "token-holdem-host-complete-verification",
      device_label: "完整验收恢复端",
    });
    await waitFor(restored, "recovery_backup_fetched", 30_000);
    await waitFor(restored, "identity_ready", 30_000);
    assert(restored.latest("identity_ready")?.player_id === hostPlayerId, "远端恢复没有还原同一玩家身份");
    await waitUntil(
      () =>
        restored.latest("statistics_updated")?.completed_hands === 3 &&
        restored.count("receipt_fetched") >= 3,
      45_000,
      "新设备没有从重启后的志愿归档节点恢复三手战绩",
    );

    process.stdout.write(
      `${JSON.stringify({
        ok: true,
        tableId: hostRoom.table_id,
        hands: handSummaries,
        receipts: 3,
        archiveObjects: archivedObjects.length,
        recoveryObjects: recoveryObjects.length,
        restoredHands: restored.latest("statistics_updated")?.completed_hands,
        remoteIdentityRecovery: true,
        cumulativeZeroSum: true,
        privateFriendRoom: true,
        dealerRotation: [1, 2, 1],
      })}\n`,
    );
  } catch (error) {
    const probes = [archive, host, guest, restored].filter((probe) => probe !== null);
    throw new Error(
      `${error instanceof Error ? error.message : String(error)}\n\n${probes
        .map((probe) => probe.diagnostics())
        .join("\n\n")}`,
    );
  } finally {
    await Promise.all(
      [archive, host, guest, restored]
        .filter((probe) => probe !== null)
        .map((probe) => probe.stop()),
    );
    assert(
      archiveDirectory.startsWith(`${integrationRoot}${sep}`),
      "拒绝清理集成测试目录之外的路径",
    );
    await rm(archiveDirectory, { recursive: true, force: true });
  }
}

async function playHand(host, guest, handNumber) {
  await waitForBothWhere(
    host,
    guest,
    "hand_ready",
    (event) => event.hand_number === handNumber,
    90_000,
  );
  let actionCount = 0;
  while (
    host.latestWhere("hand_settled", (event) => event.hand_number === handNumber) === null ||
    guest.latestWhere("hand_settled", (event) => event.hand_number === handNumber) === null
  ) {
    assert(actionCount < 12, `第 ${String(handNumber)} 手完整摊牌需要的动作数异常`);
    await waitUntil(
      () => {
        const hostState = host.latestWhere("hand_state", (event) => event.hand_number === handNumber);
        const guestState = guest.latestWhere("hand_state", (event) => event.hand_number === handNumber);
        return hostState?.can_act === true || guestState?.can_act === true;
      },
      35_000,
      `第 ${String(handNumber)} 手等待下一位玩家行动超时`,
    );
    const hostState = host.latestWhere("hand_state", (event) => event.hand_number === handNumber);
    const guestState = guest.latestWhere("hand_state", (event) => event.hand_number === handNumber);
    const actor = hostState?.can_act === true ? host : guestState?.can_act === true ? guest : null;
    const actorState = actor === host ? hostState : guestState;
    assert(actor !== null && actorState !== null, "当前没有唯一可行动玩家");
    actor.send({
      type: "submit_action",
      action: actorState.to_call === 0 ? "check" : "call",
    });
    actionCount += 1;
    const expectedSequence = actionCount;
    await waitUntil(
      () => {
        if (
          host.latestWhere("hand_settled", (event) => event.hand_number === handNumber) !== null &&
          guest.latestWhere("hand_settled", (event) => event.hand_number === handNumber) !== null
        ) {
          return true;
        }
        const nextHost = host.latestWhere("hand_state", (event) => event.hand_number === handNumber);
        const nextGuest = guest.latestWhere("hand_state", (event) => event.hand_number === handNumber);
        return (
          nextHost?.sequence >= expectedSequence &&
          nextGuest?.sequence >= expectedSequence &&
          (nextHost.can_act === true || nextGuest.can_act === true)
        );
      },
      35_000,
      `第 ${String(handNumber)} 手双端没有确认动作 #${String(expectedSequence)}`,
    );
  }

  const hostSettlement = host.latestWhere("hand_settled", (event) => event.hand_number === handNumber);
  const guestSettlement = guest.latestWhere("hand_settled", (event) => event.hand_number === handNumber);
  const hostFinalState = host.latestWhere("hand_state", (event) => event.hand_number === handNumber);
  const guestFinalState = guest.latestWhere("hand_state", (event) => event.hand_number === handNumber);
  assert(hostFinalState.sequence === actionCount && guestFinalState.sequence === actionCount, "动作序列不完整");
  assert(hostFinalState.transcript_hash === guestFinalState.transcript_hash, "双端动作日志摘要不一致");
  assert(hostFinalState.board.length === 5 && guestFinalState.board.length === 5, "公共牌没有完整公开");
  assert(
    JSON.stringify(hostSettlement.outcomes) === JSON.stringify(guestSettlement.outcomes),
    "双端结算结果不一致",
  );
  const totalDelta = hostSettlement.outcomes.reduce((sum, outcome) => sum + outcome.delta, 0);
  assert(totalDelta === 0, "单手结算不是零和结果");
  return {
    handNumber,
    actions: actionCount,
    transcriptHash: hostSettlement.transcript_hash,
  };
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
  // Production refuses to publish loopback IPs as public addresses. Local
  // archive discovery keeps the DNS form, while the direct-dial helper selects
  // the deterministic IPv4 loopback endpoint.
  return `/dns4/localhost/tcp/${tcpPort}/p2p/${peerId}`;
}

async function waitForBoth(left, right, type, timeoutMs) {
  await Promise.all([waitFor(left, type, timeoutMs), waitFor(right, type, timeoutMs)]);
}

async function waitForAll(probes, type, timeoutMs) {
  await Promise.all(probes.map((probe) => waitFor(probe, type, timeoutMs)));
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
