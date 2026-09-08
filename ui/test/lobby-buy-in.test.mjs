import assert from "node:assert/strict";
import test from "node:test";
import { STAKE_LEVELS } from "../src/core/domain/stake-levels.ts";
import { projectBuyIn } from "../src/features/lobby/model/buy-in.ts";

test("each stake range remains browseable without inventing an available balance", () => {
  for (const level of STAKE_LEVELS) {
    const state = projectBuyIn(level, level.minimumBuyIn, null);
    assert.equal(state.affordable, false);
    assert.equal(state.maximum, level.maximumBuyIn);
    assert.equal(state.value, level.minimumBuyIn);
    assert.equal(state.limitedByBalance, false);
  }
});

test("unknown, zero and insufficient balances never authorize seating", () => {
  for (const level of STAKE_LEVELS) {
    for (const balance of [null, 0, level.minimumBuyIn - 1]) {
      assert.equal(projectBuyIn(level, level.minimumBuyIn, balance).affordable, false);
    }
    assert.equal(projectBuyIn(level, level.minimumBuyIn, level.minimumBuyIn).affordable, true);
  }
});

test("buy-in and slider maximum share the same official balance cap", () => {
  for (const level of STAKE_LEVELS) {
    const balance = level.minimumBuyIn + level.bigBlind;
    const state = projectBuyIn(level, level.maximumBuyIn, balance);
    assert.equal(state.value, balance);
    assert.equal(state.maximum, balance);
    assert.equal(state.limitedByBalance, true);
    assert.equal(projectBuyIn(level, -1, balance).value, level.minimumBuyIn);
  }
});

test("very high balances never raise a table's maximum buy-in", () => {
  for (const level of STAKE_LEVELS) {
    const state = projectBuyIn(level, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER);
    assert.equal(state.value, level.maximumBuyIn);
    assert.equal(state.maximum, level.maximumBuyIn);
    assert.equal(state.limitedByBalance, false);
  }
});
