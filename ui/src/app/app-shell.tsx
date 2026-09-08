import { ChevronDown, History } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import type { BridgeSnapshot, CommandResult, HostCommand } from "../core/bridge/contracts";
import { useI18n } from "../core/i18n/use-i18n";
import { AccountMenu } from "../features/account/account-menu";
import { projectPlayerProfile } from "../features/account/model/player-profile";
import { GameRulesDialog } from "../features/lobby/components/game-rules-dialog";
import { UpdateDialog } from "../features/update/update-dialog";
import { Button } from "../shared/ui/button";
import { CodexMark } from "../shared/ui/codex-mark";
import type { AppSubview, PrimarySurface } from "./app-state";
import "./app-shell.css";

interface AppShellProps {
  readonly primarySurface: PrimarySurface;
  readonly activeSubview: AppSubview | null;
  readonly bridge: BridgeSnapshot;
  readonly onOpenSubview: (subview: AppSubview) => void;
  readonly onSyncToken: () => void;
  readonly sendCommand: (command: HostCommand) => CommandResult;
  readonly onClose: () => void;
  readonly children: ReactNode;
}

export function AppShell({
  primarySurface, activeSubview, bridge, onOpenSubview, onSyncToken, sendCommand, onClose, children,
}: AppShellProps) {
  const { language, t, toggleLanguage, formatTokens } = useI18n();
  const profile = projectPlayerProfile(
    bridge.tokenSnapshot,
    t(bridge.mode === "preview" ? "app.previewPlayer" : "app.codexPlayer"),
  );
  const usageLoading = bridge.officialUsage.phase === "loading";
  const usageLabel = bridge.tokenSnapshot
    ? `${formatTokens(bridge.tokenSnapshot.lifetimeTokens)} Token`
    : t(usageLoading ? "app.usageReading" : "app.usageWaiting");
  const networkReady = bridge.sidecarReady && bridge.discovery.registeredNodes.size > 0;

  return (
    <div className="token-holdem-app flex h-full min-h-0 w-full flex-col overflow-hidden bg-[var(--canvas)] text-[var(--ink)]" data-surface={primarySurface}>
      <header className="app-header">
        <div className="app-brand">
          <CodexMark className="app-brand-mark" />
          <span>Token Poker</span>
          {bridge.mode === "preview" ? <span className="app-preview-label" title={t("app.previewBanner")}>{t("app.previewLabel")}</span> : null}
        </div>
        <div className="app-toolbar">
          <div className="app-utilities">
            <Button variant="ghost" size="icon" onClick={() => onOpenSubview("statistics")} aria-label={t("app.openStatistics")} title={t("app.openStatistics")} aria-pressed={activeSubview === "statistics"}>
              <History className="size-5" strokeWidth={1.6} />
            </Button>
            <GameRulesDialog />
            <UpdateDialog status={bridge.update} sendCommand={sendCommand} />
          </div>
          <button className="app-network-status" type="button" data-ready={networkReady} onClick={() => onOpenSubview("identity")} title={t("app.openIdentity")} aria-label={t(networkReady ? "lobby.communityConnected" : "lobby.communityConnecting")}>
            <span className="app-network-dot" aria-hidden="true" />
            <span>{t(networkReady ? "app.connected" : "app.connecting")}</span>
          </button>
          <Button variant="ghost" size="sm" className="app-language" onClick={toggleLanguage} aria-label={t(language === "en" ? "language.switchToChinese" : "language.switchToEnglish")}>
            {language === "en" ? "English" : "中文"}<ChevronDown className="size-3.5" strokeWidth={1.6} />
          </Button>
          <AccountMenu name={profile.displayName} avatarUrl={profile.avatarUrl} usageLabel={usageLabel} usageLoading={usageLoading} onIdentity={() => onOpenSubview("identity")} onRefresh={onSyncToken} onClose={onClose} />
        </div>
      </header>
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
