import { useEffect, useRef, useState } from "react";
import type {
  BridgeSnapshot,
  CommandResult,
  HostCommand,
} from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import { ensurePlayerIdentity } from "./ensure-player-identity";

export interface RecoveryKit {
  readonly playerId: string;
  readonly accountFingerprint: string;
  readonly recoverySecret: string;
  readonly recoveryEnvelope: string | null;
}

interface AutoIdentityState {
  readonly recoveryKit: RecoveryKit | null;
  readonly error: string | null;
}

interface RecoverySeed {
  readonly recoveryEnvelope: string;
  readonly playerId: string;
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
  const fingerprint = bridge.accountBinding?.accountFingerprint ?? null;
  const playerId = bridge.identity?.playerId ?? null;

  useEffect(
    () => () => {
      activeAttempt.current?.controller.abort();
      activeAttempt.current = null;
    },
    [],
  );

  useEffect(() => {
    if (
      activeAttempt.current !== null &&
      activeAttempt.current.fingerprint !== fingerprint
    ) {
      activeAttempt.current.controller.abort();
      activeAttempt.current = null;
    }
    if (
      playerId !== null ||
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
      expected_account_fingerprint: fingerprint,
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
        setRecoverySeed(outcome.identity.recoverySecretConfirmed ? {
          recoveryEnvelope: outcome.identity.recoveryEnvelope,
          playerId: outcome.identity.playerId,
          accountFingerprint: fingerprint,
          recoverySecret,
        } : null);
      },
    );
  }, [fingerprint, playerId, bridge.officialUsage.phase, bridge.sidecarReady, sendConfirmedCommand, t]);

  const recoveryKit =
    recoverySeed === null || recoverySeed.playerId !== playerId || recoverySeed.accountFingerprint !== fingerprint
      ? null
      : recoverySeed;

  return { recoveryKit, error };
}
