import { useEffect, useRef, useState } from "react";
import type {
  BridgeSnapshot,
  CommandResult,
  HostCommand,
} from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";

export interface RecoveryKit {
  readonly accountFingerprint: string;
  readonly recoverySecret: string;
  readonly recoveryEnvelope: string | null;
}

interface AutoIdentityState {
  readonly recoveryKit: RecoveryKit | null;
  readonly error: string | null;
}

interface RecoverySeed {
  readonly accountFingerprint: string;
  readonly recoverySecret: string;
}

function generateRecoverySecret(): string {
  const entropy = globalThis.crypto.getRandomValues(new Uint8Array(32));
  const binary = String.fromCharCode(...entropy);
  return globalThis
    .btoa(binary)
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replace(/=+$/u, "");
}

function defaultDeviceLabel(windowsLabel: string, fallbackLabel: string): string {
  return globalThis.navigator.userAgent.includes("Windows")
    ? windowsLabel
    : fallbackLabel;
}

export function useAutoIdentity(
  bridge: BridgeSnapshot,
  sendCommand: (command: HostCommand) => CommandResult,
): AutoIdentityState {
  const { t } = useI18n();
  const attemptedFingerprint = useRef<string | null>(null);
  const [recoverySeed, setRecoverySeed] = useState<RecoverySeed | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const fingerprint = bridge.accountBinding?.accountFingerprint ?? null;
    if (
      bridge.identity !== null ||
      !bridge.sidecarReady ||
      bridge.officialUsage.phase !== "ready" ||
      fingerprint === null ||
      attemptedFingerprint.current === fingerprint
    ) {
      return;
    }

    attemptedFingerprint.current = fingerprint;
    const recoverySecret = generateRecoverySecret();
    const result = sendCommand({
      type: "ensure_identity",
      recovery_secret: recoverySecret,
      device_label: defaultDeviceLabel(
        t("identity.windowsWorkstation"),
        t("identity.currentDeviceFallback"),
      ),
    });
    if (!result.ok) {
      globalThis.queueMicrotask(() => setError(result.error));
      return;
    }
    globalThis.queueMicrotask(() => {
      setError(null);
      setRecoverySeed({
        accountFingerprint: fingerprint,
        recoverySecret,
      });
    });
  }, [bridge.accountBinding, bridge.identity, bridge.officialUsage.phase, bridge.sidecarReady, sendCommand, t]);

  const recoveryKit =
    recoverySeed === null
      ? null
      : {
          ...recoverySeed,
          recoveryEnvelope: bridge.identity?.recoveryEnvelope ?? null,
        };

  return { recoveryKit, error };
}
