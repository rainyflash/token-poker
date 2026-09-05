import assert from "node:assert/strict";
import test from "node:test";
import { dispatchHostCommand } from "../src/command-router.mjs";
import { hostCommandSchema } from "../src/tool-contracts.mjs";

test("离桌命令必须经过运行时确认通道", async () => {
  const calls = [];
  const runtime = {
    leaveTable: async (command, requestId) => calls.push([command, requestId]),
    send: async () => assert.fail("离桌命令不得走即发即忘通道"),
  };

  const status = await dispatchHostCommand(runtime, { type: "leave_table" }, "request-a");

  assert.equal(status.status, "confirmed");
  assert.deepEqual(calls, [[{ type: "leave_table" }, "request-a"]]);
});

test("身份创建与恢复命令共用请求相关的确认通道", async () => {
  const calls = [];
  const runtime = {
    ensureIdentity: async (command, requestId) => calls.push([command, requestId]),
    send: async () => assert.fail("身份命令不得走即发即忘通道"),
  };
  const restore = {
    type: "restore_identity",
    recovery_envelope: "THR1-envelope",
    recovery_secret: "recovery-secret",
    device_label: "Windows 工作站",
  };

  const status = await dispatchHostCommand(runtime, restore, "request-restore");

  assert.equal(status.status, "confirmed");
  assert.deepEqual(calls, [[restore, "request-restore"]]);
});

test("普通控制命令仍使用接受语义", async () => {
  const calls = [];
  const runtime = {
    send: async (command) => calls.push(command),
  };

  const status = await dispatchHostCommand(runtime, { type: "cancel_public_pool" }, "request-b");

  assert.equal(status.status, "accepted");
  assert.deepEqual(calls, [{ type: "cancel_public_pool" }]);
});

test("远端恢复必须等待身份确认而不是入队成功", async () => {
  const identity = { player_id: "a", account_fingerprint: "account-a", recovery_secret_confirmed: true };
  const result = await dispatchHostCommand({ ensureIdentity: async () => identity,
    send: async () => assert.fail("不得绕过确认") }, { type: "restore_remote_identity" }, "request-a");
  assert.deepEqual(result, { status: "confirmed", identity_confirmation: identity });
});

test("下注等待内核确认且命令必须包含状态条件", async () => {
  const command = { type: "submit_action", action: "call", expected: {
    table_id: "table-a", hand_number: 1, sequence: 2, public_state_hash: "ab".repeat(32),
  } };
  assert.equal(hostCommandSchema.safeParse(command).success, true);
  assert.equal(hostCommandSchema.safeParse({ type: "submit_action", action: "call" }).success, false);
  const result = await dispatchHostCommand({ submitAction: async (received) => assert.deepEqual(received, command),
    send: async () => assert.fail("下注不得即发即忘") }, command, "request-action");
  assert.equal(result.status, "confirmed");
});

test("所有身份入口必须绑定账户且不得裁剪恢复密语", async () => {
  for (const type of ["ensure_identity", "create_identity", "restore_identity", "restore_remote_identity"]) {
    const command = { type, expected_account_fingerprint: "account-a",
      recovery_secret: "  recovery secret with spaces  ", device_label: "设备",
      ...(type === "restore_identity" ? { recovery_envelope: "THR1-envelope" } : {}) };
    assert.equal(hostCommandSchema.parse(command).recovery_secret, command.recovery_secret);
    const { expected_account_fingerprint, ...unbound } = command;
    assert.equal(hostCommandSchema.safeParse(unbound).success, false);
    const identity = { account_fingerprint: expected_account_fingerprint };
    const result = await dispatchHostCommand({ ensureIdentity: async (received, request) => {
      assert.deepEqual(received, command);
      assert.equal(request, "identity-request");
      return identity;
    }, send: async () => assert.fail("身份入口不得绕过确认") }, command, "identity-request");
    assert.deepEqual(result, { status: "confirmed", identity_confirmation: identity });
  }
});
