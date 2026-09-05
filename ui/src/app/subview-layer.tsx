import { ArrowLeft } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useRef, type ReactNode } from "react";
import { useI18n } from "../core/i18n/use-i18n";
import type { MessageKey } from "../core/i18n/messages";
import type { AppSubview } from "./app-state";

interface SubviewLayerProps {
  readonly subview: AppSubview;
  readonly onClose: () => void;
  readonly children: ReactNode;
}

const TITLE_KEYS: Readonly<Record<AppSubview, MessageKey>> = {
  statistics: "subview.statistics",
  identity: "subview.identity",
};

export function SubviewLayer({ subview, onClose, children }: SubviewLayerProps) {
  const { t } = useI18n();
  const title = t(TITLE_KEYS[subview]);
  const backButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const previousFocus = document.activeElement;
    backButton.current?.focus();
    return () => {
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) {
        previousFocus.focus();
      }
    };
  }, []);

  return (
    <motion.section
      role="region"
      aria-label={title}
      onKeyDown={(event) => {
        if (event.key === "Escape" && !event.defaultPrevented) {
          event.stopPropagation();
          onClose();
        }
      }}
      className="absolute inset-0 z-40 flex min-h-0 flex-col bg-white"
      initial={{ opacity: 0, x: 18 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 18 }}
      transition={{ type: "spring", stiffness: 390, damping: 36, mass: 0.72 }}
    >
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-[var(--line)] px-4 min-[760px]:px-6">
        <button
          ref={backButton}
          type="button"
          onClick={onClose}
          className="grid size-8 place-items-center rounded-[9px] text-[var(--muted)] outline-none transition-colors hover:bg-black/[.04] hover:text-[var(--ink)] focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)]"
          aria-label={t("subview.back")}
        >
          <ArrowLeft className="size-4" strokeWidth={1.7} />
        </button>
        <p className="text-[14px] font-semibold tracking-[-0.02em]">{title}</p>
        <p className="ml-auto hidden text-xs text-[var(--muted)] min-[620px]:block">{t("subview.connectionActive")}</p>
      </div>
      <div className="min-h-0 flex-1">{children}</div>
    </motion.section>
  );
}
