export type BridgeMode = "codex" | "preview";

export interface TokenSnapshot {
  readonly lifetimeTokens: number;
  readonly username: string | null;
  readonly displayName: string | null;
  readonly avatarUrl: string | null;
  readonly observedAtUnixMs: number;
  readonly source:
    | "codex_app_server_account_usage"
    | "shared_runtime_replay"
    | "legacy_agent_profile_observation"
    | "preview";
}

export interface AccountBindingSnapshot {
  readonly accountFingerprint: string;
  readonly peerVerifiable: boolean;
}

export type OfficialUsagePhase = "idle" | "loading" | "ready" | "error";

export interface OfficialUsageState {
  readonly phase: OfficialUsagePhase;
  readonly error: string | null;
}

export type UpdatePhase =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "ready"
  | "installing"
  | "restart_required"
  | "error";

export interface UpdateStatus {
  readonly phase: UpdatePhase;
  readonly currentVersion: string;
  readonly latestVersion: string | null;
  readonly releaseUrl: string | null;
  readonly artifactBytes: number | null;
  readonly downloadedBytes: number;
  readonly sha256Verified: boolean;
  readonly error: string | null;
}

export interface IdentitySnapshot {
  readonly accountFingerprint: string;
  readonly playerId: string;
  readonly devicePublicKey: string;
  readonly deviceLabel: string;
  readonly certificateExpiresAtUnixMs: number;
  readonly recoveryEnvelope: string;
  readonly remoteReplicas: number;
}

export type FriendRoomStatus = "idle" | "created" | "joining" | "joined";

export type PublicPoolStatus = "idle" | "searching" | "joining" | "creating" | "in_room";

export interface PublicPoolSnapshot {
  readonly status: PublicPoolStatus;
  readonly topic: string | null;
  readonly levelId: string | null;
  readonly buyIn: number;
  readonly discoveredTables: number;
  readonly waitingPlayers: number;
  readonly targetTableId: string | null;
}

export type LocalRoomRole = "joining" | "waiting" | "seated" | "playing" | "leaving";

export interface RoomSeatSnapshot {
  readonly physicalSeat: number;
  readonly playerId: string;
  readonly buyIn: number;
}

export interface RoomSnapshot {
  readonly tableId: string | null;
  readonly membershipVersion: number;
  readonly seats: readonly RoomSeatSnapshot[];
  readonly waiting: readonly string[];
  readonly capacity: number;
  readonly localRole: LocalRoomRole | null;
  readonly handNumber: number | null;
  readonly nextHandCountdownMs: number | null;
  readonly membershipConfirmed: number;
  readonly membershipRequired: number;
  readonly rosterConfirmed: number;
  readonly rosterRequired: number;
  readonly safeLeaveAfterHand: number | null;
  readonly safeLeaveForceAfterUnixMs: number | null;
}

export type HandProtocolPhase =
  | "idle"
  | "key_exchange"
  | "shuffling"
  | "dealing"
  | "playing"
  | "revealing"
  | "settled"
  | "receipt_consensus"
  | "between_hands"
  | "conflicted"
  | "interrupted";

export type ReceiptArchiveStatus =
  | "idle"
  | "signing"
  | "finalized"
  | "archiving"
  | "archived"
  | "unarchived";

export interface HandCardSnapshot {
  readonly rank: number;
  readonly suit: "club" | "diamond" | "heart" | "spade";
}

export type HandActionKind = "fold" | "check" | "call" | "raise";

export interface HandSeatSnapshot {
  readonly seat: number;
  readonly playerId: string;
  readonly stack: number;
  readonly committed: number;
  readonly status: "active" | "folded" | "all_in";
  readonly lastAction: HandActionKind | null;
}

export interface HandOutcomeSnapshot {
  readonly seat: number;
  readonly playerId: string;
  readonly startingStack: number;
  readonly endingStack: number;
  readonly delta: number;
}

