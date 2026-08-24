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
        this.#clearHand();
        this.#slots.set(SLOT.handStarted, entry);
        break;
      case "hand_protocol_progress":
        this.#slots.set(SLOT.handProgress, entry);
        break;
      case "hand_ready":
        this.#slots.set(SLOT.handReady, entry);
        break;
      case "hand_state":
        this.#slots.set(SLOT.handState, entry);
        break;
      case "hand_left":
        this.#clearHand();
        break;
      default:
        if (POOL_PHASE_EVENTS.has(eventType)) this.#slots.set(SLOT.poolPhase, entry);
        else if (FRIEND_ROOM_EVENTS.has(eventType)) this.#slots.set(SLOT.friendRoom, entry);
        else if (HAND_TERMINAL_EVENTS.has(eventType)) this.#slots.set(SLOT.handTerminal, entry);
        else if (RECEIPT_EVENTS.has(eventType)) this.#slots.set(SLOT.receipt, entry);
        else if (INTERRUPTION_EVENTS.has(eventType)) this.#slots.set(SLOT.interruption, entry);
        break;
    }
  }

  merge(retainedEvents, afterSequence) {
    const bySequence = new Map();
    for (const entry of this.#slots.values()) {
      if (entry.sequence > afterSequence) bySequence.set(entry.sequence, entry);
    }
    for (const entry of retainedEvents) {
      if (entry.sequence > afterSequence) bySequence.set(entry.sequence, entry);
    }
    return [...bySequence.values()].sort((left, right) => left.sequence - right.sequence);
  }

  clear() {
    this.#slots.clear();
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
