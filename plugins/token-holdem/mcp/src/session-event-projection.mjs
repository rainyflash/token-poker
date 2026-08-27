const SLOT = Object.freeze({
  identity: "identity",
  pool: "pool",
  poolDirectory: "pool-directory",
  poolPhase: "pool-phase",
  friendRoom: "friend-room",
  roomEntered: "room-entered",
  roomSnapshot: "room-snapshot",
  membership: "membership",
  roster: "roster",
  nextHand: "next-hand",
  safeLeave: "safe-leave",
  handStarted: "hand-started",
  handProgress: "hand-progress",
  handReady: "hand-ready",
  handState: "hand-state",
  handTerminal: "hand-terminal",
  receipt: "receipt",
  interruption: "interruption",
});

const POOL_PHASE_EVENTS = new Set([
  "pool_joining_table",
  "pool_join_attempt_expired",
  "pool_creating_table",
  "pool_table_joined",
]);

const FRIEND_ROOM_EVENTS = new Set([
  "friend_room_created",
  "friend_room_joining",
  "friend_room_joined",
]);

const HAND_TERMINAL_EVENTS = new Set([
  "hand_action_conflict",
  "hand_settled",
]);

const RECEIPT_EVENTS = new Set([
  "receipt_consensus_progress",
  "receipt_finalized",
]);

const INTERRUPTION_EVENTS = new Set([
  "hand_session_interrupted",
  "hand_session_resumed",
]);

const HAND_PROGRESS_ORDER = Object.freeze({
  key_exchange: 0,
  shuffling: 1,
  dealing: 2,
});

export class SessionEventProjection {
  #slots = new Map();

