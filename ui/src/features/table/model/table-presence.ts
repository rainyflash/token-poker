import type { BridgeSnapshot } from "../../../core/bridge/contracts";

export function hasConfirmedOpponent(
  bridge: Pick<BridgeSnapshot, "room" | "hand">,
): boolean {
  return bridge.room.seats.length > 1 || bridge.hand.players.length > 1;
}
