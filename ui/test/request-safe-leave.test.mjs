import assert from "node:assert/strict";
import test from "node:test";
import { requestSafeLeave } from "../src/features/session/model/request-safe-leave.ts";

test("安全离桌只有收到宿主确认后才进入已确认状态", async () => {
  const commands = [];
  const outcome = await requestSafeLeave(async (command) => {
    commands.push(command);
    return { ok: true };
  }, "fallback");

  assert.deepEqual(commands, [{ type: "leave_table" }]);
  assert.deepEqual(outcome, { status: "confirmed" });
});

test("安全离桌失败会保留明确错误并允许调用方重试", async () => {
  let attempts = 0;
  const sender = async () => {
    attempts += 1;
    return attempts === 1
      ? { ok: false, error: "table runtime rejected leave" }
      : { ok: true };
  };

  assert.deepEqual(await requestSafeLeave(sender, "fallback"), {
    status: "failed",
    error: "table runtime rejected leave",
  });
  assert.deepEqual(await requestSafeLeave(sender, "fallback"), { status: "confirmed" });
});

test("安全离桌把异常收敛为可展示的失败结果", async () => {
  const explicit = await requestSafeLeave(async () => {
    throw new Error("MCP proxy unavailable");
  }, "fallback");
  const fallback = await requestSafeLeave(async () => {
    throw "unknown failure";
  }, "fallback");

  assert.deepEqual(explicit, { status: "failed", error: "MCP proxy unavailable" });
  assert.deepEqual(fallback, { status: "failed", error: "fallback" });
});
