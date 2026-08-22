import { useI18n } from "../../../core/i18n/use-i18n";
import { cn } from "../../../shared/lib/cn";
import { CodexMark } from "../../../shared/ui/codex-mark";
import type { CardValue, Suit } from "../model/table-state";

const SUIT_SYMBOL: Record<Suit, string> = {
  spade: "♠",
  heart: "♥",
  diamond: "♦",
  club: "♣",
};

interface PlayingCardProps {
  readonly card?: CardValue | null;
  readonly back?: boolean;
  readonly compact?: boolean;
  readonly className?: string;
}

export function PlayingCard({ card, back = false, compact = false, className }: PlayingCardProps) {
  const { t } = useI18n();
  const isRed = card?.suit === "heart" || card?.suit === "diamond";
  return (
    <div
      className={cn(
        "playing-card grid shrink-0 place-items-center overflow-hidden rounded-[8px] border bg-white shadow-[0_2px_6px_rgba(24,29,25,.08),0_12px_24px_rgba(24,29,25,.05)]",
        compact ? "h-[62px] w-[44px]" : "h-[90px] w-[64px]",
        back ? "border-white/80 bg-[#f3f4f2]" : "border-black/[.10]",
        className,
      )}
      aria-label={back ? t("card.hidden") : card ? `${card.rank}${SUIT_SYMBOL[card.suit]}` : t("card.empty")}
    >
      {back ? (
        <div className="grid size-[74%] place-items-center rounded-[6px] border border-black/[.08] bg-[#fafafa] text-[#202522] shadow-[inset_0_0_0_1px_rgba(255,255,255,.72)]">
          <CodexMark className={compact ? "size-[18px]" : "size-[24px]"} />
        </div>
      ) : card ? (
        <div className={cn("flex h-full w-full flex-col items-center justify-center", isRed ? "text-[#d44a43]" : "text-[#161a18]")}>
          <span className={cn("font-semibold leading-none tracking-[-0.05em]", compact ? "text-[16px]" : "text-[20px]")}>
            {card.rank}
          </span>
          <span className={cn("leading-none", compact ? "mt-1 text-[18px]" : "mt-1.5 text-[25px]")}>
            {SUIT_SYMBOL[card.suit]}
          </span>
        </div>
      ) : null}
    </div>
  );
}
