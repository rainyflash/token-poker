import type { HandProtocolPhase } from "./contracts";

type ProtocolPhase = "key_exchange" | "shuffling" | "dealing";

export interface HandEventCursor {
  readonly tableId: string | null;
  readonly handNumber: number;
  readonly phase: HandProtocolPhase;
  readonly progressCompleted: number;
  readonly sequence: number;
}

export interface HandScope {
  readonly table_id: string;
  readonly hand_number: number;
}

export interface HandProgressScope extends HandScope {
  readonly phase: ProtocolPhase;
  readonly completed: number;
}

const PROTOCOL_PHASE_ORDER: Readonly<Record<ProtocolPhase, number>> = Object.freeze({
  key_exchange: 0,
  shuffling: 1,
  dealing: 2,
});

const TERMINAL_PHASES: ReadonlySet<HandProtocolPhase> = new Set([
  "settled",
  "receipt_consensus",
  "between_hands",
  "conflicted",
]);

export function isCurrentHand(cursor: HandEventCursor, event: HandScope): boolean {
  return cursor.tableId === event.table_id && cursor.handNumber === event.hand_number;
}

export function shouldAcceptHandStart(cursor: HandEventCursor, event: HandScope): boolean {
  if (cursor.tableId === null || cursor.phase === "idle") return true;
  return cursor.tableId === event.table_id && event.hand_number > cursor.handNumber;
}

export function shouldAcceptHandProgress(
  cursor: HandEventCursor,
  event: HandProgressScope,
): boolean {
  if (!isCurrentHand(cursor, event)) return false;
  if (!isProtocolPhase(cursor.phase)) return false;
  const currentOrder = PROTOCOL_PHASE_ORDER[cursor.phase];
  const incomingOrder = PROTOCOL_PHASE_ORDER[event.phase];
  if (incomingOrder < currentOrder) return false;
  return incomingOrder !== currentOrder || event.completed >= cursor.progressCompleted;
}

export function shouldAcceptHandReady(cursor: HandEventCursor, event: HandScope): boolean {
  return isCurrentHand(cursor, event) && !TERMINAL_PHASES.has(cursor.phase);
}

export function phaseAfterHandReady(phase: HandProtocolPhase): HandProtocolPhase {
  return isProtocolPhase(phase) || phase === "idle" ? "playing" : phase;
}

export function shouldAcceptHandState(
  cursor: HandEventCursor,
  event: HandScope & { readonly sequence: number },
): boolean {
  return (
    isCurrentHand(cursor, event) &&
    !TERMINAL_PHASES.has(cursor.phase) &&
    event.sequence >= cursor.sequence
  );
}

export function phaseAfterHandState(
  phase: HandProtocolPhase,
  awaitingReveal: boolean,
): HandProtocolPhase {
  if (phase === "interrupted") return phase;
  return awaitingReveal ? "revealing" : "playing";
}

function isProtocolPhase(phase: HandProtocolPhase): phase is ProtocolPhase {
  return Object.hasOwn(PROTOCOL_PHASE_ORDER, phase);
}
