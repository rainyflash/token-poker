import { useI18n } from "../../../core/i18n/use-i18n";
import { Button } from "../../../shared/ui/button";
import { CodexSlider } from "../../../shared/ui/codex-slider";
import { BET_PRESETS, BET_STEP, clampRaise, presetRaise, selectedBetPreset } from "../model/bet-sizing";

interface ActionConsoleProps {
  readonly amount: number;
  readonly minimum: number;
  readonly maximum: number;
  readonly currentBet: number;
  readonly pot: number;
  readonly toCall: number;
  readonly canAct: boolean;
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
  onAmountChange,
  onAction,
}: ActionConsoleProps) {
  const { t, formatTokens } = useI18n();
  const effectiveMinimum = Math.min(minimum, maximum);
  const canRaise = canAct && maximum > currentBet;
  const selectedPreset = selectedBetPreset(amount, pot, minimum, maximum);

  return (
    <div className="action-console mx-auto w-full max-w-[680px]">
      <div className="action-sizing flex flex-col gap-2 rounded-[20px] border border-[var(--line)] bg-white p-2 shadow-[var(--codex-shadow-md)] min-[620px]:flex-row min-[620px]:items-center min-[620px]:rounded-full">
        <div className="grid w-full grid-cols-4 items-center gap-1 min-[620px]:flex min-[620px]:w-auto" role="group" aria-label={t("action.presets")}>
          {BET_PRESETS.map((preset) => (
            <Button
              key={preset}
              size="sm"
              variant={preset === selectedPreset ? "accent" : "ghost"}
              aria-pressed={preset === selectedPreset}
              className="h-8 min-w-0 rounded-full px-2 text-[13px] min-[620px]:min-w-[48px]"
              disabled={!canRaise}
              onClick={() =>
                onAmountChange(presetRaise(pot, preset, minimum, maximum))
              }
            >
              {String(preset)}%
            </Button>
          ))}
        </div>
        <div className="flex w-full min-w-0 flex-1 items-center gap-3 min-[620px]:ml-auto min-[620px]:border-l min-[620px]:border-[var(--line)] min-[620px]:pl-3">
          <CodexSlider
            className="min-w-0 flex-1"
            value={amount}
            minimum={effectiveMinimum}
            maximum={maximum}
            step={BET_STEP}
            disabled={!canRaise}
            ariaLabel={t("action.raiseAria")}
            valueText={`${formatTokens(amount)} Token`}
            onValueChange={(value) => onAmountChange(clampRaise(value, minimum, maximum))}
          />
          <span className="w-[100px] shrink-0 pr-2 text-right text-[14px] font-semibold tabular-nums">
            {formatTokens(amount)} <span className="text-xs font-normal text-[var(--muted)]">Token</span>
          </span>
        </div>
      </div>
      <div className="mx-auto mt-2 grid w-full max-w-[500px] grid-cols-[.85fr_1.1fr_1.3fr] gap-2">
        <Button className="rounded-full px-2 text-[13px]" size="md" disabled={!canAct} onClick={() => onAction("fold")}>
          {t("action.fold")}
        </Button>
        <Button
          size="md"
          className="rounded-full px-2 text-[13px]"
          disabled={!canAct}
          onClick={() => onAction(toCall === 0 ? "check" : "call")}
        >
          {toCall === 0 ? t("action.check") : t("action.call", { amount: formatTokens(toCall) })}
        </Button>
        <Button className="min-h-10 rounded-full px-2 text-[13px]" variant="primary" size="md" disabled={!canRaise} onClick={() => onAction("raise")}>
          {t("action.raiseTo", { amount: formatTokens(amount) })}
        </Button>
      </div>
    </div>
  );
}
