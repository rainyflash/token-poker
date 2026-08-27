import assert from "node:assert/strict";
import test from "node:test";
import { runtimePipeName } from "../src/runtime-protocol.mjs";

test("共享运行时按插件版本和安装路径隔离", () => {
  const originalOverride = process.env.TOKEN_HOLDEM_RUNTIME_PIPE;
  delete process.env.TOKEN_HOLDEM_RUNTIME_PIPE;
  try {
    const current = runtimePipeName("C:/Codex/cache/token-holdem/0.4.18\0v0.4.18");
    const same = runtimePipeName("c:/codex/cache/token-holdem/0.4.18\0v0.4.18");
    const previous = runtimePipeName("C:/Codex/cache/token-holdem/0.4.17\0v0.4.17");

    assert.equal(current, same);
    assert.notEqual(current, previous);
    assert.match(current, /^\\\\\.\\pipe\\token-holdem-runtime-v6-[0-9a-f]{24}$/u);
  } finally {
    if (originalOverride === undefined) delete process.env.TOKEN_HOLDEM_RUNTIME_PIPE;
    else process.env.TOKEN_HOLDEM_RUNTIME_PIPE = originalOverride;
  }
});

test("共享运行时拒绝缺少版本隔离范围", () => {
  const originalOverride = process.env.TOKEN_HOLDEM_RUNTIME_PIPE;
  delete process.env.TOKEN_HOLDEM_RUNTIME_PIPE;
  try {
    assert.throws(() => runtimePipeName(""), /缺少版本隔离范围/u);
  } finally {
    if (originalOverride === undefined) delete process.env.TOKEN_HOLDEM_RUNTIME_PIPE;
    else process.env.TOKEN_HOLDEM_RUNTIME_PIPE = originalOverride;
  }
});
