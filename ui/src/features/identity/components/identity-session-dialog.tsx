import * as Dialog from "@radix-ui/react-dialog";
import { Cloud, KeyRound, X } from "lucide-react";
import { useState } from "react";
import type { CommandResult, HostCommand } from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import type { MessageKey } from "../../../core/i18n/messages";
import { Button } from "../../../shared/ui/button";

export type IdentityAction = "restore" | "remote";

const ACTION_PRESENTATION = {
  restore: {
    title: "identity.dialogRestoreTitle",
    description: "identity.dialogRestoreDescription",
    caution: "identity.dialogRestoreCaution",
    button: "identity.dialogRestoreButton",
    pending: "identity.dialogRestorePending",
    icon: KeyRound,
  },
  remote: {
    title: "identity.dialogRemoteTitle",
    description: "identity.dialogRemoteDescription",
    caution: "identity.dialogRemoteCaution",
    button: "identity.dialogRemoteButton",
    pending: "identity.dialogRemotePending",
    icon: Cloud,
  },
} as const satisfies Readonly<Record<IdentityAction, {
  readonly title: MessageKey;
  readonly description: MessageKey;
  readonly caution: MessageKey;
  readonly button: MessageKey;
  readonly pending: MessageKey;
  readonly icon: typeof KeyRound;
}>>;

interface IdentitySessionDialogProps {
  readonly action: IdentityAction | null;
  readonly onActionChange: (action: IdentityAction | null) => void;
  readonly onAccepted: (label: string) => void;
  readonly sendCommand: (command: HostCommand) => CommandResult;
}

export function IdentitySessionDialog({
  action,
  onActionChange,
  onAccepted,
  sendCommand,
}: IdentitySessionDialogProps) {
  const { t } = useI18n();
  const [customDeviceLabel, setCustomDeviceLabel] = useState<string | null>(null);
  const [recoverySecret, setRecoverySecret] = useState("");
  const [recoveryEnvelope, setRecoveryEnvelope] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const mode = action ?? "restore";
  const presentation = ACTION_PRESENTATION[mode];
  const ActionIcon = presentation.icon;
  const isManualRestore = mode === "restore";
  const deviceLabel = customDeviceLabel ?? t("identity.windowsWorkstation");

  const clearSensitiveFields = (): void => {
    setRecoverySecret("");
    setRecoveryEnvelope("");
  };

  const changeOpen = (open: boolean): void => {
    if (!open) {
      clearSensitiveFields();
      setCustomDeviceLabel(null);
      setMessage(null);
      onActionChange(null);
    }
  };

  const submit = (): void => {
    if (action === null) return;
    const normalizedLabel = deviceLabel.trim();
    if (new TextEncoder().encode(normalizedLabel).length > 80 || normalizedLabel.length === 0) {
      setMessage(t("identity.deviceNameInvalid"));
      return;
    }
    const secretLength = Array.from(recoverySecret).length;
    if (secretLength < 12 || secretLength > 256) {
      setMessage(t("identity.secretInvalid"));
      return;
    }
    if (isManualRestore && !recoveryEnvelope.trim().startsWith("THR1-")) {
      setMessage(t("identity.envelopeInvalid"));
      return;
    }

    const commands: Record<IdentityAction, HostCommand> = {
      restore: {
        type: "restore_identity",
        recovery_envelope: recoveryEnvelope.trim(),
        recovery_secret: recoverySecret,
        device_label: normalizedLabel,
      },
      remote: {
        type: "restore_remote_identity",
        recovery_secret: recoverySecret,
        device_label: normalizedLabel,
      },
    };
    const result = sendCommand(commands[action]);
    if (!result.ok) {
      setMessage(result.error);
      return;
    }
    clearSensitiveFields();
    setMessage(null);
    onAccepted(t(presentation.pending));
    onActionChange(null);
  };

  return (
    <Dialog.Root open={action !== null} onOpenChange={changeOpen}>
      <Dialog.Portal container={globalThis.__tokenHoldemPortalRoot}>
        <Dialog.Overlay className="fixed inset-0 z-[999999] bg-black/15 backdrop-blur-[2px] data-[state=open]:animate-in" />
        <Dialog.Content className="fixed left-1/2 top-1/2 z-[1000000] w-[460px] -translate-x-1/2 -translate-y-1/2 rounded-[18px] border border-black/[.10] bg-white p-5 text-[var(--ink)] shadow-[0_30px_90px_rgba(18,24,20,.22)] outline-none">
          <div className="flex items-start justify-between">
            <div className="flex items-start gap-3">
              <div className="grid size-9 place-items-center rounded-[10px] border border-[#dce9df] bg-[#f7fbf7] text-[#477d50]">
                <ActionIcon className="size-[17px]" />
              </div>
              <div>
                <Dialog.Title className="text-[17px] font-semibold tracking-[-0.035em]">
                  {t(presentation.title)}
                </Dialog.Title>
                <Dialog.Description className="mt-1 max-w-[330px] text-[10px] leading-4 text-[var(--muted)]">
                  {t(presentation.description)}
                </Dialog.Description>
              </div>
            </div>
            <Dialog.Close asChild>
              <Button variant="ghost" size="icon" className="size-8" aria-label={t("common.close")}>
                <X className="size-4" />
              </Button>
            </Dialog.Close>
          </div>

          <form
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
          >
            <div className="mt-5 space-y-4">
              {isManualRestore ? (
                <label className="block text-[10px] font-medium text-[var(--muted)]">
                  {t("identity.encryptedKit")}
                  <textarea
                    value={recoveryEnvelope}
                    onChange={(event) => setRecoveryEnvelope(event.target.value)}
                    placeholder="THR1-…"
                    className="mt-2 h-24 w-full resize-none rounded-[11px] border border-[var(--line-strong)] bg-white p-3 font-mono text-[10px] font-normal outline-none placeholder:text-[var(--muted-light)] focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
                  />
                </label>
              ) : null}

              <label className="block text-[10px] font-medium text-[var(--muted)]">
                {t("identity.currentDeviceName")}
                <input
                  autoComplete="off"
                  value={deviceLabel}
                  onChange={(event) => setCustomDeviceLabel(event.target.value)}
                  className="mt-2 h-10 w-full rounded-[10px] border border-[var(--line-strong)] px-3 text-[11px] font-normal outline-none focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
                />
              </label>

              <label className="block text-[10px] font-medium text-[var(--muted)]">
                {t("identity.recoveryPhrase")}
                <input
                  type="password"
                  autoComplete="current-password"
                  value={recoverySecret}
                  onChange={(event) => setRecoverySecret(event.target.value)}
                  className="mt-2 h-10 w-full rounded-[10px] border border-[var(--line-strong)] px-3 text-[11px] font-normal outline-none focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
                />
              </label>

            </div>

            <div className="mt-5 rounded-[10px] border border-[#eadfca] bg-[#fffcf6] px-3 py-2.5 text-[9px] leading-4 text-[#846b44]">
              {t(presentation.caution)}
            </div>
            {message ? <p className="mt-3 text-[10px] text-[#a14038]">{message}</p> : null}
            <Button type="submit" variant="primary" size="lg" className="mt-4 w-full">
              {t(presentation.button)}
            </Button>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
