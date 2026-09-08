import * as Slider from "@radix-ui/react-slider";
import { motion } from "motion/react";
import type { CSSProperties } from "react";
import { cn } from "../lib/cn";

const TICK_COUNT = 6;

interface CodexSliderProps {
  readonly value: number;
  readonly minimum: number;
  readonly maximum: number;
  readonly step: number;
  readonly ariaLabel: string;
  readonly valueText?: string;
  readonly disabled?: boolean;
  readonly className?: string;
  readonly size?: "regular" | "large";
  readonly onValueChange: (value: number) => void;
}

export function CodexSlider({
  value,
  minimum,
  maximum,
  step,
  ariaLabel,
  valueText,
  disabled = false,
  className,
  size = "regular",
  onValueChange,
}: CodexSliderProps) {
  const effectiveMaximum = Math.max(minimum, maximum);
  const clampedValue = Math.min(effectiveMaximum, Math.max(minimum, value));
  const span = effectiveMaximum - minimum;
  const selectedPercentage = span === 0 ? 0 : ((clampedValue - minimum) / span) * 100;
  const thumbSize = size === "large" ? 48 : 28;

  return (
    <div
      className={cn(
        "codex-slider flex flex-col justify-center px-1.5 py-0.5 data-[disabled=true]:opacity-60",
        size === "large" ? "h-12" : "h-8",
        className,
      )}
      data-disabled={disabled}
    >
      <Slider.Root
        className={cn("group relative flex w-full touch-none select-none items-center", size === "large" ? "h-12" : "h-7")}
        value={[clampedValue]}
        min={minimum}
        max={effectiveMaximum}
        step={step}
        disabled={disabled}
        onValueChange={(values) => {
          const nextValue = values[0];
          if (nextValue !== undefined) onValueChange(nextValue);
        }}
      >
        <Slider.Track className={cn("relative flex-1 overflow-hidden rounded-full bg-[color-mix(in_srgb,var(--codex-text)_10%,transparent)] shadow-[inset_0_0_0_.5px_var(--codex-border)]", size === "large" ? "h-[30px]" : "h-6")}>
          <Slider.Range className="absolute h-full rounded-l-full bg-[var(--codex-blue-300)]" />
          <span className="pointer-events-none absolute inset-0 z-10" aria-hidden="true">
            {Array.from({ length: TICK_COUNT }, (_, index) => {
              const percentage = (index / (TICK_COUNT - 1)) * 100;
              const offset = 13 - (percentage / 50) * 13;
              const style = {
                left: `calc(${String(percentage)}% + ${String(offset)}px)`,
              } satisfies CSSProperties;

              return (
                <span
                  key={index}
                  data-selected={percentage <= selectedPercentage}
                  style={style}
                  className="absolute top-1/2 size-1 -translate-x-1/2 -translate-y-1/2 rounded-full bg-[color-mix(in_srgb,var(--muted)_50%,transparent)] transition-[background-color,transform] duration-150 data-[selected=true]:bg-white/30"
                />
              );
            })}
          </span>
        </Slider.Track>
        <Slider.Thumb aria-label={ariaLabel} aria-valuetext={valueText} aria-disabled={disabled} asChild>
          <motion.span
            whileTap={disabled ? undefined : { scale: (thumbSize + 4) / thumbSize }}
            transition={{ type: "spring", stiffness: 420, damping: 38, mass: 1 }}
            className={cn("relative z-20 block rounded-full bg-white outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] group-data-[disabled]:shadow-none", size === "large"
              ? "size-12 border border-[var(--codex-gray-200)] shadow-[0_2px_4px_#00000014]"
              : "size-7 border-[0.5px] border-[var(--line-strong)] shadow-[0_1px_3px_#00000014]")}
          />
        </Slider.Thumb>
      </Slider.Root>
    </div>
  );
}
