export const BET_PRESETS = [25, 33, 75, 133] as const;
export const BET_STEP = 10_000;

export function clampRaise(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(Math.min(minimum, maximum), value));
}

export function presetRaise(pot: number, percentage: number, minimum: number, maximum: number): number {
  const rounded = Math.round((Math.max(pot, BET_STEP) * percentage) / 100 / BET_STEP) * BET_STEP;
  return clampRaise(rounded, minimum, maximum);
}

export function selectedBetPreset(amount: number, pot: number, minimum: number, maximum: number): number | null {
  const matches = BET_PRESETS.filter((preset) => presetRaise(pot, preset, minimum, maximum) === amount);
  return matches.length === 1 ? matches[0] ?? null : null;
}
