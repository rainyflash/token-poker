import { useEffect, useState } from "react";
import { useI18n } from "../../../core/i18n/use-i18n";
import { cn } from "../../../shared/lib/cn";

interface TurnTimerProps {
  readonly deadlineUnixMs: number | null | undefined;
  readonly durationMs: number | undefined;
  readonly className?: string;
}

function remainingUntil(deadlineUnixMs: number): number {
  return Math.max(0, deadlineUnixMs - Date.now());
}

export function TurnTimer({ deadlineUnixMs, durationMs, className }: TurnTimerProps) {
  const { t } = useI18n();
  const [remainingMs, setRemainingMs] = useState(() =>
    deadlineUnixMs === null || deadlineUnixMs === undefined
      ? 0
      : remainingUntil(deadlineUnixMs),
  );

  useEffect(() => {
    if (deadlineUnixMs === null || deadlineUnixMs === undefined) return undefined;
    const update = (): void => setRemainingMs(remainingUntil(deadlineUnixMs));
    update();
    const interval = globalThis.setInterval(update, 200);
    return () => globalThis.clearInterval(interval);
  }, [deadlineUnixMs]);

  if (
    deadlineUnixMs === null ||
    deadlineUnixMs === undefined ||
    durationMs === undefined ||
    durationMs <= 0
  ) {
    return null;
  }

  const seconds = Math.ceil(remainingMs / 1_000);
  const progress = Math.min(1, Math.max(0, remainingMs / durationMs));
  const urgent = remainingMs <= 5_000;

  return (
    <div
      className={cn(
        "flex h-6 min-w-[108px] items-center gap-2 rounded-full border border-black/[.075] bg-white/94 px-2.5 shadow-sm backdrop-blur-sm",
        className,
      )}
      aria-label={t("player.turnRemaining", { seconds })}
    >
      <span className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-black/[.08]">
        <span
          className={cn(
            "block h-full rounded-full transition-[width,background-color] duration-200",
            urgent ? "bg-[#e45735]" : "bg-[var(--codex-blue-500)]",
          )}
          style={{ width: `${String(progress * 100)}%` }}
        />
      </span>
      <span className={cn("w-6 text-right text-[9px] font-semibold tabular-nums", urgent && "text-[#c84728]") }>
        {seconds}s
      </span>
    </div>
  );
}
