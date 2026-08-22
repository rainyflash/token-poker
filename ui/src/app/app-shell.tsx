import { ChartNoAxesCombined, ChevronLeft, KeyRound, Languages, RefreshCw } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import type { BridgeSnapshot } from "../core/bridge/contracts";
import { useI18n } from "../core/i18n/use-i18n";
import { projectPlayerProfile } from "../features/account/model/player-profile";
import { AccountAvatar } from "../shared/ui/account-avatar";
import { Button } from "../shared/ui/button";
import { CodexMark } from "../shared/ui/codex-mark";
import type { AppSubview, PrimarySurface } from "./app-state";

interface AppShellProps {
  readonly primarySurface: PrimarySurface;
  readonly activeSubview: AppSubview | null;
  readonly bridge: BridgeSnapshot;
  readonly onOpenSubview: (subview: AppSubview) => void;
  readonly onSyncToken: () => void;
  readonly onClose: () => void;
  readonly children: ReactNode;
}

export function AppShell({
  primarySurface,
  activeSubview,
  bridge,
  onOpenSubview,
  onSyncToken,
  onClose,
  children,
}: AppShellProps) {
  const { language, t, toggleLanguage, formatTokens } = useI18n();
  const profile = projectPlayerProfile(
    bridge.tokenSnapshot,
    t(bridge.mode === "preview" ? "app.previewPlayer" : "app.codexPlayer"),
  );
  const usageLoading = bridge.officialUsage.phase === "loading";
  const usageLabel = bridge.tokenSnapshot
    ? `${formatTokens(bridge.tokenSnapshot.lifetimeTokens)} Token`
    : usageLoading
      ? t("app.usageReading")
      : t("app.usageWaiting");
  const surfaceLabel = {
    table: t("app.surfaceTable"),
    matching: t("app.surfaceMatching"),
    lobby: t("app.surfaceLobby"),
  }[primarySurface];

  return (
    <div className="token-holdem-app flex h-full min-h-0 w-full flex-col overflow-hidden bg-[var(--canvas)] text-[var(--ink)]">
      <header className="app-header relative z-50 flex h-[62px] shrink-0 items-center border-b border-[var(--line)] bg-[var(--rail)] px-3 min-[760px]:px-5">
        <div className="flex min-w-0 items-center gap-2.5">
          <div className="grid size-8 place-items-center rounded-[10px] border border-black/[.07] bg-white shadow-[var(--codex-shadow-sm)]">
            <CodexMark className="size-[18px]" />
          </div>
          <span className="hidden text-[14px] font-semibold tracking-[-0.025em] min-[560px]:block">
            Token Poker
          </span>
        </div>

        <div className="absolute left-1/2 hidden -translate-x-1/2 text-center min-[760px]:block">
          <p className="text-[11px] font-medium text-[var(--muted)]">
            {surfaceLabel}
          </p>
        </div>

        <div className="ml-auto flex items-center gap-1.5">
          <Button
            variant="ghost"
            size="icon"
            onClick={() => onOpenSubview("statistics")}
            aria-label={t("app.openStatistics")}
            aria-pressed={activeSubview === "statistics"}
          >
            <ChartNoAxesCombined className="size-4" strokeWidth={1.7} />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => onOpenSubview("identity")}
            aria-label={t("app.openIdentity")}
            aria-pressed={activeSubview === "identity"}
          >
            <KeyRound className="size-4" strokeWidth={1.7} />
          </Button>
          <button
            type="button"
            onClick={onSyncToken}
            disabled={usageLoading}
            className="group flex items-center gap-2 rounded-[10px] px-1.5 py-1 text-left outline-none transition-colors hover:bg-black/[.035] focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] disabled:cursor-wait disabled:opacity-70"
            aria-label={t("app.refreshUsage")}
          >
            <div className="relative">
              <AccountAvatar name={profile.displayName} src={profile.avatarUrl} />
              <span
                className={`absolute -bottom-px -right-px size-2.5 rounded-full border-2 border-[var(--rail)] ${
                  bridge.officialUsage.phase === "error"
                    ? "bg-[var(--codex-red-500)]"
                    : bridge.sidecarReady
                      ? "bg-[var(--codex-green-500)]"
                      : "bg-[var(--codex-orange-500)]"
                }`}
              />
            </div>
            <div className="hidden min-w-0 min-[980px]:block">
              <p className="max-w-32 truncate text-[12px] font-medium leading-4">
                {profile.displayName}
              </p>
              <p className="text-[10px] leading-4 text-[var(--muted-light)]">{usageLabel}</p>
            </div>
            <RefreshCw
              className={`hidden size-3.5 text-[var(--muted-light)] group-hover:block min-[980px]:block ${usageLoading ? "animate-spin" : ""}`}
              strokeWidth={1.7}
            />
          </button>
          <Button
            variant="ghost"
            size="sm"
            className="gap-1.5 px-2 text-[10px] font-medium"
            onClick={toggleLanguage}
            aria-label={t(language === "en" ? "language.switchToChinese" : "language.switchToEnglish")}
          >
            <Languages className="size-3.5" strokeWidth={1.7} />
            <span>{language === "en" ? "EN" : "中文"}</span>
          </Button>
          <Button variant="ghost" size="icon" onClick={onClose} aria-label={t("app.returnToCodex")}>
            <ChevronLeft className="size-4" strokeWidth={1.7} />
          </Button>
        </div>
      </header>

      {bridge.mode === "preview" ? (
        <div className="border-b border-[var(--line)] bg-[var(--codex-blue-50)] px-4 py-1 text-center text-[9px] text-[var(--codex-blue-500)]">
          {t("app.previewBanner")}
        </div>
      ) : null}

      <main className="relative min-h-0 min-w-0 flex-1 overflow-hidden">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={primarySurface}
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -3 }}
            transition={{ type: "spring", stiffness: 380, damping: 34, mass: 0.68 }}
            className="h-full"
          >
            {children}
          </motion.div>
        </AnimatePresence>
      </main>
    </div>
  );
}
