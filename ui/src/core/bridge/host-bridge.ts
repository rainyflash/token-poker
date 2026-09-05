import { useSyncExternalStore } from "react";
import packageManifest from "../../../package.json";
import type {
  BridgeSnapshot,
  CommandResult,
  HandSnapshot,
  HostCommand,
  PublicPoolSnapshot,
  RoomSnapshot,
  SidecarEvent,
  VolunteerSnapshot,
} from "./contracts";
import {
  parseAccountBinding,
  parseOfficialUsageState,
  parseSidecarEvent,
  parseTokenSnapshot,
  parseUpdateStatus,
} from "./guards";
import { parseCurrentStateProjection } from "./current-state-policy";
import {
  isCurrentHand,
  phaseAfterHandReady,
  phaseAfterHandState,
  shouldAcceptHandProgress,
  shouldAcceptHandReady,
  shouldAcceptHandStart,
  shouldAcceptHandState,
} from "./hand-event-policy";
import { findStakeLevel } from "../domain/stake-levels";
import {
  readStoredLanguage,
  resolveInitialLanguage,
  type LanguageStorage,
} from "../i18n/language-preference";
import { translate, type MessageKey, type MessageVariables } from "../i18n/messages";

declare global {
  var __tokenHoldemBridgeInstalled: boolean | undefined;
  var __tokenHoldemLastSnapshot: unknown;
  var __tokenHoldemOfficialUsageState: unknown;
  var __tokenHoldemLastAccountBinding: unknown;
  var __tokenHoldemCurrentState: unknown;
  var __tokenPokerUpdateStatus: unknown;
  var __tokenHoldemBufferedSidecarEvents: unknown[] | undefined;
  var __tokenHoldemMountRoot: HTMLElement | undefined;
  var __tokenHoldemPortalRoot: HTMLElement | undefined;
  var __tokenHoldemCodexMarkSource: string | undefined;
  var __tokenHoldemBootError: string | undefined;
  var tokenHoldemCommand: ((payload: string) => Promise<CommandResult>) | undefined;
}

const PREVIEW_IDENTITY_DELAY_MS = 520;
const PREVIEW_SNAPSHOT = Object.freeze({
  lifetimeTokens: 35_500_000_000,
  username: null,
  displayName: null,
  avatarUrl: null,
  observedAtUnixMs: Date.now(),
  source: "preview" as const,
});

function bridgeText(key: MessageKey, variables?: MessageVariables): string {
  let storage: LanguageStorage | null = null;
  let languages: readonly string[] = [];
  try {
    storage = globalThis.localStorage;
    languages = globalThis.navigator.languages;
  } catch {
    // Fall back to English when the Codex host does not expose browser locale.
  }
  return translate(resolveInitialLanguage(readStoredLanguage(storage), languages), key, variables);
}

const HOST_BRIDGE_INSTALLED =
  globalThis.__tokenHoldemBridgeInstalled === true ||
  (import.meta.env.DEV && new URLSearchParams(globalThis.location.search).get("host-preview") === "1");
const PREVIEW_UPDATE_VERSION = import.meta.env.DEV
  ? new URLSearchParams(globalThis.location.search).get("update-preview")
  : null;
const HAS_PREVIEW_UPDATE =
  PREVIEW_UPDATE_VERSION !== null && /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/u.test(PREVIEW_UPDATE_VERSION);

const PREVIEW_ACCOUNT_FINGERPRINT =
  "8f3c58d35e4a9db2b6a00d54a1a8d88a7ab3e9114f8924a7ce5510d23f9b8af6";

const INITIAL_TOKEN_SNAPSHOT =
  parseTokenSnapshot(globalThis.__tokenHoldemLastSnapshot) ??
  (HOST_BRIDGE_INSTALLED ? null : PREVIEW_SNAPSHOT);

const INITIAL_ACCOUNT_BINDING =
  parseAccountBinding(globalThis.__tokenHoldemLastAccountBinding) ??
  (HOST_BRIDGE_INSTALLED
    ? null
    : {
        accountFingerprint: PREVIEW_ACCOUNT_FINGERPRINT,
        peerVerifiable: false,
      });

const INITIAL_UPDATE_STATUS =
  parseUpdateStatus(globalThis.__tokenPokerUpdateStatus) ??
  Object.freeze({
    phase: HAS_PREVIEW_UPDATE ? "available" as const : "idle" as const,
    currentVersion: packageManifest.version,
    latestVersion: HAS_PREVIEW_UPDATE ? PREVIEW_UPDATE_VERSION : null,
    releaseUrl: HAS_PREVIEW_UPDATE ? "https://github.com/rainyflash/token-poker/releases" : null,
    artifactBytes: HAS_PREVIEW_UPDATE ? 8_388_608 : null,
    downloadedBytes: 0,
    sha256Verified: false,
    error: null,
  });

const EMPTY_HAND: HandSnapshot = Object.freeze({
  publicStateHash: null,
  phase: "idle",
  tableId: null,
  handNumber: 0,
  localSeat: null,
  dealerSeat: null,
  players: [],
  levelId: null,
  smallBlind: 0,
  bigBlind: 0,
  buyIns: [],
  progressCompleted: 0,
  progressRequired: 0,
  holeCards: [],
  board: [],
  sequence: 0,
  street: "preflop",
  pot: 0,
  currentBet: 0,
  nextSeat: null,
  toCall: 0,
  minimumRaiseTo: 0,
  maximumRaiseTo: 0,
  canAct: false,
  awaitingReveal: false,
  actionTimeoutMs: 30_000,
  turnDeadlineUnixMs: null,
  seats: [],
  pendingSequence: null,
  transcriptHash: null,
  outcomes: [],
  receiptStatus: "idle",
  receiptSigned: 0,
  receiptRequired: 0,
  receiptId: null,
  receiptAddress: null,
  sessionInterrupted: false,
});

const EMPTY_POOL: PublicPoolSnapshot = Object.freeze({
  status: "idle" as const,
  topic: null,
  levelId: null,
  buyIn: 0,
  discoveredTables: 0,
  waitingPlayers: 0,
  targetTableId: null,
});

