import {
  Check,
  Copy,
  Fingerprint,
  KeyRound,
  Laptop,
  ShieldCheck,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { IdentitySnapshot } from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import { AccountAvatar } from "../../../shared/ui/account-avatar";
import { Button } from "../../../shared/ui/button";
import type { RecoveryKit } from "../model/use-auto-identity";

interface IdentitySecurityOverviewProps {
  readonly identity: IdentitySnapshot | null;
  readonly accountFingerprint: string | null;
  readonly displayName: string;
  readonly avatarUrl: string | null;
  readonly recoveryKit: RecoveryKit | null;
  readonly copied: boolean;
  readonly statusMessage: string | null;
  readonly onCopyRecoveryKit: () => void;
}

function compactKey(value: string): string {
  return value.length <= 20 ? value : `…${value.slice(-20)}`;
}

function compactPlayerId(value: string): string {
  return `TH-${value.slice(0, 4).toUpperCase()}-${value.slice(4, 8).toUpperCase()}-${value
    .slice(8, 12)
    .toUpperCase()}`;
}

interface DeviceDetailProps {
  readonly icon: LucideIcon;
  readonly label: string;
  readonly value: string;
  readonly metadata: string;
  readonly mono?: boolean;
}

function DeviceDetail({ icon: Icon, label, value, metadata, mono = false }: DeviceDetailProps) {
  return (
    <div className="grid grid-cols-[112px_minmax(0,1fr)] gap-x-4 gap-y-1.5 border-b border-[var(--line)] py-3.5 last:border-b-0 min-[680px]:grid-cols-[180px_minmax(0,1fr)_auto] min-[680px]:items-center min-[680px]:py-2.5">
      <dt className="flex items-center gap-3 text-[var(--muted)]">
        <Icon className="size-4" strokeWidth={1.65} />
        {label}
      </dt>
      <dd className="min-w-0 font-medium">{value}</dd>
      <dd
        className={`col-start-2 min-w-0 truncate text-[var(--muted-light)] min-[680px]:col-start-auto ${mono ? "font-mono" : ""}`}
      >
        {metadata}
      </dd>
    </div>
  );
}

export function IdentitySecurityOverview({
  identity,
  accountFingerprint,
  displayName,
  avatarUrl,
  recoveryKit,
  copied,
  statusMessage,
  onCopyRecoveryKit,
}: IdentitySecurityOverviewProps) {
  const { t, formatDate } = useI18n();
  const identityReady = identity !== null;
  const recoveryReady = recoveryKit?.recoveryEnvelope != null;

  return (
    <div>
      <section className="flex flex-col gap-5 border-b border-[var(--line)] pb-8 min-[680px]:flex-row min-[680px]:items-center">
        <AccountAvatar name={displayName} src={avatarUrl} className="size-20 text-[24px]" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2.5">
            <h1 className="truncate text-[28px] font-semibold tracking-[-0.045em]">{displayName}</h1>
            <ShieldCheck className="size-5 text-[var(--codex-blue-400)]" strokeWidth={1.8} />
          </div>
          <p className="mt-1.5 flex items-center gap-2 text-[12px] text-[var(--muted)]">
            <Fingerprint className="size-3.5" strokeWidth={1.7} />
            {t("identity.accountBound")}
          </p>
          <div className="mt-3 flex flex-wrap items-center gap-x-5 gap-y-2 text-[11px] text-[var(--muted-light)]">
            <code className="font-mono">
              {identityReady ? compactPlayerId(identity.playerId) : t("identity.creating")}
            </code>
            <span className="flex items-center gap-2 text-[var(--codex-green-500)]">
              <span className="size-2 rounded-full bg-current" />
              {t(identityReady ? "identity.healthy" : "identity.initializing")}
            </span>
          </div>
          {statusMessage ? (
            <p className="mt-3 text-[10px] leading-4 text-[var(--codex-red-500)]">
              {statusMessage}
            </p>
          ) : null}
        </div>
      </section>

      <section aria-labelledby="current-device-title" className="border-b border-[var(--line)] py-8">
        <h2 id="current-device-title" className="text-[18px] font-semibold tracking-[-0.025em]">
          {t("identity.currentDevice")}
        </h2>
        <dl className="mt-4 text-[12px]">
          <DeviceDetail
            icon={Laptop}
            label={t("identity.device")}
            value={identity?.deviceLabel ?? t("identity.windowsWorkstation")}
            metadata={t("identity.currentSession")}
          />
          <DeviceDetail
            icon={KeyRound}
            label={t("identity.sessionKey")}
            value={t(identityReady ? "identity.created" : "identity.generating")}
            metadata={identityReady ? compactKey(identity.devicePublicKey) : "—"}
            mono
          />
          <DeviceDetail
            icon={ShieldCheck}
            label={t("identity.deviceCertificate")}
            value={t(identityReady ? "identity.valid" : "identity.pendingIssue")}
            metadata={
              identityReady
                ? t("identity.validUntil", { date: formatDate(identity.certificateExpiresAtUnixMs) })
                : "—"
            }
          />
        </dl>
      </section>

      <section aria-labelledby="recovery-title" className="border-b border-[var(--line)] py-8">
        <h2 id="recovery-title" className="text-[18px] font-semibold tracking-[-0.025em]">
          {t("identity.recoverySecurity")}
        </h2>
        <div className="mt-5 flex flex-col gap-4 min-[760px]:flex-row min-[760px]:items-center">
          <div className="flex min-w-0 flex-1 items-start gap-3">
            <KeyRound className="mt-0.5 size-4 shrink-0 text-[var(--muted)]" strokeWidth={1.65} />
            <div>
              <p className="text-[12px] font-medium">{t("identity.recoveryBackup")}</p>
              <p className="mt-1 text-[10px] leading-5 text-[var(--muted-light)]">
                {recoveryReady
                  ? t("identity.recoveryReady")
                  : identityReady
                    ? t("identity.recoveryUnavailable")
                    : t("identity.recoveryPending")}
              </p>
            </div>
          </div>
          <Button disabled={!recoveryReady} onClick={onCopyRecoveryKit}>
            {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
            {t(copied ? "identity.backupCopied" : "identity.exportBackup")}
          </Button>
        </div>
        <div className="mt-5 flex items-center justify-between border-t border-[var(--line)] pt-4 text-[10px] text-[var(--muted-light)]">
          <span>{t("identity.boundFingerprint")}</span>
          <code className="max-w-[72%] truncate font-mono">
            {accountFingerprint ?? t("identity.awaitingBinding")}
          </code>
        </div>
      </section>

    </div>
  );
}