export interface HandSnapshot {
  readonly publicStateHash: string | null;
  readonly phase: HandProtocolPhase;
  readonly tableId: string | null;
  readonly handNumber: number;
  readonly localSeat: number | null;
  readonly dealerSeat: number | null;
  readonly players: readonly string[];
  readonly levelId: string | null;
  readonly smallBlind: number;
  readonly bigBlind: number;
  readonly buyIns: readonly number[];
  readonly progressCompleted: number;
  readonly progressRequired: number;
  readonly holeCards: readonly HandCardSnapshot[];
  readonly board: readonly HandCardSnapshot[];
  readonly sequence: number;
  readonly street: string;
  readonly pot: number;
  readonly currentBet: number;
  readonly nextSeat: number | null;
  readonly toCall: number;
  readonly minimumRaiseTo: number;
  readonly maximumRaiseTo: number;
  readonly canAct: boolean;
  readonly awaitingReveal: boolean;
  readonly actionTimeoutMs: number;
  readonly turnDeadlineUnixMs: number | null;
  readonly seats: readonly HandSeatSnapshot[];
  readonly pendingSequence: number | null;
  readonly transcriptHash: string | null;
  readonly outcomes: readonly HandOutcomeSnapshot[];
  readonly receiptStatus: ReceiptArchiveStatus;
  readonly receiptSigned: number;
  readonly receiptRequired: number;
  readonly receiptId: string | null;
  readonly receiptAddress: string | null;
  readonly sessionInterrupted: boolean;
}

export interface RecentHandSnapshot {
  readonly address: string;
  readonly receiptId: string;
  readonly handNumber: number;
  readonly levelId: string;
  readonly players: number;
  readonly settledAtUnixMs: number;
  readonly delta: number;
  readonly archived: boolean;
}

export interface StatisticsSnapshot {
  readonly completedHands: number;
  readonly wonHands: number;
  readonly lostHands: number;
  readonly splitHands: number;
  readonly grossWon: number;
  readonly grossLost: number;
  readonly netChips: number;
  readonly largestWin: number;
  readonly largestLoss: number;
  readonly recentHands: readonly RecentHandSnapshot[];
}

export interface ArchiveSnapshot {
  readonly nodePublicKey: string | null;
  readonly peers: readonly string[];
  readonly minimumConfirmedReplicas: number;
  readonly lastAddress: string | null;
  readonly lastStatus: "idle" | "archiving" | "archived" | "failed";
  readonly lastError: string | null;
  readonly confirmedReplicas: number;
}

export interface DiscoverySnapshot {
  readonly nodes: readonly string[];
  readonly namespace: string | null;
  readonly registeredNodes: ReadonlySet<string>;
  readonly lastDiscoveredPeers: number;
}

export interface VolunteerSnapshot {
  readonly consent: "undecided" | "granted" | "declined";
  readonly networkCost: "unmetered" | "metered" | "unknown";
  readonly powerSource: "ac" | "battery" | "unknown";
  readonly policyReason:
    | "eligible"
    | "consent_required"
    | "declined"
    | "metered_network"
    | "battery_power"
    | "host_conditions_unknown";
  readonly reachability: "unknown" | "private" | "public";
  readonly reachabilityEvidence: string;
  readonly role:
    | "disabled"
    | "discovery_candidate"
    | "relay_candidate"
    | "active_discovery"
    | "active_discovery_relay";
  readonly discoveryServerEnabled: boolean;
  readonly relayServerEnabled: boolean;
  readonly upnpEnabled: boolean;
  readonly activeReservations: number;
  readonly activeCircuits: number;
  readonly maxReservations: number;
  readonly maxCircuits: number;
  readonly maxCircuitDurationSeconds: number;
  readonly maxCircuitBytes: number;
  readonly restartRequired: boolean;
  readonly coldStartAvailable: boolean;
  readonly directoryRendezvousNodes: number;
  readonly directoryRelayNodes: number;
  readonly directoryArchiveNodes: number;
}

export type HostCommand =
  | {
      readonly type: "join_public_pool";
      readonly level_id: string;
      readonly buy_in: number;
    }
  | { readonly type: "cancel_public_pool" }
  | { readonly type: "set_volunteer_consent"; readonly enabled: boolean }
  | {
      readonly type: "create_friend_room";
      readonly level_id: string;
      readonly buy_in: number;
    }
  | {
      readonly type: "join_friend_room";
      readonly invite_code: string;
      readonly buy_in: number;
    }
  | {
      readonly type: "ensure_identity";
      readonly expected_account_fingerprint: string;
      readonly recovery_secret: string;
      readonly device_label: string;
    }
  | {
      readonly type: "create_identity";
      readonly expected_account_fingerprint: string;
      readonly recovery_secret: string;
      readonly device_label: string;
    }
  | {
      readonly type: "restore_identity";
      readonly expected_account_fingerprint: string;
      readonly recovery_envelope: string;
      readonly recovery_secret: string;
      readonly device_label: string;
    }
  | {
      readonly type: "restore_remote_identity";
      readonly expected_account_fingerprint: string;
      readonly recovery_secret: string;
      readonly device_label: string;
    }
  | {
      readonly type: "configure_archive_nodes";
      readonly addresses: readonly string[];
      readonly minimum_confirmed_replicas: number;
    }
  | { readonly type: "use_relay"; readonly address: string }
  | {
      readonly type: "configure_discovery";
      readonly addresses: readonly string[];
      readonly namespace?: string;
    }
  | { readonly type: "add_external_address"; readonly address: string }
  | {
      readonly type: "submit_action";
      readonly expected: {
        readonly table_id: string;
        readonly hand_number: number;
        readonly sequence: number;
        readonly public_state_hash: string;
      };
      readonly action: "fold" | "check" | "call" | "raise";
      readonly amount?: number;
    }
  | { readonly type: "leave_table" }
  | { readonly type: "request_token_refresh" }
  | { readonly type: "check_update" }
  | { readonly type: "prepare_update" }
  | { readonly type: "install_update" }
  | { readonly type: "sync_statistics" }
  | { readonly type: "close_ui" };

