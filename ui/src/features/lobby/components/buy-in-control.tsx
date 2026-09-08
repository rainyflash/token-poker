import { useI18n } from "../../../core/i18n/use-i18n";
import { CodexSlider } from "../../../shared/ui/codex-slider";
import { projectBuyIn } from "../model/buy-in";
import type { StakeLevel } from "../model/stake-levels";

interface BuyInControlProps {
  readonly level: StakeLevel;
  readonly value: number;
  readonly availableTokens: number | null;
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
  const { t, formatTokens, formatInteger } = useI18n();
  const range = projectBuyIn(level, value, availableTokens);
  const exactAmount = `${formatInteger(range.value)} Token`;

  return (
    <div
      className="buy-in-control min-w-0"
      data-locked={locked}
    >
      <p className="buy-in-label">{t("buyIn.title")}</p>
      <p className="buy-in-value" title={exactAmount}>
        {formatTokens(range.value)} <span>Token</span>
      </p>
      <CodexSlider
        size="large"
        className="buy-in-slider"
        value={range.value}
        minimum={level.minimumBuyIn}
        maximum={range.maximum}
        step={Math.max(1_000, level.bigBlind)}
        disabled={locked || !range.affordable || range.maximum === level.minimumBuyIn}
        ariaLabel={t("buyIn.aria")}
        valueText={exactAmount}
        onValueChange={onChange}
      />
      <div className="buy-in-limits">
        <span>{t("buyIn.minimum", { value: formatTokens(level.minimumBuyIn) })}</span>
        <span>{t(range.limitedByBalance ? "buyIn.availableMaximum" : "buyIn.maximum", { value: formatTokens(range.maximum) })}</span>
      </div>
    </div>
  );
}