const EMPTY_ROOM: RoomSnapshot = Object.freeze({
  tableId: null,
  membershipVersion: 0,
  seats: [],
  waiting: [],
  capacity: 6,
  localRole: null,
  handNumber: null,
  nextHandCountdownMs: null,
  membershipConfirmed: 0,
  membershipRequired: 0,
  rosterConfirmed: 0,
  rosterRequired: 0,
  safeLeaveAfterHand: null,
  safeLeaveForceAfterUnixMs: null,
});

const INITIAL_VOLUNTEER: VolunteerSnapshot = Object.freeze(
  HOST_BRIDGE_INSTALLED
    ? {
        consent: "undecided",
        networkCost: "unknown",
        powerSource: "unknown",
        policyReason: "consent_required",
        reachability: "unknown",
        reachabilityEvidence: "none",
        role: "disabled",
        discoveryServerEnabled: false,
        relayServerEnabled: false,
        upnpEnabled: false,
        activeReservations: 0,
        activeCircuits: 0,
        maxReservations: 64,
        maxCircuits: 16,
        maxCircuitDurationSeconds: 7_200,
        maxCircuitBytes: 67_108_864,
        restartRequired: false,
        coldStartAvailable: false,
        directoryRendezvousNodes: 0,
        directoryRelayNodes: 0,
        directoryArchiveNodes: 0,
      }
    : {
        consent: "granted",
        networkCost: "unmetered",
        powerSource: "ac",
        policyReason: "eligible",
        reachability: "public",
        reachabilityEvidence: "preview",
        role: "active_discovery_relay",
        discoveryServerEnabled: true,
        relayServerEnabled: true,
        upnpEnabled: true,
        activeReservations: 2,
        activeCircuits: 1,
        maxReservations: 64,
        maxCircuits: 16,
        maxCircuitDurationSeconds: 7_200,
        maxCircuitBytes: 67_108_864,
        restartRequired: false,
        coldStartAvailable: true,
        directoryRendezvousNodes: 1,
        directoryRelayNodes: 1,
        directoryArchiveNodes: 1,
      },
);

const INITIAL_STATE: BridgeSnapshot = Object.freeze({
  mode: HOST_BRIDGE_INSTALLED ? "codex" : "preview",
  sidecarReady: !HOST_BRIDGE_INSTALLED,
  peerId: HOST_BRIDGE_INSTALLED ? null : "12D3KooW-preview",
  connectedPeers: new Set<string>(),
  pool: EMPTY_POOL,
  room: EMPTY_ROOM,
  hand: EMPTY_HAND,
  tokenSnapshot: INITIAL_TOKEN_SNAPSHOT,
  officialUsage:
    parseOfficialUsageState(globalThis.__tokenHoldemOfficialUsageState) ??
    (INITIAL_TOKEN_SNAPSHOT === null
      ? { phase: "idle" as const, error: null }
      : { phase: "ready" as const, error: null }),
  update: INITIAL_UPDATE_STATUS,
  accountBinding: INITIAL_ACCOUNT_BINDING,
  identity: null,
  friendInviteCode: null,
  friendRoomId: null,
  friendRoomStatus: "idle",
  archive: Object.freeze({
    nodePublicKey: null,
    peers: [],
    minimumConfirmedReplicas: 0,
    lastAddress: null,
    lastStatus: "idle",
    lastError: null,
    confirmedReplicas: 0,
  }),
  discovery: Object.freeze({
    nodes: HOST_BRIDGE_INSTALLED ? [] : ["preview-discovery"],
    namespace: HOST_BRIDGE_INSTALLED ? null : "token-holdem/v1",
    registeredNodes:
      HOST_BRIDGE_INSTALLED
        ? new Set<string>()
        : new Set<string>(["preview-discovery"]),
    lastDiscoveredPeers: 0,
  }),
  volunteer: INITIAL_VOLUNTEER,
  statistics: Object.freeze({
    completedHands: 0,
    wonHands: 0,
    lostHands: 0,
    splitHands: 0,
    grossWon: 0,
    grossLost: 0,
    netChips: 0,
    largestWin: 0,
    largestLoss: 0,
    recentHands: [],
  }),
  lastWarning: null,
});

type Listener = () => void;

class HostBridgeStore {
  readonly #listeners = new Set<Listener>();
  #snapshot: BridgeSnapshot = INITIAL_STATE;
  #previewPoolRun = 0;
  #currentStateSequence = -1;
  #currentStreamId: string | null = null;
  readonly #retiredStreams = new Set<string>();
  #applyingProjection = false;

