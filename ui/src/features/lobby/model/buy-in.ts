import type { StakeLevel } from "../../../core/domain/stake-levels";

/** A browseable stake range is not an authorization to enter a table. */
export function projectBuyIn(level: StakeLevel, requested: number, availableTokens: number | null) {
  const affordable = availableTokens !== null && availableTokens >= level.minimumBuyIn;
  const maximum = affordable ? Math.min(level.maximumBuyIn, availableTokens) : level.maximumBuyIn;
  return {
    affordable,
    maximum,
    value: Math.min(maximum, Math.max(level.minimumBuyIn, requested)),
    limitedByBalance: affordable && maximum < level.maximumBuyIn,
  };
}
