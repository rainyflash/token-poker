import { ShieldCheck } from "lucide-react";
import { motion } from "motion/react";
import type { HandSnapshot } from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import { AccountAvatar } from "../../../shared/ui/account-avatar";
import { StatusPill } from "../../../shared/ui/status-pill";
import { PlayerSeat } from "./player-seat";
import { PlayingCard } from "./playing-card";
import {
  type CardValue,
  type SeatPosition,
  type TablePlayer,
} from "../model/table-state";

interface PokerTableProps {
  readonly heroName: string;
  readonly heroAvatarUrl: string | null;
  readonly heroStack: number;
  readonly actionMessage: string | null;
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
  }));
}

export function PokerTable({
  heroName,
  heroAvatarUrl,
  heroStack,
  actionMessage,
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
  const proofLabel =
    hand.phase === "revealing"
      ? t("table.proofReveal")
      : hand.phase === "receipt_consensus"
        ? t("table.proofReceipt")
        : hand.phase === "between_hands" || hand.phase === "settled"
        ? t("table.proofHandArchived")
        : hand.phase === "interrupted"
          ? t("table.proofDisconnected")
        : t("table.mentalPokerVerified");

  return (
    <div className="poker-stage grid h-full w-full place-items-center">
      <div className="poker-geometry relative">
        <div className="poker-table absolute inset-0" aria-hidden="true">
          <div className="poker-table-felt absolute inset-[10px]" />
        </div>

        {players.map((player) => (
          <PlayerSeat key={player.id} player={player} />
        ))}

        <div className="pot-indicator absolute left-1/2 top-[38.5%] z-10 -translate-x-1/2 -translate-y-1/2">
          <div className="inline-flex items-baseline gap-2 rounded-full border border-black/[.075] bg-white/88 px-4 py-1.5 shadow-sm backdrop-blur-sm">
          <span className="text-[10px] text-[var(--muted-light)]">{t("table.pot")}</span>
          <strong className="text-[13px] font-semibold tabular-nums">
            {formatTokens(hand.pot)} Token
          </strong>
        </div>
        </div>

        <div className="community-cards absolute left-1/2 top-[54%] z-10 flex -translate-x-1/2 -translate-y-1/2 items-center justify-center gap-2">
          {communityCards.map((card, index) => (
            <PlayingCard
              key={`${card?.rank ?? "back"}-${String(index)}`}
              card={card}
              back={card === null}
              className={card === null ? "opacity-82" : ""}
            />
          ))}
        </div>

        <StatusPill
          icon={ShieldCheck}
          label={proofLabel}
          tone="success"
          className="proof-indicator absolute left-1/2 top-[69%] z-10 h-7 -translate-x-1/2 -translate-y-1/2 bg-white/78"
        />

        <div className="hero-seat-anchor absolute left-1/2 top-full z-30 -translate-x-1/2 -translate-y-1/2">
          <motion.div
            className="hero-seat relative"
            initial={{ y: 12, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            transition={{ type: "spring", stiffness: 300, damping: 26 }}
          >
            <div className="absolute bottom-full left-1/2 flex -translate-x-1/2 -space-x-1 pb-px">
              {heroCards.map((card, index) => (
                <PlayingCard
                  key={card === null ? `private-back-${String(index)}` : `${card.rank}-${card.suit}`}
                  card={card}
                  back={card === null}
                  className={index === 0 ? "-rotate-[3deg]" : "rotate-[3deg]"}
                />
              ))}
            </div>
            <div className="flex min-w-[184px] items-center gap-2 rounded-full border-2 border-[#249af1] bg-white py-1.5 pl-1.5 pr-4 shadow-[0_3px_7px_rgba(25,31,27,.06),0_14px_36px_rgba(36,130,197,.12)]">
              <AccountAvatar name={heroName} src={heroAvatarUrl} className="size-10" />
              <div className="min-w-0">
                <p className="truncate text-[12px] font-semibold">{heroName}</p>
                <p className="mt-0.5 text-[10px] tabular-nums text-[var(--muted-light)]">
                  {formatTokens(liveHeroStack)} Token
                </p>
              </div>
              {hand.localSeat !== null && hand.dealerSeat === hand.localSeat ? (
                <span className="ml-auto grid size-5 shrink-0 place-items-center rounded-full bg-[#202623] text-[8px] font-semibold text-white">
                  D
                </span>
              ) : null}
            </div>
            <div className="absolute left-1/2 top-full mt-2 -translate-x-1/2 whitespace-nowrap rounded-full border border-black/[.075] bg-white/90 px-3 py-1 text-[10px] text-[var(--muted)] shadow-sm">
              {actionMessage ?? t(hand.canAct ? "table.yourTurn" : "table.waitingPlayers")}
            </div>
          </motion.div>
        </div>
      </div>
    </div>
  );
}
