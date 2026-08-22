import type { CSSProperties } from "react";
import { CODEX_MARK_DATA_URI } from "../assets/codex-mark";
import { cn } from "../lib/cn";

interface CodexMarkProps {
  readonly className?: string;
}

const MASK_STYLE: CSSProperties = {
  WebkitMaskImage: `url("${CODEX_MARK_DATA_URI}")`,
  WebkitMaskPosition: "center",
  WebkitMaskRepeat: "no-repeat",
  WebkitMaskSize: "contain",
  maskImage: `url("${CODEX_MARK_DATA_URI}")`,
  maskPosition: "center",
  maskRepeat: "no-repeat",
  maskSize: "contain",
};

export function CodexMark({ className }: CodexMarkProps) {
  return (
    <span
      aria-hidden="true"
      className={cn("inline-block shrink-0 bg-current", className)}
      style={MASK_STYLE}
    />
  );
}