export type SidecarEvent =
  | { readonly type: "ready"; readonly peer_id: string; readonly protocol_version: string }
  | {
      readonly type: "token_snapshot_accepted";
      readonly lifetime_tokens: number;
      readonly username: string | null;
      readonly display_name: string | null;
      readonly avatar_url?: string | null;
      readonly account_fingerprint: string;
      readonly observed_at_unix_ms: number;
      readonly peer_verifiable: boolean;
      readonly source:
        | "codex_app_server_account_usage"
        | "legacy_agent_profile_observation";
    }
  | {
      readonly type: "identity_ready";
      readonly account_fingerprint: string;
      readonly player_id: string;
      readonly device_public_key: string;
      readonly device_label: string;
      readonly certificate_expires_at_unix_ms: number;
      readonly recovery_envelope: string;
      readonly remote_replicas: number;
    }
  | { readonly type: "identity_cleared" }
  | { readonly type: "listen_address"; readonly address: string }
  | { readonly type: "peer_connected"; readonly peer_id: string }
  | { readonly type: "peer_disconnected"; readonly peer_id: string }
  | {
      readonly type: "pool_joined";
      readonly topic: string;
      readonly level_id: string;
      readonly buy_in: number;
    }
  | {
      readonly type: "pool_ticket_published";
      readonly ticket_id: string;
      readonly published_to_mesh: boolean;
    }
  | {
      readonly type: "pool_directory_updated";
      readonly discovered_tables: number;
      readonly waiting_players: number;
    }
  | {
      readonly type: "pool_joining_table";
      readonly table_id: string;
      readonly members: number;
      readonly waiting: number;
    }
  | { readonly type: "pool_join_attempt_expired"; readonly table_id: string }
  | { readonly type: "pool_creating_table"; readonly table_id: string }
  | { readonly type: "pool_table_joined"; readonly table_id: string }
  | { readonly type: "pool_cancelled" }
  | { readonly type: "room_entered"; readonly table_id: string; readonly level_id: string }
  | {
      readonly type: "room_snapshot";
      readonly table_id: string;
      readonly membership_version: number;
      readonly seats: readonly {
        readonly physical_seat: number;
        readonly player_id: string;
        readonly buy_in: number;
      }[];
      readonly waiting: readonly string[];
      readonly capacity: number;
      readonly local_role: LocalRoomRole;
      readonly hand_number: number | null;
      readonly next_hand_countdown_ms: number | null;
    }
  | {
      readonly type: "membership_confirmation";
      readonly table_id: string;
      readonly confirmed: number;
      readonly required: number;
    }
  | {
      readonly type: "hand_roster_confirmation";
      readonly table_id: string;
      readonly hand_number: number;
      readonly confirmed: number;
      readonly required: number;
    }
  | {
      readonly type: "next_hand_ready";
      readonly table_id: string;
      readonly hand_number: number;
      readonly players: number;
    }
  | {
      readonly type: "safe_leave_requested";
      readonly table_id: string;
      readonly after_hand_number: number | null;
      readonly force_after_unix_ms: number;
    }
  | {
      readonly type: "safe_leave_forced";
      readonly table_id: string;
      readonly reason: "hand_stalled" | "membership_timeout" | "absolute_timeout";
      readonly waited_ms: number;
    }
  | { readonly type: "safe_leave_completed"; readonly table_id: string }
  | {
      readonly type: "hand_aborted_for_leave";
      readonly table_id: string;
      readonly hand_number: number;
      readonly player_id: string;
      readonly evidence_hash: string;
    }
  | { readonly type: "room_closed"; readonly table_id: string }
  | {
      readonly type: "hand_protocol_started";
      readonly table_id: string;
      readonly hand_number: number;
      readonly seat: number;
      readonly dealer_seat: number;
      readonly players: readonly string[];
      readonly level_id: string;
      readonly small_blind: number;
      readonly big_blind: number;
      readonly buy_ins: readonly number[];
    }
  | {
      readonly type: "hand_protocol_progress";
      readonly table_id: string;
      readonly hand_number: number;
      readonly phase: "key_exchange" | "shuffling" | "dealing";
      readonly completed: number;
      readonly required: number;
    }
  | {
      readonly type: "hand_ready";
      readonly table_id: string;
      readonly hand_number: number;
      readonly seat: number;
      readonly hole_cards: readonly HandCardSnapshot[];
      readonly transcript_hash: string;
    }
  | {
      readonly type: "hand_state";
      readonly public_state_hash: string;
      readonly table_id: string;
      readonly hand_number: number;
      readonly sequence: number;
      readonly street: string;
      readonly pot: number;
      readonly current_bet: number;
      readonly next_seat: number | null;
      readonly local_seat: number;
      readonly to_call: number;
      readonly minimum_raise_to: number;
      readonly maximum_raise_to: number;
      readonly can_act: boolean;
      readonly awaiting_reveal: boolean;
      readonly action_timeout_ms: number;
      readonly turn_deadline_unix_ms: number | null;
      readonly board: readonly HandCardSnapshot[];
      readonly seats: readonly {
        readonly seat: number;
        readonly player_id: string;
        readonly stack: number;
        readonly committed: number;
        readonly status: "active" | "folded" | "all_in";
        readonly last_action: HandActionKind | null;
      }[];
      readonly transcript_hash: string;
    }
  | {
      readonly type: "hand_action_conflict";
      readonly table_id: string;
      readonly hand_number: number;
      readonly sequence: number;
      readonly accepted_action_hash: string;
      readonly conflicting_action_hash: string;
    }
  | {
      readonly type: "hand_settled";
      readonly table_id: string;
      readonly hand_number: number;
      readonly outcomes: readonly {
        readonly seat: number;
        readonly player_id: string;
        readonly starting_stack: number;
        readonly ending_stack: number;
        readonly delta: number;
      }[];
      readonly transcript_hash: string;
    }
  | {
      readonly type: "receipt_consensus_progress";
      readonly table_id: string;
      readonly hand_number: number;
      readonly signed: number;
      readonly required: number;
    }
  | {
      readonly type: "receipt_finalized";
      readonly table_id: string;
      readonly hand_number: number;
      readonly receipt_id: string;
      readonly local_delta: number;
      readonly signatures: number;
    }
  | {
      readonly type: "hand_session_interrupted" | "hand_session_resumed";
      readonly table_id: string;
      readonly hand_number: number;
      readonly peer_id: string;
    }
  | { readonly type: "hand_left" }
  | {
      readonly type: "friend_room_created";
      readonly invite_code: string;
      readonly room_id: string;
      readonly buy_in: number;
      readonly expires_at_unix_ms: number;
    }
  | {
      readonly type: "friend_room_joining" | "friend_room_joined";
      readonly room_id: string;
      readonly host_peer_id: string;
    }
  | {
      readonly type: "relay_candidate_added";
      readonly peer_id: string;
      readonly address: string;
      readonly source: string;
    }
  | {
      readonly type: "relay_reservation_requested";
      readonly peer_id: string;
      readonly address: string;
    }
  | {
      readonly type: "relay_reservation_accepted";
      readonly peer_id: string;
      readonly address: string;
      readonly renewal: boolean;
      readonly duration_seconds: number | null;
      readonly data_bytes: number | null;
    }
  | {
      readonly type: "relay_circuit_established";
      readonly peer_id: string;
      readonly direction: "inbound" | "outbound";
      readonly duration_seconds: number | null;
      readonly data_bytes: number | null;
    }
  | {
      readonly type: "relay_server_reservation";
      readonly peer_id: string;
      readonly action: string;
    }
  | {
      readonly type: "relay_server_circuit";
      readonly source_peer_id: string;
      readonly destination_peer_id: string;
      readonly action: string;
    }
  | {
      readonly type: "volunteer_status";
      readonly consent: VolunteerSnapshot["consent"];
      readonly network_cost: VolunteerSnapshot["networkCost"];
      readonly power_source: VolunteerSnapshot["powerSource"];
      readonly policy_reason: VolunteerSnapshot["policyReason"];
      readonly reachability: VolunteerSnapshot["reachability"];
      readonly reachability_evidence: string;
      readonly role: VolunteerSnapshot["role"];
      readonly discovery_server_enabled: boolean;
      readonly relay_server_enabled: boolean;
      readonly upnp_enabled: boolean;
      readonly active_reservations: number;
      readonly active_circuits: number;
      readonly max_reservations: number;
      readonly max_circuits: number;
      readonly max_circuit_duration_seconds: number;
      readonly max_circuit_bytes: number;
    }
  | {
      readonly type: "volunteer_preference_saved";
      readonly consent: "granted" | "declined";
      readonly restart_required: boolean;
    }
  | { readonly type: "sidecar_restarting" }
  | {
      readonly type: "community_network_loaded";
      readonly rendezvous_nodes: number;
      readonly relay_nodes: number;
      readonly archive_nodes: number;
      readonly cold_start_available: boolean;
    }
  | { readonly type: "archive_node_ready"; readonly public_key: string }
  | {
      readonly type: "archive_peers_configured";
      readonly peers: readonly string[];
      readonly minimum_confirmed_replicas: number;
    }
  | {
      readonly type: "receipt_archive_pending";
      readonly address: string;
      readonly required: number;
    }
  | {
      readonly type: "receipt_archived";
      readonly address: string;
      readonly confirmed_replicas: number;
    }
  | { readonly type: "receipt_archive_failed"; readonly address: string; readonly reason: string }
  | {
      readonly type: "receipt_fetched";
      readonly address: string;
      readonly receipt_id: string;
      readonly hand_number: number;
    }
  | {
      readonly type: "archive_index_received";
      readonly player_id: string;
      readonly addresses: number;
    }
  | {
      readonly type: "recovery_backup_pending";
      readonly locator: string;
      readonly required: number;
    }
  | {
      readonly type: "recovery_backup_stored";
      readonly locator: string;
      readonly confirmed_replicas: number;
    }
  | {
      readonly type: "recovery_backup_failed";
      readonly locator: string;
      readonly reason: string;
    }
  | { readonly type: "recovery_backup_fetched"; readonly locator: string }
  | {
      readonly type: "discovery_configured";
      readonly nodes: readonly string[];
      readonly namespace: string;
    }
  | { readonly type: "advertised_address_added"; readonly address: string }
  | {
      readonly type: "rendezvous_registered";
      readonly node: string;
      readonly address: string;
      readonly namespace: string;
      readonly ttl_seconds: number;
    }
  | {
      readonly type: "rendezvous_candidate_added";
      readonly node: string;
      readonly address: string;
      readonly source: string;
    }
  | {
      readonly type: "peers_discovered";
      readonly node: string;
      readonly peers: number;
    }
  | {
      readonly type: "statistics_updated";
      readonly completed_hands: number;
      readonly won_hands: number;
      readonly lost_hands: number;
      readonly split_hands: number;
      readonly gross_won: number;
      readonly gross_lost: number;
      readonly net_chips: number;
      readonly largest_win: number;
      readonly largest_loss: number;
      readonly recent_hands: readonly {
        readonly address: string;
        readonly receipt_id: string;
        readonly hand_number: number;
        readonly level_id: string;
        readonly players: number;
        readonly settled_at_unix_ms: number;
        readonly delta: number;
        readonly archived: boolean;
      }[];
    }
  | { readonly type: "warning"; readonly message: string }
  | { readonly type: "shutdown_complete" };

