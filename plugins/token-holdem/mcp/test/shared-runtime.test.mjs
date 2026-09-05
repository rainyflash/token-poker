import assert from "node:assert/strict";
import { randomBytes, randomUUID } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { SidecarRuntime } from "../src/sidecar-runtime.mjs";

const pluginRoot = resolve(import.meta.dirname, "..", "..");

test("两个对话客户端重新附着同一运行时并恢复 Token 快照", async () => {
  const isolatedLocalAppData = await mkdtemp(join(tmpdir(), "token-holdem-runtime-test-"));
  const environment = snapshotEnvironment([
    "LOCALAPPDATA",
    "TOKEN_HOLDEM_RUNTIME_PATH",
    "TOKEN_HOLDEM_SIDECAR_PATH",
    "TOKEN_HOLDEM_RUNTIME_PIPE",
    "TOKEN_HOLDEM_RUNTIME_IDLE_TIMEOUT_SECONDS",
  ]);
  process.env.LOCALAPPDATA = isolatedLocalAppData;
  process.env.TOKEN_HOLDEM_RUNTIME_PATH = join(pluginRoot, "bin", "token-holdem-runtime.exe");
  process.env.TOKEN_HOLDEM_SIDECAR_PATH = join(pluginRoot, "bin", "token-holdem-sidecar.exe");
  process.env.TOKEN_HOLDEM_RUNTIME_PIPE = String.raw`\\.\pipe\token-holdem-runtime-v7-${randomBytes(12).toString("hex")}`;
  process.env.TOKEN_HOLDEM_RUNTIME_IDLE_TIMEOUT_SECONDS = "30";

  let first = null;
  let second = null;
  try {
    first = new SidecarRuntime(pluginRoot, "test-build");
    await first.ensureStarted();
    const firstReady = await waitForEvent(first, "ready", 15_000);
    const firstRuntimeId = first.readEvents(0).runtime_id;
    assert.equal(typeof firstRuntimeId, "string");

    const beforeToken = first.latestSequence;
    await first.send({
      type: "token_snapshot",
      lifetime_tokens: 35_500_000_000,
      username: "@test-player",
      display_name: "幻光",
      observed_at_unix_ms: Date.now(),
    });
    await waitForEvent(first, "token_snapshot_accepted", 10_000, beforeToken);
    const identity = await first.ensureIdentity(
      {
        type: "ensure_identity",
        expected_account_fingerprint: first.accountBinding.account_fingerprint,
        recovery_secret: "跨对话共享运行时测试口令-abcdefghijkl",
        device_label: "测试设备",
      },
      randomUUID(),
    );
    assert.equal(first.currentState.identity?.player_id, identity.player_id);
    assert.equal(identity.recovery_secret_confirmed, true);
    const [retry, reused] = await Promise.all([
      first.ensureIdentity({ type: "ensure_identity", expected_account_fingerprint: identity.account_fingerprint,
        recovery_secret: "跨对话共享运行时测试口令-abcdefghijkl", device_label: "重试设备" }, randomUUID()),
      first.ensureIdentity({ type: "ensure_identity", expected_account_fingerprint: identity.account_fingerprint,
        recovery_secret: "another-task-unrelated-secret", device_label: "另一任务" }, randomUUID()),
    ]);
    assert.equal(retry.recovery_secret_confirmed, true);
    assert.equal(reused.recovery_secret_confirmed, false);
    assert.equal(reused.player_id, identity.player_id);
    assert.equal(reused.recovery_envelope, identity.recovery_envelope);
    await assert.rejects(first.ensureIdentity({ type: "restore_identity",
      expected_account_fingerprint: identity.account_fingerprint, recovery_envelope: reused.recovery_envelope,
      recovery_secret: "another-task-unrelated-secret", device_label: "错误密语恢复" }, randomUUID()), /解密/u);
    assert.equal(first.currentState.identity.player_id, identity.player_id);
    const restored = await first.ensureIdentity({ type: "restore_identity",
      expected_account_fingerprint: identity.account_fingerprint, recovery_envelope: identity.recovery_envelope,
      recovery_secret: "跨对话共享运行时测试口令-abcdefghijkl", device_label: "恢复设备" }, randomUUID());
    assert.equal(restored.recovery_secret_confirmed, true);
    assert.equal(restored.player_id, identity.player_id);
    await assert.rejects(
      first.leaveTable({ type: "leave_table" }, randomUUID()),
      /当前既没有公开匹配，也没有可离开的牌桌/u,
    );
    await first.close();
    first = null;

    second = new SidecarRuntime(pluginRoot, "test-build");
    await second.ensureStarted();
    const replayedReady = findEvent(second, "ready");
    const replayedToken = findEvent(second, "token_snapshot_accepted");
    assert.equal(second.readEvents(0).runtime_id, firstRuntimeId);
    assert.equal(replayedReady?.peer_id, firstReady.peer_id);
    assert.equal(replayedToken?.lifetime_tokens, 35_500_000_000);
    assert.equal(replayedToken?.username, "@test-player");
    assert.equal(replayedToken?.display_name, "幻光");
    assert.equal(second.tokenSnapshot?.lifetime_tokens, 35_500_000_000);
    assert.equal(second.tokenSnapshot?.username, "@test-player");
    assert.equal(second.tokenSnapshot?.display_name, "幻光");
    assert.equal(typeof second.accountBinding?.account_fingerprint, "string");
    assert.equal(second.accountBinding?.peer_verifiable, false);
    assert.equal(second.currentState.identity?.player_id, identity.player_id);

    for (let index = 0; index < 12; index += 1) {
      const cursor = second.latestSequence;
      await second.send({
        type: "token_snapshot",
        lifetime_tokens: 35_500_000_000 + index,
        username: "@test-player",
        display_name: "幻光",
        observed_at_unix_ms: Date.now() + index,
      });
      await waitForEvent(second, "token_snapshot_accepted", 10_000, cursor);
    }
    const beforeRestart = second.latestSequence;
    await second.setVolunteerConsent(false);
    await waitForEvent(second, "ready", 15_000, beforeRestart);
    assert.ok(second.latestSequence > beforeRestart, "运行时代次变化后 MCP 事件游标必须继续单调递增");
  } finally {
    await first?.close().catch(() => undefined);
    await second?.terminateRuntimeForTesting().catch(() => undefined);
    restoreEnvironment(environment);
    await rm(isolatedLocalAppData, { recursive: true, force: true });
  }
});

async function waitForEvent(runtime, type, timeoutMs, afterSequence = 0) {
  let cursor = afterSequence;
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const batch = await runtime.waitForEvents(cursor, Math.min(1_000, deadline - Date.now()));
    const match = batch.events.find((entry) => entry.event?.type === type);
    if (match !== undefined) return match.event;
    cursor = batch.latest_sequence;
  }
  throw new Error(`等待运行时事件超时：${type}`);
}

function findEvent(runtime, type) {
  return runtime.readEvents(0).events.findLast((entry) => entry.event?.type === type)?.event;
}

function snapshotEnvironment(names) {
  return new Map(names.map((name) => [name, process.env[name]]));
}

function restoreEnvironment(snapshot) {
  for (const [name, value] of snapshot) {
    if (value === undefined) delete process.env[name];
    else process.env[name] = value;
  }
}
