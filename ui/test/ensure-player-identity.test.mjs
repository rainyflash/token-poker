import assert from "node:assert/strict";
import test from "node:test";
import { ensurePlayerIdentity } from "../src/features/identity/model/ensure-player-identity.ts";

const COMMAND = Object.freeze({
  type: "ensure_identity",
  expected_account_fingerprint: "account-a",
  recovery_secret: "fixed-recovery-secret",
  device_label: "测试设备",
});

test("身份初始化在瞬时异常和失败回执后使用同一密语重试", async () => {
  const attempts = [];
  const delays = [];
  const responses = [
    new Error("MCP proxy unavailable"),
    { ok: false, error: "temporary sidecar failure" },
    { ok: true, identity: { playerId: "player-a", accountFingerprint: "account-a", recoverySecretConfirmed: true, recoveryEnvelope: "THR1-test" } },
  ];

  const outcome = await ensurePlayerIdentity(
    COMMAND,
    async (command) => {
      attempts.push(command);
      const response = responses.shift();
      if (response instanceof Error) throw response;
      return response;
    },
    {
      delaysMs: [0, 10, 20],
      wait: async (delayMs) => {
        delays.push(delayMs);
        return true;
      },
    },
  );

  assert.equal(outcome.status, "confirmed");
  assert.equal(outcome.identity.recoverySecretConfirmed, true);
  assert.deepEqual(delays, [0, 10, 20]);
  assert.equal(attempts.length, 3);
  assert.ok(attempts.every((command) => command.recovery_secret === COMMAND.recovery_secret));
});

test("身份初始化耗尽有限次数后返回最后一个明确错误", async () => {
  let attempts = 0;
  const outcome = await ensurePlayerIdentity(
    COMMAND,
    async () => {
      attempts += 1;
      return { ok: false, error: `failure-${attempts}` };
    },
    { delaysMs: [0, 1, 2], wait: async () => true },
  );

  assert.equal(attempts, 3);
  assert.deepEqual(outcome, { status: "failed", error: "failure-3" });
});

test("复用其他任务的身份不能声称当前密语已验证", async () => {
  const identity = { playerId: "existing-player", accountFingerprint: "account-a", recoverySecretConfirmed: false, recoveryEnvelope: "THR1-existing" };
  const outcome = await ensurePlayerIdentity(COMMAND, async () => ({ ok: true, identity }));
  assert.equal(outcome.status, "confirmed");
  assert.equal(outcome.identity.recoverySecretConfirmed, false);
});

test("缺少身份确认或确认来自另一个账户时拒绝恢复包导出", async () => {
  for (const result of [{ ok: true }, { ok: true, identity: { accountFingerprint: "other" } }]) {
    assert.equal((await ensurePlayerIdentity(COMMAND, async () => result)).status, "failed");
  }
});

test("身份初始化在界面卸载后立即取消且不再发送命令", async () => {
  const controller = new AbortController();
  controller.abort();
  let attempts = 0;

  const outcome = await ensurePlayerIdentity(
    COMMAND,
    async () => {
      attempts += 1;
      return { ok: true };
    },
    { signal: controller.signal },
  );

  assert.equal(attempts, 0);
  assert.deepEqual(outcome, { status: "cancelled" });
});

test("延迟结束同一微任务内被取消时也不得发送废弃密语", async () => {
  const controller = new AbortController();
  const outcome = await ensurePlayerIdentity(COMMAND, async () => assert.fail("废弃的初始化不应到达内核"), {
    signal: controller.signal, wait: async () => { controller.abort(); return true; },
  });
  assert.equal(outcome.status, "cancelled");
});