  constructor() {
    globalThis.addEventListener("token-holdem:snapshot", this.#handleTokenSnapshot);
    globalThis.addEventListener(
      "token-holdem:official-usage-status",
      this.#handleOfficialUsageState,
    );
    globalThis.addEventListener("token-holdem:account-binding", this.#handleAccountBinding);
    globalThis.addEventListener("token-poker:update-status", this.#handleUpdateStatus);
    globalThis.addEventListener("token-holdem:current-state", this.#handleCurrentState);
    globalThis.addEventListener("token-holdem:sidecar", this.#handleSidecarEvent);
    globalThis.addEventListener("token-holdem:resume", this.#handleHostResume);
    for (const rawEvent of globalThis.__tokenHoldemBufferedSidecarEvents ?? []) {
      const event = parseSidecarEvent(rawEvent);
      if (event !== null) this.#applySidecarEvent(event);
    }
    this.#applyCurrentState(globalThis.__tokenHoldemCurrentState);
  }

  readonly subscribe = (listener: Listener): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  readonly getSnapshot = (): BridgeSnapshot => this.#snapshot;

  send(command: HostCommand): CommandResult {
    if (command.type === "request_token_refresh") {
      this.#replace({
        ...this.#snapshot,
        officialUsage: { phase: "loading", error: null },
      });
    }
    const pendingUpdatePhase = {
      check_update: "checking",
      prepare_update: "downloading",
      install_update: "installing",
    } as const;
    if (command.type in pendingUpdatePhase) {
      const updateCommand = command.type as keyof typeof pendingUpdatePhase;
      this.#replace({
        ...this.#snapshot,
        update: {
          ...this.#snapshot.update,
          phase: pendingUpdatePhase[updateCommand],
          error: null,
        },
      });
    }
    if (this.#snapshot.mode === "preview") {
      this.#runPreviewCommand(command);
      return { ok: true };
    }
    if (typeof globalThis.tokenHoldemCommand !== "function") {
      if (command.type === "request_token_refresh") {
        this.#replace({
          ...this.#snapshot,
          officialUsage: { phase: "error", error: bridgeText("bridge.hostUnavailable") },
        });
      }
      if (command.type in pendingUpdatePhase) {
        this.#replace({
          ...this.#snapshot,
          update: {
            ...this.#snapshot.update,
            phase: "error",
            error: bridgeText("bridge.hostUnavailable"),
          },
        });
      }
      return { ok: false, error: bridgeText("bridge.hostUnavailable") };
    }
    try {
      void globalThis.tokenHoldemCommand(JSON.stringify(command));
      return { ok: true };
    } catch (error: unknown) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : bridgeText("bridge.commandFailed"),
      };
    }
  }

  async sendConfirmed(command: HostCommand): Promise<CommandResult> {
    if (this.#snapshot.mode === "preview") {
      this.#runPreviewCommand(command);
      if (["ensure_identity", "create_identity", "restore_identity", "restore_remote_identity"].includes(command.type)) {
        await new Promise((resolve) => globalThis.setTimeout(resolve, PREVIEW_IDENTITY_DELAY_MS));
        const identity = this.#snapshot.identity;
        if (identity === null) return { ok: false, error: bridgeText("bridge.commandFailed") };
        return { ok: true, identity: { playerId: identity.playerId, accountFingerprint: identity.accountFingerprint,
          recoveryEnvelope: identity.recoveryEnvelope, recoverySecretConfirmed: true } };
      }
      return { ok: true };
    }
    if (typeof globalThis.tokenHoldemCommand !== "function") {
      return { ok: false, error: bridgeText("bridge.hostUnavailable") };
    }
    try {
      return await globalThis.tokenHoldemCommand(JSON.stringify(command));
    } catch (error: unknown) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : bridgeText("bridge.commandFailed"),
      };
    }
  }

  #handleTokenSnapshot = (event: Event): void => {
    if (!(event instanceof CustomEvent)) return;
    const snapshot = parseTokenSnapshot(event.detail);
    if (snapshot === null) return;
    this.#replace({
      ...this.#snapshot,
      tokenSnapshot: snapshot,
      officialUsage: { phase: "ready", error: null },
    });
  };

  #handleOfficialUsageState = (event: Event): void => {
    if (!(event instanceof CustomEvent)) return;
    const officialUsage = parseOfficialUsageState(event.detail);
    if (officialUsage === null) return;
    this.#replace({ ...this.#snapshot, officialUsage });
  };

  #handleCurrentState = (event: Event): void => {
    if (!(event instanceof CustomEvent)) return;
    this.#applyCurrentState(event.detail);
  };

  #applyCurrentState(value: unknown): void {
    if (typeof value !== "object" || value === null || Array.isArray(value)) return;
    const record = value as Record<string, unknown>;
    const projection = parseCurrentStateProjection(record, parseSidecarEvent);
    if (projection !== null) {
      if (projection.streamId !== this.#currentStreamId) {
        if (this.#retiredStreams.has(projection.streamId)) return;
        if (this.#currentStreamId !== null) this.#retiredStreams.add(this.#currentStreamId);
        this.#currentStreamId = projection.streamId;
        this.#currentStateSequence = -1;
      }
      if (projection.latestSequence < this.#currentStateSequence) return;
      this.#currentStateSequence = projection.latestSequence;
      this.#applyingProjection = true;
      try {
        this.#replace({ ...this.#snapshot, identity: null, pool: EMPTY_POOL, room: EMPTY_ROOM,
          hand: EMPTY_HAND, statistics: INITIAL_STATE.statistics,
          friendInviteCode: null, friendRoomId: null, friendRoomStatus: "idle" });
        this.#applyCurrentIdentity(record.identity);
        for (const event of projection.events) this.#applySidecarEvent(event);
      } finally {
        this.#applyingProjection = false;
      }
      this.#listeners.forEach((listener) => listener());
      return;
    }
    this.#applyCurrentIdentity(record.identity);
  }

  #applyCurrentIdentity(identity: unknown): void {
    if (identity === null) {
      if (this.#snapshot.identity !== null) {
        this.#replace({ ...this.#snapshot, identity: null });
      }
      return;
    }
    if (typeof identity !== "object" || Array.isArray(identity)) return;
    const event = parseSidecarEvent({ ...identity, type: "identity_ready" });
    if (event?.type === "identity_ready") this.#applySidecarEvent(event);
  }

  #handleAccountBinding = (event: Event): void => {
    if (!(event instanceof CustomEvent)) return;
    const accountBinding = parseAccountBinding(event.detail);
    if (accountBinding === null) return;
    this.#replace({ ...this.#snapshot, accountBinding });
  };

  #handleUpdateStatus = (event: Event): void => {
    if (!(event instanceof CustomEvent)) return;
    const update = parseUpdateStatus(event.detail);
    if (update === null) return;
    this.#replace({ ...this.#snapshot, update });
  };

  #handleSidecarEvent = (event: Event): void => {
    if (!(event instanceof CustomEvent)) return;
    const sidecarEvent = parseSidecarEvent(event.detail);
    if (sidecarEvent === null) return;
    this.#applySidecarEvent(sidecarEvent);
  };

  #handleHostResume = (): void => {
    this.#replace({ ...this.#snapshot });
  };

  #applySidecarEvent(event: SidecarEvent): void {
    switch (event.type) {
      case "ready":
        this.#replace({ ...this.#snapshot, sidecarReady: true, peerId: event.peer_id });
        break;
      case "sidecar_restarting":
        this.#replace({ ...this.#snapshot, sidecarReady: false, peerId: null });
        break;
      case "community_network_loaded":
        this.#replace({
          ...this.#snapshot,
          volunteer: {
            ...this.#snapshot.volunteer,
            directoryRendezvousNodes: event.rendezvous_nodes,
            directoryRelayNodes: event.relay_nodes,
            directoryArchiveNodes: event.archive_nodes,
            coldStartAvailable: event.cold_start_available,
          },
        });
        break;
      case "volunteer_preference_saved":
        this.#replace({
          ...this.#snapshot,
          volunteer: {
            ...this.#snapshot.volunteer,
            consent: event.consent,
            restartRequired: event.restart_required,
          },
          lastWarning: event.restart_required
            ? bridgeText("bridge.volunteerDeferred")
            : null,
        });
        break;
      case "volunteer_status":
        this.#replace({
          ...this.#snapshot,
          volunteer: {
            ...this.#snapshot.volunteer,
            consent: event.consent,
            networkCost: event.network_cost,
            powerSource: event.power_source,
            policyReason: event.policy_reason,
            reachability: event.reachability,
            reachabilityEvidence: event.reachability_evidence,
            role: event.role,
            discoveryServerEnabled: event.discovery_server_enabled,
            relayServerEnabled: event.relay_server_enabled,
            upnpEnabled: event.upnp_enabled,
            activeReservations: event.active_reservations,
            activeCircuits: event.active_circuits,
            maxReservations: event.max_reservations,
            maxCircuits: event.max_circuits,
            maxCircuitDurationSeconds: event.max_circuit_duration_seconds,
            maxCircuitBytes: event.max_circuit_bytes,
            restartRequired: false,
          },
        });
        break;
      case "token_snapshot_accepted":
        this.#replace({
          ...this.#snapshot,
          tokenSnapshot: {
            lifetimeTokens: event.lifetime_tokens,
            username: event.username,
            displayName: event.display_name,
            avatarUrl: event.avatar_url ?? null,
            observedAtUnixMs: event.observed_at_unix_ms,
            source: event.source,
          },
          accountBinding: {
            accountFingerprint: event.account_fingerprint,
            peerVerifiable: event.peer_verifiable,
          },
          officialUsage: { phase: "ready", error: null },
        });
        break;
      case "identity_ready":
        this.#replace({
          ...this.#snapshot,
          identity: {
            accountFingerprint: event.account_fingerprint,
            playerId: event.player_id,
            devicePublicKey: event.device_public_key,
            deviceLabel: event.device_label,
            certificateExpiresAtUnixMs: event.certificate_expires_at_unix_ms,
            recoveryEnvelope: event.recovery_envelope,
            remoteReplicas: event.remote_replicas,
          },
          lastWarning: null,
        });
        break;
      case "identity_cleared":
        this.#replace({ ...this.#snapshot, identity: null, statistics: INITIAL_STATE.statistics });
        break;
      case "peer_connected": {
        const connectedPeers = new Set(this.#snapshot.connectedPeers);
        connectedPeers.add(event.peer_id);
        this.#replace({ ...this.#snapshot, connectedPeers });
        break;
      }
      case "peer_disconnected": {
        const connectedPeers = new Set(this.#snapshot.connectedPeers);
        connectedPeers.delete(event.peer_id);
        this.#replace({ ...this.#snapshot, connectedPeers });
        break;
      }
      case "pool_joined":
        this.#replace({
          ...this.#snapshot,
          pool: {
            status: "searching",
            topic: event.topic,
            levelId: event.level_id,
            buyIn: event.buy_in,
            discoveredTables: 0,
            waitingPlayers: 1,
            targetTableId: null,
          },
          lastWarning: null,
        });
        break;
      case "pool_ticket_published":
        break;
      case "pool_directory_updated":
        this.#replace({
          ...this.#snapshot,
          pool: {
            ...this.#snapshot.pool,
            discoveredTables: event.discovered_tables,
            waitingPlayers: event.waiting_players,
          },
        });
        break;
      case "pool_joining_table":
        this.#replace({
          ...this.#snapshot,
          pool: {
            ...this.#snapshot.pool,
            status: "joining",
            targetTableId: event.table_id,
          },
          lastWarning: null,
        });
        break;
      case "pool_join_attempt_expired":
        this.#replace({
          ...this.#snapshot,
          pool: {
            ...this.#snapshot.pool,
            status: "searching",
            targetTableId: null,
          },
        });
        break;
      case "pool_creating_table":
        this.#replace({
          ...this.#snapshot,
          pool: {
            ...this.#snapshot.pool,
            status: "creating",
            targetTableId: event.table_id,
          },
        });
        break;
      case "pool_table_joined":
        this.#replace({
          ...this.#snapshot,
          pool: {
            ...this.#snapshot.pool,
            status: "in_room",
            targetTableId: event.table_id,
          },
        });
        break;
      case "pool_cancelled":
        this.#replace({
          ...this.#snapshot,
          pool: EMPTY_POOL,
        });
        break;
      case "room_entered":
        this.#replace({
          ...this.#snapshot,
          hand: this.#snapshot.hand.tableId === event.table_id ? this.#snapshot.hand : EMPTY_HAND,
          room: { ...EMPTY_ROOM, tableId: event.table_id, localRole: "joining" },
          lastWarning: null,
        });
        break;
      case "room_snapshot":
        this.#replace({
          ...this.#snapshot,
          room: {
            ...this.#snapshot.room,
            tableId: event.table_id,
            membershipVersion: event.membership_version,
            seats: event.seats.map((seat) => ({
              physicalSeat: seat.physical_seat,
              playerId: seat.player_id,
              buyIn: seat.buy_in,
            })),
            waiting: event.waiting,
            capacity: event.capacity,
            localRole: event.local_role,
            handNumber: event.hand_number,
            nextHandCountdownMs: event.next_hand_countdown_ms,
          },
        });
        break;
      case "membership_confirmation":
        this.#replace({
          ...this.#snapshot,
          room: {
            ...this.#snapshot.room,
            membershipConfirmed: event.confirmed,
            membershipRequired: event.required,
          },
        });
        break;
      case "hand_roster_confirmation":
        this.#replace({
          ...this.#snapshot,
          room: {
            ...this.#snapshot.room,
            rosterConfirmed: event.confirmed,
            rosterRequired: event.required,
          },
        });
        break;
      case "next_hand_ready":
        this.#replace({
          ...this.#snapshot,
          room: {
            ...this.#snapshot.room,
            handNumber: event.hand_number,
            rosterConfirmed: event.players,
            rosterRequired: event.players,
            nextHandCountdownMs: 0,
          },
        });
        break;
      case "safe_leave_requested":
        this.#replace({
          ...this.#snapshot,
          room: {
            ...this.#snapshot.room,
            localRole: "leaving",
            safeLeaveAfterHand: event.after_hand_number,
            safeLeaveForceAfterUnixMs: event.force_after_unix_ms,
          },
        });
        break;
      case "safe_leave_forced":
        this.#replace({
          ...this.#snapshot,
          lastWarning: bridgeText("bridge.safeLeaveForced"),
        });
        break;
      case "safe_leave_completed":
        this.#replace({
          ...this.#snapshot,
          pool: EMPTY_POOL,
          room: EMPTY_ROOM,
          hand: EMPTY_HAND,
          friendRoomId: null,
          friendRoomStatus: "idle",
        });
        break;
      case "room_closed":
        this.#replace({
          ...this.#snapshot,
          room: EMPTY_ROOM,
          hand: EMPTY_HAND,
          friendRoomId: null,
          friendRoomStatus: "idle",
        });
        break;
      case "hand_protocol_started":
        if (!shouldAcceptHandStart(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...EMPTY_HAND,
            phase: "key_exchange",
            tableId: event.table_id,
            handNumber: event.hand_number,
            localSeat: event.seat,
            dealerSeat: event.dealer_seat,
            players: event.players,
            levelId: event.level_id,
            smallBlind: event.small_blind,
            bigBlind: event.big_blind,
            buyIns: event.buy_ins,
            progressRequired: event.players.length,
          },
          lastWarning: null,
        });
        break;
      case "hand_protocol_progress":
        if (!shouldAcceptHandProgress(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...this.#snapshot.hand,
            phase: event.phase,
            tableId: event.table_id,
            handNumber: event.hand_number,
            progressCompleted: event.completed,
            progressRequired: event.required,
          },
        });
        break;
      case "hand_ready":
        if (!shouldAcceptHandReady(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...this.#snapshot.hand,
            phase: phaseAfterHandReady(this.#snapshot.hand.phase),
            tableId: event.table_id,
            handNumber: event.hand_number,
            localSeat: event.seat,
            holeCards: event.hole_cards,
            transcriptHash: event.transcript_hash,
          },
          lastWarning: null,
        });
        break;
      case "hand_state":
        if (!shouldAcceptHandState(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...this.#snapshot.hand,
            phase: phaseAfterHandState(
              this.#snapshot.hand.phase,
              event.awaiting_reveal,
            ),
            tableId: event.table_id,
            handNumber: event.hand_number,
            localSeat: event.local_seat,
            board: event.board,
            sequence: event.sequence,
            publicStateHash: event.public_state_hash,
            street: event.street,
            pot: event.pot,
            currentBet: event.current_bet,
            nextSeat: event.next_seat,
            toCall: event.to_call,
            minimumRaiseTo: event.minimum_raise_to,
            maximumRaiseTo: event.maximum_raise_to,
            canAct: event.can_act,
            awaitingReveal: event.awaiting_reveal,
            actionTimeoutMs: event.action_timeout_ms,
            turnDeadlineUnixMs: event.turn_deadline_unix_ms,
            seats: event.seats.map((seat) => ({
              seat: seat.seat,
              playerId: seat.player_id,
              stack: seat.stack,
              committed: seat.committed,
              status: seat.status,
              lastAction: seat.last_action,
            })),
            pendingSequence: null,
            transcriptHash: event.transcript_hash,
          },
        });
        break;
      case "hand_action_conflict":
        if (!isCurrentHand(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...this.#snapshot.hand,
            phase: "conflicted",
            canAct: false,
            pendingSequence: null,
          },
          lastWarning: bridgeText("bridge.actionConflict", {
            sequence: event.sequence,
            accepted: event.accepted_action_hash.slice(0, 10),
            conflicting: event.conflicting_action_hash.slice(0, 10),
          }),
        });
        break;
      case "hand_settled":
        if (!isCurrentHand(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...this.#snapshot.hand,
            phase: "settled",
            canAct: false,
            awaitingReveal: false,
            pendingSequence: null,
            transcriptHash: event.transcript_hash,
            outcomes: event.outcomes.map((outcome) => ({
              seat: outcome.seat,
              playerId: outcome.player_id,
              startingStack: outcome.starting_stack,
              endingStack: outcome.ending_stack,
              delta: outcome.delta,
            })),
            receiptStatus: "signing",
          },
        });
        break;
      case "receipt_consensus_progress":
        if (!isCurrentHand(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...this.#snapshot.hand,
            phase: "receipt_consensus",
            canAct: false,
            receiptStatus: "signing",
            receiptSigned: event.signed,
            receiptRequired: event.required,
          },
        });
        break;
      case "receipt_finalized":
        if (!isCurrentHand(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...this.#snapshot.hand,
            phase: "between_hands",
            canAct: false,
            receiptStatus: "finalized",
            receiptSigned: event.signatures,
            receiptRequired: event.signatures,
            receiptId: event.receipt_id,
          },
        });
        break;
      case "hand_session_interrupted":
        if (!isCurrentHand(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...this.#snapshot.hand,
            phase: "interrupted",
            sessionInterrupted: true,
          },
          lastWarning: bridgeText("bridge.participantDisconnected", {
            peer: event.peer_id.slice(0, 12),
          }),
        });
        break;
      case "hand_session_resumed":
        if (!isCurrentHand(this.#snapshot.hand, event)) break;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...this.#snapshot.hand,
            phase: this.#snapshot.hand.awaitingReveal ? "revealing" : "playing",
            sessionInterrupted: false,
          },
          lastWarning: null,
        });
        break;
      case "hand_left":
        this.#replace({ ...this.#snapshot, hand: EMPTY_HAND });
        break;
      case "hand_aborted_for_leave":
        this.#replace({
          ...this.#snapshot,
          hand: EMPTY_HAND,
          lastWarning: bridgeText("bridge.handAbortedForLeave", {
            player: event.player_id.slice(0, 12),
          }),
        });
        break;
      case "friend_room_created":
        this.#replace({
          ...this.#snapshot,
          friendInviteCode: event.invite_code,
          friendRoomId: event.room_id,
          friendRoomStatus: "created",
          lastWarning: null,
        });
        break;
      case "warning":
        if (this.#isObsoleteNetworkWarning(event.message)) break;
        this.#replace({ ...this.#snapshot, lastWarning: event.message });
        break;
      case "friend_room_joining":
        this.#replace({
          ...this.#snapshot,
          friendRoomId: event.room_id,
          friendRoomStatus: "joining",
          lastWarning: null,
        });
        break;
      case "friend_room_joined":
        this.#replace({
          ...this.#snapshot,
          friendRoomId: event.room_id,
          friendRoomStatus: "joined",
          lastWarning: null,
        });
        break;
      case "archive_node_ready":
        this.#replace({
          ...this.#snapshot,
          archive: { ...this.#snapshot.archive, nodePublicKey: event.public_key },
        });
        break;
      case "archive_peers_configured":
        this.#replace({
          ...this.#snapshot,
          archive: {
            ...this.#snapshot.archive,
            peers: event.peers,
            minimumConfirmedReplicas: event.minimum_confirmed_replicas,
          },
        });
        break;
      case "receipt_archive_pending":
        this.#replace({
          ...this.#snapshot,
          archive: {
            ...this.#snapshot.archive,
            lastAddress: event.address,
            lastStatus: "archiving",
            lastError: null,
            confirmedReplicas: 0,
          },
        });
        break;
      case "receipt_archived":
        this.#replace({
          ...this.#snapshot,
          archive: {
            ...this.#snapshot.archive,
            lastAddress: event.address,
            lastStatus: "archived",
            lastError: null,
            confirmedReplicas: event.confirmed_replicas,
          },
        });
        break;
      case "receipt_archive_failed":
        this.#replace({
          ...this.#snapshot,
          archive: {
            ...this.#snapshot.archive,
            lastAddress: event.address,
            lastStatus: "failed",
            lastError: event.reason,
            confirmedReplicas: 0,
          },
        });
        break;
      case "recovery_backup_pending":
        this.#replace({ ...this.#snapshot, lastWarning: null });
        break;
      case "recovery_backup_stored":
      case "recovery_backup_fetched":
        this.#replace({ ...this.#snapshot, lastWarning: null });
        break;
      case "recovery_backup_failed":
        this.#replace({ ...this.#snapshot, lastWarning: event.reason });
        break;
      case "discovery_configured":
        this.#replace({
          ...this.#snapshot,
          discovery: {
            nodes: event.nodes,
            namespace: event.namespace,
            registeredNodes: new Set<string>(),
            lastDiscoveredPeers: 0,
          },
          lastWarning: null,
        });
        break;
      case "rendezvous_registered": {
        const registeredNodes = new Set(this.#snapshot.discovery.registeredNodes);
        registeredNodes.add(event.node);
        this.#replace({
          ...this.#snapshot,
          discovery: { ...this.#snapshot.discovery, registeredNodes },
          lastWarning: null,
        });
        break;
      }
      case "peers_discovered":
        this.#replace({
          ...this.#snapshot,
          discovery: { ...this.#snapshot.discovery, lastDiscoveredPeers: event.peers },
          lastWarning: null,
        });
        break;
      case "statistics_updated":
        this.#replace({
          ...this.#snapshot,
          statistics: {
            completedHands: event.completed_hands,
            wonHands: event.won_hands,
            lostHands: event.lost_hands,
            splitHands: event.split_hands,
            grossWon: event.gross_won,
            grossLost: event.gross_lost,
            netChips: event.net_chips,
            largestWin: event.largest_win,
            largestLoss: event.largest_loss,
            recentHands: event.recent_hands.map((hand) => ({
              address: hand.address,
              receiptId: hand.receipt_id,
              handNumber: hand.hand_number,
              levelId: hand.level_id,
              players: hand.players,
              settledAtUnixMs: hand.settled_at_unix_ms,
              delta: hand.delta,
              archived: hand.archived,
            })),
          },
        });
        break;
      case "listen_address":
      case "advertised_address_added":
      case "rendezvous_candidate_added":
      case "relay_candidate_added":
      case "relay_reservation_requested":
      case "relay_reservation_accepted":
      case "relay_circuit_established":
      case "relay_server_reservation":
      case "relay_server_circuit":
      case "receipt_fetched":
      case "archive_index_received":
      case "shutdown_complete":
        break;
    }
  }

  #isObsoleteNetworkWarning(message: string): boolean {
    if (this.#snapshot.discovery.registeredNodes.size === 0) return false;
    return (
      message === "社区引导节点暂时不可达；客户端会自动重试" ||
      (/^连接失败[（(]/u.test(message) &&
        (message.includes("Failed to negotiate transport protocol") ||
          message.includes("Multiple dial errors occurred")))
    );
  }

  #runPreviewCommand(command: HostCommand): void {
    switch (command.type) {
      case "join_public_pool": {
        const previewPlayers = import.meta.env.DEV &&
          new URLSearchParams(globalThis.location.search).get("visual-players") === "6" ? 6 : 2;
        const smallBlindIndex = previewPlayers === 2 ? 0 : 1;
        const bigBlindIndex = smallBlindIndex + 1;
        const previewLevel = findStakeLevel(command.level_id);
        this.#previewPoolRun += 1;
        this.#schedulePreviewPool(this.#previewPoolRun, 360, {
          type: "pool_joined",
          topic: `/token-holdem/table-pool/2/${command.level_id}`,
          level_id: command.level_id,
          buy_in: command.buy_in,
        });
        this.#schedulePreviewPool(this.#previewPoolRun, 760, {
          type: "pool_directory_updated",
          discovered_tables: 0,
          waiting_players: previewPlayers,
        });
        this.#schedulePreviewPool(this.#previewPoolRun, 1_080, {
          type: "pool_creating_table",
          table_id: "8f3c58d35e4a9db2-preview-table",
        });
        this.#schedulePreviewPool(this.#previewPoolRun, 1_260, {
          type: "room_entered",
          table_id: "8f3c58d35e4a9db2-preview-table",
          level_id: command.level_id,
        });
        this.#schedulePreviewPool(this.#previewPoolRun, 1_520, {
          type: "room_snapshot",
          table_id: "8f3c58d35e4a9db2-preview-table",
          membership_version: 2,
          seats: Array.from({ length: previewPlayers }, (_, index) => ({
            physical_seat: index + 1,
            player_id: `preview-${String(index)}`,
            buy_in: command.buy_in,
          })),
          waiting: [],
          capacity: 6,
          local_role: "seated",
          hand_number: null,
          next_hand_countdown_ms: 1_500,
        });
        this.#schedulePreviewPool(this.#previewPoolRun, 1_900, {
          type: "hand_protocol_started",
          table_id: "8f3c58d35e4a9db2-preview-table",
          hand_number: 1,
          seat: 1,
          dealer_seat: 1,
          players: Array.from({ length: previewPlayers }, (_, index) => `preview-${String(index)}`),
          level_id: command.level_id,
          small_blind: previewLevel.smallBlind,
          big_blind: previewLevel.bigBlind,
          buy_ins: Array.from({ length: previewPlayers }, () => command.buy_in),
        });
        this.#schedulePreviewPool(this.#previewPoolRun, 2_120, {
          type: "hand_protocol_progress",
          table_id: "8f3c58d35e4a9db2-preview-table",
          hand_number: 1,
          phase: "shuffling",
          completed: previewPlayers,
          required: previewPlayers,
        });
        this.#schedulePreviewPool(this.#previewPoolRun, 2_420, {
          type: "hand_protocol_progress",
          table_id: "8f3c58d35e4a9db2-preview-table",
          hand_number: 1,
          phase: "dealing",
          completed: previewPlayers,
          required: previewPlayers,
        });
        this.#schedulePreviewPool(this.#previewPoolRun, 2_700, {
          type: "hand_ready",
          table_id: "8f3c58d35e4a9db2-preview-table",
          hand_number: 1,
          seat: 1,
          hole_cards: [
            { rank: 2, suit: "diamond" },
            { rank: 14, suit: "diamond" },
          ],
          transcript_hash: "8f3c58d35e4a9db2b6a00d54a1a8d88a",
        });
        this.#schedulePreviewPool(this.#previewPoolRun, 2_720, {
          type: "hand_state",
          public_state_hash: "0".repeat(64),
          table_id: "8f3c58d35e4a9db2-preview-table",
          hand_number: 1,
          sequence: 0,
          street: "preflop",
          pot: previewLevel.smallBlind + previewLevel.bigBlind,
          current_bet: previewLevel.bigBlind,
          next_seat: 1,
          local_seat: 1,
          to_call: previewPlayers === 2 ? previewLevel.smallBlind : previewLevel.bigBlind,
          minimum_raise_to: previewLevel.bigBlind * 2,
          maximum_raise_to: command.buy_in,
          can_act: true,
          awaiting_reveal: false,
          action_timeout_ms: 30_000,
          turn_deadline_unix_ms: Date.now() + 30_000,
          board: [],
          seats: Array.from({ length: previewPlayers }, (_, index) => ({
            seat: index + 1,
            player_id: `preview-${String(index)}`,
            stack:
              command.buy_in -
              (index === smallBlindIndex ? previewLevel.smallBlind : index === bigBlindIndex ? previewLevel.bigBlind : 0),
            committed:
              index === smallBlindIndex ? previewLevel.smallBlind : index === bigBlindIndex ? previewLevel.bigBlind : 0,
            status: "active" as const,
            last_action: null,
          })),
          transcript_hash: "8f3c58d35e4a9db2b6a00d54a1a8d88a",
        });
        break;
      }
      case "cancel_public_pool":
        this.#previewPoolRun += 1;
        this.#applySidecarEvent({ type: "pool_cancelled" });
        break;
      case "create_friend_room":
        globalThis.setTimeout(() => {
          this.#applySidecarEvent({
            type: "friend_room_created",
            invite_code: "TH1-PREVIEW-8F3K-2D7Q",
            room_id: "preview-room",
            buy_in: command.buy_in,
            expires_at_unix_ms: Date.now() + 1_800_000,
          });
        }, 420);
        break;
      case "join_friend_room":
        globalThis.setTimeout(() => {
          this.#applySidecarEvent({
            type: "friend_room_joining",
            room_id: "preview-room",
            host_peer_id: "12D3KooW-preview-host",
          });
          globalThis.setTimeout(() => {
            this.#applySidecarEvent({
              type: "friend_room_joined",
              room_id: "preview-room",
              host_peer_id: "12D3KooW-preview-host",
            });
          }, 420);
        }, 420);
        break;
      case "ensure_identity":
      case "create_identity":
      case "restore_identity":
      case "restore_remote_identity":
        globalThis.setTimeout(() => {
          this.#applySidecarEvent({
            type: "identity_ready",
            account_fingerprint: PREVIEW_ACCOUNT_FINGERPRINT,
            player_id: "7ab3e9114f8924a7ce5510d23f9b8af64812cfc3f234c934dea155079b0025e",
            device_public_key: "9c71ec2da18e68f29ac3aa06b24f0ea34df182d4f4b22cc09c5aa42dce74fd31",
            device_label: command.device_label,
            certificate_expires_at_unix_ms: Date.now() + 31_536_000_000,
            recovery_envelope: "THR1-PREVIEW-ENCRYPTED-RECOVERY-PACKAGE",
            remote_replicas: 0,
          });
        }, PREVIEW_IDENTITY_DELAY_MS);
        break;
      case "configure_archive_nodes":
        this.#applySidecarEvent({
          type: "archive_peers_configured",
          peers: command.addresses.map((_, index) => `preview-archive-${String(index + 1)}`),
          minimum_confirmed_replicas: command.minimum_confirmed_replicas,
        });
        break;
      case "use_relay":
        this.#applySidecarEvent({
          type: "relay_reservation_requested",
          peer_id: "12D3KooW-preview-relay",
          address: command.address,
        });
        break;
      case "set_volunteer_consent": {
        const consent = command.enabled ? "granted" : "declined";
        this.#replace({
          ...this.#snapshot,
          volunteer: {
            ...this.#snapshot.volunteer,
            consent,
            policyReason: command.enabled ? "eligible" : "declined",
            role: command.enabled ? "active_discovery_relay" : "disabled",
            discoveryServerEnabled: command.enabled,
            relayServerEnabled: command.enabled,
            upnpEnabled: command.enabled,
            activeReservations: command.enabled ? 2 : 0,
            activeCircuits: command.enabled ? 1 : 0,
            restartRequired: false,
          },
          lastWarning: null,
        });
        break;
      }
      case "configure_discovery":
        this.#applySidecarEvent({
          type: "discovery_configured",
          nodes: command.addresses.map((_, index) => `preview-discovery-${String(index + 1)}`),
          namespace: command.namespace ?? "token-holdem/v1",
        });
        break;
      case "add_external_address":
        this.#applySidecarEvent({ type: "advertised_address_added", address: command.address });
        break;
      case "request_token_refresh":
        this.#replace({
          ...this.#snapshot,
          tokenSnapshot: PREVIEW_SNAPSHOT,
          officialUsage: { phase: "ready", error: null },
        });
        break;
      case "check_update":
        this.#replace({
          ...this.#snapshot,
          update: {
            ...this.#snapshot.update,
            phase: HAS_PREVIEW_UPDATE ? "available" : "current",
            error: null,
          },
        });
        break;
      case "prepare_update": {
        if (!HAS_PREVIEW_UPDATE) {
          this.#replace({
            ...this.#snapshot,
            update: {
              ...this.#snapshot.update,
              phase: "error",
              error: "No preview update is configured",
            },
          });
          break;
        }
        globalThis.setTimeout(() => {
          this.#replace({
            ...this.#snapshot,
            update: {
              ...this.#snapshot.update,
              phase: "ready",
              downloadedBytes: this.#snapshot.update.artifactBytes ?? 0,
              sha256Verified: true,
              error: null,
            },
          });
        }, 260);
        break;
      }
      case "install_update":
        globalThis.setTimeout(() => {
          this.#replace({
            ...this.#snapshot,
            update: {
              ...this.#snapshot.update,
              phase: "restart_required",
              error: null,
            },
          });
        }, 260);
        break;
      case "sync_statistics":
        break;
      case "submit_action": {
        const hand = this.#snapshot.hand;
        const localSeat = hand.localSeat;
        const localState =
          localSeat === null ? undefined : hand.seats.find((seat) => seat.seat === localSeat);
        const contribution =
          command.action === "raise"
            ? Math.max(0, (command.amount ?? hand.currentBet) - (localState?.committed ?? 0))
            : command.action === "call"
              ? hand.toCall
              : 0;
        this.#replace({
          ...this.#snapshot,
          hand: {
            ...hand,
            canAct: false,
            pendingSequence: hand.sequence + 1,
          },
        });
        globalThis.setTimeout(() => {
          const current = this.#snapshot.hand;
          const nextSeat =
            current.nextSeat === null
              ? null
              : (current.nextSeat % Math.max(2, current.players.length)) + 1;
          this.#replace({
            ...this.#snapshot,
            hand: {
              ...current,
              sequence: current.sequence + 1,
              canAct: false,
              pendingSequence: null,
              pot: current.pot + contribution,
              nextSeat,
              turnDeadlineUnixMs:
                nextSeat === null ? null : Date.now() + current.actionTimeoutMs,
              seats: current.seats.map((seat) =>
                seat.seat === localSeat
                  ? {
                      ...seat,
                      stack: Math.max(0, seat.stack - contribution),
                      committed: seat.committed + contribution,
                      status: command.action === "fold" ? "folded" : seat.status,
                      lastAction: command.action,
                    }
                  : seat,
              ),
            },
          });
        }, 260);
        break;
      }
      case "leave_table":
        this.#previewPoolRun += 1;
        this.#replace({
          ...this.#snapshot,
          pool: EMPTY_POOL,
          room: EMPTY_ROOM,
          hand: EMPTY_HAND,
          friendRoomId: null,
          friendRoomStatus: "idle",
        });
        break;
      case "close_ui":
        document.getElementById("token-holdem-host")?.remove();
        break;
    }
  }

  #schedulePreviewPool(run: number, delay: number, event: SidecarEvent): void {
    globalThis.setTimeout(() => {
      if (run === this.#previewPoolRun) this.#applySidecarEvent(event);
    }, delay);
  }

  #replace(snapshot: BridgeSnapshot): void {
    this.#snapshot = Object.freeze(snapshot);
    if (!this.#applyingProjection) this.#listeners.forEach((listener) => listener());
  }
}

const bridgeStore = new HostBridgeStore();
const sendHostCommand = (command: HostCommand): CommandResult => bridgeStore.send(command);
const sendConfirmedHostCommand = (command: HostCommand): Promise<CommandResult> =>
  bridgeStore.sendConfirmed(command);

export function useHostBridge(): readonly [
  BridgeSnapshot,
  (command: HostCommand) => CommandResult,
  (command: HostCommand) => Promise<CommandResult>,
] {
  const snapshot = useSyncExternalStore(bridgeStore.subscribe, bridgeStore.getSnapshot);
  return [snapshot, sendHostCommand, sendConfirmedHostCommand] as const;
}
