import { cn } from "../../../shared/lib/cn";
import { CodexMark } from "../../../shared/ui/codex-mark";

type AvatarTone = "ink" | "blue" | "violet" | "mint" | "coral";

const TONE_CLASSES: Record<AvatarTone, string> = {
  ink: "bg-[#0f2e35] text-[#6fd3c0]",
  blue: "bg-[#173e82] text-[#65a7f5]",
  violet: "bg-[#302b72] text-[#8583ea]",
  mint: "bg-[#0d7969] text-[#62d9ac]",
  coral: "bg-[#ca5e41] text-[#ffc183]",
};

export function AvatarGlyph({ tone, className }: { readonly tone: AvatarTone; readonly className?: string }) {
  return (
    <span
      className={cn(
        "grid size-9 shrink-0 place-items-center overflow-hidden rounded-full ring-1 ring-black/[.08]",
        TONE_CLASSES[tone],
        className,
      )}
      aria-hidden="true"
    >
      <CodexMark className="size-[52%] text-white/90" />
    </span>
  );
}
