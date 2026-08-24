import { useCallback, useRef, useState } from "react";
import type { ConfirmedHostCommandSender } from "../../../core/bridge/contracts";
import { requestSafeLeave, type SafeLeaveOutcome } from "./request-safe-leave";

type SafeLeaveState =
  | { readonly status: "idle" }
  | { readonly status: "requesting" }
  | SafeLeaveOutcome;

interface SafeLeaveController {
  readonly state: SafeLeaveState;
  readonly isPending: boolean;
  readonly request: () => Promise<void>;
}

export function useSafeLeave(
  sendCommand: ConfirmedHostCommandSender,
  fallbackError: string,
): SafeLeaveController {
  const [state, setState] = useState<SafeLeaveState>({ status: "idle" });
  const requestLocked = useRef(false);

  const request = useCallback(async (): Promise<void> => {
    if (requestLocked.current) return;
    requestLocked.current = true;
    setState({ status: "requesting" });
    const outcome = await requestSafeLeave(sendCommand, fallbackError);
    if (outcome.status === "failed") requestLocked.current = false;
    setState(outcome);
  }, [fallbackError, sendCommand]);

  return {
    state,
    isPending: state.status === "requesting" || state.status === "confirmed",
    request,
  };
}
