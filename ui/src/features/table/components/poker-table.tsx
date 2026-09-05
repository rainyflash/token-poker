import { motion } from "motion/react";
import type { HandSnapshot } from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import { AccountAvatar } from "../../../shared/ui/account-avatar";
import { PlayerSeat } from "./player-seat";
import { PlayingCard } from "./playing-card";
import { SeatBadges } from "./seat-badges";
import { SeatStatus } from "./seat-status";
import { TurnTimer } from "./turn-timer";
import {
  type BlindRole,
  type CardValue,
  type SeatPosition,
  type TablePlayer,
} from "../model/table-state";

interface PokerTableProps {
  readonly heroName: string;
  readonly heroAvatarUrl: string | null;
  readonly heroStack: number;
  readonly hand: HandSnapshot;
}

const POSITION_LAYOUTS: Readonly<Record<number, readonly SeatPosition[]>> = {
  1: ["top"],
  2: ["upper-left", "upper-right"],
  3: ["top", "upper-left", "upper-right"],
  4: ["top", "upper-right", "lower-right", "upper-left"],
  5: ["top", "upper-right", "lower-right", "lower-left", "upper-left"],
};

const AVATAR_TONES = ["ink", "blue", "violet", "mint", "coral"] as const;
const FALLBACK_POSITIONS: readonly SeatPosition[] = [
  "top",
  "upper-right",
  "lower-right",
  "lower-left",
  "upper-left",
];

function rankLabel(rank: number): string {
  if (rank === 14) return "A";
  if (rank === 13) return "K";
  if (rank === 12) return "Q";
  if (rank === 11) return "J";
  return String(rank);
}

function cardValue(card: HandSnapshot["board"][number]): CardValue {
  return { rank: rankLabel(card.rank), suit: card.suit };
}

function blindForSeat(
  seat: number,
  dealerSeat: number | null,
  playerCount: number,
): BlindRole | undefined {
  if (dealerSeat === null || playerCount < 2) return undefined;
  const smallBlindSeat = playerCount === 2 ? dealerSeat : (dealerSeat % playerCount) + 1;
  const bigBlindSeat = (smallBlindSeat % playerCount) + 1;
  if (seat === smallBlindSeat) return "small";
  if (seat === bigBlindSeat) return "big";
  return undefined;
}

function livePlayers(
  hand: HandSnapshot,
  playerName: (seat: number, id: string) => string,
): readonly TablePlayer[] {
  const localSeat = hand.localSeat;
  if (hand.tableId === null || localSeat === null) return [];
  const seats =
    hand.seats.length >= 2
      ? hand.seats
      : hand.players.map((playerId, index) => ({
          seat: index + 1,
          playerId,
          stack: hand.buyIns[index] ?? 0,
          committed: 0,
          status: "active" as const,
          lastAction: null,
        }));
  const opponents = seats
    .filter((seat) => seat.seat !== localSeat)
    .sort(
      (left, right) =>
        ((left.seat - localSeat + seats.length) % seats.length) -
        ((right.seat - localSeat + seats.length) % seats.length),
    );
  const positions = POSITION_LAYOUTS[opponents.length] ?? FALLBACK_POSITIONS;
  return opponents.map((seat, index) => ({
    id: seat.playerId,
    name: playerName(seat.seat, seat.playerId.slice(0, 6)),
    stack: seat.stack,
    seat: positions[index] ?? "top",
    avatarTone: AVATAR_TONES[index % AVATAR_TONES.length] ?? "ink",
    status:
      seat.status === "folded"
        ? "folded"
        : seat.status === "all_in"
          ? "all-in"
          : hand.nextSeat === seat.seat
            ? "thinking"
            : "waiting",
    committed: seat.committed,
    dealer: hand.dealerSeat === seat.seat,
    blind: blindForSeat(seat.seat, hand.dealerSeat, seats.length),
    lastAction: seat.lastAction,
    turnDeadlineUnixMs:
      hand.nextSeat === seat.seat ? hand.turnDeadlineUnixMs : null,
    actionTimeoutMs: hand.actionTimeoutMs,
  }));
}

