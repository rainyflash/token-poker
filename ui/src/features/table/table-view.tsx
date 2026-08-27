import { AnimatePresence } from "motion/react";
import { History, LogOut, PanelRight, ShieldCheck, Wifi } from "lucide-react";
import { useState } from "react";
import type {
  BridgeSnapshot,
  CommandResult,
  ConfirmedHostCommandSender,
  HostCommand,
} from "../../core/bridge/contracts";
import { useI18n } from "../../core/i18n/use-i18n";
import type { MessageKey } from "../../core/i18n/messages";
import { projectPlayerProfile } from "../account/model/player-profile";
import { Button } from "../../shared/ui/button";
import { StatusPill } from "../../shared/ui/status-pill";
import { useSafeLeave } from "../session/model/use-safe-leave";
import { ActionConsole } from "./components/action-console";
import { HandInfoPanel } from "./components/hand-info-panel";
import { PokerTable } from "./components/poker-table";
import { hasConfirmedOpponent } from "./model/table-presence";

interface TableViewProps {
  readonly bridge: BridgeSnapshot;
  readonly sendCommand: (command: HostCommand) => CommandResult;
  readonly sendConfirmedCommand: ConfirmedHostCommandSender;
  readonly onOpenStatistics: () => void;
}

export function TableView({ bridge, sendCommand, sendConfirmedCommand, onOpenStatistics }: TableViewProps) {
  const { t, formatTokens } = useI18n();
  const [requestedBetAmount, setRequestedBetAmount] = useState(0);
  const [actionFeedback, setActionFeedback] = useState<{
    readonly sequence: number;
    readonly message: string;
  } | null>(null);
  const [showInfo, setShowInfo] = useState(false);
  const safeLeave = useSafeLeave(sendConfirmedCommand, t("bridge.commandFailed"));
  const hand = bridge.hand;
  const profile = projectPlayerProfile(
    bridge.tokenSnapshot,
    t(bridge.mode === "preview" ? "app.previewPlayer" : "app.codexPlayer"),
  );
  const localSeat = hand.seats.find((seat) => seat.seat === hand.localSeat);
  const heroStack = localSeat?.stack ?? hand.buyIns.at((hand.localSeat ?? 1) - 1) ?? 0;
  const playerCount = hand.players.length;
  const opponentConnected = hasConfirmedOpponent(bridge);
  const minimumBuyIn = hand.buyIns.length === 0 ? 0 : Math.min(...hand.buyIns);
  const maximumBuyIn = hand.buyIns.length === 0 ? 0 : Math.max(...hand.buyIns);
  const minimumRaise = hand.minimumRaiseTo;
  const maximumRaise = hand.maximumRaiseTo;
  const betAmount = Math.min(maximumRaise, Math.max(minimumRaise, requestedBetAmount));
  const leaving = safeLeave.isPending || bridge.room.localRole === "leaving";
  const canAct =
    hand.canAct && hand.pendingSequence === null && !hand.sessionInterrupted && !leaving;
  const awaitingReveal = hand.awaitingReveal;
  const localOutcome = hand.outcomes.find((outcome) => outcome.seat === hand.localSeat);
  const settledMessage =
    localOutcome === undefined
      ? t("table.settledZeroSum")
      : t("table.settledDelta", {
          delta: `${localOutcome.delta >= 0 ? "+" : "−"}${formatTokens(Math.abs(localOutcome.delta))}`,
        });
  const protocolMessage =
    hand.phase === "conflicted"
      ? t("table.conflict")
      : hand.phase === "interrupted"
      ? t("table.interrupted")
      : hand.phase === "receipt_consensus"
        ? t("table.collectingSignatures", { signed: hand.receiptSigned, required: hand.receiptRequired })
        : hand.phase === "between_hands"
          ? t("table.nextRoster", { hand: hand.handNumber + 1 })
          : hand.phase === "settled"
            ? settledMessage
      : hand.awaitingReveal
        ? t("table.verifyingReveal")
        : hand.pendingSequence === null
          ? null
          : t("table.confirmingAction", { sequence: hand.pendingSequence });
  const proofLabel =
    hand.phase === "key_exchange" || hand.phase === "shuffling" || hand.phase === "dealing"
      ? t("table.proofGenerating")
      : hand.phase === "receipt_consensus"
        ? t("table.receiptSigning")
        : hand.phase === "between_hands" || hand.phase === "settled"
        ? t("table.proofArchived")
        : hand.phase === "conflicted"
          ? t("table.protocolFrozen")
          : hand.phase === "interrupted"
          ? t("table.protocolPaused")
        : t("table.proofVerified");

  const actionMessage =
    leaving
      ? t("table.leaving")
      : safeLeave.state.status === "failed"
        ? safeLeave.state.error
      : actionFeedback?.sequence === hand.sequence
        ? actionFeedback.message
        : null;

  const submitAction = (action: "fold" | "check" | "call" | "raise"): void => {
    const command: HostCommand =
      action === "raise"
        ? { type: "submit_action", action, amount: betAmount }
        : { type: "submit_action", action };
    const result = sendCommand(command);
    const actionKeys: Readonly<Record<typeof action, MessageKey>> = {
      fold: "table.actionFolded",
      check: "table.actionChecked",
      call: "table.actionCalled",
      raise: "table.actionRaised",
    };
    setActionFeedback({
      sequence: hand.sequence,
      message: result.ok
        ? t(actionKeys[action], { amount: formatTokens(betAmount) })
        : result.error,
    });
  };

  const leaveTable = (): void => {
    if (leaving) return;
    void safeLeave.request();
  };

  return (
    <section className="flex h-full min-h-0 flex-col bg-[var(--canvas)]">
      <header className="flex h-[60px] shrink-0 items-center border-b border-[var(--line)] bg-white/88 px-3 backdrop-blur-xl sm:px-5">
        <div className="min-w-0">
          <h1 className="truncate text-[14px] font-semibold tracking-[-0.025em]">
            {t("table.title")}
            <span className="hidden min-[520px]:inline">
              <span className="mx-1.5 text-[var(--muted-light)]">·</span>{" "}
              {formatTokens(hand.smallBlind)}/{formatTokens(hand.bigBlind)}
              <span className="mx-1.5 text-[var(--muted-light)]">·</span>
              {t("table.playerCount", { count: playerCount })}
            </span>
          </h1>
          <p className="mt-0.5 text-[10px] text-[var(--muted-light)]">
            {t("table.buyInSummary", { minimum: formatTokens(minimumBuyIn), maximum: formatTokens(maximumBuyIn) })}
          </p>
        </div>
        <div className="ml-auto flex items-center gap-1">
          <span className="mr-2 hidden text-[11px] text-[var(--muted)] min-[720px]:inline">
            {t("table.handNumber", { number: hand.handNumber })}
          </span>
          <Button className="hidden min-[520px]:inline-flex" variant="ghost" size="icon" aria-label={t("table.history")} onClick={onOpenStatistics}>
            <History className="size-4" strokeWidth={1.7} />
          </Button>
          <Button variant="ghost" size="icon" onClick={() => setShowInfo((value) => !value)} aria-label={t("table.info")}>
            <PanelRight className="size-4" strokeWidth={1.7} />
          </Button>
          <Button className="ml-1 min-[520px]:ml-2" size="sm" disabled={leaving} onClick={leaveTable} aria-label={t("table.leave")}>
            <LogOut className="size-3.5" />
            <span className="hidden min-[520px]:inline">
              {t(leaving ? "table.leavingShort" : "table.leave")}
            </span>
          </Button>
        </div>
      </header>

      <div className="relative min-h-0 flex-1 overflow-hidden">
        <div className="absolute left-5 top-4 z-30 flex items-center gap-2">
          <StatusPill
            label={t(bridge.sidecarReady ? "table.p2pConnected" : "table.p2pConnecting")}
            tone={bridge.sidecarReady ? "success" : "attention"}
            dot
          />
          <StatusPill
            icon={Wifi}
            label={t(opponentConnected ? "table.opponentsConnected" : "table.waitingOpponent")}
          />
          <StatusPill icon={ShieldCheck} label={proofLabel} tone="success" />
        </div>

        <div className="table-stage-frame absolute inset-x-0">
          <PokerTable
            heroName={profile.displayName}
            heroAvatarUrl={profile.avatarUrl}
            heroStack={heroStack}
            actionMessage={
              actionMessage ?? protocolMessage
            }
            hand={hand}
          />
        </div>

        <ActionConsole
          amount={betAmount}
          minimum={minimumRaise}
          maximum={maximumRaise}
          currentBet={hand.currentBet}
          pot={hand.pot}
          toCall={hand.toCall}
          canAct={canAct}
          awaitingReveal={awaitingReveal}
          inactiveLabel={inactiveLabel(hand.phase, t)}
          onAmountChange={setRequestedBetAmount}
          onAction={submitAction}
        />

        <p className="absolute bottom-5 right-5 text-[9px] text-[var(--muted-light)]">
          {t("table.disclaimer")}
        </p>

        <AnimatePresence>
          {showInfo ? <HandInfoPanel bridge={bridge} onClose={() => setShowInfo(false)} /> : null}
        </AnimatePresence>
      </div>
    </section>
  );
}

function inactiveLabel(
  phase: BridgeSnapshot["hand"]["phase"],
  t: (key: MessageKey) => string,
): string | undefined {
  const labels: Partial<Record<BridgeSnapshot["hand"]["phase"], MessageKey>> = {
    settled: "table.inactiveSettled",
    receipt_consensus: "table.inactiveReceipt",
    between_hands: "table.inactiveRoster",
    conflicted: "table.inactiveConflict",
    interrupted: "table.inactiveInterrupted",
    key_exchange: "table.inactiveKeys",
    shuffling: "table.inactiveShuffle",
    dealing: "table.inactiveDeal",
  };
  const key = labels[phase];
  return key === undefined ? undefined : t(key);
}
