import type { CommandResult, HostCommand } from "../../../core/bridge/contracts";

export type EnsureIdentityCommand = Extract<HostCommand, { readonly type: "ensure_identity" }>;

export type IdentityEnsureOutcome =
  | { readonly status: "confirmed" }
  | { readonly status: "failed"; readonly error: string }
  | { readonly status: "cancelled" };

interface RetryOptions {
  readonly delaysMs?: readonly number[];
  readonly signal?: AbortSignal;
  readonly wait?: (delayMs: number, signal: AbortSignal | undefined) => Promise<boolean>;
}

export const IDENTITY_RETRY_DELAYS_MS = Object.freeze([0, 400, 1_200]);

export async function ensurePlayerIdentity(
  command: EnsureIdentityCommand,
  sendConfirmed: (command: EnsureIdentityCommand) => Promise<CommandResult>,
  options: RetryOptions = {},
): Promise<IdentityEnsureOutcome> {
  const delays = options.delaysMs ?? IDENTITY_RETRY_DELAYS_MS;
  const wait = options.wait ?? waitForDelay;
  let lastError = "玩家身份初始化失败";

  for (const delayMs of delays) {
    if (!(await wait(delayMs, options.signal))) return { status: "cancelled" };
    let result: CommandResult;
    try {
      result = await sendConfirmed(command);
    } catch (error: unknown) {
      lastError = error instanceof Error ? error.message : String(error);
      continue;
    }
    if (options.signal?.aborted === true) return { status: "cancelled" };
    if (result.ok) return { status: "confirmed" };
    lastError = result.error;
  }

  return { status: "failed", error: lastError };
}

function waitForDelay(delayMs: number, signal: AbortSignal | undefined): Promise<boolean> {
  if (signal?.aborted === true) return Promise.resolve(false);
  if (delayMs === 0) return Promise.resolve(true);
  return new Promise((resolve) => {
    const timeout = globalThis.setTimeout(() => finish(true), delayMs);
    const cancel = () => finish(false);
    const finish = (completed: boolean) => {
      globalThis.clearTimeout(timeout);
      signal?.removeEventListener("abort", cancel);
      resolve(completed);
    };
    signal?.addEventListener("abort", cancel, { once: true });
  });
}
