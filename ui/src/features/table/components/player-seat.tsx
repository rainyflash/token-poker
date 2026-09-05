import { motion } from "motion/react";
import { useI18n } from "../../../core/i18n/use-i18n";
import { cn } from "../../../shared/lib/cn";
import type { TablePlayer } from "../model/table-state";
import { AvatarGlyph } from "./avatar-glyph";
import { PlayingCard } from "./playing-card";
import { SeatBadges } from "./seat-badges";
import { SeatStatus } from "./seat-status";
import { TurnTimer } from "./turn-timer";

export function PlayerSeat({ player }: { readonly player: TablePlayer }) {
  const { formatTokens } = useI18n();
  const committed = player.committed ?? 0;
  return (
    <div
      className={cn(
        "player-seat-anchor absolute z-20",
        `seat-${player.seat}`,
        player.status === "folded" && "is-folded",
      )}
    >
      <motion.div
        className="player-seat relative"
        data-acting={player.status === "thinking"}
        initial={{ opacity: 0, y: 8, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ type: "spring", stiffness: 320, damping: 27 }}
      >
        <div className="opponent-hole-cards absolute bottom-full left-1/2 flex -translate-x-1/2 -space-x-1 pb-1">
          <PlayingCard back compact className="-rotate-[4deg]" />
          <PlayingCard back compact className="rotate-[4deg]" />
        </div>
        <div className="player-seat-card relative flex items-center gap-2 rounded-full border border-[var(--line-strong)] bg-white py-1.5 pl-1.5 pr-3 shadow-[var(--codex-shadow-md)]">
          <AvatarGlyph tone={player.avatarTone} className="player-seat-avatar" />
          <div className="min-w-0">
            <p className="player-seat-name truncate text-[13px] font-semibold tracking-[-0.02em]" title={player.name}>{player.name}</p>
            <p className="mt-0.5 whitespace-nowrap text-xs tabular-nums text-[var(--muted)]">
              {formatTokens(player.stack)} Token
            </p>
          </div>
          {player.status === "thinking" ? (
            <span className="absolute -right-1 -top-1 size-2.5 rounded-full border-2 border-white bg-[#2d96e9]" />
          ) : null}
          <span className="ml-auto shrink-0">
            <SeatBadges dealer={player.dealer === true} blind={player.blind} />
          </span>
        </div>
        <div className="player-seat-status absolute left-1/2 top-full mt-1.5 flex -translate-x-1/2 flex-col items-center gap-1">
          <SeatStatus status={player.status} lastAction={player.lastAction} committed={committed} />
          {player.status === "thinking" ? (
            <TurnTimer
              deadlineUnixMs={player.turnDeadlineUnixMs}
              durationMs={player.actionTimeoutMs}
            />
          ) : null}
        </div>
      </motion.div>
    </div>
  );
}
