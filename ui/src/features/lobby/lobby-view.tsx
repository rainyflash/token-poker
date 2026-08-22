import { Radio, RefreshCw, ShieldCheck, Users, X } from "lucide-react";
import { useMemo, useState } from "react";
import type {
  BridgeSnapshot,
  CommandResult,
  HostCommand,
} from "../../core/bridge/contracts";
import { useI18n } from "../../core/i18n/use-i18n";
import { Button } from "../../shared/ui/button";
import { SectionHeader } from "../../shared/ui/section-header";
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
  const { t, formatTokens } = useI18n();
  const availableTokens = bridge.tokenSnapshot?.lifetimeTokens ?? 0;
  const usageLoading = bridge.officialUsage.phase === "loading";
  const usageFailed = bridge.officialUsage.phase === "error";
  const [selectedLevelId, setSelectedLevelId] = useState(DEFAULT_STAKE_LEVEL_ID);
  const level = useMemo(() => findStakeLevel(selectedLevelId), [selectedLevelId]);
  const [requestedBuyIn, setRequestedBuyIn] = useState(level.minimumBuyIn);
  const [message, setMessage] = useState<string | null>(null);
  const isMatching = bridge.pool.status !== "idle";
  const canEnter =
    availableTokens >= level.minimumBuyIn && bridge.sidecarReady && bridge.identity !== null;
  const canAutoMatch = canEnter && bridge.discovery.registeredNodes.size > 0;
  const buyIn = Math.min(level.maximumBuyIn, Math.max(level.minimumBuyIn, requestedBuyIn));

  const selectLevel = (levelId: string): void => {
    const next = findStakeLevel(levelId);
    setSelectedLevelId(levelId);
    setRequestedBuyIn(
      Math.min(next.maximumBuyIn, Math.max(next.minimumBuyIn, Math.min(availableTokens, next.minimumBuyIn))),
    );
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
    <section className="h-full overflow-y-auto bg-[var(--canvas)]">
      <div className="min-h-full w-full px-[clamp(24px,4vw,64px)] py-[clamp(24px,4vh,44px)]">
        <SectionHeader
          eyebrow={t("lobby.eyebrow")}
          title={t("lobby.title")}
          description={t("lobby.description")}
          action={
            <div className="flex items-center gap-2">
              <StatusPill label={t(bridge.sidecarReady ? "lobby.localNodeReady" : "lobby.localNodeStarting")} tone={bridge.sidecarReady ? "success" : "attention"} dot />
              <StatusPill
                label={
                  bridge.discovery.registeredNodes.size > 0
                    ? t("lobby.communityConnected")
                    : bridge.discovery.nodes.length > 0
                      ? t("lobby.communityConnecting")
                      : t("lobby.discoveryMissing")
                }
                tone={bridge.discovery.registeredNodes.size > 0 ? "success" : "attention"}
              />
            </div>
          }
        />

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
                className={`text-[11px] font-medium ${
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
                className={`mt-1 text-[10px] leading-4 ${
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

        <div className="mt-8 grid grid-cols-1 gap-3 min-[640px]:grid-cols-2 xl:grid-cols-4">
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

        <div className="mt-5 grid grid-cols-1 gap-5 min-[960px]:grid-cols-[minmax(0,1.45fr)_minmax(280px,.85fr)]">
          <div>
            <BuyInControl
              level={level}
              value={buyIn}
              availableTokens={availableTokens}
              locked={isMatching}
              onChange={setRequestedBuyIn}
            />
          </div>

          <aside className="rounded-[16px] border border-black/[.09] bg-[#f8faf8] p-5 shadow-[0_12px_30px_rgba(26,34,29,.05)]">
            <div className="flex items-center justify-between">
              <div className="grid size-9 place-items-center rounded-[10px] border border-black/[.06] bg-white shadow-sm">
                <Radio className="size-[17px] text-[#2b8acb]" strokeWidth={1.8} />
              </div>
              <span className="text-[10px] font-medium text-[var(--muted-light)]">{level.id}</span>
            </div>
            <h2 className="mt-5 text-[17px] font-semibold tracking-[-0.035em]">{t(level.nameKey)}</h2>
            <dl className="mt-4 space-y-2.5 text-[11px]">
              <div className="flex justify-between">
                <dt className="text-[var(--muted-light)]">{t("lobby.buyIn")}</dt>
                <dd className="font-medium tabular-nums">{formatTokens(buyIn)} Token</dd>
              </div>
              <div className="flex justify-between">
                <dt className="text-[var(--muted-light)]">{t("lobby.strategy")}</dt>
                <dd className="font-medium">{t("lobby.strategyValue")}</dd>
              </div>
              <div className="flex justify-between">
                <dt className="text-[var(--muted-light)]">{t("lobby.transport")}</dt>
                <dd className="font-medium">{t("lobby.transportValue")}</dd>
              </div>
            </dl>

            <Button
              variant={isMatching ? "danger" : "primary"}
              size="lg"
              className="mt-6 w-full"
              disabled={!canAutoMatch}
              onClick={toggleMatchmaking}
            >
              {isMatching ? <X className="size-4" /> : <Users className="size-4" />}
              {t(isMatching ? "lobby.stopSearch" : "lobby.quickSeat")}
            </Button>
            {bridge.identity === null ? (
              <p className="mt-2 text-center text-[9px] text-[#9a6c22]">{t("lobby.identityRequired")}</p>
            ) : null}
            {bridge.identity !== null && bridge.discovery.nodes.length === 0 ? (
              <p className="mt-2 text-center text-[9px] text-[#9a6c22]">
                {t("lobby.discoveryRequired")}
              </p>
            ) : null}
            {bridge.identity !== null &&
            bridge.discovery.nodes.length > 0 &&
            bridge.discovery.registeredNodes.size === 0 ? (
              <p className="mt-2 text-center text-[9px] text-[#9a6c22]">
                {t("lobby.discoveryConnecting")}
              </p>
            ) : null}
            <div className="my-3 flex items-center gap-3 text-[9px] text-[var(--muted-light)] before:h-px before:flex-1 before:bg-black/[.07] after:h-px after:flex-1 after:bg-black/[.07]">
              {t("lobby.or")}
            </div>
            <FriendRoomDialog
              level={level}
              buyIn={buyIn}
              inviteCode={bridge.friendInviteCode}
              identityReady={bridge.identity !== null}
              roomStatus={bridge.friendRoomStatus}
              sendCommand={sendCommand}
            />
            {message || bridge.lastWarning ? (
              <p className="mt-3 text-[10px] leading-4 text-[var(--muted)]">{bridge.lastWarning ?? message}</p>
            ) : null}
          </aside>
        </div>

        <div className="mt-5 flex items-start gap-3 rounded-[13px] border border-[#dce9df] bg-[#f8fbf8] px-4 py-3.5">
          <ShieldCheck className="mt-0.5 size-4 shrink-0 text-[#477d50]" strokeWidth={1.8} />
          <div>
            <p className="text-[11px] font-medium text-[#315e39]">{t("lobby.securityTitle")}</p>
            <p className="mt-1 text-[10px] leading-4 text-[#66806b]">
              {t("lobby.securityDescription")}
            </p>
          </div>
        </div>
      </div>
    </section>
  );
}
