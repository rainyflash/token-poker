import { ChevronRight } from "lucide-react";
import { useId } from "react";
import { useI18n } from "../../../core/i18n/use-i18n";
import { STAKE_LEVELS } from "../model/stake-levels";

interface StakeLevelListProps {
  readonly selectedId: string;
  readonly locked: boolean;
  readonly onSelect: (id: string) => void;
}

export function StakeLevelList({ selectedId, locked, onSelect }: StakeLevelListProps) {
  const { t, formatTokens } = useI18n();
  const groupId = useId();

  return (
    <fieldset className="stake-list" disabled={locked}>
      <legend className="sr-only">{t("lobby.chooseLevel")}</legend>
      <div className="stake-list-heading" aria-hidden="true">
        <span>{t("lobby.reasoningLevel")}</span>
        <span>{t("stakeCard.blinds")}</span>
        <span>{t("stakeCard.range")}</span>
      </div>
      {STAKE_LEVELS.map((level) => (
        <label key={level.id} className="stake-row" data-selected={selectedId === level.id}>
          <span className="stake-row-name">
            <input
              type="radio"
              name={groupId}
              value={level.id}
              checked={selectedId === level.id}
              onChange={() => onSelect(level.id)}
              aria-label={t(level.nameKey)}
              aria-describedby={`${groupId}-${level.id}-blinds ${groupId}-${level.id}-range`}
            />
            <span>{t(level.nameKey)}</span>
          </span>
          <span className="stake-row-blinds" id={`${groupId}-${level.id}-blinds`}>
            <span className="sr-only">{t("stakeCard.blinds")}: </span>
            {formatTokens(level.smallBlind)} / {formatTokens(level.bigBlind)}
          </span>
          <span className="stake-row-range" id={`${groupId}-${level.id}-range`}>
            <span className="sr-only">{t("stakeCard.range")}: </span>
            {formatTokens(level.minimumBuyIn)} – {formatTokens(level.maximumBuyIn)}
          </span>
          <ChevronRight className="stake-row-chevron" size={20} strokeWidth={1.5} aria-hidden="true" />
        </label>
      ))}
    </fieldset>
  );
}
