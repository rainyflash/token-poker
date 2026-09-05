import assert from "node:assert/strict";
import test from "node:test";
import { clampRaise, presetRaise, selectedBetPreset } from "../src/features/table/model/bet-sizing.ts";

test("预设高亮由当前金额决定，滑条离开预设后不残留选中", () => {
  assert.equal(selectedBetPreset(7_500_000, 10_000_000, 2_000_000, 50_000_000), 75);
  assert.equal(selectedBetPreset(3_300_000, 10_000_000, 2_000_000, 50_000_000), 33);
  assert.equal(selectedBetPreset(7_600_000, 10_000_000, 2_000_000, 50_000_000), null);
});

test("多个预设因最小加注或全下上限重合时不显示虚假选中", () => {
  assert.equal(selectedBetPreset(4_000_000, 3_000_000, 4_000_000, 80_000_000), null);
  assert.equal(selectedBetPreset(2_000_000, 10_000_000, 4_000_000, 2_000_000), null);
});

test("下注预设和滑条金额都服从同一上下限，支持短筹码全下", () => {
  assert.equal(presetRaise(10_000_000, 25, 4_000_000, 80_000_000), 4_000_000);
  assert.equal(presetRaise(10_000_000, 133, 2_000_000, 8_000_000), 8_000_000);
  assert.equal(clampRaise(6_000_000, 4_000_000, 3_000_000), 3_000_000);
  assert.equal(clampRaise(0, 4_000_000, 80_000_000), 4_000_000);
});
