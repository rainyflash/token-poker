import { RefreshCw, X } from "lucide-react";
import { useState } from "react";
import type { BridgeSnapshot, CommandResult, HostCommand } from "../../core/bridge/contracts";
import { useI18n } from "../../core/i18n/use-i18n";
import { cn } from "../../shared/lib/cn";
import { Button } from "../../shared/ui/button";
import { BuyInControl } from "./components/buy-in-control";
import { FriendRoomDialog } from "./components/friend-room-dialog";
import { StakeLevelList } from "./components/stake-level-list";
import { projectBuyIn } from "./model/buy-in";
import { DEFAULT_STAKE_LEVEL_ID, findStakeLevel } from "./model/stake-levels";
import "./lobby.css";

interface LobbyViewProps {
  readonly bridge: BridgeSnapshot;
  readonly sendCommand: (command: HostCommand) => CommandResult;
}

export function LobbyView({ bridge, sendCommand }: LobbyViewProps) {
  const { t, formatTokens } = useI18n();
  const availableTokens = bridge.tokenSnapshot?.lifetimeTokens ?? null;
  const usageLoading = bridge.officialUsage.phase === "loading";
  const usageFailed = bridge.officialUsage.phase === "error";
  const [selectedLevelId, setSelectedLevelId] = useState(DEFAULT_STAKE_LEVEL_ID);
  const level = findStakeLevel(selectedLevelId);
  const [requestedBuyIn, setRequestedBuyIn] = useState(level.minimumBuyIn);
  const [message, setMessage] = useState<string | null>(null);
  const isMatching = bridge.pool.status !== "idle";
  const buyIn = projectBuyIn(level, requestedBuyIn, availableTokens);
  const canEnter = buyIn.affordable && bridge.sidecarReady && bridge.identity !== null;
  const canAutoMatch = canEnter && bridge.discovery.registeredNodes.size > 0;
  const entryHint = availableTokens === null
    ? null
    : !buyIn.affordable
      ? t("lobby.insufficientTokens", { value: formatTokens(level.minimumBuyIn) })
      : !bridge.sidecarReady
        ? t("lobby.localNodeStarting")
        : bridge.identity === null
          ? t("lobby.identityPreparing")
          : bridge.discovery.nodes.length === 0
            ? t("lobby.discoveryRequired")
            : bridge.discovery.registeredNodes.size === 0
              ? t("lobby.discoveryConnecting")
              : null;

  const selectLevel = (levelId: string): void => {
    const next = findStakeLevel(levelId);
    setSelectedLevelId(levelId);
    setRequestedBuyIn(next.minimumBuyIn);
    setMessage(null);
  };

  const toggleMatchmaking = (): void => {
    if (!isMatching && !canAutoMatch) return;
    const result = sendCommand(isMatching
      ? { type: "cancel_public_pool" }
      : { type: "join_public_pool", level_id: level.id, buy_in: buyIn.value });
    setMessage(result.ok ? t(isMatching ? "lobby.stopped" : "lobby.searching") : result.error);
  };

  return (
    <section className="lobby-view h-full overflow-y-auto bg-[var(--canvas)]">
      <div className="lobby-content">
        <header className="lobby-heading">
          <h1>{t("lobby.title")}</h1>
          <p>{t("lobby.shortDescription")}</p>
        </header>

        <div className="lobby-workspace">
          <StakeLevelList selectedId={selectedLevelId} locked={isMatching} onSelect={selectLevel} />
          <div className="lobby-composer">
            <p className="lobby-selected-level">{t("lobby.selectedLevel", { level: t(level.nameKey) })}</p>
            <BuyInControl
              level={level}
              value={buyIn.value}
              availableTokens={availableTokens}
              locked={isMatching}
              onChange={setRequestedBuyIn}
            />
            <div className="lobby-entry-actions">
              <Button
                variant={isMatching ? "danger" : "primary"}
                size="lg"
                className="lobby-entry-button"
                disabled={!isMatching && !canAutoMatch}
                aria-describedby={entryHint ? "lobby-entry-hint" : availableTokens === null ? "lobby-usage-status" : undefined}
                onClick={toggleMatchmaking}
              >
                {isMatching ? <X className="size-4" /> : null}
                {t(isMatching ? "lobby.stopSearch" : "lobby.quickSeat")}
              </Button>
              <FriendRoomDialog
                level={level}
                buyIn={buyIn.value}
                inviteCode={bridge.friendInviteCode}
                identityReady={bridge.identity !== null}
                roomStatus={bridge.friendRoomStatus}
                sendCommand={sendCommand}
              />
            </div>

            {availableTokens === null ? (
              <div className="lobby-notice" data-error={usageFailed} role="status" id="lobby-usage-status">
                <div className="min-w-0">
                  <p className="font-medium">{t(usageFailed ? "lobby.usageFailed" : usageLoading ? "lobby.usageLoading" : "lobby.usageMissing")}</p>
                  <p className="mt-1 break-words">{usageFailed ? bridge.officialUsage.error : t("lobby.usageWaitingHint")}</p>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="shrink-0"
                  disabled={usageLoading}
                  aria-label={t(usageFailed ? "common.retry" : "common.refresh")}
                  onClick={() => sendCommand({ type: "request_token_refresh" })}
                >
                  <RefreshCw className={cn("size-4", usageLoading && "animate-spin")} />
                </Button>
              </div>
            ) : entryHint ? <p className="lobby-entry-hint" role="status" id="lobby-entry-hint">{entryHint}</p> : null}
            {message || bridge.lastWarning ? (
              <p className="lobby-entry-hint" role="status">{bridge.lastWarning ?? message}</p>
            ) : null}
          </div>
        </div>

        <footer className="lobby-footer"><p>{t("lobby.seatingNote")}</p></footer>
      </div>
    </section>
  );
}
