import { useEffect, useRef, useState } from "react";
import type {
  BridgeSnapshot,
  CommandResult,
  HostCommand,
} from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import { ensurePlayerIdentity } from "./ensure-player-identity";

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
  sendConfirmedCommand: (command: HostCommand) => Promise<CommandResult>,
): AutoIdentityState {
  const { t } = useI18n();
  const activeAttempt = useRef<{
    readonly fingerprint: string;
    readonly controller: AbortController;
  } | null>(null);
  const [recoverySeed, setRecoverySeed] = useState<RecoverySeed | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(
    () => () => {
      activeAttempt.current?.controller.abort();
      activeAttempt.current = null;
    },
    [],
  );

  useEffect(() => {
    const fingerprint = bridge.accountBinding?.accountFingerprint ?? null;
    if (
      activeAttempt.current !== null &&
      activeAttempt.current.fingerprint !== fingerprint
    ) {
      activeAttempt.current.controller.abort();
      activeAttempt.current = null;
    }
    if (
      bridge.identity !== null ||
      !bridge.sidecarReady ||
      bridge.officialUsage.phase !== "ready" ||
      fingerprint === null ||
      activeAttempt.current?.fingerprint === fingerprint
    ) {
      return;
    }

    const controller = new AbortController();
    activeAttempt.current = { fingerprint, controller };
    const recoverySecret = generateRecoverySecret();
    const command = {
      type: "ensure_identity",
      recovery_secret: recoverySecret,
      device_label: defaultDeviceLabel(
        t("identity.windowsWorkstation"),
        t("identity.currentDeviceFallback"),
      ),
    } as const;
    void ensurePlayerIdentity(command, sendConfirmedCommand, { signal: controller.signal }).then(
      (outcome) => {
        if (activeAttempt.current?.controller !== controller) return;
        activeAttempt.current = null;
        if (outcome.status === "cancelled") return;
        if (outcome.status === "failed") {
          setError(outcome.error);
          return;
        }
        setError(null);
        setRecoverySeed({
          accountFingerprint: fingerprint,
          recoverySecret,
        });
      },
    );
  }, [bridge.accountBinding, bridge.identity, bridge.officialUsage.phase, bridge.sidecarReady, sendConfirmedCommand, t]);

  const recoveryKit =
    recoverySeed === null
      ? null
      : {
          ...recoverySeed,
          recoveryEnvelope: bridge.identity?.recoveryEnvelope ?? null,
        };

  return { recoveryKit, error };
}
