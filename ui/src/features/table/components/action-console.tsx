import { useI18n } from "../../../core/i18n/use-i18n";
import { Button } from "../../../shared/ui/button";
import { CodexSlider } from "../../../shared/ui/codex-slider";

const BET_PRESETS = [25, 33, 75, 133] as const;

interface ActionConsoleProps {
  readonly amount: number;
  readonly minimum: number;
  readonly maximum: number;
  readonly currentBet: number;
  readonly pot: number;
  readonly toCall: number;
  readonly canAct: boolean;
  readonly awaitingReveal: boolean;
  readonly inactiveLabel?: string;
  readonly onAmountChange: (amount: number) => void;
  readonly onAction: (action: "fold" | "check" | "call" | "raise") => void;
}

export function ActionConsole({
  amount,
  minimum,
  maximum,
  currentBet,
  pot,
  toCall,
  canAct,
  awaitingReveal,
  inactiveLabel,
  onAmountChange,
  onAction,
}: ActionConsoleProps) {
  const { t, formatTokens } = useI18n();
  const step = 10_000;
  const effectiveMinimum = Math.min(minimum, maximum);
  const clamp = (value: number): number =>
    Math.min(maximum, Math.max(effectiveMinimum, value));
  const canRaise = canAct && maximum > currentBet;

  return (
    <div className="action-console absolute bottom-5 left-1/2 z-40 w-[620px] -translate-x-1/2">
      <div className="flex flex-col gap-2 rounded-[20px] border border-black/[.09] bg-white/96 p-2.5 shadow-[0_2px_5px_rgba(20,25,22,.05),0_18px_44px_rgba(20,25,22,.08)] backdrop-blur-xl min-[520px]:flex-row min-[520px]:items-center min-[520px]:rounded-full">
        <div className="grid w-full grid-cols-4 items-center gap-1 min-[520px]:flex min-[520px]:w-auto">
          {BET_PRESETS.map((preset) => (
            <Button
              key={preset}
              size="sm"
              variant={preset === 75 ? "accent" : "ghost"}
              className="h-7 min-w-0 rounded-[8px] px-2 text-[11px] min-[520px]:min-w-[52px]"
              disabled={!canRaise}
              onClick={() =>
                onAmountChange(clamp(Math.round((Math.max(pot, step) * preset) / 100 / step) * step))
              }
            >
              {String(preset)}%
            </Button>
          ))}
        </div>
        <div className="flex w-full min-w-0 flex-1 items-center gap-2 min-[520px]:ml-auto min-[520px]:pl-3">
          <CodexSlider
            className="min-w-0 flex-1 min-[520px]:min-w-[150px]"
            value={amount}
            minimum={effectiveMinimum}
            maximum={maximum}
            step={step}
            disabled={!canRaise}
            ariaLabel={t("action.raiseAria")}
            onValueChange={(value) => onAmountChange(clamp(value))}
          />
          <span className="w-[86px] text-right text-[11px] font-semibold tabular-nums">
            {formatTokens(amount)} Token
          </span>
        </div>
      </div>
      <div className="mx-auto mt-2 grid w-full max-w-[450px] grid-cols-[1fr_1.15fr_1.25fr] gap-1.5 rounded-full border border-black/[.09] bg-white/96 p-1.5 shadow-[0_2px_5px_rgba(20,25,22,.05),0_18px_44px_rgba(20,25,22,.09)] backdrop-blur-xl">
        <Button className="rounded-full px-2 text-[11px] min-[520px]:text-[13px]" size="md" disabled={!canAct} onClick={() => onAction("fold")}>
          {t("action.fold")}
        </Button>
        <Button
          size="md"
          className="rounded-full px-2 text-[11px] min-[520px]:text-[13px]"
          disabled={!canAct}
          onClick={() => onAction(toCall === 0 ? "check" : "call")}
        >
          {toCall === 0 ? t("action.check") : t("action.call", { amount: formatTokens(toCall) })}
        </Button>
        <Button className="overflow-hidden rounded-full px-2 text-[11px] min-[520px]:text-[13px]" variant="primary" size="md" disabled={!canRaise} onClick={() => onAction("raise")}>
          {awaitingReveal
            ? t("action.verifyingBoard")
            : canAct
              ? t("action.raiseTo", { amount: formatTokens(amount) })
              : (inactiveLabel ?? t("table.waitingPlayers"))}
        </Button>
      </div>
    </div>
  );
}
