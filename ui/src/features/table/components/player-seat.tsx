import { motion } from "motion/react";
import { useI18n } from "../../../core/i18n/use-i18n";
import { cn } from "../../../shared/lib/cn";
import type { TablePlayer } from "../model/table-state";
import { AvatarGlyph } from "./avatar-glyph";
import { PlayingCard } from "./playing-card";

export function PlayerSeat({ player }: { readonly player: TablePlayer }) {
  const { t, formatTokens } = useI18n();
  return (
    <div
      className={cn(
        "player-seat-anchor absolute z-20",
        `seat-${player.seat}`,
        player.status === "folded" && "opacity-45 grayscale",
      )}
    >
      <motion.div
        className="player-seat relative"
        initial={{ opacity: 0, y: 8, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ type: "spring", stiffness: 320, damping: 27 }}
      >
        <div className="absolute bottom-full left-1/2 flex -translate-x-1/2 -space-x-1 pb-px opacity-80">
          <PlayingCard back compact className="-rotate-[4deg]" />
          <PlayingCard back compact className="rotate-[4deg]" />
        </div>
        <div className="player-seat-card relative flex min-w-[168px] items-center gap-2 rounded-full border border-black/[.085] bg-white/95 py-1.5 pl-1.5 pr-4 shadow-[0_2px_5px_rgba(25,31,27,.05),0_12px_28px_rgba(25,31,27,.07)] backdrop-blur-sm">
          <AvatarGlyph tone={player.avatarTone} className="player-seat-avatar" />
          <div className="min-w-0">
            <p className="truncate text-[12px] font-semibold tracking-[-0.02em]">{player.name}</p>
            <p className="mt-0.5 text-[10px] tabular-nums text-[var(--muted-light)]">
              {formatTokens(player.stack)} Token
            </p>
          </div>
          {player.status === "thinking" ? (
            <span className="absolute -right-1 -top-1 size-2.5 rounded-full border-2 border-white bg-[#2d96e9]" />
          ) : null}
          {player.dealer === true ? (
            <span className="absolute -bottom-1 -right-1 grid size-5 place-items-center rounded-full border border-black/10 bg-[#202623] text-[8px] font-semibold text-white shadow-sm">
              D
            </span>
          ) : null}
        </div>
        {(player.committed ?? 0) > 0 ? (
          <div className="absolute left-1/2 top-full mt-2 -translate-x-1/2 whitespace-nowrap rounded-full border border-black/[.075] bg-white/92 px-3 py-1 text-[10px] text-[var(--muted)] shadow-sm">
            {t(player.status === "all-in" ? "player.allIn" : "player.bet")}{" "}
            <strong className="font-semibold text-[var(--ink)]">
              {formatTokens(player.committed ?? 0)}
            </strong>
          </div>
        ) : null}
      </motion.div>
    </div>
  );
}
