import { Fingerprint, RadioTower, ScrollText, ShieldCheck, X } from "lucide-react";
import { motion } from "motion/react";
import type { BridgeSnapshot } from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import { Button } from "../../../shared/ui/button";
import { hasConfirmedOpponent } from "../model/table-presence";

interface HandInfoPanelProps {
  readonly bridge: BridgeSnapshot;
  readonly onClose: () => void;
}

export function HandInfoPanel({ bridge, onClose }: HandInfoPanelProps) {
  const { t } = useI18n();
  const hand = bridge.hand;
  const opponentConnected = hasConfirmedOpponent(bridge);
  const proofValue =
    hand.phase === "settled" || hand.phase === "receipt_consensus" || hand.phase === "between_hands"
      ? t("handInfo.proofSealed")
      : hand.phase === "playing" || hand.phase === "revealing"
        ? t("handInfo.proofVerified")
        : t("handInfo.proofWaiting");
  const rows = [
    {
      icon: ScrollText,
      label: t("handInfo.receipt"),
      value:
        hand.receiptStatus === "signing"
          ? t("handInfo.signatureCount", { signed: hand.receiptSigned, required: hand.receiptRequired })
          : hand.receiptId === null
            ? t("handInfo.awaitingSettlement")
            : t("handInfo.coSigned", { id: hand.receiptId.slice(0, 10) }),
      tone: hand.receiptId === null ? "text-[var(--muted)]" : "text-[#3b7445]",
    },
    {
      icon: ShieldCheck,
      label: t("handInfo.shuffleProof"),
      value: proofValue,
      tone: "text-[#3b7445]",
    },
    {
      icon: RadioTower,
      label: t("handInfo.transport"),
      value: t(opponentConnected ? "handInfo.connected" : "handInfo.waitingConnection"),
      tone: "text-[var(--ink)]",
    },
    {
      icon: Fingerprint,
      label: t("handInfo.tokenSnapshot"),
      value: t(bridge.accountBinding?.peerVerifiable === true ? "handInfo.peerVerifiable" : "handInfo.officialUsage"),
      tone: "text-[#916821]",
    },
  ] as const;

  return (
    <motion.aside
      initial={{ x: 30, opacity: 0 }}
      animate={{ x: 0, opacity: 1 }}
      exit={{ x: 24, opacity: 0 }}
      transition={{ type: "spring", stiffness: 380, damping: 34 }}
      className="absolute right-3 top-3 z-50 max-h-[calc(100%-24px)] w-[min(340px,calc(100%-24px))] overflow-y-auto rounded-[16px] border border-[var(--line-strong)] bg-white p-5 shadow-[var(--codex-shadow-2xl)]"
    >
      <div className="flex items-center justify-between">
        <div>
          <p className="text-xs font-medium text-[var(--muted)]">
            {t("table.handNumber", { number: hand.handNumber })}
          </p>
          <h2 className="mt-1 text-[16px] font-semibold tracking-[-0.03em]">{t("handInfo.title")}</h2>
        </div>
        <Button variant="ghost" size="icon" className="size-8" onClick={onClose} aria-label={t("handInfo.close")}>
          <X className="size-4" />
        </Button>
      </div>

      <div className="mt-5 space-y-1">
        {rows.map((row) => {
          const Icon = row.icon;
          return (
            <div key={row.label} className="flex items-center gap-3 rounded-[10px] px-2 py-3 hover:bg-black/[.025]">
              <div className="grid size-8 place-items-center rounded-[8px] border border-black/[.06] bg-[var(--surface-subtle)]">
                <Icon className="size-4 text-[var(--muted)]" strokeWidth={1.7} />
              </div>
              <div>
                <p className="text-xs text-[var(--muted)]">{row.label}</p>
                <p className={`mt-1 text-[13px] font-medium ${row.tone}`}>{row.value}</p>
              </div>
            </div>
          );
        })}
      </div>

      <div className="mt-5 border-t border-black/[.06] pt-4">
        <p className="text-xs font-medium text-[var(--muted)]">{t("handInfo.transcript")}</p>
        <code className="mt-2 block break-all rounded-[9px] bg-[var(--surface-subtle)] p-3 font-mono text-xs leading-5 text-[var(--muted)]">
          {hand.transcriptHash ?? t("handInfo.noTranscript")}
        </code>
        <p className="mt-3 text-xs leading-5 text-[var(--muted)]">
          {t("handInfo.receiptRule")}
        </p>
        <p className="mt-3 text-xs text-[var(--muted)]">{t("table.disclaimer")}</p>
      </div>
    </motion.aside>
  );
}
