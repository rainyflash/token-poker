import { cva, type VariantProps } from "class-variance-authority";
import { motion, type HTMLMotionProps } from "motion/react";
import { cn } from "../lib/cn";

const buttonVariants = cva(
  "codex-corner inline-flex select-none items-center justify-center gap-2 whitespace-nowrap rounded-[var(--codex-radius-lg)] border text-[13px] font-medium tracking-[-0.01em] outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-45",
  {
    variants: {
      variant: {
        primary:
          "border-[var(--codex-gray-1000)] bg-[var(--codex-gray-1000)] text-white shadow-[var(--codex-shadow-sm)] hover:bg-[var(--codex-gray-800)]",
        secondary:
          "border-[var(--line-strong)] bg-white text-[var(--ink)] shadow-[var(--codex-shadow-sm)] hover:bg-[var(--codex-gray-50)]",
        ghost:
          "border-transparent bg-transparent text-[var(--muted)] hover:bg-black/[.045] hover:text-[var(--ink)]",
        accent:
          "border-transparent bg-[var(--codex-gray-100)] text-[var(--ink)] hover:bg-[var(--codex-gray-150)]",
        danger:
          "border-[color-mix(in_oklab,var(--codex-red-500)_18%,transparent)] bg-white text-[var(--codex-red-500)] hover:bg-[var(--codex-red-25)]",
      },
      size: {
        sm: "h-8 px-3",
        md: "h-10 px-4",
        lg: "h-12 px-5 text-[14px]",
        icon: "size-9 p-0",
      },
    },
    defaultVariants: {
      variant: "secondary",
      size: "md",
    },
  },
);

export type ButtonProps = HTMLMotionProps<"button"> & VariantProps<typeof buttonVariants>;

export function Button({ className, variant, size, type = "button", ...props }: ButtonProps) {
  return (
    <motion.button
      type={type}
      whileTap={{ scale: 0.975 }}
      transition={{ type: "spring", stiffness: 540, damping: 34 }}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  );
}
