import type { HandSnapshot, HostCommand } from "../../../core/bridge/contracts";

export function handActionScope(hand: HandSnapshot): string {
  return [hand.tableId ?? "", String(hand.handNumber), String(hand.sequence), hand.publicStateHash ?? ""].join("/");
}

export function createHandActionCommand(
  hand: HandSnapshot,
  action: "fold" | "check" | "call" | "raise",
  amount: number,
): Extract<HostCommand, { readonly type: "submit_action" }> | null {
  if (!hand.canAct || hand.sessionInterrupted || hand.tableId === null || hand.publicStateHash === null) return null;
  return {
    type: "submit_action",
    expected: { table_id: hand.tableId, hand_number: hand.handNumber,
      sequence: hand.sequence, public_state_hash: hand.publicStateHash },
    action,
    ...(action === "raise" ? { amount } : {}),
  };
}
