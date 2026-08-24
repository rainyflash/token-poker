import assert from "node:assert/strict";
import test from "node:test";
import { dispatchHostCommand } from "../src/command-router.mjs";

test("离桌命令必须经过运行时确认通道", async () => {
  const calls = [];
  const runtime = {
    leaveTable: async (command, requestId) => calls.push([command, requestId]),
    send: async () => assert.fail("离桌命令不得走即发即忘通道"),
  };

  const status = await dispatchHostCommand(runtime, { type: "leave_table" }, "request-a");

  assert.equal(status, "confirmed");
  assert.deepEqual(calls, [[{ type: "leave_table" }, "request-a"]]);
});

test("普通控制命令仍使用接受语义", async () => {
  const calls = [];
  const runtime = {
    send: async (command) => calls.push(command),
  };

  const status = await dispatchHostCommand(runtime, { type: "cancel_public_pool" }, "request-b");

  assert.equal(status, "accepted");
  assert.deepEqual(calls, [{ type: "cancel_public_pool" }]);
});
