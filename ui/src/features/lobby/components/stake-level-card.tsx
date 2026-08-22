import { Check } from "lucide-react";
import { motion } from "motion/react";
import { useI18n } from "../../../core/i18n/use-i18n";
import { cn } from "../../../shared/lib/cn";
import type { StakeLevel } from "../model/stake-levels";

interface StakeLevelCardProps {
  readonly level: StakeLevel;
  readonly selected: boolean;
  readonly affordable: boolean;
  readonly locked?: boolean;
  readonly onSelect: () => void;
}

export function StakeLevelCard({
  level,
  selected,
  affordable,
  locked = false,
  onSelect,
}: StakeLevelCardProps) {
  const { t, formatTokens } = useI18n();
  const interactive = affordable && !locked;
  return (
    <motion.button
      type="button"
      onClick={onSelect}
      disabled={!interactive}
      whileTap={interactive ? { scale: 0.985 } : undefined}
      transition={{ type: "spring", stiffness: 480, damping: 32 }}
      className={cn(
        "relative min-h-[142px] rounded-[14px] border p-4 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)]",
        !affordable && "disabled:cursor-not-allowed disabled:opacity-45",
        locked && affordable && "disabled:cursor-default disabled:opacity-100",
        selected
          ? "border-[#8fc8ee] bg-[#f7fbfe] shadow-[0_0_0_1px_rgba(53,150,217,.08),0_12px_28px_rgba(39,106,151,.07)]"
          : "border-[var(--line)] bg-white hover:border-[var(--line-strong)] hover:bg-[var(--surface-subtle)]",
      )}
    >
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="text-[13px] font-semibold tracking-[-0.02em]">{t(level.nameKey)}</p>
          <p className="mt-1 text-[10px] text-[var(--muted-light)]">{t(level.noteKey)}</p>
        </div>
        {selected ? (
          <span className="grid size-5 place-items-center rounded-full bg-[#2d98e6] text-white">
            <Check className="size-3" strokeWidth={2.5} />
          </span>
        ) : null}
      </div>
      <div className="mt-6 flex items-end justify-between">
        <div>
          <p className="text-[10px] text-[var(--muted-light)]">{t("stakeCard.blinds")}</p>
          <p className="mt-1 text-[17px] font-semibold tabular-nums tracking-[-0.035em]">
            {formatTokens(level.smallBlind)} / {formatTokens(level.bigBlind)}
          </p>
        </div>
        <div className="text-right">
          <p className="text-[10px] text-[var(--muted-light)]">{t("stakeCard.range")}</p>
          <p className="mt-1 text-[11px] font-medium tabular-nums">
            {formatTokens(level.minimumBuyIn)}–{formatTokens(level.maximumBuyIn)}
          </p>
        </div>
      </div>
    </motion.button>
  );
}
