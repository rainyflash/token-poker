import type {
  ConfirmedHostCommandSender,
  CommandResult,
} from "../../../core/bridge/contracts";

export type SafeLeaveOutcome =
  | { readonly status: "confirmed" }
  | { readonly status: "failed"; readonly error: string };

export async function requestSafeLeave(
  sendCommand: ConfirmedHostCommandSender,
  fallbackError: string,
): Promise<SafeLeaveOutcome> {
  let result: CommandResult;
  try {
    result = await sendCommand({ type: "leave_table" });
  } catch (error: unknown) {
    return {
      status: "failed",
      error: error instanceof Error && error.message.length > 0 ? error.message : fallbackError,
    };
  }
  return result.ok
    ? { status: "confirmed" }
    : { status: "failed", error: result.error };
}
