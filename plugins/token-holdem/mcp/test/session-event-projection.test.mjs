import assert from "node:assert/strict";
import test from "node:test";
import { SessionEventProjection } from "../src/session-event-projection.mjs";

function entry(sequence, type, fields = {}) {
  return Object.freeze({ sequence, event: Object.freeze({ type, ...fields }) });
}

test("事件缓存截断后仍回放当前房间和手牌投影", () => {
  const projection = new SessionEventProjection();
  const current = [
    entry(1, "identity_ready", { player_id: "player-a" }),
    entry(2, "pool_joined", { level_id: "1m-2m", buy_in: 80_000_000 }),
    entry(8, "room_entered", { table_id: "table-a", level_id: "1m-2m" }),
    entry(11, "room_snapshot", { table_id: "table-a", local_role: "playing" }),
    entry(15, "hand_protocol_started", { table_id: "table-a", hand_number: 9 }),
    entry(19, "hand_ready", { table_id: "table-a", hand_number: 9 }),
    entry(5_100, "hand_state", { table_id: "table-a", hand_number: 9, sequence: 31 }),
  ];
  for (const event of current) projection.observe(event);

  const replay = projection.merge(
    [entry(5_099, "warning", { message: "retained" }), current.at(-1)],
    0,
  );

  assert.deepEqual(
    replay.map((event) => event.event.type),
    [
      "identity_ready",
      "pool_joined",
      "room_entered",
      "room_snapshot",
      "hand_protocol_started",
      "hand_ready",
      "warning",
      "hand_state",
    ],
  );
});

test("身份事件超过普通缓存上限后仍存在于当前状态投影", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(1, "identity_ready", { player_id: "player-a" }));

  assert.deepEqual(
    projection.merge([], 0).map((event) => event.event.type),
    ["identity_ready"],
  );
});

test("当前状态快照按序包含牌桌与可操作手牌", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(2, "room_entered", { table_id: "table-a" }));
  projection.observe(entry(4, "room_snapshot", {
    table_id: "table-a",
    seats: [{ player_id: "player-a" }, { player_id: "player-b" }],
  }));
  projection.observe(entry(6, "hand_protocol_started", {
    table_id: "table-a",
    hand_number: 4,
  }));
  projection.observe(entry(8, "hand_state", {
    table_id: "table-a",
    hand_number: 4,
    sequence: 0,
  }));

  assert.deepEqual(
    projection.snapshot().map((event) => [event.sequence, event.event.type]),
    [
      [2, "room_entered"],
      [4, "room_snapshot"],
      [6, "hand_protocol_started"],
      [8, "hand_state"],
    ],
  );
});

test("离桌完成后不会复活旧房间投影", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(1, "room_entered", { table_id: "table-a" }));
  projection.observe(entry(2, "hand_protocol_started", { table_id: "table-a" }));
  projection.observe(entry(3, "room_closed", { table_id: "table-a" }));

  assert.deepEqual(projection.merge([], 0), []);
});

test("加入超时关闭临时房间时保留公开匹配投影", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(1, "pool_joined", { level_id: "1m-2m" }));
  projection.observe(entry(2, "room_entered", { table_id: "stale-table" }));
  projection.observe(entry(3, "room_closed", { table_id: "stale-table" }));
  projection.observe(entry(4, "pool_join_attempt_expired", { table_id: "stale-table" }));

  assert.deepEqual(
    projection.merge([], 0).map((event) => event.event.type),
    ["pool_joined", "pool_join_attempt_expired"],
  );
});

test("新一手开始会淘汰上一手私有与终态事件", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(1, "hand_protocol_started", { hand_number: 1 }));
  projection.observe(entry(2, "hand_ready", { hand_number: 1 }));
  projection.observe(entry(3, "receipt_finalized", { hand_number: 1 }));
  projection.observe(entry(4, "hand_protocol_started", { hand_number: 2 }));

  assert.deepEqual(
    projection.merge([], 0).map((event) => [event.event.type, event.event.hand_number]),
    [["hand_protocol_started", 2]],
  );
});

test("签名离桌导致手牌作废时只清理手牌并保留房间", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(1, "room_entered", { table_id: "table-a" }));
  projection.observe(entry(2, "room_snapshot", { table_id: "table-a", local_role: "seated" }));
  projection.observe(entry(3, "hand_protocol_started", { table_id: "table-a", hand_number: 7 }));
  projection.observe(entry(4, "hand_ready", { table_id: "table-a", hand_number: 7 }));
  projection.observe(entry(5, "hand_aborted_for_leave", { table_id: "table-a", hand_number: 7 }));

  assert.deepEqual(
    projection.merge([], 0).map((event) => event.event.type),
    ["room_entered", "room_snapshot"],
  );
});

test("强制安全离桌完成后清空匹配房间与手牌投影", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(1, "pool_joined"));
  projection.observe(entry(2, "room_entered"));
  projection.observe(entry(3, "hand_protocol_started"));
  projection.observe(entry(4, "safe_leave_requested"));
  projection.observe(entry(5, "safe_leave_forced"));
  projection.observe(entry(6, "safe_leave_completed"));

  assert.deepEqual(projection.merge([], 0), []);
});

test("下注态建立后迟到的旧协议阶段不会进入恢复投影", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(1, "hand_protocol_started", {
    table_id: "table-a",
    hand_number: 3,
  }));
  projection.observe(entry(2, "hand_protocol_progress", {
    table_id: "table-a",
    hand_number: 3,
    phase: "dealing",
    completed: 1,
  }));
  projection.observe(entry(3, "hand_ready", {
    table_id: "table-a",
    hand_number: 3,
  }));
  projection.observe(entry(4, "hand_state", {
    table_id: "table-a",
    hand_number: 3,
    sequence: 0,
  }));
  projection.observe(entry(5, "hand_protocol_progress", {
    table_id: "table-a",
    hand_number: 3,
    phase: "key_exchange",
    completed: 2,
  }));

  assert.deepEqual(
    projection.merge([], 0).map((event) => event.event.type),
    ["hand_protocol_started", "hand_ready", "hand_state"],
  );
});

test("上一手的迟到事件不会清空当前手牌投影", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(1, "hand_protocol_started", {
    table_id: "table-a",
    hand_number: 7,
  }));
  projection.observe(entry(2, "hand_state", {
    table_id: "table-a",
    hand_number: 7,
    sequence: 4,
  }));
  projection.observe(entry(3, "hand_protocol_started", {
    table_id: "table-a",
    hand_number: 6,
  }));

  assert.deepEqual(
    projection.merge([], 0).map((event) => [event.event.type, event.event.hand_number]),
    [
      ["hand_protocol_started", 7],
      ["hand_state", 7],
    ],
  );
});
test("身份切换会移除旧身份与旧战绩投影", () => {
  const projection = new SessionEventProjection();
  projection.observe(entry(1, "identity_ready", { player_id: "a" }));
  projection.observe(entry(2, "statistics_updated", { completed_hands: 99 }));
  projection.observe(entry(3, "identity_cleared"));
  assert.deepEqual(projection.snapshot(), []);
  projection.observe(entry(4, "identity_ready", { player_id: "b" }));
  projection.observe(entry(5, "statistics_updated", { completed_hands: 3 }));
  projection.observe(entry(6, "identity_ready", { player_id: "c" }));
  assert.deepEqual(projection.snapshot().map((item) => item.event.type), ["identity_ready"]);
});