  observe(entry) {
    const eventType = entry?.event?.type;
    if (typeof eventType !== "string") return;

    switch (eventType) {
      case "identity_ready":
        this.#slots.set(SLOT.identity, entry);
        break;
      case "pool_joined":
        this.#clearPool();
        this.#slots.set(SLOT.pool, entry);
        break;
      case "pool_directory_updated":
        this.#slots.set(SLOT.poolDirectory, entry);
        break;
      case "pool_cancelled":
        this.#clearPool();
        break;
      case "room_entered":
        this.#clearRoom();
        this.#clearHand();
        this.#slots.set(SLOT.roomEntered, entry);
        break;
      case "room_snapshot":
        this.#slots.set(SLOT.roomSnapshot, entry);
        if (entry.event.local_role !== "leaving") this.#slots.delete(SLOT.safeLeave);
        break;
      case "membership_confirmation":
        this.#slots.set(SLOT.membership, entry);
        break;
      case "hand_roster_confirmation":
        this.#slots.set(SLOT.roster, entry);
        break;
      case "next_hand_ready":
        this.#slots.set(SLOT.nextHand, entry);
        break;
      case "safe_leave_requested":
      case "safe_leave_forced":
        this.#slots.set(SLOT.safeLeave, entry);
        break;
      case "safe_leave_completed":
        this.#clearPool();
        this.#clearRoom();
        this.#clearHand();
        break;
      case "room_closed":
        this.#clearRoom();
        this.#clearHand();
        break;
      case "hand_protocol_started":
        if (!this.#shouldReplaceHand(entry)) break;
        this.#clearHand();
        this.#slots.set(SLOT.handStarted, entry);
        break;
      case "hand_protocol_progress":
        if (!this.#shouldStoreHandProgress(entry)) break;
        this.#slots.set(SLOT.handProgress, entry);
        break;
      case "hand_ready":
        if (!this.#belongsToProjectedHand(entry)) break;
        this.#slots.delete(SLOT.handProgress);
        this.#slots.set(SLOT.handReady, entry);
        break;
      case "hand_state":
        if (!this.#belongsToProjectedHand(entry)) break;
        this.#slots.delete(SLOT.handProgress);
        this.#slots.set(SLOT.handState, entry);
        break;
      case "hand_left":
      case "hand_aborted_for_leave":
        this.#clearHand();
        break;
      default:
        if (POOL_PHASE_EVENTS.has(eventType)) this.#slots.set(SLOT.poolPhase, entry);
        else if (FRIEND_ROOM_EVENTS.has(eventType)) this.#slots.set(SLOT.friendRoom, entry);
        else if (HAND_TERMINAL_EVENTS.has(eventType) && this.#belongsToProjectedHand(entry)) {
          this.#slots.delete(SLOT.handProgress);
          this.#slots.set(SLOT.handTerminal, entry);
        }
        else if (RECEIPT_EVENTS.has(eventType) && this.#belongsToProjectedHand(entry)) {
          this.#slots.set(SLOT.receipt, entry);
        }
        else if (INTERRUPTION_EVENTS.has(eventType) && this.#belongsToProjectedHand(entry)) {
          this.#slots.set(SLOT.interruption, entry);
        }
        break;
    }
  }

  merge(retainedEvents, afterSequence) {
    const bySequence = new Map();
    for (const entry of this.snapshot()) {
      if (entry.sequence > afterSequence) bySequence.set(entry.sequence, entry);
    }
    for (const entry of retainedEvents) {
      if (entry.sequence > afterSequence) bySequence.set(entry.sequence, entry);
    }
    return [...bySequence.values()].sort((left, right) => left.sequence - right.sequence);
  }

  snapshot() {
    return Object.freeze(
      [...this.#slots.values()].sort((left, right) => left.sequence - right.sequence),
    );
  }

  clear() {
    this.#slots.clear();
  }

  #shouldReplaceHand(entry) {
    const current = this.#slots.get(SLOT.handStarted);
    if (current === undefined) return true;
    const currentScope = handScope(current);
    const incomingScope = handScope(entry);
    if (currentScope === null || incomingScope === null) return true;
    return (
      currentScope.tableId === incomingScope.tableId &&
      incomingScope.handNumber > currentScope.handNumber
    );
  }

  #belongsToProjectedHand(entry) {
    const started = this.#slots.get(SLOT.handStarted);
    if (started === undefined) return true;
    const startedScope = handScope(started);
    const incomingScope = handScope(entry);
    return startedScope === null || incomingScope === null || sameHand(startedScope, incomingScope);
  }

  #shouldStoreHandProgress(entry) {
    if (!this.#belongsToProjectedHand(entry)) return false;
    const scope = handScope(entry);
    if (scope !== null) {
      for (const slot of [SLOT.handReady, SLOT.handState, SLOT.handTerminal]) {
        const projected = this.#slots.get(slot);
        const projectedScope = projected === undefined ? null : handScope(projected);
        if (projectedScope !== null && sameHand(scope, projectedScope)) return false;
      }
    }
    const current = this.#slots.get(SLOT.handProgress);
    if (current === undefined || !sameScopedEntry(current, entry)) return true;
    const currentOrder = HAND_PROGRESS_ORDER[current.event.phase];
    const incomingOrder = HAND_PROGRESS_ORDER[entry.event.phase];
    if (!Number.isInteger(currentOrder) || !Number.isInteger(incomingOrder)) return true;
    if (incomingOrder < currentOrder) return false;
    return incomingOrder !== currentOrder || entry.event.completed >= current.event.completed;
  }

  #clearPool() {
    this.#slots.delete(SLOT.pool);
    this.#slots.delete(SLOT.poolDirectory);
    this.#slots.delete(SLOT.poolPhase);
  }

  #clearRoom() {
    this.#slots.delete(SLOT.friendRoom);
    this.#slots.delete(SLOT.roomEntered);
    this.#slots.delete(SLOT.roomSnapshot);
    this.#slots.delete(SLOT.membership);
    this.#slots.delete(SLOT.roster);
    this.#slots.delete(SLOT.nextHand);
    this.#slots.delete(SLOT.safeLeave);
  }

  #clearHand() {
    this.#slots.delete(SLOT.handStarted);
    this.#slots.delete(SLOT.handProgress);
    this.#slots.delete(SLOT.handReady);
    this.#slots.delete(SLOT.handState);
    this.#slots.delete(SLOT.handTerminal);
    this.#slots.delete(SLOT.receipt);
    this.#slots.delete(SLOT.interruption);
  }
}

function handScope(entry) {
  const tableId = entry?.event?.table_id;
  const handNumber = entry?.event?.hand_number;
  return typeof tableId === "string" && Number.isSafeInteger(handNumber)
    ? { tableId, handNumber }
    : null;
}

function sameHand(left, right) {
  return left.tableId === right.tableId && left.handNumber === right.handNumber;
}

function sameScopedEntry(left, right) {
  const leftScope = handScope(left);
  const rightScope = handScope(right);
  return leftScope !== null && rightScope !== null && sameHand(leftScope, rightScope);
}
