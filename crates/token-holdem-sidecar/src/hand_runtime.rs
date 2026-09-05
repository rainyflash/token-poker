use crate::{receipt_runtime::ReceiptConsensus, table_session_runtime::ReadyTable};
use anyhow::{Context, Result};
use libp2p::{
    gossipsub,
    request_response::OutboundRequestId,
    swarm::dial_opts::{DialOpts, PeerCondition},
    PeerId,
};
use rand_core::OsRng;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use token_holdem_domain::{
    ActionOutcome, Card, Chips, HandOutcome, HandReceipt, HoldemHand, HoldemSettlement, MatchId,
    PhysicalSeat, PlayerAction, PlayerId, SeatStatus, Street, Suit, TranscriptHash,
};
use token_holdem_identity::{
    CoSignedReceipt, DeviceCertificate, DeviceIdentity, ParticipantSignature,
};
use token_holdem_mental_poker::{
    AggregateKey, KeyAnnouncement, MentalPokerEngine, PlayerKeyMaterial, ProtocolTranscript,
    RevealSharePacket, ShufflePacket, VerifiedDeck, VerifiedParticipantKey,
};
use token_holdem_network::{
    ControlRequest, HandPrivateMessage, HandPublicMessage, NetworkBehaviour, SignedHandAction,
    TABLE_TOPIC_PREFIX,
};

