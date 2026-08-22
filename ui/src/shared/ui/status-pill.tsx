import type { LucideIcon } from "lucide-react";
import { cn } from "../lib/cn";

type StatusTone = "neutral" | "success" | "attention" | "info";

const TONE_CLASSES: Record<StatusTone, string> = {
  neutral: "border-[var(--line)] bg-white text-[var(--muted)]",
  success:
    "border-[color-mix(in_oklab,var(--codex-green-500)_15%,transparent)] bg-[var(--codex-green-25)] text-[var(--codex-green-500)]",
  attention:
    "border-[color-mix(in_oklab,var(--codex-orange-500)_15%,transparent)] bg-[var(--codex-orange-25)] text-[var(--codex-orange-500)]",
  info:
    "border-[color-mix(in_oklab,var(--codex-blue-300)_18%,transparent)] bg-[var(--codex-blue-50)] text-[var(--codex-blue-500)]",
};

interface StatusPillProps {
  readonly icon?: LucideIcon;
  readonly label: string;
  readonly tone?: StatusTone;
  readonly dot?: boolean;
  readonly className?: string;
}

export function StatusPill({
  icon: Icon,
  label,
  tone = "neutral",
  dot = false,
  className,
}: StatusPillProps) {
  return (
    <span
      className={cn(
        "codex-corner inline-flex h-8 items-center gap-2 rounded-[var(--codex-radius-md)] border px-3 text-[12px] font-medium",
        TONE_CLASSES[tone],
        className,
      )}
    >
      {dot ? <span className="size-1.5 rounded-full bg-current" aria-hidden="true" /> : null}
      {Icon ? <Icon className="size-3.5" strokeWidth={1.8} aria-hidden="true" /> : null}
      {label}
    </span>
  );
}
