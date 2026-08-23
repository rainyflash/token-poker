export type Suit = "spade" | "heart" | "diamond" | "club";

export interface CardValue {
  readonly rank: string;
  readonly suit: Suit;
}

export type SeatPosition = "top" | "upper-left" | "lower-left" | "upper-right" | "lower-right";
export type BlindRole = "small" | "big";
export type TableAction = "fold" | "check" | "call" | "raise";

export interface TablePlayer {
  readonly id: string;
  readonly name: string;
  readonly stack: number;
  readonly seat: SeatPosition;
  readonly avatarTone: "ink" | "blue" | "violet" | "mint" | "coral";
  readonly status?: "thinking" | "acted" | "waiting" | "folded" | "all-in";
  readonly committed?: number;
  readonly dealer?: boolean;
  readonly blind?: BlindRole;
  readonly lastAction?: TableAction | null;
  readonly turnDeadlineUnixMs?: number | null;
  readonly actionTimeoutMs?: number;
}
