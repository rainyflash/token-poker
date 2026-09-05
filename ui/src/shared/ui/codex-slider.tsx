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
  onValueChange,
}: CodexSliderProps) {
  const effectiveMaximum = Math.max(minimum, maximum);
  const clampedValue = Math.min(effectiveMaximum, Math.max(minimum, value));
  const span = effectiveMaximum - minimum;
  const selectedPercentage = span === 0 ? 0 : ((clampedValue - minimum) / span) * 100;

  return (
    <div
      className={cn(
        "codex-slider flex h-8 flex-col justify-center px-1.5 py-0.5 data-[disabled=true]:opacity-60",
        className,
      )}
      data-disabled={disabled}
    >
      <Slider.Root
        className="group relative flex h-7 w-full touch-none select-none items-center"
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
        <Slider.Track className="relative h-6 flex-1 overflow-hidden rounded-xl bg-[color-mix(in_srgb,var(--codex-text)_10%,transparent)] shadow-[inset_0_0_0_.5px_var(--codex-border)]">
          <Slider.Range className="absolute h-full rounded-l-xl bg-[var(--codex-blue-300)]" />
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
        <Slider.Thumb aria-label={ariaLabel} aria-valuetext={valueText} asChild>
          <motion.span
            whileTap={disabled ? undefined : { scale: 32 / 28 }}
            transition={{ type: "spring", stiffness: 420, damping: 38, mass: 1 }}
            className="relative z-20 block size-7 rounded-full border-[0.5px] border-[var(--line-strong)] bg-white shadow-[0_0_2px_#0000001a] outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] group-data-[disabled]:shadow-none"
          />
        </Slider.Thumb>
      </Slider.Root>
    </div>
  );
}