export interface BridgeSnapshot {
  readonly mode: BridgeMode;
  readonly sidecarReady: boolean;
  readonly peerId: string | null;
  readonly connectedPeers: ReadonlySet<string>;
  readonly pool: PublicPoolSnapshot;
  readonly room: RoomSnapshot;
  readonly hand: HandSnapshot;
  readonly tokenSnapshot: TokenSnapshot | null;
  readonly officialUsage: OfficialUsageState;
  readonly update: UpdateStatus;
  readonly accountBinding: AccountBindingSnapshot | null;
  readonly identity: IdentitySnapshot | null;
  readonly friendInviteCode: string | null;
  readonly friendRoomId: string | null;
  readonly friendRoomStatus: FriendRoomStatus;
  readonly archive: ArchiveSnapshot;
  readonly discovery: DiscoverySnapshot;
  readonly volunteer: VolunteerSnapshot;
  readonly statistics: StatisticsSnapshot;
  readonly lastWarning: string | null;
}

export interface IdentityConfirmation {
  readonly recoveryEnvelope: string;
  readonly playerId: string;
  readonly accountFingerprint: string;
  readonly recoverySecretConfirmed: boolean;
}

export type CommandResult =
  | { readonly ok: true; readonly identity?: IdentityConfirmation }
  | { readonly ok: false; readonly error: string };

export type ConfirmedHostCommandSender = (command: HostCommand) => Promise<CommandResult>;
