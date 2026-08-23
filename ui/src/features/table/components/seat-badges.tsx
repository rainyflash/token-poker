import { useI18n } from "../../../core/i18n/use-i18n";
import type { BlindRole } from "../model/table-state";

interface SeatBadgesProps {
  readonly dealer: boolean;
  readonly blind: BlindRole | undefined;
}

export function SeatBadges({ dealer, blind }: SeatBadgesProps) {
  const { t } = useI18n();
  if (!dealer && blind === undefined) return null;

  return (
    <span className="flex shrink-0 items-center gap-1">
      {blind !== undefined ? (
        <span
          className="grid h-5 min-w-5 place-items-center rounded-full border border-[#b9dffc] bg-[#edf8ff] px-1 text-[7px] font-bold text-[#1579ba] shadow-sm"
          title={t(blind === "small" ? "player.smallBlind" : "player.bigBlind")}
        >
          {t(blind === "small" ? "player.smallBlindShort" : "player.bigBlindShort")}
        </span>
      ) : null}
      {dealer ? (
        <span
          className="grid size-5 place-items-center rounded-full bg-[#202623] text-[8px] font-semibold text-white shadow-sm"
          title={t("player.dealer")}
        >
          D
        </span>
      ) : null}
    </span>
  );
}
