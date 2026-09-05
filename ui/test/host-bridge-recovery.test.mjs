import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { registerHooks } from "node:module";
import { fileURLToPath } from "node:url";
import test from "node:test";
import ts from "typescript";

const sourceRoot = new URL("../src/", import.meta.url).href;
const reactMock = "data:text/javascript," + encodeURIComponent(
  "export function useSyncExternalStore(subscribe, get) { globalThis.reviewSubscribe = subscribe; return get(); }",
);
registerHooks({
  resolve(specifier, context, next) {
    if (context.parentURL?.startsWith(sourceRoot) && specifier === "react") {
      return { url: reactMock, shortCircuit: true };
    }
    if (context.parentURL?.startsWith(sourceRoot) && specifier.startsWith(".")) {
      const url = new URL(specifier, context.parentURL);
      if (!existsSync(url) && existsSync(new URL(`${url.href}.ts`))) url.pathname += ".ts";
      return { url: url.href, shortCircuit: true };
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (url.startsWith(sourceRoot) && url.endsWith(".ts")) {
      const source = readFileSync(fileURLToPath(url), "utf8").replaceAll("import.meta.env.DEV", "false");
      return { format: "module", shortCircuit: true, source: ts.transpileModule(source, {
        compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2023 },
      }).outputText };
    }
    if (url === new URL("../package.json", import.meta.url).href) {
      return { format: "module", shortCircuit: true, source: `export default ${readFileSync(fileURLToPath(url), "utf8")}` };
    }
    return next(url, context);
  },
});
const target = new EventTarget();
globalThis.addEventListener = target.addEventListener.bind(target);
globalThis.dispatchEvent = target.dispatchEvent.bind(target);
globalThis.__tokenHoldemBridgeInstalled = true;

function room(table) { return { type: "room_entered", table_id: table, level_id: "1m-2m" }; }
function started(table, hand = 1) {
  return { type: "hand_protocol_started", table_id: table, hand_number: hand, seat: 1,
    dealer_seat: 1, players: ["a", "b"], level_id: "1m-2m", small_blind: 1, big_blind: 2, buy_ins: [100, 100] };
}
function projection(stream, sequence, events) {
  return { stream_id: stream, identity: null, latest_sequence: sequence,
    events: events.map((event, index) => ({ sequence: index + 1, event })) };
}
globalThis.__tokenHoldemBufferedSidecarEvents = [room("stale"), started("stale")];
globalThis.__tokenHoldemCurrentState = projection("boot", 5, []);
const { useHostBridge } = await import("../src/core/bridge/host-bridge.ts");
const snapshot = () => useHostBridge()[0];
const publish = (detail) => target.dispatchEvent(new CustomEvent("token-holdem:current-state", { detail }));

test("挂载时完整空快照优先于历史缓冲，旧房间不会复活", () => {
  assert.equal(snapshot().room.tableId, null);
  assert.equal(snapshot().hand.tableId, null);
});

test("内核清空后，新房间和新手牌原子替换旧会话", () => {
  publish(projection("boot", 10, [room("old"), started("old", 8)]));
  assert.equal(snapshot().hand.handNumber, 8);
  const changes = [];
  const unsubscribe = globalThis.reviewSubscribe(() => changes.push(snapshot()));
  publish(projection("boot", 11, []));
  assert.equal(snapshot().room.tableId, null);
  assert.equal(snapshot().hand.phase, "idle");
  publish(projection("boot", 12, [room("new"), started("new")]));
  unsubscribe();
  assert.equal(changes.length, 2);
  assert.equal(changes[1].room.tableId, "new");
  assert.equal(changes[1].hand.tableId, "new");
  assert.equal(changes[1].hand.handNumber, 1);
});

test("新 MCP 事件流可以重置序号，但旧流迟到回复不能覆盖新流", () => {
  publish(projection("replacement", 2, [room("current"), started("current")]));
  publish(projection("boot", 999, [room("stale"), started("stale")]));
  assert.equal(snapshot().room.tableId, "current");
  publish(projection("replacement", 1, []));
  assert.equal(snapshot().hand.tableId, "current");
});

test("相同水位的完整快照也会纠正较旧的原始回放", () => {
  target.dispatchEvent(new CustomEvent("token-holdem:sidecar", { detail: room("stale") }));
  publish(projection("replacement", 2, [room("current"), started("current")]));
  assert.equal(snapshot().hand.tableId, "current");
});

test("完整快照恢复动作状态哈希，清除身份时同步清除战绩", () => {
  const state = { type: "hand_state", table_id: "current", hand_number: 1, sequence: 2,
    street: "preflop", pot: 3, current_bet: 2, next_seat: 1, local_seat: 1, to_call: 1,
    minimum_raise_to: 4, maximum_raise_to: 100, can_act: true, awaiting_reveal: false,
    action_timeout_ms: 30000, turn_deadline_unix_ms: null, board: [], seats: [],
    public_state_hash: "ab".repeat(32), transcript_hash: "trace" };
  publish(projection("replacement", 3, [room("current"), started("current"), state]));
  assert.equal(snapshot().hand.publicStateHash, state.public_state_hash);
  assert.equal(snapshot().hand.canAct, true);
  publish(projection("replacement", 4, []));
  assert.equal(snapshot().identity, null);
  assert.equal(snapshot().hand.canAct, false);
  assert.equal(snapshot().statistics.completedHands, 0);
});
