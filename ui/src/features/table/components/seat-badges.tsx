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
          className="seat-blind grid h-6 min-w-6 shrink-0 place-items-center whitespace-nowrap rounded-full border border-[#b9dffc] bg-[#edf8ff] px-1.5 text-[10px] font-semibold text-[#12659c]"
          title={t(blind === "small" ? "player.smallBlind" : "player.bigBlind")}
        >
          {t(blind === "small" ? "player.smallBlindShort" : "player.bigBlindShort")}
        </span>
      ) : null}
      {dealer ? (
        <span
          className="grid size-6 place-items-center rounded-full bg-[#202623] text-[10px] font-semibold text-white"
          title={t("player.dealer")}
        >
          D
        </span>
      ) : null}
    </span>
  );
}
