import type { TokenSnapshot } from "../../../core/bridge/contracts";

export interface PlayerProfile {
  readonly displayName: string;
  readonly avatarUrl: string | null;
  readonly accountHandle: string | null;
}

export function projectPlayerProfile(
  snapshot: TokenSnapshot | null,
  fallbackDisplayName: string,
): PlayerProfile {
  const accountHandle = snapshot?.username?.trim() || null;
  const displayName = snapshot?.displayName?.trim() || accountHandle || fallbackDisplayName;

  return {
    displayName,
    avatarUrl: snapshot?.avatarUrl ?? null,
    accountHandle,
  };
}
