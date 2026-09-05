import assert from "node:assert/strict";
import test from "node:test";
import { createHandActionCommand, handActionScope } from "../src/features/table/model/action-command.ts";

const hand = { tableId: "table-a", handNumber: 4, sequence: 7, publicStateHash: "ab".repeat(32),
  canAct: true, sessionInterrupted: false };

test("下注命令绑定提交页面的房间、手牌、序号和状态哈希", () => {
  const command = createHandActionCommand(hand, "raise", 200);
  assert.deepEqual(command.expected, { table_id: "table-a", hand_number: 4, sequence: 7, public_state_hash: hand.publicStateHash });
  assert.equal(command.amount, 200);
  assert.equal(createHandActionCommand(hand, "call", 200).amount, undefined);
  assert.notEqual(handActionScope(hand), handActionScope({ ...hand, handNumber: 5 }));
  assert.notEqual(handActionScope(hand), handActionScope({ ...hand, publicStateHash: "cd".repeat(32) }));
});

test("没有可验证最新状态时禁止提交下注", () => {
  for (const change of [{ canAct: false }, { sessionInterrupted: true }, { publicStateHash: null }, { tableId: null }]) {
    assert.equal(createHandActionCommand({ ...hand, ...change }, "call", 0), null);
  }
});
