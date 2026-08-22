import type { MessageKey } from "../i18n/messages";

export interface StakeLevel {
  readonly id: string;
  readonly nameKey: MessageKey;
  readonly smallBlind: number;
  readonly bigBlind: number;
  readonly minimumBuyIn: number;
  readonly maximumBuyIn: number;
  readonly noteKey: MessageKey;
}

export const DEFAULT_STAKE_LEVEL_ID = "1m-2m";

export const STAKE_LEVELS: readonly StakeLevel[] = [
  {
    id: "100k-200k",
    nameKey: "stake.light.name",
    smallBlind: 100_000,
    bigBlind: 200_000,
    minimumBuyIn: 8_000_000,
    maximumBuyIn: 20_000_000,
    noteKey: "stake.light.note",
  },
  {
    id: "1m-2m",
    nameKey: "stake.standard.name",
    smallBlind: 1_000_000,
    bigBlind: 2_000_000,
    minimumBuyIn: 80_000_000,
    maximumBuyIn: 200_000_000,
    noteKey: "stake.standard.note",
  },
  {
    id: "10m-20m",
    nameKey: "stake.deep.name",
    smallBlind: 10_000_000,
    bigBlind: 20_000_000,
    minimumBuyIn: 800_000_000,
    maximumBuyIn: 2_000_000_000,
    noteKey: "stake.deep.note",
  },
  {
    id: "100m-200m",
    nameKey: "stake.extreme.name",
    smallBlind: 100_000_000,
    bigBlind: 200_000_000,
    minimumBuyIn: 8_000_000_000,
    maximumBuyIn: 20_000_000_000,
    noteKey: "stake.extreme.note",
  },
] as const;

export function findStakeLevel(id: string): StakeLevel {
  const matched = STAKE_LEVELS.find((level) => level.id === id);
  const defaultLevel = STAKE_LEVELS.find((level) => level.id === DEFAULT_STAKE_LEVEL_ID);
  if (matched !== undefined) return matched;
  if (defaultLevel !== undefined) return defaultLevel;
  throw new Error("Stake-level catalog cannot be empty");
}
