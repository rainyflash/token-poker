import type {
  AccountBindingSnapshot,
  HandCardSnapshot,
  OfficialUsageState,
  SidecarEvent,
  TokenSnapshot,
} from "./contracts";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isString(value: unknown): value is string {
  return typeof value === "string";
}

function isNonNegativeSafeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0;
}

function isPositiveSafeInteger(value: unknown): value is number {
  return isNonNegativeSafeInteger(value) && value > 0;
}

function isSafeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every(isString);
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isNullablePositiveSafeInteger(value: unknown): value is number | null {
  return value === null || isPositiveSafeInteger(value);
}

function isNullableNonNegativeSafeInteger(value: unknown): value is number | null {
  return value === null || isNonNegativeSafeInteger(value);
}

function isStringMember<const T extends readonly string[]>(
  value: unknown,
  allowed: T,
): value is T[number] {
  return typeof value === "string" && allowed.some((candidate) => candidate === value);
}

function isHandCard(value: unknown): value is HandCardSnapshot {
  return (
    isRecord(value) &&
    isPositiveSafeInteger(value.rank) &&
    value.rank >= 2 &&
    value.rank <= 14 &&
    (value.suit === "club" ||
      value.suit === "diamond" ||
      value.suit === "heart" ||
      value.suit === "spade")
  );
}

function isHandCardArray(value: unknown): value is HandCardSnapshot[] {
  return Array.isArray(value) && value.every(isHandCard);
}

function isHandSeatArray(value: unknown): value is {
  seat: number;
  player_id: string;
  stack: number;
  committed: number;
  status: "active" | "folded" | "all_in";
}[] {
  return (
    Array.isArray(value) &&
    value.every(
      (seat) =>
        isRecord(seat) &&
        isPositiveSafeInteger(seat.seat) &&
        isString(seat.player_id) &&
        isNonNegativeSafeInteger(seat.stack) &&
        isNonNegativeSafeInteger(seat.committed) &&
        (seat.status === "active" || seat.status === "folded" || seat.status === "all_in"),
    )
  );
}

function isRoomSeatArray(value: unknown): value is {
  physical_seat: number;
  player_id: string;
  buy_in: number;
}[] {
  return (
    Array.isArray(value) &&
    value.length <= 6 &&
    value.every(
      (seat) =>
        isRecord(seat) &&
        isPositiveSafeInteger(seat.physical_seat) &&
        seat.physical_seat <= 6 &&
        isString(seat.player_id) &&
        isPositiveSafeInteger(seat.buy_in),
    )
  );
}

function isHandOutcomeArray(value: unknown): value is {
  seat: number;
  player_id: string;
  starting_stack: number;
  ending_stack: number;
  delta: number;
}[] {
  return (
    Array.isArray(value) &&
    value.every(
      (outcome) =>
        isRecord(outcome) &&
        isPositiveSafeInteger(outcome.seat) &&
        isString(outcome.player_id) &&
        isNonNegativeSafeInteger(outcome.starting_stack) &&
        isNonNegativeSafeInteger(outcome.ending_stack) &&
        Number.isSafeInteger(outcome.delta),
    )
  );
}

function isRecentHandArray(value: unknown): value is {
  address: string;
  receipt_id: string;
  hand_number: number;
  level_id: string;
  players: number;
  settled_at_unix_ms: number;
  delta: number;
  archived: boolean;
}[] {
  return (
    Array.isArray(value) &&
    value.every(
      (hand) =>
        isRecord(hand) &&
        isString(hand.address) &&
        isString(hand.receipt_id) &&
        isPositiveSafeInteger(hand.hand_number) &&
        isString(hand.level_id) &&
        isPositiveSafeInteger(hand.players) &&
        isNonNegativeSafeInteger(hand.settled_at_unix_ms) &&
        isSafeInteger(hand.delta) &&
        typeof hand.archived === "boolean",
    )
  );
}