const MAX_HAND_MESSAGE_BYTES: usize = 256 * 1_024;
const PUBLIC_HAND_STATE_DOMAIN: &[u8] = b"token-holdem/public-hand-state/v1\0";
const PUBLIC_RETRY_INTERVAL_TICKS: u32 = 4;
// Broadcast public messages once; per-peer request queues provide reliability.
// Re-signing retransmissions makes Gossipsub treat one semantic message as many
// messages and creates pointless amplification at a six-player table.
const MAX_PUBLIC_MESSAGE_ATTEMPTS: u8 = 1;
const DIRECT_RETRY_INTERVAL_TICKS: u32 = 4;
const MAX_DIRECT_MESSAGE_ATTEMPTS: u8 = 24;
const NON_OWNER_DIAL_FALLBACK_TICKS: u32 = 4;
const MAX_EARLY_HAND_MESSAGES: usize = 128;
const TURN_ACTION_TIMEOUT_MS: u64 = 30_000;
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum HandEvent {
    HandProtocolStarted {
        table_id: String,
        hand_number: u64,
        seat: u8,
        dealer_seat: u8,
        players: Vec<String>,
        level_id: String,
        small_blind: u64,
        big_blind: u64,
        buy_ins: Vec<u64>,
    },
    HandProtocolProgress {
        table_id: String,
        hand_number: u64,
        phase: &'static str,
        completed: u8,
        required: u8,
    },
    HandReady {
        table_id: String,
        hand_number: u64,
        seat: u8,
        hole_cards: [VisibleCard; 2],
        transcript_hash: String,
    },
    HandState {
        table_id: String,
        hand_number: u64,
        sequence: u64,
        street: &'static str,
        pot: u64,
        current_bet: u64,
        next_seat: Option<u8>,
        local_seat: u8,
        to_call: u64,
        minimum_raise_to: u64,
        maximum_raise_to: u64,
        can_act: bool,
        awaiting_reveal: bool,
        action_timeout_ms: u64,
        turn_deadline_unix_ms: Option<u64>,
        board: Vec<VisibleCard>,
        seats: Vec<VisibleSeat>,
        transcript_hash: String,
        public_state_hash: String,
        betting_state_hash: String,
        mental_transcript_hash: String,
        action_transcript_hash: String,
    },
    HandActionConflict {
        table_id: String,
        hand_number: u64,
        sequence: u64,
        accepted_action_hash: String,
        conflicting_action_hash: String,
    },
    HandSettled {
        table_id: String,
        hand_number: u64,
        outcomes: Vec<VisibleOutcome>,
        transcript_hash: String,
    },
    ReceiptConsensusProgress {
        table_id: String,
        hand_number: u64,
        signed: u8,
        required: u8,
    },
    ReceiptFinalized {
        table_id: String,
        hand_number: u64,
        receipt_id: String,
        local_delta: i128,
        signatures: u8,
    },
    HandSessionInterrupted {
        table_id: String,
        hand_number: u64,
        peer_id: String,
    },
    HandSessionResumed {
        table_id: String,
        hand_number: u64,
        peer_id: String,
    },
    HandLeft,
    Warning {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct VisibleCard {
    rank: u8,
    suit: &'static str,
}

impl From<Card> for VisibleCard {
    fn from(card: Card) -> Self {
        Self {
            rank: card.rank(),
            suit: match card.suit() {
                Suit::Clubs => "club",
                Suit::Diamonds => "diamond",
                Suit::Hearts => "heart",
                Suit::Spades => "spade",
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct VisibleSeat {
    seat: u8,
    player_id: String,
    stack: u64,
    committed: u64,
    status: &'static str,
    last_action: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct VisibleOutcome {
    seat: u8,
    player_id: String,
    starting_stack: u64,
    ending_stack: u64,
    delta: i128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RevealReason {
    Street,
    Showdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionOrigin {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy)]
struct ActionConflictEvidence {
    sequence: u64,
    accepted_action_hash: [u8; 32],
    conflicting_action_hash: [u8; 32],
}

struct PendingReveal {
    reason: RevealReason,
    indices: Vec<u8>,
}

struct RetriedPublicMessage {
    message: HandPublicMessage,
    attempts: u8,
    next_retry_tick: u32,
}

struct RetriedDirectMessage {
    peer: PeerId,
    message: ReliableHandMessage,
    attempts: u8,
    next_retry_tick: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReliableHandMessage {
    Public(Box<HandPublicMessage>),
    Private(HandPrivateMessage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BufferedInboundHandMessage {
    Public(Box<HandPublicMessage>),
    Private(HandPrivateMessage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BufferedInboundHandDelivery {
    source: PeerId,
    message: BufferedInboundHandMessage,
}

impl ReliableHandMessage {
    fn into_control_request(self) -> ControlRequest {
        match self {
            Self::Public(message) => ControlRequest::HandPublic(*message),
            Self::Private(message) => ControlRequest::HandPrivate(message),
        }
    }
}

struct ActiveHand {
    table: ReadyTable,
    topic: String,
    hand_number: u64,
    dealer_index: usize,
    engine: MentalPokerEngine,
    local_key: PlayerKeyMaterial,
    announcements: BTreeMap<u8, KeyAnnouncement>,
    verified_keys: Option<Vec<VerifiedParticipantKey>>,
    aggregate_key: Option<AggregateKey>,
    shuffle_packets: BTreeMap<u8, ShufflePacket>,
    shuffle_stage: u8,
    deck: Option<VerifiedDeck>,
    hole_distribution_started: bool,
    direct_outbox: Vec<RetriedDirectMessage>,
    direct_requests: HashMap<OutboundRequestId, (PeerId, ReliableHandMessage)>,
    hole_shares: BTreeMap<u8, BTreeMap<u8, RevealSharePacket>>,
    local_hole_cards: Option<[Card; 2]>,
    ready_seats: BTreeSet<u8>,
    public_shares: BTreeMap<u8, BTreeMap<u8, RevealSharePacket>>,
    public_cards: BTreeMap<u8, Card>,
    pending_reveal: Option<PendingReveal>,
    hand: Option<HoldemHand>,
    sequence: u64,
    committed_actions: BTreeMap<u64, SignedHandAction>,
    pending_actions: BTreeMap<u64, SignedHandAction>,
    action_conflict: Option<ActionConflictEvidence>,
    public_outbox: Vec<RetriedPublicMessage>,
    relayed_public_messages: BTreeSet<[u8; 32]>,
    mental_transcript: ProtocolTranscript,
    action_transcript: blake3::Hasher,
    last_action_at_unix_ms: Option<u64>,
    turn_started_at_unix_ms: Option<u64>,
    last_actions: BTreeMap<u8, PlayerAction>,
    receipt_consensus: Option<ReceiptConsensus>,
    pending_receipt_signatures: BTreeMap<PlayerId, (HandReceipt, ParticipantSignature)>,
    finalized_receipt: Option<CoSignedReceipt>,
    finalized_receipt_exported: bool,
    boundary_exported: bool,
    safe_leave_requested: bool,
    disconnected_peers: BTreeMap<PeerId, u32>,
    settled: bool,
    tick_count: u32,
}

#[derive(Default)]
pub(crate) struct HandRuntime {
    active: Option<ActiveHand>,
    pending_exports: Vec<CoSignedReceipt>,
    early_messages: Vec<BufferedInboundHandDelivery>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HandBoundary {
    pub(crate) receipt_hash: [u8; 32],
    pub(crate) dealer_seat: PhysicalSeat,
}

#[derive(Clone, Copy)]
pub(crate) struct HandExecutionContext<'a> {
    now_unix_ms: u64,
    device: &'a DeviceIdentity,
    certificate: &'a DeviceCertificate,
}

impl<'a> HandExecutionContext<'a> {
    pub(crate) const fn new(
        now_unix_ms: u64,
        device: &'a DeviceIdentity,
        certificate: &'a DeviceCertificate,
    ) -> Self {
        Self {
            now_unix_ms,
            device,
            certificate,
        }
    }
}

impl HandRuntime {
    pub(crate) fn start(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        table: ReadyTable,
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
        now_unix_ms: u64,
    ) -> Result<Vec<HandEvent>> {
        register_roster_addresses(swarm, &table)?;
        let table_id = hex::encode(table.table_id);
        let topic = format!("{TABLE_TOPIC_PREFIX}{table_id}");
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&gossipsub::IdentTopic::new(topic.clone()))
            .with_context(|| format!("无法订阅牌桌频道：{topic}"))?;
        let hand_number = table.hand_number;
        let dealer_index = table.dealer_index;
        let execution = HandExecutionContext::new(now_unix_ms, device, certificate);
        let (active, mut events) =
            initialize_hand(swarm, table, topic, hand_number, dealer_index, execution)?;
        if let Some(mut completed) = self.active.take() {
            let receipt = completed
                .finalized_receipt
                .take()
                .context("启动下一手时上一手尚未形成联合签名凭证")?;
            if !completed.finalized_receipt_exported {
                self.pending_exports.push(receipt);
            }
        }
        self.active = Some(active);
        let early_messages = std::mem::take(&mut self.early_messages);
        for delivery in early_messages {
            let result = match delivery.message {
                BufferedInboundHandMessage::Public(message) => {
                    self.handle_direct_public(swarm, delivery.source, *message, execution)
                }
                BufferedInboundHandMessage::Private(message) => self.handle_private(
                    swarm,
                    delivery.source,
                    message,
                    now_unix_ms,
                    device,
                    certificate,
                ),
            };
            match result {
                Ok(buffered_events) => events.extend(buffered_events),
                Err(error) => events.push(HandEvent::Warning {
                    message: format!("已丢弃与新手牌不匹配的提前消息：{error:#}"),
                }),
            }
        }
        Ok(events)
    }

    pub(crate) fn tick(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
        now_unix_ms: u64,
    ) -> Result<Vec<HandEvent>> {
        let Some(active) = self.active.as_mut() else {
            return Ok(Vec::new());
        };
        active.tick_count = active.tick_count.saturating_add(1);
        // Public shuffling also depends on the live topology, so connection
        // maintenance cannot wait until private dealing. Once the hand roster is
        // active, keep the same small full mesh for the entire hand.
        ensure_roster_connections(
            swarm,
            &active.table,
            &active.disconnected_peers,
            active.tick_count,
        )?;
        active.retry_public_messages(swarm)?;
        active.retry_direct_messages(swarm);
        let mut events = active.drive(swarm, device, certificate, now_unix_ms)?;
        if let Some(action) = active.automatic_local_action(now_unix_ms)? {
            events.extend(active.commit_local_action(
                swarm,
                action,
                now_unix_ms,
                device,
                certificate,
            )?);
            events.extend(active.drive(swarm, device, certificate, now_unix_ms)?);
        }
        Ok(events)
    }

    pub(crate) fn leave(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    ) -> Option<HandEvent> {
        let active = self.active.take()?;
        swarm
            .behaviour_mut()
            .gossipsub
            .unsubscribe(&gossipsub::IdentTopic::new(active.topic));
        Some(HandEvent::HandLeft)
    }

    pub(crate) fn abort_for_signed_leave(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        table_id: &str,
        hand_number: u64,
    ) -> Result<()> {
        let active = self.active.as_ref().context("待作废的手牌运行时不存在")?;
        anyhow::ensure!(
            active.table_id_string() == table_id && active.hand_number == hand_number,
            "签名离桌证据与当前手牌范围不一致"
        );
        let active = self.active.take().context("待作废的手牌运行时丢失")?;
        swarm
            .behaviour_mut()
            .gossipsub
            .unsubscribe(&gossipsub::IdentTopic::new(active.topic));
        Ok(())
    }

    pub(crate) fn handle_public(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        topic: &str,
        source: Option<PeerId>,
        payload: &[u8],
        execution: HandExecutionContext<'_>,
    ) -> Result<Vec<HandEvent>> {
        let active = self.active.as_mut().context("尚未建立牌桌会话")?;
        if topic != active.topic {
            return Ok(Vec::new());
        }
        if payload.is_empty() || payload.len() > MAX_HAND_MESSAGE_BYTES {
            anyhow::bail!("牌桌消息必须为 1 字节到 256 KiB");
        }
        let message: HandPublicMessage =
            cbor4ii::serde::from_slice(payload).context("牌桌消息不是合法 CBOR")?;
        active.handle_public_message(swarm, source, message, execution)
    }

    pub(crate) fn handle_direct_public(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        source: PeerId,
        message: HandPublicMessage,
        execution: HandExecutionContext<'_>,
    ) -> Result<Vec<HandEvent>> {
        if self.should_buffer_next_hand(message.table_id(), message.hand_number()) {
            self.buffer_early_message(BufferedInboundHandDelivery {
                source,
                message: BufferedInboundHandMessage::Public(Box::new(message)),
            })?;
            return Ok(Vec::new());
        }
        self.active
            .as_mut()
            .context("尚未建立牌桌会话")?
            .handle_public_message(swarm, Some(source), message, execution)
    }

    pub(crate) fn handle_private(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        source: PeerId,
        message: HandPrivateMessage,
        now_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
    ) -> Result<Vec<HandEvent>> {
        if self.should_buffer_next_hand(message.table_id(), message.hand_number()) {
            self.buffer_early_message(BufferedInboundHandDelivery {
                source,
                message: BufferedInboundHandMessage::Private(message),
            })?;
            return Ok(Vec::new());
        }
        let active = self.active.as_mut().context("尚未建立牌桌会话")?;
        let mut events = Vec::new();
        match message {
            HandPrivateMessage::HoleRevealShare {
                table_id,
                hand_number,
                roster_hash,
                from_seat,
                to_seat,
                card_index,
                packet,
            } => {
                active.verify_scope(&table_id, hand_number, &roster_hash)?;
                active.verify_seat_source(from_seat, source)?;
                if usize::from(to_seat) != active.table.local_seat + 1 {
                    anyhow::bail!("私密底牌份额收件座位不是本机")
                }
                if !active.local_hole_indices().contains(&card_index) {
                    anyhow::bail!("私密底牌份额引用了不属于本机的牌")
                }
                insert_consistent(
                    active.hole_shares.entry(card_index).or_default(),
                    from_seat,
                    packet,
                    "私密底牌份额",
                )?;
            }
        }
        events.extend(active.drive(swarm, device, certificate, now_unix_ms)?);
        Ok(events)
    }

    fn should_buffer_next_hand(&self, table_id: &[u8; 32], hand_number: u64) -> bool {
        let Some(active) = self.active.as_ref() else {
            return true;
        };
        active.finalized_receipt.is_some()
            && table_id == &active.table.table_id
            && hand_number == active.hand_number.saturating_add(1)
    }

    fn buffer_early_message(&mut self, delivery: BufferedInboundHandDelivery) -> Result<()> {
        if self
            .early_messages
            .iter()
            .any(|pending| pending == &delivery)
        {
            return Ok(());
        }
        anyhow::ensure!(
            self.early_messages.len() < MAX_EARLY_HAND_MESSAGES,
            "提前到达的下一手消息超过 {MAX_EARLY_HAND_MESSAGES} 条上限"
        );
        self.early_messages.push(delivery);
        Ok(())
    }

    pub(crate) fn submit_action(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        expected: &token_holdem_sidecar::HandActionPrecondition,
        action: PlayerAction,
        now_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Vec<HandEvent>> {
        let active = self.active.as_mut().context("尚未建立牌桌会话")?;
        anyhow::ensure!(
            expected.matches(
                &hex::encode(active.table.table_id),
                active.hand_number,
                active.sequence,
                &hex::encode(active.public_state_hash()?)
            ),
            "手牌状态已变化，已拒绝过期动作；请查看最新牌桌后重试"
        );
        if active.safe_leave_requested {
            anyhow::bail!("已请求安全离桌，本手牌将由客户端在轮到你时自动弃牌")
        }
        let mut events =
            active.commit_local_action(swarm, action, now_unix_ms, device, &certificate)?;
        events.extend(active.drive(swarm, device, &certificate, now_unix_ms)?);
        Ok(events)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn request_safe_leave(&mut self) {
        debug_assert!(self.active.is_some(), "请求安全离桌时必须有进行中的手牌");
        if let Some(active) = self.active.as_mut() {
            active.safe_leave_requested = true;
        }
    }

    pub(crate) fn safe_leave_is_stalled(&self) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active.safe_leave_requested
                && (!active.disconnected_peers.is_empty() || active.action_conflict.is_some())
        })
    }

    pub(crate) fn handle_direct_response(
        &mut self,
        request_id: OutboundRequestId,
        accepted: bool,
    ) -> bool {
        if !accepted {
            return self.handle_direct_failure(request_id);
        }
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        let Some((peer, message)) = active.direct_requests.remove(&request_id) else {
            return false;
        };
        active
            .direct_outbox
            .retain(|pending| pending.peer != peer || pending.message != message);
        active
            .direct_requests
            .retain(|_, pending| pending.0 != peer || pending.1 != message);
        true
    }

    pub(crate) fn handle_direct_failure(&mut self, request_id: OutboundRequestId) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        let Some((peer, message)) = active.direct_requests.remove(&request_id) else {
            return false;
        };
        if let Some(pending) = active
            .direct_outbox
            .iter_mut()
            .find(|pending| pending.peer == peer && pending.message == message)
        {
            pending.next_retry_tick = active
                .tick_count
                .saturating_add(DIRECT_RETRY_INTERVAL_TICKS);
            if pending.attempts >= MAX_DIRECT_MESSAGE_ATTEMPTS {
                active
                    .direct_outbox
                    .retain(|candidate| candidate.peer != peer || candidate.message != message);
            }
        }
        true
    }

    pub(crate) fn take_hand_boundary(&mut self) -> Result<Option<HandBoundary>> {
        let Some(active) = self.active.as_mut() else {
            return Ok(None);
        };
        if active.boundary_exported {
            return Ok(None);
        }
        let Some(receipt) = active.finalized_receipt.as_ref() else {
            return Ok(None);
        };
        let physical_seat = *active
            .table
            .physical_seats
            .get(active.dealer_index)
            .context("本手庄位缺少对应物理席位")?;
        let dealer_seat = PhysicalSeat::new(physical_seat).context("本手庄家物理席位无效")?;
        active.boundary_exported = true;
        Ok(Some(HandBoundary {
            receipt_hash: *receipt.receipt.id().as_bytes(),
            dealer_seat,
        }))
    }

    pub(crate) fn peer_disconnected(&mut self, peer_id: PeerId) -> Result<Vec<HandEvent>> {
        let Some(active) = self.active.as_mut() else {
            return Ok(Vec::new());
        };
        if !active.table.peer_ids.contains(&peer_id)
            || peer_id == active.table.peer_ids[active.table.local_seat]
            || active
                .disconnected_peers
                .insert(peer_id, active.tick_count)
                .is_some()
        {
            return Ok(Vec::new());
        }
        active.turn_started_at_unix_ms = None;
        let mut events = vec![HandEvent::HandSessionInterrupted {
            table_id: active.table_id_string(),
            hand_number: active.hand_number,
            peer_id: peer_id.to_string(),
        }];
        if active.hand.is_some() {
            events.push(active.state_event()?);
        }
        Ok(events)
    }

    pub(crate) fn peer_connected(&mut self, peer_id: PeerId) -> Result<Vec<HandEvent>> {
        let Some(active) = self.active.as_mut() else {
            return Ok(Vec::new());
        };
        if active.disconnected_peers.remove(&peer_id).is_none() {
            return Ok(Vec::new());
        }
        let mut events = vec![HandEvent::HandSessionResumed {
            table_id: active.table_id_string(),
            hand_number: active.hand_number,
            peer_id: peer_id.to_string(),
        }];
        if active.hand.is_some() {
            events.push(active.state_event()?);
        }
        Ok(events)
    }

    pub(crate) fn take_newly_finalized_receipts(&mut self) -> Vec<CoSignedReceipt> {
        let mut receipts = std::mem::take(&mut self.pending_exports);
        let Some(active) = self.active.as_mut() else {
            return receipts;
        };
        if active.finalized_receipt_exported {
            return receipts;
        }
        let Some(receipt) = active.finalized_receipt.clone() else {
            return receipts;
        };
        active.finalized_receipt_exported = true;
        receipts.push(receipt);
        receipts
    }
}

fn register_roster_addresses(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    table: &ReadyTable,
) -> Result<()> {
    anyhow::ensure!(
        table.peer_ids.len() == table.peer_addresses.len(),
        "逐手玩家端点数量与 PeerId 数量不一致"
    );
    for (peer_id, addresses) in table.peer_ids.iter().zip(&table.peer_addresses) {
        if peer_id == swarm.local_peer_id() {
            continue;
        }
        for address in addresses {
            swarm.add_peer_address(*peer_id, address.clone());
        }
    }
    Ok(())
}

fn ensure_roster_connections(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    table: &ReadyTable,
    disconnected_peers: &BTreeMap<PeerId, u32>,
    tick_count: u32,
) -> Result<()> {
    let local_peer_id = *swarm.local_peer_id();
    let mut dial_started = false;
    for (remote_index, (peer_id, addresses)) in
        table.peer_ids.iter().zip(&table.peer_addresses).enumerate()
    {
        if *peer_id == local_peer_id {
            continue;
        }
        swarm.behaviour_mut().retain_peer_connection(*peer_id);
        let owns_dial = table.local_seat > remote_index;
        let fallback_due = disconnected_peers
            .get(peer_id)
            .is_some_and(|disconnected_at| {
                tick_count.saturating_sub(*disconnected_at) >= NON_OWNER_DIAL_FALLBACK_TICKS
            });
        if swarm.is_connected(peer_id) || (!owns_dial && !fallback_due) || dial_started {
            continue;
        }
        if swarm
            .dial(
                DialOpts::peer_id(*peer_id)
                    .condition(PeerCondition::DisconnectedAndNotDialing)
                    .addresses(addresses.clone())
                    .build(),
            )
            .is_ok()
        {
            dial_started = true;
        }
    }
    Ok(())
}

fn initialize_hand(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    table: ReadyTable,
    topic: String,
    hand_number: u64,
    dealer_index: usize,
    execution: HandExecutionContext<'_>,
) -> Result<(ActiveHand, Vec<HandEvent>)> {
    if hand_number == 0 {
        anyhow::bail!("手牌编号必须从 1 开始")
    }
    if dealer_index >= table.players.len() {
        anyhow::bail!("庄位超出牌桌座位范围")
    }

    let table_id = hex::encode(table.table_id);
    let transcript_context =
        format!("token-holdem/table/{table_id}/hand/{hand_number}").into_bytes();
    let engine =
        MentalPokerEngine::new(transcript_context).context("无法初始化 Mental Poker 引擎")?;
    let local_key =
        PlayerKeyMaterial::generate(&engine, &mut OsRng).context("无法生成本手牌临时密钥")?;
    let local_seat = u8::try_from(table.local_seat + 1).context("本地座位超出范围")?;
    let announcement = local_key.announcement().clone();
    let key_message = HandPublicMessage::KeyAnnouncement {
        table_id: table.table_id,
        hand_number,
        roster_hash: table.roster_hash,
        seat: local_seat,
        announcement: announcement.clone(),
    };
    let players = table
        .players
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let buy_ins = table.buy_ins.iter().map(|value| value.value()).collect();
    let mut active = ActiveHand {
        table,
        topic,
        hand_number,
        dealer_index,
        engine,
        local_key,
        announcements: BTreeMap::from([(local_seat, announcement)]),
        verified_keys: None,
        aggregate_key: None,
        shuffle_packets: BTreeMap::new(),
        shuffle_stage: 0,
        deck: None,
        hole_distribution_started: false,
        direct_outbox: Vec::new(),
        direct_requests: HashMap::new(),
        hole_shares: BTreeMap::new(),
        local_hole_cards: None,
        ready_seats: BTreeSet::new(),
        public_shares: BTreeMap::new(),
        public_cards: BTreeMap::new(),
        pending_reveal: None,
        hand: None,
        sequence: 0,
        committed_actions: BTreeMap::new(),
        pending_actions: BTreeMap::new(),
        action_conflict: None,
        public_outbox: Vec::new(),
        relayed_public_messages: BTreeSet::new(),
        mental_transcript: ProtocolTranscript::default(),
        action_transcript: blake3::Hasher::new(),
        last_action_at_unix_ms: None,
        turn_started_at_unix_ms: None,
        last_actions: BTreeMap::new(),
        receipt_consensus: None,
        pending_receipt_signatures: BTreeMap::new(),
        finalized_receipt: None,
        finalized_receipt_exported: false,
        boundary_exported: false,
        safe_leave_requested: false,
        disconnected_peers: BTreeMap::new(),
        settled: false,
        tick_count: 0,
    };
    active.queue_public_message(swarm, key_message)?;
    let mut events = vec![
        HandEvent::HandProtocolStarted {
            table_id,
            hand_number,
            seat: local_seat,
            dealer_seat: u8::try_from(active.dealer_index + 1).context("庄位超出范围")?,
            players,
            level_id: active.table.level.id().to_owned(),
            small_blind: active.table.level.small_blind().value(),
            big_blind: active.table.level.big_blind().value(),
            buy_ins,
        },
        active.progress("key_exchange", active.announcements.len())?,
    ];
    events.extend(active.drive(
        swarm,
        execution.device,
        execution.certificate,
        execution.now_unix_ms,
    )?);
    Ok((active, events))
}

impl ActiveHand {
    fn queue_public_message(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        message: HandPublicMessage,
    ) -> Result<()> {
        self.relayed_public_messages
            .insert(public_message_id(&message)?);
        let local_peer_id = *swarm.local_peer_id();
        let recipients = self
            .table
            .peer_ids
            .iter()
            .copied()
            .filter(|peer_id| *peer_id != local_peer_id)
            .collect::<Vec<_>>();
        for peer_id in recipients {
            self.queue_direct_message(
                swarm,
                peer_id,
                ReliableHandMessage::Public(Box::new(message.clone())),
            );
        }
        self.publish_public_message(swarm, message)
    }

    fn relay_verified_public_message(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        _source: PeerId,
        message: HandPublicMessage,
    ) -> Result<()> {
        if !is_relayable_public_message(&message)
            || !self
                .relayed_public_messages
                .insert(public_message_id(&message)?)
        {
            return Ok(());
        }
        self.publish_public_message(swarm, message)
    }

    fn publish_public_message(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        message: HandPublicMessage,
    ) -> Result<()> {
        let published = publish_public(swarm, &self.topic, &message)?;
        self.public_outbox.push(RetriedPublicMessage {
            message: message.clone(),
            attempts: u8::from(published),
            next_retry_tick: self.tick_count.saturating_add(PUBLIC_RETRY_INTERVAL_TICKS),
        });
        Ok(())
    }

    fn retry_public_messages(&mut self, swarm: &mut libp2p::Swarm<NetworkBehaviour>) -> Result<()> {
        let due = self
            .public_outbox
            .iter()
            .enumerate()
            .filter_map(|(index, pending)| {
                (pending.attempts < MAX_PUBLIC_MESSAGE_ATTEMPTS
                    && self.tick_count >= pending.next_retry_tick)
                    .then_some((index, pending.message.clone()))
            })
            .collect::<Vec<_>>();
        for (index, message) in due {
            let published = publish_public(swarm, &self.topic, &message)?;
            let pending = &mut self.public_outbox[index];
            if published {
                pending.attempts = pending.attempts.saturating_add(1);
            }
            pending.next_retry_tick = self.tick_count.saturating_add(PUBLIC_RETRY_INTERVAL_TICKS);
        }
        self.public_outbox
            .retain(|pending| pending.attempts < MAX_PUBLIC_MESSAGE_ATTEMPTS);
        Ok(())
    }

    fn queue_direct_message(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        peer: PeerId,
        message: ReliableHandMessage,
    ) {
        if self
            .direct_outbox
            .iter()
            .any(|pending| pending.peer == peer && pending.message == message)
        {
            return;
        }
        self.direct_outbox.push(RetriedDirectMessage {
            peer,
            message,
            attempts: 0,
            next_retry_tick: self.tick_count,
        });
        self.dispatch_direct_messages(swarm);
    }

    fn retry_direct_messages(&mut self, swarm: &mut libp2p::Swarm<NetworkBehaviour>) {
        self.dispatch_direct_messages(swarm);
    }

    fn dispatch_direct_messages(&mut self, swarm: &mut libp2p::Swarm<NetworkBehaviour>) {
        let mut busy_peers = self
            .direct_requests
            .values()
            .map(|(peer, _)| *peer)
            .collect::<BTreeSet<_>>();
        let mut due = Vec::new();
        for pending in &mut self.direct_outbox {
            if !busy_peers.contains(&pending.peer)
                && swarm.is_connected(&pending.peer)
                && pending.attempts < MAX_DIRECT_MESSAGE_ATTEMPTS
                && self.tick_count >= pending.next_retry_tick
            {
                pending.attempts = pending.attempts.saturating_add(1);
                pending.next_retry_tick =
                    self.tick_count.saturating_add(DIRECT_RETRY_INTERVAL_TICKS);
                due.push((pending.peer, pending.message.clone()));
                busy_peers.insert(pending.peer);
            }
        }
        for (peer, message) in due {
            let request_id = swarm
                .behaviour_mut()
                .control
                .send_request(&peer, message.clone().into_control_request());
            self.direct_requests.insert(request_id, (peer, message));
        }
    }

    fn handle_public_message(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        source: Option<PeerId>,
        message: HandPublicMessage,
        execution: HandExecutionContext<'_>,
    ) -> Result<Vec<HandEvent>> {
        self.verify_message_scope(&message)?;
        self.verify_public_source(source, &message)?;
        let relay_message = message.clone();
        let mut events = Vec::new();
        match message {
            HandPublicMessage::KeyAnnouncement {
                seat, announcement, ..
            } => {
                self.engine
                    .verify_key_announcement(&announcement)
                    .context("临时密钥所有权证明无效")?;
                insert_consistent(&mut self.announcements, seat, announcement, "临时密钥")?;
                events.push(self.progress("key_exchange", self.announcements.len())?);
            }
            HandPublicMessage::Shuffle { seat, packet, .. } => {
                insert_consistent(&mut self.shuffle_packets, seat, packet, "洗牌证明")?;
            }
            HandPublicMessage::CommunityRevealShare {
                seat,
                card_index,
                packet,
                ..
            } => {
                let index = usize::from(seat.checked_sub(1).context("公共份额座位无效")?);
                let verified_key = *self
                    .verified_keys
                    .as_ref()
                    .context("公共份额到达时临时密钥尚未聚合")?
                    .get(index)
                    .context("公共份额座位超出范围")?;
                self.engine
                    .verify_reveal_share(
                        self.deck
                            .as_ref()
                            .context("公共份额到达时最终牌组尚未就绪")?,
                        usize::from(card_index),
                        verified_key,
                        &packet,
                    )
                    .context("公共解密份额证明无效")?;
                insert_consistent(
                    self.public_shares.entry(card_index).or_default(),
                    seat,
                    packet,
                    "公共解密份额",
                )?;
            }
            HandPublicMessage::DealReady { seat, .. } => {
                let inserted = self.ready_seats.insert(seat);
                if inserted {
                    events.push(self.progress("dealing", self.ready_seats.len())?);
                    if self.all_players_ready() && self.hand.is_some() {
                        events.push(self.state_event()?);
                    }
                }
            }
            HandPublicMessage::ReceiptSignature {
                receipt, signature, ..
            } => {
                events.extend(self.accept_receipt_signature(
                    swarm,
                    receipt,
                    signature,
                    execution.now_unix_ms,
                )?);
            }
            HandPublicMessage::ActionCommitted {
                sequence, action, ..
            } => {
                events.extend(
                    self.accept_or_defer_remote_action(swarm, sequence, action, execution)?,
                );
            }
        }
        events.extend(self.drive(
            swarm,
            execution.device,
            execution.certificate,
            execution.now_unix_ms,
        )?);
        events.extend(self.commit_deferred_actions(swarm, execution)?);
        if let Some(source) = source {
            self.relay_verified_public_message(swarm, source, relay_message)?;
        }
        Ok(events)
    }

    fn accept_or_defer_remote_action(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        sequence: u64,
        action: SignedHandAction,
        execution: HandExecutionContext<'_>,
    ) -> Result<Vec<HandEvent>> {
        if sequence <= self.sequence {
            return self.commit_action(swarm, sequence, action, ActionOrigin::Remote, execution);
        }
        anyhow::ensure!(
            sequence == self.sequence.saturating_add(1),
            "动作序号越过本地下一动作"
        );
        let seat = action.seat();
        let index = usize::from(seat.checked_sub(1).context("动作座位无效")?);
        let player_id = *self.table.players.get(index).context("动作座位超出范围")?;
        let device_public_key = *self
            .table
            .device_public_keys
            .get(index)
            .context("动作设备映射缺失")?;
        action
            .verify_for(
                &self.table.table_id,
                self.hand_number,
                &self.table.roster_hash,
                sequence,
                action.expected_public_state_hash(),
                seat,
                player_id,
                device_public_key,
                execution.now_unix_ms,
            )
            .context("动作身份签名验证失败")?;
        let next_player = self
            .hand
            .as_ref()
            .context("当前手牌尚未完成私密发牌")?
            .next_player();
        anyhow::ensure!(next_player == Some(player_id), "动作玩家不是当前行动者");
        let state_matches = self.public_state_hash()? == *action.expected_public_state_hash();
        if self.pending_reveal.is_some() || !state_matches {
            insert_consistent(&mut self.pending_actions, sequence, action, "待执行动作")?;
            return Ok(Vec::new());
        }
        self.commit_action(swarm, sequence, action, ActionOrigin::Remote, execution)
    }

    fn commit_deferred_actions(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        execution: HandExecutionContext<'_>,
    ) -> Result<Vec<HandEvent>> {
        let mut events = Vec::new();
        loop {
            if self.pending_reveal.is_some() || self.settled {
                break;
            }
            let sequence = self.sequence.saturating_add(1);
            let Some(action) = self.pending_actions.get(&sequence).cloned() else {
                break;
            };
            if self.public_state_hash()? != *action.expected_public_state_hash() {
                break;
            }
            self.pending_actions.remove(&sequence);
            events.extend(self.commit_action(
                swarm,
                sequence,
                action,
                ActionOrigin::Remote,
                execution,
            )?);
        }
        Ok(events)
    }

    fn automatic_local_action(&self, now_unix_ms: u64) -> Result<Option<PlayerAction>> {
        if !self.disconnected_peers.is_empty()
            || self.action_conflict.is_some()
            || self.pending_reveal.is_some()
            || self.settled
            || !self.all_players_ready()
        {
            return Ok(None);
        }
        let hand = self.hand.as_ref().context("下注状态缺失")?;
        if hand.next_player() != Some(self.local_player()) {
            return Ok(None);
        }
        if self.safe_leave_requested {
            return Ok(Some(PlayerAction::Fold));
        }
        let Some(started_at) = self.turn_started_at_unix_ms else {
            return Ok(None);
        };
        if now_unix_ms < started_at.saturating_add(TURN_ACTION_TIMEOUT_MS) {
            return Ok(None);
        }
        let to_call = hand
            .amount_to_call(self.local_player())
            .context("无法计算超时自动动作的跟注额")?;
        Ok(Some(timeout_action(to_call)))
    }

    fn ensure_turn_started(&mut self, now_unix_ms: u64) -> bool {
        if self.turn_started_at_unix_ms.is_some()
            || !self.disconnected_peers.is_empty()
            || self.action_conflict.is_some()
            || self.pending_reveal.is_some()
            || self.settled
            || !self.all_players_ready()
            || self
                .hand
                .as_ref()
                .is_none_or(|hand| hand.next_player().is_none())
        {
            return false;
        }
        self.turn_started_at_unix_ms = Some(now_unix_ms);
        true
    }

    fn commit_local_action(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        action: PlayerAction,
        now_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
    ) -> Result<Vec<HandEvent>> {
        if !self.disconnected_peers.is_empty() {
            anyhow::bail!("关键参与者已断线，牌桌会话暂停")
        }
        if self.action_conflict.is_some() {
            anyhow::bail!("本手牌存在签名动作冲突，已经冻结")
        }
        if self.pending_reveal.is_some() || self.settled {
            anyhow::bail!("当前正在验证开牌或手牌已经结束")
        }
        if !self.all_players_ready() {
            anyhow::bail!("尚未收到所有座位的私牌就绪确认")
        }
        let hand = self.hand.as_ref().context("当前手牌尚未完成私密发牌")?;
        if hand.next_player() != Some(self.local_player()) {
            anyhow::bail!("当前没有轮到本机玩家行动")
        }
        let sequence = self.sequence.checked_add(1).context("动作序号溢出")?;
        let signed = SignedHandAction::issue(
            self.table.table_id,
            self.hand_number,
            self.table.roster_hash,
            sequence,
            self.public_state_hash()?,
            self.local_seat_u8()?,
            action,
            now_unix_ms,
            device,
            certificate.clone(),
        )
        .context("无法签名牌桌动作")?;
        self.commit_action(
            swarm,
            sequence,
            signed,
            ActionOrigin::Local,
            HandExecutionContext::new(now_unix_ms, device, certificate),
        )
    }

    fn drive(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
        now_unix_ms: u64,
    ) -> Result<Vec<HandEvent>> {
        if self.action_conflict.is_some() {
            return Ok(Vec::new());
        }
        let mut events = Vec::new();
        let player_count = self.player_count();
        if self.verified_keys.is_none() && self.announcements.len() == player_count {
            let verified = (1..=player_count)
                .map(|index| {
                    let seat = u8::try_from(index).context("座位超出范围")?;
                    let announcement = self
                        .announcements
                        .get(&seat)
                        .context("临时密钥集合缺少座位")?;
                    self.engine
                        .verify_key_announcement(announcement)
                        .context("临时密钥所有权证明无效")
                })
                .collect::<Result<Vec<_>>>()?;
            let aggregate = self
                .engine
                .aggregate_key(&verified)
                .context("无法聚合临时牌组密钥")?;
            for index in 1..=player_count {
                let seat = u8::try_from(index).context("座位超出范围")?;
                self.mental_transcript.append_key(
                    self.announcements
                        .get(&seat)
                        .context("临时密钥集合缺少座位")?,
                );
            }
            self.verified_keys = Some(verified);
            self.aggregate_key = Some(aggregate);
            events.push(self.progress("shuffling", 0)?);
        }

        if self.aggregate_key.is_some() {
            self.ensure_local_shuffle(swarm)?;
            loop {
                let next = self.shuffle_stage.saturating_add(1);
                let Some(packet) = self.shuffle_packets.get(&next).cloned() else {
                    break;
                };
                let aggregate = self.aggregate_key.context("缺少聚合密钥")?;
                let verified = if next == 1 {
                    self.engine
                        .verify_initial_shuffle(aggregate, &packet)
                        .context("首位玩家的初始洗牌证明无效")?
                } else {
                    let previous = self.deck.as_ref().context("缺少上一阶段牌组")?;
                    self.engine
                        .verify_reshuffle(aggregate, previous, &packet)
                        .context("顺序玩家的洗牌证明无效")?
                };
                self.mental_transcript.append_shuffle(&packet);
                self.deck = Some(verified);
                self.shuffle_stage = next;
                events.push(self.progress("shuffling", usize::from(next))?);
                if usize::from(next) >= player_count {
                    break;
                }
                self.ensure_local_shuffle(swarm)?;
            }
        }

        if usize::from(self.shuffle_stage) == player_count && !self.hole_distribution_started {
            self.begin_hole_distribution(swarm)?;
            events.push(self.progress("dealing", 0)?);
        }
        if self.local_hole_cards.is_none() && self.hole_distribution_started {
            if let Some(cards) = self.reveal_local_hole_cards()? {
                self.local_hole_cards = Some(cards);
                self.start_betting_hand()?;
                let local_seat = self.local_seat_u8()?;
                self.ready_seats.insert(local_seat);
                let ready_message = HandPublicMessage::DealReady {
                    table_id: self.table.table_id,
                    hand_number: self.hand_number,
                    roster_hash: self.table.roster_hash,
                    seat: local_seat,
                };
                self.queue_public_message(swarm, ready_message)?;
                events.push(HandEvent::HandReady {
                    table_id: self.table_id_string(),
                    hand_number: self.hand_number,
                    seat: self.local_seat_u8()?,
                    hole_cards: cards.map(VisibleCard::from),
                    transcript_hash: self.transcript_hash(),
                });
                events.push(self.progress("dealing", self.ready_seats.len())?);
                events.push(self.state_event()?);
            }
        }
        events.extend(self.try_complete_public_reveal(swarm, device, certificate, now_unix_ms)?);
        if self.ensure_turn_started(now_unix_ms) && self.hand.is_some() {
            events.push(self.state_event()?);
        }
        Ok(events)
    }

    fn ensure_local_shuffle(&mut self, swarm: &mut libp2p::Swarm<NetworkBehaviour>) -> Result<()> {
        let next = self.shuffle_stage.saturating_add(1);
        if usize::from(next) > self.player_count()
            || usize::from(next) != self.table.local_seat + 1
            || self.shuffle_packets.contains_key(&next)
        {
            return Ok(());
        }
        let aggregate = self.aggregate_key.context("缺少聚合密钥")?;
        let packet = if next == 1 {
            self.engine
                .initial_shuffle(&mut OsRng, aggregate)
                .context("无法生成初始洗牌")?
        } else {
            self.engine
                .reshuffle(
                    &mut OsRng,
                    aggregate,
                    self.deck.as_ref().context("缺少上一阶段牌组")?,
                )
                .context("无法生成顺序洗牌")?
        };
        let message = HandPublicMessage::Shuffle {
            table_id: self.table.table_id,
            hand_number: self.hand_number,
            roster_hash: self.table.roster_hash,
            seat: next,
            packet: packet.clone(),
        };
        self.shuffle_packets.insert(next, packet);
        self.queue_public_message(swarm, message)?;
        Ok(())
    }

    fn begin_hole_distribution(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    ) -> Result<()> {
        let deck = self.deck.as_ref().context("最终牌组尚未就绪")?;
        let from_seat = self.local_seat_u8()?;
        let mut outbound = Vec::new();
        for owner_index in 0..self.player_count() {
            let to_seat = u8::try_from(owner_index + 1).context("座位超出范围")?;
            for card_index in hole_indices(owner_index) {
                let packet = self
                    .engine
                    .create_reveal_share(&mut OsRng, deck, usize::from(card_index), &self.local_key)
                    .context("无法生成私密底牌解密份额")?;
                if owner_index == self.table.local_seat {
                    self.hole_shares
                        .entry(card_index)
                        .or_default()
                        .insert(from_seat, packet);
                } else {
                    let message = HandPrivateMessage::HoleRevealShare {
                        table_id: self.table.table_id,
                        hand_number: self.hand_number,
                        roster_hash: self.table.roster_hash,
                        from_seat,
                        to_seat,
                        card_index,
                        packet,
                    };
                    let peer = self.table.peer_ids[owner_index];
                    outbound.push((peer, message));
                }
            }
        }
        for (peer, message) in outbound {
            self.queue_direct_message(swarm, peer, ReliableHandMessage::Private(message));
        }
        self.hole_distribution_started = true;
        Ok(())
    }

    fn reveal_local_hole_cards(&self) -> Result<Option<[Card; 2]>> {
        let indices = self.local_hole_indices();
        let first = self.reveal_if_complete(indices[0])?;
        let second = self.reveal_if_complete(indices[1])?;
        Ok(match (first, second) {
            (Some(first), Some(second)) => Some([first, second]),
            _ => None,
        })
    }

    fn reveal_if_complete(&self, card_index: u8) -> Result<Option<Card>> {
        let packets = self.hole_shares.get(&card_index);
        if packets.is_none_or(|packets| packets.len() != self.player_count()) {
            return Ok(None);
        }
        let packets = packets.context("底牌份额集合缺失")?;
        let verified_keys = self.verified_keys.as_ref().context("缺少已验证临时密钥")?;
        let deck = self.deck.as_ref().context("最终牌组尚未就绪")?;
        let shares = (1..=self.player_count())
            .map(|index| {
                let seat = u8::try_from(index).context("座位超出范围")?;
                self.engine
                    .verify_reveal_share(
                        deck,
                        usize::from(card_index),
                        verified_keys[index - 1],
                        packets.get(&seat).context("底牌份额集合缺少座位")?,
                    )
                    .context("私密底牌解密份额证明无效")
            })
            .collect::<Result<Vec<_>>>()?;
        self.engine
            .reveal_holdem_card(deck, usize::from(card_index), &shares, self.player_count())
            .map(Some)
            .context("无法聚合私密底牌解密份额")
    }

    fn start_betting_hand(&mut self) -> Result<()> {
        let seats = self
            .table
            .players
            .iter()
            .copied()
            .zip(self.table.buy_ins.iter().copied())
            .collect::<Vec<_>>();
        self.hand = Some(
            HoldemHand::start(self.table.level.clone(), seats, self.dealer_index)
                .context("无法初始化桌内下注状态")?,
        );
        Ok(())
    }

    fn commit_action(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        sequence: u64,
        action: SignedHandAction,
        origin: ActionOrigin,
        execution: HandExecutionContext<'_>,
    ) -> Result<Vec<HandEvent>> {
        if let Some(existing) = self.committed_actions.get(&sequence).cloned() {
            if existing == action {
                return Ok(Vec::new());
            }
            return self.resolve_replayed_action(sequence, existing, action, execution.now_unix_ms);
        }
        if self.action_conflict.is_some() {
            anyhow::bail!("本手牌存在签名动作冲突，已经冻结")
        }
        if sequence != self.sequence + 1 {
            anyhow::bail!(
                "动作序号不连续：预期 {}，实际 {sequence}",
                self.sequence + 1
            )
        }
        let seat = action.seat();
        let index = usize::from(seat.checked_sub(1).context("动作座位无效")?);
        let player_id = *self.table.players.get(index).context("动作座位超出范围")?;
        let device_public_key = *self
            .table
            .device_public_keys
            .get(index)
            .context("动作设备映射缺失")?;
        let expected_public_state_hash = self.public_state_hash()?;
        action
            .verify_for(
                &self.table.table_id,
                self.hand_number,
                &self.table.roster_hash,
                sequence,
                &expected_public_state_hash,
                seat,
                player_id,
                device_public_key,
                execution.now_unix_ms,
            )
            .context("动作设备签名验证失败")?;
        if self.pending_reveal.is_some() || self.settled {
            anyhow::bail!("当前不能提交新动作")
        }
        let player_action = action.action();
        let outcome = self
            .hand
            .as_mut()
            .context("当前手牌尚未完成私密发牌")?
            .act(player_id, player_action)
            .context("桌内动作不合法")?;
        self.sequence = sequence;
        self.last_action_at_unix_ms = Some(action.issued_at_unix_ms());
        self.turn_started_at_unix_ms = Some(execution.now_unix_ms);
        self.last_actions.insert(seat, player_action);
        self.append_action(&action)?;
        self.committed_actions.insert(sequence, action.clone());
        if origin == ActionOrigin::Local {
            let message = HandPublicMessage::ActionCommitted {
                table_id: self.table.table_id,
                hand_number: self.hand_number,
                roster_hash: self.table.roster_hash,
                sequence,
                action,
            };
            self.queue_public_message(swarm, message)?;
        }
        self.handle_action_outcome(
            swarm,
            outcome,
            execution.device,
            execution.certificate,
            execution.now_unix_ms,
        )
    }

    fn resolve_replayed_action(
        &mut self,
        sequence: u64,
        accepted: SignedHandAction,
        conflicting: SignedHandAction,
        now_unix_ms: u64,
    ) -> Result<Vec<HandEvent>> {
        if conflicting.seat() != accepted.seat()
            || conflicting.player_id() != accepted.player_id()
            || conflicting.expected_public_state_hash() != accepted.expected_public_state_hash()
        {
            anyhow::bail!("重复动作没有绑定已接受动作的行动者或公共前置状态")
        }
        let seat = accepted.seat();
        let index = usize::from(seat.checked_sub(1).context("动作座位无效")?);
        let player_id = *self.table.players.get(index).context("动作座位超出范围")?;
        let device_public_key = *self
            .table
            .device_public_keys
            .get(index)
            .context("动作设备映射缺失")?;
        conflicting
            .verify_for(
                &self.table.table_id,
                self.hand_number,
                &self.table.roster_hash,
                sequence,
                accepted.expected_public_state_hash(),
                seat,
                player_id,
                device_public_key,
                now_unix_ms,
            )
            .context("重复动作签名无效")?;
        if conflicting.payload_hash() == accepted.payload_hash() {
            return Ok(Vec::new());
        }
        let evidence = ActionConflictEvidence {
            sequence,
            accepted_action_hash: *accepted.payload_hash(),
            conflicting_action_hash: *conflicting.payload_hash(),
        };
        self.action_conflict = Some(evidence);
        Ok(vec![HandEvent::HandActionConflict {
            table_id: self.table_id_string(),
            hand_number: self.hand_number,
            sequence: evidence.sequence,
            accepted_action_hash: hex::encode(evidence.accepted_action_hash),
            conflicting_action_hash: hex::encode(evidence.conflicting_action_hash),
        }])
    }

    fn handle_action_outcome(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        outcome: ActionOutcome,
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
        now_unix_ms: u64,
    ) -> Result<Vec<HandEvent>> {
        match outcome {
            ActionOutcome::WaitingFor(_) => Ok(vec![self.state_event()?]),
            ActionOutcome::StreetAdvanced(street, _) => {
                self.last_actions.clear();
                self.turn_started_at_unix_ms = None;
                let indices = match street {
                    Street::Flop => flop_indices(self.player_count()).to_vec(),
                    Street::Turn => vec![turn_index(self.player_count())],
                    Street::River => vec![river_index(self.player_count())],
                    _ => anyhow::bail!("动作推进到了不支持的街道"),
                };
                self.request_public_reveal(swarm, indices, RevealReason::Street)?;
                Ok(vec![self.state_event()?])
            }
            ActionOutcome::ShowdownReady => {
                self.turn_started_at_unix_ms = None;
                let mut indices = board_indices(self.player_count()).to_vec();
                let hand = self.hand.as_ref().context("下注状态缺失")?;
                for (index, seat) in hand.seats().iter().enumerate() {
                    if seat.status() != SeatStatus::Folded {
                        indices.extend(hole_indices(index));
                    }
                }
                self.request_public_reveal(swarm, indices, RevealReason::Showdown)?;
                Ok(vec![self.state_event()?])
            }
            ActionOutcome::HandComplete(settlement) => {
                self.finish_settlement(swarm, settlement, device, certificate, now_unix_ms)
            }
        }
    }

    fn request_public_reveal(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        mut indices: Vec<u8>,
        reason: RevealReason,
    ) -> Result<()> {
        indices.sort_unstable();
        indices.dedup();
        let deck = self.deck.as_ref().context("最终牌组尚未就绪")?;
        let local_seat = self.local_seat_u8()?;
        let mut outbound = Vec::new();
        for card_index in &indices {
            if self
                .public_shares
                .get(card_index)
                .is_some_and(|shares| shares.contains_key(&local_seat))
            {
                continue;
            }
            let packet = self
                .engine
                .create_reveal_share(&mut OsRng, deck, usize::from(*card_index), &self.local_key)
                .context("无法生成公共牌解密份额")?;
            self.public_shares
                .entry(*card_index)
                .or_default()
                .insert(local_seat, packet.clone());
            let message = HandPublicMessage::CommunityRevealShare {
                table_id: self.table.table_id,
                hand_number: self.hand_number,
                roster_hash: self.table.roster_hash,
                seat: local_seat,
                card_index: *card_index,
                packet,
            };
            outbound.push(message);
        }
        for message in outbound {
            self.queue_public_message(swarm, message)?;
        }
        self.pending_reveal = Some(PendingReveal { reason, indices });
        Ok(())
    }

    fn try_complete_public_reveal(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
        now_unix_ms: u64,
    ) -> Result<Vec<HandEvent>> {
        let Some(pending) = self.pending_reveal.as_ref() else {
            return Ok(Vec::new());
        };
        let indices = pending.indices.clone();
        let reason = pending.reason;
        for card_index in &indices {
            if self.public_cards.contains_key(card_index) {
                continue;
            }
            let Some(packets) = self.public_shares.get(card_index) else {
                return Ok(Vec::new());
            };
            if packets.len() != self.player_count() {
                return Ok(Vec::new());
            }
            let verified_keys = self.verified_keys.as_ref().context("缺少已验证临时密钥")?;
            let deck = self.deck.as_ref().context("最终牌组尚未就绪")?;
            let shares = (1..=self.player_count())
                .map(|index| {
                    let seat = u8::try_from(index).context("座位超出范围")?;
                    let packet = packets.get(&seat).context("公共份额集合缺少座位")?;
                    self.engine
                        .verify_reveal_share(
                            deck,
                            usize::from(*card_index),
                            verified_keys[index - 1],
                            packet,
                        )
                        .context("公共牌解密份额证明无效")
                })
                .collect::<Result<Vec<_>>>()?;
            let card = self
                .engine
                .reveal_holdem_card(deck, usize::from(*card_index), &shares, self.player_count())
                .context("无法聚合公共牌解密份额")?;
            for index in 1..=self.player_count() {
                let seat = u8::try_from(index).context("座位超出范围")?;
                self.mental_transcript
                    .append_reveal_share(packets.get(&seat).context("公共份额集合缺少座位")?);
            }
            self.public_cards.insert(*card_index, card);
        }
        if !indices
            .iter()
            .all(|card_index| self.public_cards.contains_key(card_index))
        {
            return Ok(Vec::new());
        }
        self.pending_reveal = None;
        if reason == RevealReason::Street {
            self.turn_started_at_unix_ms = Some(now_unix_ms);
            return Ok(vec![self.state_event()?]);
        }
        let board = board_indices(self.player_count()).map(|index| {
            self.public_cards
                .get(&index)
                .copied()
                .context("摊牌缺少公共牌")
        });
        let [first, second, third, fourth, fifth] = board;
        let board = [first?, second?, third?, fourth?, fifth?];
        let hand = self.hand.as_ref().context("下注状态缺失")?;
        let mut holes = BTreeMap::new();
        for (index, seat) in hand.seats().iter().enumerate() {
            if seat.status() == SeatStatus::Folded {
                continue;
            }
            let [first, second] = hole_indices(index);
            holes.insert(
                seat.player_id(),
                [
                    *self.public_cards.get(&first).context("摊牌缺少玩家底牌")?,
                    *self.public_cards.get(&second).context("摊牌缺少玩家底牌")?,
                ],
            );
        }
        let settlement = self
            .hand
            .as_mut()
            .context("下注状态缺失")?
            .settle_showdown(board, holes)
            .context("摊牌结算失败")?;
        self.finish_settlement(swarm, settlement, device, certificate, now_unix_ms)
    }

    fn finish_settlement(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        settlement: HoldemSettlement,
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
        now_unix_ms: u64,
    ) -> Result<Vec<HandEvent>> {
        self.settled = true;
        self.turn_started_at_unix_ms = None;
        let receipt = self.build_receipt(&settlement)?;
        let outcomes = settlement
            .players
            .iter()
            .map(|outcome| {
                let seat = self
                    .table
                    .players
                    .iter()
                    .position(|player| *player == outcome.player_id)
                    .and_then(|index| u8::try_from(index + 1).ok())
                    .unwrap_or(0);
                VisibleOutcome {
                    seat,
                    player_id: outcome.player_id.to_string(),
                    starting_stack: outcome.starting_stack.value(),
                    ending_stack: outcome.ending_stack.value(),
                    delta: outcome.delta.value(),
                }
            })
            .collect();
        let (consensus, local_signature) =
            ReceiptConsensus::start(receipt.clone(), device, certificate.clone(), now_unix_ms)?;
        self.receipt_consensus = Some(consensus);
        let message = HandPublicMessage::ReceiptSignature {
            table_id: self.table.table_id,
            hand_number: self.hand_number,
            roster_hash: self.table.roster_hash,
            receipt,
            signature: local_signature,
        };
        self.queue_public_message(swarm, message)?;

        let mut events = vec![
            self.state_event()?,
            HandEvent::HandSettled {
                table_id: self.table_id_string(),
                hand_number: self.hand_number,
                outcomes,
                transcript_hash: self.transcript_hash(),
            },
            self.receipt_progress()?,
        ];
        for (_, (pending_receipt, signature)) in
            std::mem::take(&mut self.pending_receipt_signatures)
        {
            match self.accept_receipt_signature(swarm, pending_receipt, signature, now_unix_ms) {
                Ok(receipt_events) => events.extend(receipt_events),
                Err(error) => events.push(HandEvent::Warning {
                    message: format!("对手签署了与本地结算不一致的凭证：{error:#}"),
                }),
            }
        }
        events.extend(self.try_finalize_receipt(swarm, now_unix_ms)?);
        Ok(events)
    }

    fn build_receipt(&self, settlement: &HoldemSettlement) -> Result<HandReceipt> {
        let settled_at_unix_ms = self
            .last_action_at_unix_ms
            .context("已结算手牌缺少最后动作时间")?;
        let mut match_id = [0_u8; 16];
        match_id.copy_from_slice(&self.table.table_id[..16]);
        let outcomes = settlement
            .players
            .iter()
            .map(|outcome| {
                let index = self
                    .table
                    .players
                    .iter()
                    .position(|player| *player == outcome.player_id)
                    .context("结算玩家不属于当前牌桌")?;
                Ok(HandOutcome {
                    player_id: outcome.player_id,
                    device_public_key: *self
                        .table
                        .device_public_keys
                        .get(index)
                        .context("结算玩家缺少设备映射")?,
                    starting_stack: outcome.starting_stack,
                    ending_stack: outcome.ending_stack,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        HandReceipt::settle(
            MatchId::new(match_id),
            self.hand_number,
            self.table.level.id(),
            TranscriptHash::new(self.transcript_hash_bytes()),
            settled_at_unix_ms,
            outcomes,
        )
        .context("无法构造确定性结算凭证")
    }

    fn accept_receipt_signature(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        receipt: HandReceipt,
        signature: ParticipantSignature,
        now_unix_ms: u64,
    ) -> Result<Vec<HandEvent>> {
        self.validate_receipt_header(&receipt)?;
        signature
            .verify_for(&receipt, now_unix_ms)
            .context("结算凭证参与者签名无效")?;
        if self.receipt_consensus.is_none() {
            let player_id = signature.player_id();
            insert_consistent(
                &mut self.pending_receipt_signatures,
                player_id,
                (receipt, signature),
                "待结算签名",
            )?;
            return Ok(Vec::new());
        }

        let inserted = self
            .receipt_consensus
            .as_mut()
            .context("结算共识状态缺失")?
            .accept(&receipt, signature, now_unix_ms)?;
        let mut events = Vec::new();
        if inserted {
            events.push(self.receipt_progress()?);
        }
        events.extend(self.try_finalize_receipt(swarm, now_unix_ms)?);
        Ok(events)
    }

    fn try_finalize_receipt(
        &mut self,
        _swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        now_unix_ms: u64,
    ) -> Result<Vec<HandEvent>> {
        let Some(consensus) = self.receipt_consensus.as_mut() else {
            return Ok(Vec::new());
        };
        if consensus.is_finalized() {
            return Ok(Vec::new());
        }
        let Some(receipt) = consensus.try_finalize(now_unix_ms)? else {
            return Ok(Vec::new());
        };
        let local_delta = receipt
            .receipt
            .outcome_for(self.local_player())
            .context("联合签名凭证缺少本地玩家")?
            .delta()
            .value();
        let signatures = u8::try_from(receipt.signatures.len()).context("签名数超出范围")?;
        let receipt_id = hex::encode(receipt.receipt.id().as_bytes());
        self.finalized_receipt = Some(receipt);
        Ok(vec![HandEvent::ReceiptFinalized {
            table_id: self.table_id_string(),
            hand_number: self.hand_number,
            receipt_id,
            local_delta,
            signatures,
        }])
    }

    fn validate_receipt_header(&self, receipt: &HandReceipt) -> Result<()> {
        let mut expected_match_id = [0_u8; 16];
        expected_match_id.copy_from_slice(&self.table.table_id[..16]);
        if receipt.match_id() != MatchId::new(expected_match_id)
            || receipt.hand_number() != self.hand_number
            || receipt.stake_level_id() != self.table.level.id()
        {
            anyhow::bail!("结算凭证不属于当前牌桌、手牌或级别")
        }
        if receipt.outcomes().len() != self.player_count() {
            anyhow::bail!("结算凭证参与者数量与当前牌桌不一致")
        }
        for (index, player_id) in self.table.players.iter().enumerate() {
            let outcome = receipt
                .outcome_for(*player_id)
                .context("结算凭证缺少当前牌桌玩家")?;
            if outcome.device_public_key != self.table.device_public_keys[index]
                || outcome.starting_stack != self.table.buy_ins[index]
            {
                anyhow::bail!("结算凭证中的设备或起始筹码与牌桌承诺不一致")
            }
        }
        Ok(())
    }

    fn receipt_progress(&self) -> Result<HandEvent> {
        let consensus = self
            .receipt_consensus
            .as_ref()
            .context("结算共识尚未启动")?;
        Ok(HandEvent::ReceiptConsensusProgress {
            table_id: self.table_id_string(),
            hand_number: self.hand_number,
            signed: u8::try_from(consensus.signature_count()).context("签名进度超出范围")?,
            required: u8::try_from(self.player_count()).context("玩家数超出范围")?,
        })
    }

    fn state_event(&self) -> Result<HandEvent> {
        let hand = self.hand.as_ref().context("下注状态缺失")?;
        let local_player = self.local_player();
        let local_seat_state = hand
            .seats()
            .get(self.table.local_seat)
            .context("本地座位状态缺失")?;
        let maximum_raise_to = local_seat_state
            .street_committed()
            .checked_add(local_seat_state.stack())
            .context("本地最大加注额溢出")?;
        let next_seat = hand.next_player().and_then(|player| {
            self.table
                .players
                .iter()
                .position(|candidate| *candidate == player)
                .and_then(|index| u8::try_from(index + 1).ok())
        });
        let board_order = board_indices(self.player_count());
        let board = board_order
            .iter()
            .filter_map(|index| self.public_cards.get(index).copied())
            .map(VisibleCard::from)
            .collect();
        let seats = hand
            .seats()
            .iter()
            .enumerate()
            .map(|(index, seat)| {
                let seat_number = u8::try_from(index + 1).unwrap_or(0);
                VisibleSeat {
                    seat: seat_number,
                    player_id: seat.player_id().to_string(),
                    stack: seat.stack().value(),
                    committed: seat.total_committed().value(),
                    status: match seat.status() {
                        SeatStatus::Active => "active",
                        SeatStatus::Folded => "folded",
                        SeatStatus::AllIn => "all_in",
                    },
                    last_action: self.last_actions.get(&seat_number).map(action_name),
                }
            })
            .collect();
        let turn_deadline_unix_ms = if next_seat.is_some()
            && self.all_players_ready()
            && self.pending_reveal.is_none()
            && self.action_conflict.is_none()
            && self.disconnected_peers.is_empty()
            && !self.settled
        {
            self.turn_started_at_unix_ms
                .map(|started_at| started_at.saturating_add(TURN_ACTION_TIMEOUT_MS))
        } else {
            None
        };
        Ok(HandEvent::HandState {
            table_id: self.table_id_string(),
            hand_number: self.hand_number,
            sequence: self.sequence,
            street: street_name(hand.street()),
            pot: hand.pot().context("底池计算失败")?.value(),
            current_bet: hand.current_bet().value(),
            next_seat,
            local_seat: self.local_seat_u8()?,
            to_call: hand
                .amount_to_call(local_player)
                .context("无法计算本地跟注额")?
                .value(),
            minimum_raise_to: hand
                .minimum_raise_to()
                .context("无法计算最小加注额")?
                .value(),
            maximum_raise_to: maximum_raise_to.value(),
            can_act: hand.next_player() == Some(local_player)
                && self.all_players_ready()
                && self.pending_reveal.is_none()
                && self.action_conflict.is_none()
                && self.disconnected_peers.is_empty()
                && !self.settled,
            awaiting_reveal: self.pending_reveal.is_some(),
            action_timeout_ms: TURN_ACTION_TIMEOUT_MS,
            turn_deadline_unix_ms,
            board,
            seats,
            transcript_hash: self.transcript_hash(),
            public_state_hash: hex::encode(self.public_state_hash()?),
            betting_state_hash: hex::encode(hand.public_state_hash().as_bytes()),
            mental_transcript_hash: hex::encode(self.mental_transcript.hash()),
            action_transcript_hash: hex::encode(
                self.action_transcript.clone().finalize().as_bytes(),
            ),
        })
    }

    fn verify_message_scope(&self, message: &HandPublicMessage) -> Result<()> {
        self.verify_scope(
            message.table_id(),
            message.hand_number(),
            message.roster_hash(),
        )
    }

    fn verify_scope(
        &self,
        table_id: &[u8; 32],
        hand_number: u64,
        roster_hash: &[u8; 32],
    ) -> Result<()> {
        if table_id != &self.table.table_id || hand_number != self.hand_number {
            anyhow::bail!("牌桌消息不属于当前桌或当前手牌")
        }
        if roster_hash != &self.table.roster_hash {
            anyhow::bail!("牌桌消息不属于当前冻结参与者名单")
        }
        Ok(())
    }

    fn verify_public_source(
        &self,
        source: Option<PeerId>,
        message: &HandPublicMessage,
    ) -> Result<()> {
        let source = source.context("严格签名的牌桌消息缺少源 PeerId")?;
        anyhow::ensure!(
            self.table.peer_ids.contains(&source),
            "牌桌消息源不属于冻结参与者名单"
        );
        if is_relayable_public_message(message) {
            return Ok(());
        }
        let seat = match message {
            HandPublicMessage::KeyAnnouncement { seat, .. }
            | HandPublicMessage::Shuffle { seat, .. }
            | HandPublicMessage::CommunityRevealShare { seat, .. }
            | HandPublicMessage::DealReady { seat, .. } => *seat,
            HandPublicMessage::ReceiptSignature { signature, .. } => self
                .table
                .players
                .iter()
                .position(|player| *player == signature.player_id())
                .and_then(|index| u8::try_from(index + 1).ok())
                .context("结算签名玩家不属于当前牌桌")?,
            HandPublicMessage::ActionCommitted { action, .. } => action.seat(),
        };
        self.verify_seat_source(seat, source)
    }

    fn verify_seat_source(&self, seat: u8, source: PeerId) -> Result<()> {
        let index = usize::from(seat.checked_sub(1).context("牌桌消息座位无效")?);
        let expected = self
            .table
            .peer_ids
            .get(index)
            .context("牌桌消息座位超出范围")?;
        if expected != &source {
            anyhow::bail!("牌桌消息源与声明座位的会话 PeerId 不一致")
        }
        Ok(())
    }

    fn append_action(&mut self, action: &SignedHandAction) -> Result<()> {
        let payload = cbor4ii::serde::to_vec(Vec::new(), action).context("无法序列化动作转录")?;
        self.action_transcript
            .update(&(payload.len() as u32).to_be_bytes());
        self.action_transcript.update(&payload);
        Ok(())
    }

    fn public_state_hash(&self) -> Result<[u8; 32]> {
        let hand = self.hand.as_ref().context("下注状态缺失")?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(PUBLIC_HAND_STATE_DOMAIN);
        hasher.update(&self.table.table_id);
        hasher.update(&self.hand_number.to_be_bytes());
        hasher.update(&self.table.roster_hash);
        hasher.update(&self.sequence.to_be_bytes());
        hasher.update(hand.public_state_hash().as_bytes());
        hasher.update(&self.mental_transcript.hash());
        hasher.update(self.action_transcript.clone().finalize().as_bytes());
        hasher.update(&[self.public_cards.len() as u8]);
        for (card_index, card) in &self.public_cards {
            hasher.update(&[*card_index, card.rank(), suit_tag(card.suit())]);
        }
        Ok(*hasher.finalize().as_bytes())
    }

    fn transcript_hash(&self) -> String {
        hex::encode(self.transcript_hash_bytes())
    }

    fn transcript_hash_bytes(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"token-holdem/combined-hand-transcript/v1\0");
        hasher.update(&self.mental_transcript.hash());
        hasher.update(self.action_transcript.clone().finalize().as_bytes());
        *hasher.finalize().as_bytes()
    }

    fn progress(&self, phase: &'static str, completed: usize) -> Result<HandEvent> {
        Ok(HandEvent::HandProtocolProgress {
            table_id: self.table_id_string(),
            hand_number: self.hand_number,
            phase,
            completed: u8::try_from(completed).context("协议进度超出范围")?,
            required: u8::try_from(self.player_count()).context("玩家数超出范围")?,
        })
    }

    fn table_id_string(&self) -> String {
        hex::encode(self.table.table_id)
    }

    fn player_count(&self) -> usize {
        self.table.players.len()
    }

    fn all_players_ready(&self) -> bool {
        self.ready_seats.len() == self.player_count()
    }

    fn local_player(&self) -> PlayerId {
        self.table.players[self.table.local_seat]
    }

    fn local_seat_u8(&self) -> Result<u8> {
        u8::try_from(self.table.local_seat + 1).context("本地座位超出范围")
    }

    fn local_hole_indices(&self) -> [u8; 2] {
        hole_indices(self.table.local_seat)
    }
}

fn insert_consistent<K, V>(map: &mut BTreeMap<K, V>, key: K, value: V, label: &str) -> Result<()>
where
    K: Ord + Copy,
    V: PartialEq,
{
    if let Some(existing) = map.get(&key) {
        if existing == &value {
            return Ok(());
        }
        anyhow::bail!("{label}出现互相冲突的重复消息")
    }
    map.insert(key, value);
    Ok(())
}

fn is_relayable_public_message(message: &HandPublicMessage) -> bool {
    matches!(
        message,
        HandPublicMessage::CommunityRevealShare { .. }
            | HandPublicMessage::ReceiptSignature { .. }
            | HandPublicMessage::ActionCommitted { .. }
    )
}

fn public_message_id(message: &HandPublicMessage) -> Result<[u8; 32]> {
    let payload =
        cbor4ii::serde::to_vec(Vec::new(), message).context("无法序列化牌桌协议消息标识")?;
    Ok(*blake3::hash(&payload).as_bytes())
}

fn publish_public(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    topic: &str,
    message: &HandPublicMessage,
) -> Result<bool> {
    let payload = cbor4ii::serde::to_vec(Vec::new(), message).context("无法序列化牌桌协议消息")?;
    if payload.len() > MAX_HAND_MESSAGE_BYTES {
        anyhow::bail!("牌桌协议消息超过 256 KiB 上限")
    }
    Ok(swarm
        .behaviour_mut()
        .gossipsub
        .publish(gossipsub::IdentTopic::new(topic.to_owned()), payload)
        .is_ok())
}

fn hole_indices(seat_index: usize) -> [u8; 2] {
    let first = u8::try_from(seat_index.saturating_mul(2)).unwrap_or(u8::MAX);
    [first, first.saturating_add(1)]
}

fn board_indices(player_count: usize) -> [u8; 5] {
    let base = u8::try_from(player_count.saturating_mul(2)).unwrap_or(u8::MAX);
    [
        base.saturating_add(1),
        base.saturating_add(2),
        base.saturating_add(3),
        base.saturating_add(5),
        base.saturating_add(7),
    ]
}

fn flop_indices(player_count: usize) -> [u8; 3] {
    let board = board_indices(player_count);
    [board[0], board[1], board[2]]
}

fn turn_index(player_count: usize) -> u8 {
    board_indices(player_count)[3]
}

fn river_index(player_count: usize) -> u8 {
    board_indices(player_count)[4]
}

fn street_name(street: Street) -> &'static str {
    match street {
        Street::Preflop => "preflop",
        Street::Flop => "flop",
        Street::Turn => "turn",
        Street::River => "river",
        Street::Showdown => "showdown",
        Street::Complete => "complete",
    }
}

fn action_name(action: &PlayerAction) -> &'static str {
    match action {
        PlayerAction::Fold => "fold",
        PlayerAction::Check => "check",
        PlayerAction::Call => "call",
        PlayerAction::RaiseTo(_) => "raise",
    }
}

fn timeout_action(to_call: Chips) -> PlayerAction {
    if to_call == Chips::ZERO {
        PlayerAction::Check
    } else {
        PlayerAction::Fold
    }
}

const fn suit_tag(suit: Suit) -> u8 {
    match suit {
        Suit::Clubs => 0,
        Suit::Diamonds => 1,
        Suit::Hearts => 2,
        Suit::Spades => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn 六人桌发牌索引包含烧牌间隔且不重叠() {
        let holes = (0..6).flat_map(hole_indices).collect::<BTreeSet<_>>();
        let board = board_indices(6).into_iter().collect::<BTreeSet<_>>();
        assert_eq!(holes.len(), 12);
        assert_eq!(board.len(), 5);
        assert!(holes.is_disjoint(&board));
        assert_eq!(board_indices(6), [13, 14, 15, 17, 19]);
    }

    #[test]
    fn 操作超时在无需跟注时过牌否则弃牌() {
        assert!(matches!(timeout_action(Chips::ZERO), PlayerAction::Check));
        assert!(matches!(timeout_action(Chips::new(1)), PlayerAction::Fold));
    }
}
