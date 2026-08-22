import type { BridgeSnapshot } from "../core/bridge/contracts";

export type PrimarySurface = "lobby" | "matching" | "table";
export type AppSubview = "statistics" | "identity";

export function projectPrimarySurface(bridge: BridgeSnapshot): PrimarySurface {
  if (bridge.hand.tableId !== null && bridge.hand.phase !== "idle") {
    return "table";
  }
  if (
    bridge.pool.status !== "idle" ||
    bridge.room.tableId !== null ||
    bridge.friendRoomStatus !== "idle"
  ) {
    return "matching";
  }
  return "lobby";
}
