import { ChevronDown, Info, RefreshCw, ShieldCheck, Users, X } from "lucide-react";
import { useState } from "react";
import type {
  BridgeSnapshot,
  CommandResult,
  HostCommand,
} from "../../core/bridge/contracts";
import { useI18n } from "../../core/i18n/use-i18n";
import { Button } from "../../shared/ui/button";
import { StatusPill } from "../../shared/ui/status-pill";
import { BuyInControl } from "./components/buy-in-control";
import { FriendRoomDialog } from "./components/friend-room-dialog";
import { StakeLevelCard } from "./components/stake-level-card";
import {
  DEFAULT_STAKE_LEVEL_ID,
  findStakeLevel,
  STAKE_LEVELS,
} from "./model/stake-levels";

interface LobbyViewProps {
  readonly bridge: BridgeSnapshot;
  readonly sendCommand: (command: HostCommand) => CommandResult;
}

export function LobbyView({ bridge, sendCommand }: LobbyViewProps) {
  const { t } = useI18n();
  const availableTokens = bridge.tokenSnapshot?.lifetimeTokens ?? 0;
  const usageLoading = bridge.officialUsage.phase === "loading";
  const usageFailed = bridge.officialUsage.phase === "error";
  const [selectedLevelId, setSelectedLevelId] = useState(DEFAULT_STAKE_LEVEL_ID);
  const level = findStakeLevel(selectedLevelId);
  const [requestedBuyIn, setRequestedBuyIn] = useState(level.minimumBuyIn);
  const [message, setMessage] = useState<string | null>(null);
  const isMatching = bridge.pool.status !== "idle";
  const canEnter =
    availableTokens >= level.minimumBuyIn && bridge.sidecarReady && bridge.identity !== null;
  const canAutoMatch = canEnter && bridge.discovery.registeredNodes.size > 0;
  const buyIn = Math.min(
    Math.max(level.minimumBuyIn, Math.min(level.maximumBuyIn, availableTokens)),
    Math.max(level.minimumBuyIn, requestedBuyIn),
  );
  const networkReady = bridge.sidecarReady && bridge.discovery.registeredNodes.size > 0;

  const selectLevel = (levelId: string): void => {
    const next = findStakeLevel(levelId);
    setSelectedLevelId(levelId);
    setRequestedBuyIn(next.minimumBuyIn);
    setMessage(null);
  };

  const toggleMatchmaking = (): void => {
    const command: HostCommand = isMatching
      ? { type: "cancel_public_pool" }
      : { type: "join_public_pool", level_id: level.id, buy_in: buyIn };
    const result = sendCommand(command);
    setMessage(
      result.ok
        ? isMatching
          ? t("lobby.stopped")
          : t("lobby.searching")
        : result.error,
    );
  };

  return (
    <section className="lobby-view h-full overflow-y-auto bg-[var(--canvas)]">
      <div className="page-content lobby-content">
        <header className="lobby-heading flex flex-wrap items-end justify-between gap-4">
          <div>
            <h1 className="text-[30px] font-semibold tracking-[-0.04em]">{t("lobby.title")}</h1>
            <p className="mt-2 text-sm leading-6 text-[var(--muted)]">{t("lobby.shortDescription")}</p>
          </div>
          <StatusPill
            label={t(networkReady ? "lobby.networkReady" : "lobby.communityConnecting")}
            tone={networkReady ? "neutral" : "attention"}
            dot
          />
        </header>

        {bridge.tokenSnapshot === null ? (
          <div
            className={`mt-5 flex items-center justify-between gap-5 rounded-[13px] border px-4 py-3.5 ${
              usageFailed
                ? "border-[color-mix(in_oklab,var(--codex-red-500)_18%,transparent)] bg-[var(--codex-red-25)]"
                : "border-[#d7e8f5] bg-[#f7fbfe]"
            }`}
            aria-live="polite"
          >
            <div className="min-w-0">
              <p
                className={`text-[13px] font-medium ${
                  usageFailed ? "text-[var(--codex-red-500)]" : "text-[#276b9b]"
                }`}
              >
                {usageFailed
                  ? t("lobby.usageFailed")
                  : usageLoading
                    ? t("lobby.usageLoading")
                    : t("lobby.usageMissing")}
              </p>
              <p
                className={`mt-1 text-[12px] leading-5 ${
                  usageFailed ? "text-[#8b4a47]" : "text-[#628299]"
                }`}
              >
                {usageFailed
                  ? bridge.officialUsage.error
                  : t("lobby.usageReadOnly")}
              </p>
            </div>
            <Button
              variant="secondary"
              size="sm"
              className="shrink-0"
              disabled={usageLoading}
              onClick={() => sendCommand({ type: "request_token_refresh" })}
            >
              <RefreshCw className={`size-3.5 ${usageLoading ? "animate-spin" : ""}`} />
              {usageFailed ? t("common.retry") : usageLoading ? t("common.loading") : t("common.refresh")}
            </Button>
          </div>
        ) : null}

        <div className="stake-level-grid mt-7 grid grid-cols-2 gap-3 min-[960px]:grid-cols-4" role="group" aria-label={t("lobby.chooseLevel")}>
          {STAKE_LEVELS.map((stakeLevel) => (
            <StakeLevelCard
              key={stakeLevel.id}
              level={stakeLevel}
              selected={stakeLevel.id === selectedLevelId}
              affordable={availableTokens >= stakeLevel.minimumBuyIn}
              locked={isMatching}
              onSelect={() => selectLevel(stakeLevel.id)}
            />
          ))}
        </div>

        <div className="lobby-entry mt-5 grid items-center gap-6 rounded-2xl border border-[var(--line)] bg-white p-5 min-[760px]:grid-cols-[minmax(0,1fr)_260px] min-[960px]:gap-10 min-[960px]:p-6">
          <div className="min-w-0">
            <BuyInControl
              level={level}
              value={buyIn}
              availableTokens={availableTokens}
              locked={isMatching}
              onChange={setRequestedBuyIn}
            />
          </div>

          <div className="lobby-entry-actions grid gap-3">
            <Button
              variant={isMatching ? "danger" : "primary"}
              size="lg"
              className="w-full rounded-full"
              disabled={!canAutoMatch}
              onClick={toggleMatchmaking}
            >
              {isMatching ? <X className="size-4" /> : <Users className="size-4" />}
              {t(isMatching ? "lobby.stopSearch" : "lobby.quickSeat")}
            </Button>
            {bridge.identity === null ? (
              <p className="text-xs leading-5 text-[#805919]">{t("lobby.identityRequired")}</p>
            ) : null}
            {bridge.identity !== null && bridge.discovery.nodes.length === 0 ? (
              <p className="text-xs leading-5 text-[#805919]">
                {t("lobby.discoveryRequired")}
              </p>
            ) : null}
            {bridge.identity !== null &&
            bridge.discovery.nodes.length > 0 &&
            bridge.discovery.registeredNodes.size === 0 ? (
              <p className="text-xs leading-5 text-[#805919]">
                {t("lobby.discoveryConnecting")}
              </p>
            ) : null}
            <FriendRoomDialog
              level={level}
              buyIn={buyIn}
              inviteCode={bridge.friendInviteCode}
              identityReady={bridge.identity !== null}
              roomStatus={bridge.friendRoomStatus}
              sendCommand={sendCommand}
            />
            {message || bridge.lastWarning ? (
              <p role="status" className="text-xs leading-5 text-[var(--muted)]">{bridge.lastWarning ?? message}</p>
            ) : null}
          </div>
        </div>

        <details className="lobby-details group mt-5 border-t border-[var(--line)]">
          <summary className="flex cursor-pointer list-none items-center gap-2 py-4 text-xs text-[var(--muted)] outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] [&::-webkit-details-marker]:hidden">
            <Info className="size-4" />
            <span>{t("lobby.details")}</span>
            <ChevronDown className="ml-auto size-4 transition-transform group-open:rotate-180" />
          </summary>
          <div className="grid gap-4 pb-5 text-xs leading-6 text-[var(--muted)] min-[760px]:grid-cols-2">
            <div>
              <p>{t("lobby.description")}</p>
              <p className="mt-2">{t("lobby.transport")}: {t("lobby.transportValue")}</p>
            </div>
            <div>
              <p className="flex items-center gap-2 font-medium text-[var(--ink)]"><ShieldCheck className="size-4" />{t("lobby.securityTitle")}</p>
              <p className="mt-2">{t("lobby.securityDescription")}</p>
            </div>
          </div>
        </details>
      </div>
    </section>
  );
}