export function parseTokenSnapshot(value: unknown): TokenSnapshot | null {
  if (!isRecord(value)) return null;
  const lifetimeTokens = value.lifetime_tokens;
  const username = value.username;
  const displayName = value.display_name;
  const avatarUrl = value.avatar_url ?? null;
  const observedAtUnixMs = value.observed_at_unix_ms;
  const source = value.source ?? "legacy_agent_profile_observation";
  if (
    !isNonNegativeSafeInteger(lifetimeTokens) ||
    !isNullableString(username) ||
    !isNullableString(displayName) ||
    !isNullableString(avatarUrl) ||
    !isNonNegativeSafeInteger(observedAtUnixMs) ||
    !isStringMember(source, [
      "codex_app_server_account_usage",
      "shared_runtime_replay",
      "legacy_agent_profile_observation",
      "preview",
    ] as const)
  ) {
    return null;
  }
  return { lifetimeTokens, username, displayName, avatarUrl, observedAtUnixMs, source };
}

export function parseAccountBinding(value: unknown): AccountBindingSnapshot | null {
  if (
    !isRecord(value) ||
    !isString(value.account_fingerprint) ||
    value.account_fingerprint.length === 0 ||
    typeof value.peer_verifiable !== "boolean"
  ) {
    return null;
  }
  return {
    accountFingerprint: value.account_fingerprint,
    peerVerifiable: value.peer_verifiable,
  };
}

export function parseOfficialUsageState(value: unknown): OfficialUsageState | null {
  if (!isRecord(value) || !isStringMember(value.phase, ["idle", "loading", "ready", "error"] as const)) {
    return null;
  }
  const error = value.error ?? null;
  if (!isNullableString(error) || (value.phase === "error" && (error === null || error.length === 0))) {
    return null;
  }
  return { phase: value.phase, error: value.phase === "error" ? error : null };
}

