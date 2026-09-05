import { useI18n } from "../../../core/i18n/use-i18n";
import type { MessageKey } from "../../../core/i18n/messages";
import type { TableAction, TablePlayer } from "../model/table-state";

const ACTION_LABELS: Readonly<Record<TableAction, MessageKey>> = {
  fold: "player.folded",
  check: "player.checked",
  call: "player.called",
  raise: "player.raised",
};

interface SeatStatusProps {
  readonly status: TablePlayer["status"];
  readonly lastAction: TableAction | null | undefined;
  readonly committed: number;
}

export function SeatStatus({ status, lastAction, committed }: SeatStatusProps) {
  const { t, formatTokens } = useI18n();
  const label =
    status === "all-in"
      ? t("player.allIn")
      : lastAction === null || lastAction === undefined
        ? committed > 0
          ? t("player.bet")
          : null
        : t(ACTION_LABELS[lastAction]);

  if (label === null) return null;

  return (
    <div className="seat-status inline-flex h-7 items-center gap-1.5 whitespace-nowrap rounded-full border border-[var(--line)] bg-white px-3 text-xs text-[var(--muted)] shadow-[var(--codex-shadow-sm)]" data-action={status === "all-in" ? "all-in" : lastAction}>
      <span>{label}</span>
      {committed > 0 ? (
        <strong className="font-semibold tabular-nums text-[var(--ink)]">
          {formatTokens(committed)}
        </strong>
      ) : null}
    </div>
  );
}
