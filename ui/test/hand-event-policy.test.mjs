import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

const sourceUrl = new URL("../src/core/bridge/hand-event-policy.ts", import.meta.url);
const source = await readFile(sourceUrl, "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2023,
  },
}).outputText;
const policy = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);

function cursor(overrides = {}) {
  return Object.freeze({
    tableId: "table-a",
    handNumber: 7,
    phase: "key_exchange",
    progressCompleted: 1,
    sequence: 0,
    ...overrides,
  });
}

function scope(overrides = {}) {
  return Object.freeze({
    table_id: "table-a",
    hand_number: 7,
    ...overrides,
  });
}

test("协议阶段只允许向前推进", () => {
  assert.equal(
    policy.shouldAcceptHandProgress(
      cursor({ phase: "dealing", progressCompleted: 1 }),
      scope({ phase: "key_exchange", completed: 2 }),
    ),
    false,
  );
  assert.equal(
    policy.shouldAcceptHandProgress(
      cursor({ phase: "shuffling", progressCompleted: 2 }),
      scope({ phase: "shuffling", completed: 1 }),
    ),
    false,
  );
  assert.equal(
    policy.shouldAcceptHandProgress(
      cursor({ phase: "shuffling", progressCompleted: 2 }),
      scope({ phase: "dealing", completed: 0 }),
    ),
    true,
  );
});

test("下注态到达后迟到的发牌事件不能覆盖它", () => {
  assert.equal(
    policy.shouldAcceptHandProgress(
      cursor({ phase: "playing", progressCompleted: 2 }),
      scope({ phase: "dealing", completed: 2 }),
    ),
    false,
  );
  assert.equal(
    policy.shouldAcceptHandState(
      cursor({ phase: "playing", sequence: 4 }),
      scope({ sequence: 3 }),
    ),
    false,
  );
  assert.equal(
    policy.shouldAcceptHandState(
      cursor({ phase: "playing", sequence: 4 }),
      scope({ sequence: 4 }),
    ),
    true,
  );
});

test("旧手牌事件不能复活或重置当前手牌", () => {
  assert.equal(
    policy.shouldAcceptHandStart(cursor({ handNumber: 8 }), scope({ hand_number: 7 })),
    false,
  );
  assert.equal(
    policy.shouldAcceptHandReady(cursor({ handNumber: 8 }), scope({ hand_number: 7 })),
    false,
  );
  assert.equal(
    policy.shouldAcceptHandStart(cursor({ handNumber: 8 }), scope({ hand_number: 9 })),
    true,
  );
});

test("断线态吸收状态更新但不会被误标为可操作", () => {
  assert.equal(policy.phaseAfterHandState("interrupted", false), "interrupted");
  assert.equal(policy.phaseAfterHandState("dealing", false), "playing");
  assert.equal(policy.phaseAfterHandState("playing", true), "revealing");
});