export function parseSidecarEvent(value: unknown): SidecarEvent | null {
  if (!isRecord(value) || !isString(value.type)) return null;

  switch (value.type) {
    case "ready":
      return isString(value.peer_id) && isString(value.protocol_version)
        ? { type: value.type, peer_id: value.peer_id, protocol_version: value.protocol_version }
        : null;
    case "token_snapshot_accepted": {
      const source = value.source ?? "legacy_agent_profile_observation";
      const avatarUrl = value.avatar_url ?? null;
      return isNonNegativeSafeInteger(value.lifetime_tokens) &&
        isNullableString(value.username) &&
        isNullableString(value.display_name) &&
        isNullableString(avatarUrl) &&
        isString(value.account_fingerprint) &&
        isNonNegativeSafeInteger(value.observed_at_unix_ms) &&
        typeof value.peer_verifiable === "boolean" &&
        isStringMember(source, [
          "codex_app_server_account_usage",
          "legacy_agent_profile_observation",
        ] as const)
        ? {
            type: value.type,
            lifetime_tokens: value.lifetime_tokens,
            username: value.username,
            display_name: value.display_name,
            avatar_url: avatarUrl,
            account_fingerprint: value.account_fingerprint,
            observed_at_unix_ms: value.observed_at_unix_ms,
            peer_verifiable: value.peer_verifiable,
            source,
          }
        : null;
    }
    case "identity_ready":
      return isString(value.player_id) &&
        isString(value.device_public_key) &&
        isString(value.device_label) &&
        isNonNegativeSafeInteger(value.certificate_expires_at_unix_ms) &&
        isString(value.recovery_envelope) &&
        isNonNegativeSafeInteger(value.remote_replicas)
        ? {
            type: value.type,
            player_id: value.player_id,
            device_public_key: value.device_public_key,
            device_label: value.device_label,
            certificate_expires_at_unix_ms: value.certificate_expires_at_unix_ms,
            recovery_envelope: value.recovery_envelope,
            remote_replicas: value.remote_replicas,
          }
        : null;
    case "listen_address":
      return isString(value.address) ? { type: value.type, address: value.address } : null;
    case "sidecar_restarting":
      return { type: value.type };
    case "community_network_loaded":
      return isNonNegativeSafeInteger(value.rendezvous_nodes) &&
        isNonNegativeSafeInteger(value.relay_nodes) &&
        isNonNegativeSafeInteger(value.archive_nodes) &&
        typeof value.cold_start_available === "boolean"
        ? {
            type: value.type,
            rendezvous_nodes: value.rendezvous_nodes,
            relay_nodes: value.relay_nodes,
            archive_nodes: value.archive_nodes,
            cold_start_available: value.cold_start_available,
          }
        : null;
    case "volunteer_preference_saved":
      return isStringMember(value.consent, ["granted", "declined"] as const) &&
        typeof value.restart_required === "boolean"
        ? {
            type: value.type,
            consent: value.consent,
            restart_required: value.restart_required,
          }
        : null;
    case "volunteer_status":
      return isStringMember(value.consent, ["undecided", "granted", "declined"] as const) &&
        isStringMember(value.network_cost, ["unmetered", "metered", "unknown"] as const) &&
        isStringMember(value.power_source, ["ac", "battery", "unknown"] as const) &&
        isStringMember(value.policy_reason, [
          "eligible",
          "consent_required",
          "declined",
          "metered_network",
          "battery_power",
          "host_conditions_unknown",
        ] as const) &&
        isStringMember(value.reachability, ["unknown", "private", "public"] as const) &&
        isString(value.reachability_evidence) &&
        isStringMember(value.role, [
          "disabled",
          "discovery_candidate",
          "relay_candidate",
          "active_discovery",
          "active_discovery_relay",
        ] as const) &&
        typeof value.discovery_server_enabled === "boolean" &&
        typeof value.relay_server_enabled === "boolean" &&
        typeof value.upnp_enabled === "boolean" &&
        isNonNegativeSafeInteger(value.active_reservations) &&
        isNonNegativeSafeInteger(value.active_circuits) &&
        isPositiveSafeInteger(value.max_reservations) &&
        isPositiveSafeInteger(value.max_circuits) &&
        isPositiveSafeInteger(value.max_circuit_duration_seconds) &&
        isPositiveSafeInteger(value.max_circuit_bytes)
        ? {
            type: value.type,
            consent: value.consent,
            network_cost: value.network_cost,
            power_source: value.power_source,
            policy_reason: value.policy_reason,
            reachability: value.reachability,
            reachability_evidence: value.reachability_evidence,
            role: value.role,
            discovery_server_enabled: value.discovery_server_enabled,
            relay_server_enabled: value.relay_server_enabled,
            upnp_enabled: value.upnp_enabled,
            active_reservations: value.active_reservations,
            active_circuits: value.active_circuits,
            max_reservations: value.max_reservations,
            max_circuits: value.max_circuits,
            max_circuit_duration_seconds: value.max_circuit_duration_seconds,
            max_circuit_bytes: value.max_circuit_bytes,
          }
        : null;
    case "relay_candidate_added":
      return isString(value.peer_id) && isString(value.address) && isString(value.source)
        ? {
            type: value.type,
            peer_id: value.peer_id,
            address: value.address,
            source: value.source,
          }
        : null;
    case "relay_reservation_requested":
      return isString(value.peer_id) && isString(value.address)
        ? { type: value.type, peer_id: value.peer_id, address: value.address }
        : null;
    case "relay_reservation_accepted":
      return isString(value.peer_id) &&
        isString(value.address) &&
        typeof value.renewal === "boolean" &&
        isNullableNonNegativeSafeInteger(value.duration_seconds) &&
        isNullableNonNegativeSafeInteger(value.data_bytes)
        ? {
            type: value.type,
            peer_id: value.peer_id,
            address: value.address,
            renewal: value.renewal,
            duration_seconds: value.duration_seconds,
            data_bytes: value.data_bytes,
          }
        : null;
    case "relay_circuit_established":
      return isString(value.peer_id) &&
        isStringMember(value.direction, ["inbound", "outbound"] as const) &&
        isNullableNonNegativeSafeInteger(value.duration_seconds) &&
        isNullableNonNegativeSafeInteger(value.data_bytes)
        ? {
            type: value.type,
            peer_id: value.peer_id,
            direction: value.direction,
            duration_seconds: value.duration_seconds,
            data_bytes: value.data_bytes,
          }
        : null;
    case "relay_server_reservation":
      return isString(value.peer_id) && isString(value.action)
        ? { type: value.type, peer_id: value.peer_id, action: value.action }
        : null;
    case "relay_server_circuit":
      return isString(value.source_peer_id) &&
        isString(value.destination_peer_id) &&
        isString(value.action)
        ? {
            type: value.type,
            source_peer_id: value.source_peer_id,
            destination_peer_id: value.destination_peer_id,
            action: value.action,
          }
        : null;
    case "peer_connected":
    case "peer_disconnected":
      return isString(value.peer_id) ? { type: value.type, peer_id: value.peer_id } : null;
    case "pool_joined":
      return isString(value.topic) &&
        isString(value.level_id) &&
        isPositiveSafeInteger(value.buy_in)
        ? {
            type: value.type,
            topic: value.topic,
            level_id: value.level_id,
            buy_in: value.buy_in,
          }
        : null;
    case "pool_ticket_published":
      return isString(value.ticket_id) && typeof value.published_to_mesh === "boolean"
        ? {
            type: value.type,
            ticket_id: value.ticket_id,
            published_to_mesh: value.published_to_mesh,
          }
        : null;
    case "pool_directory_updated":
      return isNonNegativeSafeInteger(value.discovered_tables) &&
        isNonNegativeSafeInteger(value.waiting_players)
        ? {
            type: value.type,
            discovered_tables: value.discovered_tables,
            waiting_players: value.waiting_players,
          }
        : null;
    case "pool_joining_table":
      return isString(value.table_id) &&
        isNonNegativeSafeInteger(value.members) &&
        isNonNegativeSafeInteger(value.waiting)
        ? {
            type: value.type,
            table_id: value.table_id,
            members: value.members,
            waiting: value.waiting,
          }
        : null;
    case "pool_join_attempt_expired":
    case "pool_creating_table":
    case "pool_table_joined":
      return isString(value.table_id) ? { type: value.type, table_id: value.table_id } : null;
    case "pool_cancelled":
      return { type: value.type };
    case "room_entered":
      return isString(value.table_id) && isString(value.level_id)
        ? { type: value.type, table_id: value.table_id, level_id: value.level_id }
        : null;
    case "room_snapshot":
      return isString(value.table_id) &&
        isNonNegativeSafeInteger(value.membership_version) &&
        isRoomSeatArray(value.seats) &&
        isStringArray(value.waiting) &&
        isPositiveSafeInteger(value.capacity) &&
        value.capacity <= 6 &&
        isStringMember(value.local_role, ["joining", "seated", "waiting", "playing", "leaving"] as const) &&
        isNullablePositiveSafeInteger(value.hand_number) &&
        isNullableNonNegativeSafeInteger(value.next_hand_countdown_ms)
        ? {
            type: value.type,
            table_id: value.table_id,
            membership_version: value.membership_version,
            seats: value.seats,
            waiting: value.waiting,
            capacity: value.capacity,
            local_role: value.local_role,
            hand_number: value.hand_number,
            next_hand_countdown_ms: value.next_hand_countdown_ms,
          }
        : null;
    case "membership_confirmation":
      return isString(value.table_id) &&
        isNonNegativeSafeInteger(value.confirmed) &&
        isPositiveSafeInteger(value.required)
        ? {
            type: value.type,
            table_id: value.table_id,
            confirmed: value.confirmed,
            required: value.required,
          }
        : null;
    case "hand_roster_confirmation":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isNonNegativeSafeInteger(value.confirmed) &&
        isPositiveSafeInteger(value.required)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            confirmed: value.confirmed,
            required: value.required,
          }
        : null;
    case "next_hand_ready":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isPositiveSafeInteger(value.players)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            players: value.players,
          }
        : null;
    case "safe_leave_requested":
      return isString(value.table_id) && isNullablePositiveSafeInteger(value.after_hand_number)
        ? {
            type: value.type,
            table_id: value.table_id,
            after_hand_number: value.after_hand_number,
          }
        : null;
    case "safe_leave_completed":
    case "room_closed":
      return isString(value.table_id) ? { type: value.type, table_id: value.table_id } : null;
    case "hand_protocol_started":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isPositiveSafeInteger(value.seat) &&
        isPositiveSafeInteger(value.dealer_seat) &&
        isStringArray(value.players) &&
        isString(value.level_id) &&
        isPositiveSafeInteger(value.small_blind) &&
        isPositiveSafeInteger(value.big_blind) &&
        Array.isArray(value.buy_ins) &&
        value.buy_ins.every(isPositiveSafeInteger) &&
        value.buy_ins.length === value.players.length
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            seat: value.seat,
            dealer_seat: value.dealer_seat,
            players: value.players,
            level_id: value.level_id,
            small_blind: value.small_blind,
            big_blind: value.big_blind,
            buy_ins: value.buy_ins,
          }
        : null;
    case "hand_protocol_progress":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        (value.phase === "key_exchange" ||
          value.phase === "shuffling" ||
          value.phase === "dealing") &&
        isNonNegativeSafeInteger(value.completed) &&
        isPositiveSafeInteger(value.required)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            phase: value.phase,
            completed: value.completed,
            required: value.required,
          }
        : null;
    case "hand_ready":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isPositiveSafeInteger(value.seat) &&
        isHandCardArray(value.hole_cards) &&
        value.hole_cards.length === 2 &&
        isString(value.transcript_hash)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            seat: value.seat,
            hole_cards: value.hole_cards,
            transcript_hash: value.transcript_hash,
          }
        : null;
    case "hand_state":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isNonNegativeSafeInteger(value.sequence) &&
        isString(value.street) &&
        isNonNegativeSafeInteger(value.pot) &&
        isNonNegativeSafeInteger(value.current_bet) &&
        isNullablePositiveSafeInteger(value.next_seat) &&
        isPositiveSafeInteger(value.local_seat) &&
        isNonNegativeSafeInteger(value.to_call) &&
        isNonNegativeSafeInteger(value.minimum_raise_to) &&
        isNonNegativeSafeInteger(value.maximum_raise_to) &&
        typeof value.can_act === "boolean" &&
        typeof value.awaiting_reveal === "boolean" &&
        isHandCardArray(value.board) &&
        isHandSeatArray(value.seats) &&
        isString(value.transcript_hash)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            sequence: value.sequence,
            street: value.street,
            pot: value.pot,
            current_bet: value.current_bet,
            next_seat: value.next_seat,
            local_seat: value.local_seat,
            to_call: value.to_call,
            minimum_raise_to: value.minimum_raise_to,
            maximum_raise_to: value.maximum_raise_to,
            can_act: value.can_act,
            awaiting_reveal: value.awaiting_reveal,
            board: value.board,
            seats: value.seats,
            transcript_hash: value.transcript_hash,
          }
        : null;
    case "hand_action_conflict":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isPositiveSafeInteger(value.sequence) &&
        isString(value.accepted_action_hash) &&
        isString(value.conflicting_action_hash)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            sequence: value.sequence,
            accepted_action_hash: value.accepted_action_hash,
            conflicting_action_hash: value.conflicting_action_hash,
          }
        : null;
    case "hand_settled":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isHandOutcomeArray(value.outcomes) &&
        isString(value.transcript_hash)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            outcomes: value.outcomes,
            transcript_hash: value.transcript_hash,
          }
        : null;
    case "receipt_consensus_progress":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isPositiveSafeInteger(value.signed) &&
        isPositiveSafeInteger(value.required)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            signed: value.signed,
            required: value.required,
          }
        : null;
    case "receipt_finalized":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isString(value.receipt_id) &&
        isSafeInteger(value.local_delta) &&
        isPositiveSafeInteger(value.signatures)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            receipt_id: value.receipt_id,
            local_delta: value.local_delta,
            signatures: value.signatures,
          }
        : null;
    case "hand_session_interrupted":
    case "hand_session_resumed":
      return isString(value.table_id) &&
        isPositiveSafeInteger(value.hand_number) &&
        isString(value.peer_id)
        ? {
            type: value.type,
            table_id: value.table_id,
            hand_number: value.hand_number,
            peer_id: value.peer_id,
          }
        : null;
    case "hand_left":
      return { type: value.type };
    case "shutdown_complete":
      return { type: value.type };
    case "friend_room_created":
      return isString(value.invite_code) &&
        isString(value.room_id) &&
        isPositiveSafeInteger(value.buy_in) &&
        isNonNegativeSafeInteger(value.expires_at_unix_ms)
        ? {
            type: value.type,
            invite_code: value.invite_code,
            room_id: value.room_id,
            buy_in: value.buy_in,
            expires_at_unix_ms: value.expires_at_unix_ms,
          }
        : null;
    case "friend_room_joining":
    case "friend_room_joined":
      return isString(value.room_id) && isString(value.host_peer_id)
        ? { type: value.type, room_id: value.room_id, host_peer_id: value.host_peer_id }
        : null;
    case "archive_node_ready":
      return isString(value.public_key)
        ? { type: value.type, public_key: value.public_key }
        : null;
    case "archive_peers_configured":
      return isStringArray(value.peers) && isPositiveSafeInteger(value.minimum_confirmed_replicas)
        ? {
            type: value.type,
            peers: value.peers,
            minimum_confirmed_replicas: value.minimum_confirmed_replicas,
          }
        : null;
    case "receipt_archive_pending":
      return isString(value.address) && isPositiveSafeInteger(value.required)
        ? { type: value.type, address: value.address, required: value.required }
        : null;
    case "receipt_archived":
      return isString(value.address) && isPositiveSafeInteger(value.confirmed_replicas)
        ? {
            type: value.type,
            address: value.address,
            confirmed_replicas: value.confirmed_replicas,
          }
        : null;
    case "receipt_archive_failed":
      return isString(value.address) && isString(value.reason)
        ? { type: value.type, address: value.address, reason: value.reason }
        : null;
    case "receipt_fetched":
      return isString(value.address) &&
        isString(value.receipt_id) &&
        isPositiveSafeInteger(value.hand_number)
        ? {
            type: value.type,
            address: value.address,
            receipt_id: value.receipt_id,
            hand_number: value.hand_number,
          }
        : null;
    case "archive_index_received":
      return isString(value.player_id) && isNonNegativeSafeInteger(value.addresses)
        ? { type: value.type, player_id: value.player_id, addresses: value.addresses }
        : null;
    case "recovery_backup_pending":
      return isString(value.locator) && isPositiveSafeInteger(value.required)
        ? { type: value.type, locator: value.locator, required: value.required }
        : null;
    case "recovery_backup_stored":
      return isString(value.locator) && isPositiveSafeInteger(value.confirmed_replicas)
        ? {
            type: value.type,
            locator: value.locator,
            confirmed_replicas: value.confirmed_replicas,
          }
        : null;
    case "recovery_backup_failed":
      return isString(value.locator) && isString(value.reason)
        ? { type: value.type, locator: value.locator, reason: value.reason }
        : null;
    case "recovery_backup_fetched":
      return isString(value.locator) ? { type: value.type, locator: value.locator } : null;
    case "discovery_configured":
      return isStringArray(value.nodes) && isString(value.namespace)
        ? { type: value.type, nodes: value.nodes, namespace: value.namespace }
        : null;
    case "advertised_address_added":
      return isString(value.address) ? { type: value.type, address: value.address } : null;
    case "rendezvous_registered":
      return isString(value.node) &&
        isString(value.address) &&
        isString(value.namespace) &&
        isPositiveSafeInteger(value.ttl_seconds)
        ? {
            type: value.type,
            node: value.node,
            address: value.address,
            namespace: value.namespace,
            ttl_seconds: value.ttl_seconds,
          }
        : null;
    case "rendezvous_candidate_added":
      return isString(value.node) && isString(value.address) && isString(value.source)
        ? {
            type: value.type,
            node: value.node,
            address: value.address,
            source: value.source,
          }
        : null;
    case "peers_discovered":
      return isString(value.node) && isNonNegativeSafeInteger(value.peers)
        ? { type: value.type, node: value.node, peers: value.peers }
        : null;
    case "statistics_updated":
      return isNonNegativeSafeInteger(value.completed_hands) &&
        isNonNegativeSafeInteger(value.won_hands) &&
        isNonNegativeSafeInteger(value.lost_hands) &&
        isNonNegativeSafeInteger(value.split_hands) &&
        isNonNegativeSafeInteger(value.gross_won) &&
        isNonNegativeSafeInteger(value.gross_lost) &&
        isSafeInteger(value.net_chips) &&
        isNonNegativeSafeInteger(value.largest_win) &&
        isNonNegativeSafeInteger(value.largest_loss) &&
        isRecentHandArray(value.recent_hands)
        ? {
            type: value.type,
            completed_hands: value.completed_hands,
            won_hands: value.won_hands,
            lost_hands: value.lost_hands,
            split_hands: value.split_hands,
            gross_won: value.gross_won,
            gross_lost: value.gross_lost,
            net_chips: value.net_chips,
            largest_win: value.largest_win,
            largest_loss: value.largest_loss,
            recent_hands: value.recent_hands,
          }
        : null;
    case "warning":
      return isString(value.message) ? { type: value.type, message: value.message } : null;
    default:
      return null;
  }
}
