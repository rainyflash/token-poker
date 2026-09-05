import { ChevronDown, Cloud, KeyRound, Settings2 } from "lucide-react";
import { useState } from "react";
import type {
  BridgeSnapshot,
  CommandResult,
  ConfirmedHostCommandSender,
  HostCommand,
} from "../../core/bridge/contracts";
import { useI18n } from "../../core/i18n/use-i18n";
import { projectPlayerProfile } from "../account/model/player-profile";
import { ArchiveNodeConfig } from "./components/archive-node-config";
import { DiscoveryNodeConfig } from "./components/discovery-node-config";
import { IdentitySecurityOverview } from "./components/identity-security-overview";
import {
  IdentitySessionDialog,
  type IdentityAction,
} from "./components/identity-session-dialog";
import { VolunteerNetworkPanel } from "./components/volunteer-network-panel";
import type { RecoveryKit } from "./model/use-auto-identity";

interface IdentityViewProps {
  readonly bridge: BridgeSnapshot;
  readonly recoveryKit: RecoveryKit | null;
  readonly autoIdentityError: string | null;
  readonly sendCommand: (command: HostCommand) => CommandResult;
  readonly sendConfirmedCommand: ConfirmedHostCommandSender;
}

interface RecoveryExport {
  readonly schema: "token-holdem/recovery-kit/v1";
  readonly playerId: string;
  readonly accountFingerprint: string;
  readonly recoverySecret: string;
  readonly recoveryEnvelope: string;
}

function recoveryExport(
  bridge: BridgeSnapshot,
  recoveryKit: RecoveryKit | null,
): RecoveryExport | null {
  if (bridge.identity === null || recoveryKit?.recoveryEnvelope == null ||
      recoveryKit.playerId !== bridge.identity.playerId ||
      recoveryKit.accountFingerprint !== bridge.accountBinding?.accountFingerprint) return null;

  return {
    schema: "token-holdem/recovery-kit/v1",
    playerId: bridge.identity.playerId,
    accountFingerprint: recoveryKit.accountFingerprint,
    recoverySecret: recoveryKit.recoverySecret,
    recoveryEnvelope: recoveryKit.recoveryEnvelope,
  };
}

export function IdentityView({
  bridge,
  recoveryKit,
  autoIdentityError,
  sendCommand,
  sendConfirmedCommand,
}: IdentityViewProps) {
  const { t } = useI18n();
  const profile = projectPlayerProfile(
    bridge.tokenSnapshot,
    t(bridge.mode === "preview" ? "app.previewPlayer" : "app.codexPlayer"),
  );
  const [copied, setCopied] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [identityAction, setIdentityAction] = useState<IdentityAction | null>(null);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);

  const copyRecoveryKit = (): void => {
    const payload = recoveryExport(bridge, recoveryKit);
    if (payload === null) {
      setLocalError(t("identity.noRecoveryExport"));
      return;
    }

    void globalThis.navigator.clipboard
      .writeText(JSON.stringify(payload, null, 2))
      .then(() => {
        setCopied(true);
        setLocalError(null);
        globalThis.setTimeout(() => setCopied(false), 2_000);
      })
      .catch((error: unknown) =>
        setLocalError(
          t("identity.copyFailed", {
            reason: error instanceof Error ? error.message : t("identity.clipboardDenied"),
          }),
        ),
      );
  };

  const identityStatus =
    localError ??
    autoIdentityError ??
    bridge.officialUsage.error ??
    statusMessage;

  return (
    <section className="h-full overflow-y-auto bg-white">
      <div className="min-h-full w-full px-[clamp(24px,4vw,64px)] py-[clamp(24px,4vh,44px)]">
        <IdentitySecurityOverview
          identity={bridge.identity}
          accountFingerprint={bridge.accountBinding?.accountFingerprint ?? null}
          displayName={profile.displayName}
          avatarUrl={profile.avatarUrl}
          recoveryKit={recoveryKit}
          copied={copied}
          statusMessage={identityStatus}
          onCopyRecoveryKit={copyRecoveryKit}
        />

        <details className="group border-b border-[var(--line)]">
          <summary className="flex cursor-pointer list-none items-center gap-3 py-5 text-[12px] font-medium text-[var(--muted)] outline-none transition-colors hover:text-[var(--ink)] focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] [&::-webkit-details-marker]:hidden">
            <Settings2 className="size-4" strokeWidth={1.65} />
            <span>{t("identity.advanced")}</span>
            <span className="text-[10px] font-normal text-[var(--muted-light)]">
              {t("identity.advancedDescription")}
            </span>
            <ChevronDown className="ml-auto size-4 transition-transform duration-200 group-open:rotate-180" strokeWidth={1.65} />
          </summary>

          <div className="pb-8">
            <div className="grid gap-3 rounded-[14px] border border-[var(--line)] bg-[var(--surface-subtle)] p-4 min-[680px]:grid-cols-2">
              <button
                type="button"
                onClick={() => setIdentityAction("restore")}
                className="flex items-center gap-3 rounded-[11px] border border-[var(--line)] bg-white p-3.5 text-left outline-none transition-colors hover:bg-[var(--codex-gray-50)] focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)]"
              >
                <KeyRound className="size-4 text-[var(--muted)]" strokeWidth={1.65} />
                <span>
                  <span className="block text-[11px] font-medium">{t("identity.restoreBackup")}</span>
                  <span className="mt-1 block text-[9px] text-[var(--muted-light)]">{t("identity.restoreBackupDescription")}</span>
                </span>
              </button>
              <button
                type="button"
                onClick={() => setIdentityAction("remote")}
                className="flex items-center gap-3 rounded-[11px] border border-[var(--line)] bg-white p-3.5 text-left outline-none transition-colors hover:bg-[var(--codex-gray-50)] focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)]"
              >
                <Cloud className="size-4 text-[var(--muted)]" strokeWidth={1.65} />
                <span>
                  <span className="block text-[11px] font-medium">{t("identity.restoreRemote")}</span>
                  <span className="mt-1 block text-[9px] text-[var(--muted-light)]">{t("identity.restoreRemoteDescription")}</span>
                </span>
              </button>
            </div>

            <VolunteerNetworkPanel
              volunteer={bridge.volunteer}
              sidecarReady={bridge.sidecarReady}
              sendCommand={sendCommand}
            />
            <DiscoveryNodeConfig
              discovery={bridge.discovery}
              sidecarReady={bridge.sidecarReady}
              sendCommand={sendCommand}
            />
            <ArchiveNodeConfig
              archive={bridge.archive}
              sidecarReady={bridge.sidecarReady}
              sendCommand={sendCommand}
            />
          </div>
        </details>
      </div>

      <IdentitySessionDialog
        key={bridge.accountBinding?.accountFingerprint ?? "unbound"}
        accountFingerprint={bridge.accountBinding?.accountFingerprint ?? null}
        action={identityAction}
        onActionChange={setIdentityAction}
        onAccepted={setStatusMessage}
        sendConfirmedCommand={sendConfirmedCommand}
      />
    </section>
  );
}
