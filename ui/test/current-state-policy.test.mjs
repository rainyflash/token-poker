import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

const sourceUrl = new URL("../src/core/bridge/current-state-policy.ts", import.meta.url);
const source = await readFile(sourceUrl, "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2023,
  },
}).outputText;
const policy = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);

function parseEvent(value) {
  return typeof value === "object" && value !== null && typeof value.type === "string"
    ? value
    : null;
}

test("当前状态投影按事件序号恢复牌桌与手牌", () => {
  const projection = policy.parseCurrentStateProjection(
    {
      latest_sequence: 84,
      events: [
        { sequence: 84, event: { type: "hand_state", hand_number: 7 } },
        { sequence: 61, event: { type: "room_snapshot", seats: ["a", "b"] } },
        { sequence: 73, event: { type: "hand_protocol_started", hand_number: 7 } },
      ],
    },
    parseEvent,
  );

  assert.equal(projection.latestSequence, 84);
  assert.deepEqual(
    projection.events.map((event) => event.type),
    ["room_snapshot", "hand_protocol_started", "hand_state"],
  );
});

test("损坏或越过水位的当前状态投影会被整体拒绝", () => {
  assert.equal(
    policy.parseCurrentStateProjection(
      {
        latest_sequence: 10,
        events: [{ sequence: 11, event: { type: "hand_state" } }],
      },
      parseEvent,
    ),
    null,
  );
  assert.equal(
    policy.parseCurrentStateProjection(
      {
        latest_sequence: 10,
        events: [{ sequence: 9, event: null }],
      },
      parseEvent,
    ),
    null,
  );
});
