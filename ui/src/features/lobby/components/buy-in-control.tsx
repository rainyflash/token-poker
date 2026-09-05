import { useI18n } from "../../../core/i18n/use-i18n";
import { CodexSlider } from "../../../shared/ui/codex-slider";
import type { StakeLevel } from "../model/stake-levels";

interface BuyInControlProps {
  readonly level: StakeLevel;
  readonly value: number;
  readonly availableTokens: number;
  readonly locked?: boolean;
  readonly onChange: (value: number) => void;
}

export function BuyInControl({
  level,
  value,
  availableTokens,
  locked = false,
  onChange,
}: BuyInControlProps) {
  const { t, formatTokens } = useI18n();
  const effectiveMaximum = Math.min(level.maximumBuyIn, availableTokens);
  const rawPercentage = availableTokens === 0 ? 0 : (value / availableTokens) * 100;
  const percentage = Math.round(rawPercentage * 100) / 100;
  const percentageLabel = rawPercentage > 0 && rawPercentage < 0.01 ? "<0.01%" : `${String(percentage)}%`;

  return (
    <div
      className="buy-in-control min-w-0"
      data-locked={locked}
    >
      <div className="flex items-start justify-between gap-5">
        <div>
          <p className="text-[13px] font-medium text-[var(--muted)]">{t("buyIn.title")}</p>
          <p className="mt-1 text-[30px] font-semibold tabular-nums tracking-[-0.045em]">
            {formatTokens(value)} <span className="text-[13px] font-medium text-[var(--muted)]">Token</span>
          </p>
        </div>
        <div className="text-right">
          <p className="text-xs text-[var(--muted)]">{t("buyIn.share")}</p>
          <p className="mt-1 text-[13px] font-medium tabular-nums">{percentageLabel}</p>
        </div>
      </div>

      <CodexSlider
        className="mt-4"
        value={value}
        minimum={level.minimumBuyIn}
        maximum={Math.max(level.minimumBuyIn, effectiveMaximum)}
        step={Math.max(1_000, level.bigBlind)}
        disabled={locked || effectiveMaximum < level.minimumBuyIn}
        ariaLabel={t("buyIn.aria")}
        valueText={`${formatTokens(value)} Token`}
        onValueChange={onChange}
      />
      <div className="mt-2 flex flex-wrap justify-between gap-2 text-xs tabular-nums text-[var(--muted)]">
        <span>{t("buyIn.minimum", { value: formatTokens(level.minimumBuyIn) })}</span>
        <span>{t("buyIn.availableMaximum", { value: formatTokens(effectiveMaximum) })}</span>
      </div>
    </div>
  );
}