export function PokerTable({
  heroName,
  heroAvatarUrl,
  heroStack,
  hand,
}: PokerTableProps) {
  const { t, formatTokens } = useI18n();
  const players = livePlayers(hand, (seat, id) => t("table.opponentName", { seat, id }));
  const communityCards: readonly (CardValue | null)[] =
    Array.from({ length: 5 }, (_, index) => {
      const card = hand.board[index];
      return card === undefined ? null : cardValue(card);
    });
  const heroCards: readonly (CardValue | null)[] =
    hand.holeCards.length === 2 ? hand.holeCards.map(cardValue) : [null, null];
  const liveHeroStack =
    hand.localSeat === null
      ? heroStack
      : (hand.seats.find((seat) => seat.seat === hand.localSeat)?.stack ?? heroStack);
  const heroSeat =
    hand.localSeat === null
      ? undefined
      : hand.seats.find((seat) => seat.seat === hand.localSeat);
  const heroStatus: TablePlayer["status"] =
    heroSeat?.status === "folded"
      ? "folded"
      : heroSeat?.status === "all_in"
        ? "all-in"
        : hand.localSeat !== null && hand.nextSeat === hand.localSeat
          ? "thinking"
          : "waiting";
  const heroCommitted = heroSeat?.committed ?? 0;
  const heroBlind =
    hand.localSeat === null
      ? undefined
      : blindForSeat(hand.localSeat, hand.dealerSeat, hand.seats.length);
  return (
    <div className="poker-stage">
      <div className="poker-geometry relative">
        <div className="poker-table absolute inset-0" aria-hidden="true">
          <div className="poker-table-felt absolute inset-[10px]" />
        </div>

        {players.map((player) => (
          <PlayerSeat key={player.id} player={player} />
        ))}

        <div className="pot-indicator absolute left-1/2 z-10 -translate-x-1/2 -translate-y-1/2">
          <div className="inline-flex items-baseline gap-2 whitespace-nowrap rounded-full border border-[var(--line)] bg-white px-4 py-1.5 shadow-[var(--codex-shadow-sm)]">
            <span className="text-xs text-[var(--muted)]">{t("table.pot")}</span>
            <strong className="text-base font-semibold tabular-nums">
              {formatTokens(hand.pot)} Token
            </strong>
          </div>
        </div>

        <div className="community-cards absolute left-1/2 z-10 flex -translate-x-1/2 -translate-y-1/2 items-center justify-center gap-2">
          {communityCards.map((card, index) => (
            <PlayingCard
              key={`${card?.rank ?? "back"}-${String(index)}`}
              card={card}
              back={card === null}
              className={card === null ? "opacity-82" : ""}
            />
          ))}
        </div>

        <div className="hero-seat-anchor absolute left-1/2 top-full z-30 -translate-x-1/2 -translate-y-1/2">
          <motion.div
            className="hero-seat relative"
            data-acting={heroStatus === "thinking"}
            initial={{ y: 12, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            transition={{ type: "spring", stiffness: 300, damping: 26 }}
          >
            <div className="hero-hole-cards absolute bottom-full left-1/2 flex -translate-x-1/2 -space-x-1 pb-1">
              {heroCards.map((card, index) => (
                <PlayingCard
                  key={card === null ? `private-back-${String(index)}` : `${card.rank}-${card.suit}`}
                  card={card}
                  back={card === null}
                  className={index === 0 ? "-rotate-[3deg]" : "rotate-[3deg]"}
                />
              ))}
            </div>
            <div className="hero-seat-card flex items-center gap-2 rounded-full border-2 border-[var(--codex-blue-300)] bg-white py-1.5 pl-1.5 pr-3 shadow-[var(--codex-shadow-md)]">
              <AccountAvatar name={heroName} src={heroAvatarUrl} className="size-10" />
              <div className="min-w-0">
                <p className="hero-seat-name truncate text-[13px] font-semibold" title={heroName}>{heroName}</p>
                <p className="mt-0.5 whitespace-nowrap text-xs tabular-nums text-[var(--muted)]">
                  {formatTokens(liveHeroStack)} Token
                </p>
              </div>
              <span className="ml-auto shrink-0">
                <SeatBadges
                  dealer={hand.localSeat !== null && hand.dealerSeat === hand.localSeat}
                  blind={heroBlind}
                />
              </span>
            </div>
            <div className="hero-seat-status">
              <SeatStatus
                status={heroStatus}
                lastAction={heroSeat?.lastAction}
                committed={heroCommitted}
              />
            </div>
            {heroStatus === "thinking" ? (
              <TurnTimer
                className="hero-seat-timer"
                deadlineUnixMs={hand.turnDeadlineUnixMs}
                durationMs={hand.actionTimeoutMs}
              />
            ) : null}
          </motion.div>
        </div>
      </div>
    </div>
  );
}
