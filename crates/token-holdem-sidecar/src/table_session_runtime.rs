use crate::network_address::{preferred_dial_address, should_initiate_peer_dial};
use anyhow::{Context, Result};
use libp2p::{
    gossipsub,
    request_response::OutboundRequestId,
    swarm::dial_opts::{DialOpts, PeerCondition},
    Multiaddr, PeerId,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};
use token_holdem_domain::{
    Chips, DevicePublicKey, JoinClaimId, PhysicalSeat, PlayerId, ReadyHandRoster, StakeLevel,
    TableId, TableLifecycle, TABLE_CAPACITY, WAITING_CAPACITY,
};
use token_holdem_identity::{DeviceCertificate, DeviceIdentity};
use token_holdem_network::{
    verify_hand_roster_acceptances, verify_membership_acceptances, ControlRequest,
    HandRosterAcceptance, HandRosterMessage, HandRosterProposal, JoinIntent, LeaveIntent,
    MembershipAcceptance, MembershipProposal, MembershipSeatClaim, NetworkBehaviour, PoolTicket,
    SignedHandRosterProposal, SignedMembershipProposal, TableSessionMessage,
    TABLE_SESSION_TOPIC_PREFIX,
};

const JOIN_INTENT_LIFETIME_MS: u64 = 30 * 60 * 1_000;
const SESSION_TICKET_LIFETIME_MS: u64 = 30 * 60 * 1_000;
const JOIN_RENEWAL_MARGIN_MS: u64 = 5 * 60 * 1_000;
const HAND_START_DELAY: Duration = Duration::from_secs(3);
const SESSION_MESSAGE_REPUBLISH_INTERVAL: Duration = Duration::from_secs(1);
const MEMBERSHIP_CERTIFICATE_REPUBLISH_INTERVAL: Duration = Duration::from_secs(5);
const CONSENSUS_GOSSIP_REPUBLISH_INTERVAL: Duration = Duration::from_secs(1);
const MAX_CONSENSUS_GOSSIP_ATTEMPTS: u8 = 6;
const NON_OWNER_DIAL_FALLBACK_DELAY: Duration = Duration::from_secs(2);
const MAX_MESSAGE_BYTES: usize = 256 * 1_024;
const ROOM_WIRE_ENVELOPE_VERSION: u8 = 2;
const MAX_GOSSIPED_CONSENSUS_MESSAGES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalRoomRole {
    Joining,
    Waiting,
    Seated,
    Playing,
    Leaving,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RoomSeatProjection {
    pub(crate) physical_seat: u8,
    pub(crate) player_id: String,
    pub(crate) buy_in: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum TableSessionEvent {
    RoomEntered {
        table_id: String,
        level_id: String,
    },
    RoomSnapshot {
        table_id: String,
        membership_version: u64,
        seats: Vec<RoomSeatProjection>,
        waiting: Vec<String>,
        capacity: u8,
        local_role: LocalRoomRole,
        hand_number: Option<u64>,
        next_hand_countdown_ms: Option<u64>,
    },
    MembershipConfirmation {
        table_id: String,
        confirmed: u8,
        required: u8,
    },
    HandRosterConfirmation {
        table_id: String,
        hand_number: u64,
        confirmed: u8,
        required: u8,
    },
    NextHandReady {
        table_id: String,
        hand_number: u64,
        players: u8,
    },
    SafeLeaveRequested {
        table_id: String,
        after_hand_number: Option<u64>,
    },
    SafeLeaveCompleted {
        table_id: String,
    },
    RoomClosed {
        table_id: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ReadyTable {
    pub(crate) table_id: [u8; 32],
    pub(crate) roster_hash: [u8; 32],
    pub(crate) hand_number: u64,
    pub(crate) level: StakeLevel,
    pub(crate) local_seat: usize,
    pub(crate) dealer_index: usize,
    pub(crate) physical_seats: Vec<u8>,
    pub(crate) players: Vec<PlayerId>,
    pub(crate) device_public_keys: Vec<DevicePublicKey>,
    pub(crate) peer_ids: Vec<PeerId>,
    pub(crate) peer_addresses: Vec<Vec<Multiaddr>>,
    pub(crate) buy_ins: Vec<Chips>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
enum RoomWireMessage {
    Session(TableSessionMessage),
    Roster(HandRosterMessage),
    CertifiedMembership(Box<CertifiedMembership>),
    CertifiedRoster(Box<CertifiedRoster>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoomWireEnvelope {
    version: u8,
    emission_nonce: [u8; 16],
    message: RoomWireMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CertifiedMembership {
    proposal: SignedMembershipProposal,
    acceptances: Vec<MembershipAcceptance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DirectRoomTarget {
    peer_id: PeerId,
    message_hash: [u8; 32],
}

#[derive(Debug, Clone)]
struct DirectRoomRecipient {
    peer_id: PeerId,
    addresses: Vec<Multiaddr>,
}

#[derive(Debug, Clone)]
struct ConsensusGossipDelivery {
    message_hash: [u8; 32],
    successful_attempts: u8,
    last_attempted_at: Option<Instant>,
}

impl CertifiedMembership {
    fn assemble(
        proposal: SignedMembershipProposal,
        acceptances: impl IntoIterator<Item = MembershipAcceptance>,
        now_unix_ms: u64,
    ) -> Result<Self> {
        let mut acceptances = acceptances.into_iter().collect::<Vec<_>>();
        acceptances.sort_by_key(MembershipAcceptance::claim_id);
        verify_membership_acceptances(&proposal, &acceptances, now_unix_ms)?;
        Ok(Self {
            proposal,
            acceptances,
        })
    }

    fn verify_at(&self, now_unix_ms: u64) -> Result<()> {
        verify_membership_acceptances(&self.proposal, &self.acceptances, now_unix_ms)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CertifiedRoster {
    proposal: SignedHandRosterProposal,
    acceptances: Vec<HandRosterAcceptance>,
}

impl CertifiedRoster {
    fn assemble(
        proposal: SignedHandRosterProposal,
        acceptances: impl IntoIterator<Item = HandRosterAcceptance>,
        now_unix_ms: u64,
    ) -> Result<Self> {
        let mut acceptances = acceptances.into_iter().collect::<Vec<_>>();
        acceptances.sort_by_key(HandRosterAcceptance::claim_id);
        verify_hand_roster_acceptances(&proposal, &acceptances, now_unix_ms)?;
        Ok(Self {
            proposal,
            acceptances,
        })
    }

    fn verify_at(&self, now_unix_ms: u64) -> Result<()> {
        verify_hand_roster_acceptances(&self.proposal, &self.acceptances, now_unix_ms)?;
        Ok(())
    }
}

struct ActiveSession {
    table_id: TableId,
    creator_player_id: PlayerId,
    level: StakeLevel,
    topic: String,
    local_join: JoinIntent,
    joins: BTreeMap<JoinClaimId, JoinIntent>,
    leaves: BTreeMap<PlayerId, LeaveIntent>,
    membership: Option<SignedMembershipProposal>,
    membership_certificate: Option<CertifiedMembership>,
    last_session_messages_published_at: Option<Instant>,
    last_membership_certificate_published_at: Option<Instant>,
    pending_room_deliveries: HashMap<OutboundRequestId, DirectRoomTarget>,
    completed_room_deliveries: HashSet<DirectRoomTarget>,
    consensus_gossip_deliveries: VecDeque<ConsensusGossipDelivery>,
    explicit_peers: BTreeSet<PeerId>,
    disconnected_explicit_peers: BTreeMap<PeerId, Instant>,
    owns_explicit_peers: bool,
    pending_membership: Option<SignedMembershipProposal>,
    membership_acceptances: BTreeMap<JoinClaimId, MembershipAcceptance>,
    local_membership_acceptance: Option<MembershipAcceptance>,
    pending_roster: Option<SignedHandRosterProposal>,
    active_roster: Option<SignedHandRosterProposal>,
    roster_certificate: Option<CertifiedRoster>,
    last_roster_certificate_published_at: Option<Instant>,
    roster_acceptances: BTreeMap<JoinClaimId, HandRosterAcceptance>,
    local_roster_acceptance: Option<HandRosterAcceptance>,
    countdown_started_at: Option<Instant>,
    hand_active: bool,
    hand_clock_known: bool,
    next_hand_number: u64,
    previous_receipt_hash: Option<[u8; 32]>,
    previous_dealer_seat: Option<PhysicalSeat>,
    ready_table: Option<ReadyTable>,
    local_leave: Option<LeaveIntent>,
    leave_completed: bool,
}

#[derive(Default)]
pub(crate) struct TableSessionRuntime {
    active: Option<ActiveSession>,
}

impl TableSessionRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        table_id: TableId,
        creator_player_id: PlayerId,
        ticket: PoolTicket,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
        now_unix_ms: u64,
        now_monotonic: Instant,
    ) -> Result<Vec<TableSessionEvent>> {
        let local_join =
            issue_join_intent(table_id, ticket, device, certificate.clone(), now_unix_ms)?;
        let topic = session_topic(table_id);
        subscribe(swarm, &topic)?;
        let seat = MembershipSeatClaim::new(
            PhysicalSeat::new(1).context("创建牌桌时首个物理席位无效")?,
            local_join.clone(),
        );
        let proposal = MembershipProposal::assemble(
            table_id,
            local_join.level().clone(),
            1,
            None,
            vec![seat],
            Vec::new(),
            Vec::new(),
            now_unix_ms,
        )?;
        let signed =
            SignedMembershipProposal::issue(proposal, now_unix_ms, device, certificate.clone())?;
        let acceptance = MembershipAcceptance::issue(
            &signed,
            local_join.claim_id(),
            now_unix_ms,
            device,
            certificate,
        )?;
        let membership_certificate =
            CertifiedMembership::assemble(signed.clone(), [acceptance], now_unix_ms)?;
        let mut active = ActiveSession::new(table_id, creator_player_id, local_join, topic);
        active.hand_clock_known = true;
        active.membership = Some(signed);
        active.membership_certificate = Some(membership_certificate);
        self.active = Some(active);
        let active = self.active.as_ref().context("创建后的牌桌会话缺失")?;
        Ok(vec![
            TableSessionEvent::RoomEntered {
                table_id: table_id.to_string(),
                level_id: active.level.id().to_owned(),
            },
            active.snapshot(now_monotonic)?,
        ])
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn join(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        table_id: TableId,
        creator_player_id: PlayerId,
        ticket: PoolTicket,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
        now_unix_ms: u64,
        now_monotonic: Instant,
    ) -> Result<Vec<TableSessionEvent>> {
        let local_join = issue_join_intent(table_id, ticket, device, certificate, now_unix_ms)?;
        let topic = session_topic(table_id);
        subscribe(swarm, &topic)?;
        self.active = Some(ActiveSession::new(
            table_id,
            creator_player_id,
            local_join,
            topic,
        ));
        let active = self.active.as_ref().context("加入后的牌桌会话缺失")?;
        Ok(vec![
            TableSessionEvent::RoomEntered {
                table_id: table_id.to_string(),
                level_id: active.level.id().to_owned(),
            },
            active.snapshot(now_monotonic)?,
        ])
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn handles_topic(&self, topic: &str) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.topic == topic)
    }

    pub(crate) fn table_id(&self) -> Option<TableId> {
        self.active.as_ref().map(|active| active.table_id)
    }

    pub(crate) fn local_role(&self) -> Option<LocalRoomRole> {
        self.active.as_ref().map(ActiveSession::local_role)
    }

    pub(crate) fn local_admission_acknowledged(&self) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active
                .pending_membership
                .as_ref()
                .or(active.membership.as_ref())
                .is_some_and(|membership| active.contains_local_player(membership.proposal()))
        })
    }

    pub(crate) fn advertisement(
        &self,
    ) -> Option<super::table_pool_runtime::LocalTableAdvertisement> {
        let active = self.active.as_ref()?;
        let membership = active.membership.as_ref()?;
        let proposal = membership.proposal();
        // A table advertisement is the authoritative membership snapshot and may
        // be signed only by the current coordinator. If every member advertised
        // the same table, one transition would amplify into O(n) signed messages
        // and crowd out actual hand traffic at a six-player table.
        if proposal.coordinator_claim_id() != Some(active.local_join.claim_id()) {
            return None;
        }
        Some(super::table_pool_runtime::LocalTableAdvertisement {
            table_id: active.table_id,
            member_count: u8::try_from(proposal.seats().len()).unwrap_or(TABLE_CAPACITY),
            waiting_count: u8::try_from(proposal.waiting().len())
                .unwrap_or(u8::try_from(WAITING_CAPACITY).unwrap_or(6)),
            lifecycle: if active.hand_active {
                TableLifecycle::HandInProgress
            } else {
                TableLifecycle::Waiting
            },
            membership_version: proposal.membership_version(),
            membership_hash: *proposal.proposal_hash(),
            creator_player_id: active.creator_player_id,
            convergence_eligible: active.can_converge_singleton(),
        })
    }

    pub(crate) fn take_ready_table(&mut self) -> Option<ReadyTable> {
        self.active
            .as_mut()
            .and_then(|active| active.ready_table.take())
    }

    pub(crate) fn take_leave_completed(&mut self) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        std::mem::take(&mut active.leave_completed)
    }

    pub(crate) fn adopt_explicit_peers(&mut self, peers: impl IntoIterator<Item = PeerId>) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        active.explicit_peers.extend(peers);
        active.owns_explicit_peers = true;
    }

    pub(crate) fn peer_connected(&mut self, peer_id: PeerId) {
        if let Some(active) = self.active.as_mut() {
            active.disconnected_explicit_peers.remove(&peer_id);
        }
    }

    pub(crate) fn peer_disconnected(&mut self, peer_id: PeerId, now_monotonic: Instant) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.explicit_peers.contains(&peer_id) {
            active
                .disconnected_explicit_peers
                .entry(peer_id)
                .or_insert(now_monotonic);
        }
    }

    pub(crate) fn handle_direct_response(
        &mut self,
        request_id: OutboundRequestId,
        accepted: bool,
    ) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        let Some(target) = active.pending_room_deliveries.remove(&request_id) else {
            return false;
        };
        if accepted {
            active.completed_room_deliveries.insert(target);
        }
        true
    }

    pub(crate) fn handle_direct_failure(&mut self, request_id: OutboundRequestId) -> bool {
        self.active
            .as_mut()
            .is_some_and(|active| active.pending_room_deliveries.remove(&request_id).is_some())
    }

    pub(crate) fn close(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    ) -> Option<TableSessionEvent> {
        let active = self.active.take()?;
        if active.owns_explicit_peers {
            for peer_id in active.explicit_peers {
                swarm
                    .behaviour_mut()
                    .gossipsub
                    .remove_explicit_peer(&peer_id);
                swarm.behaviour_mut().release_peer_connection(peer_id);
            }
        }
        swarm
            .behaviour_mut()
            .gossipsub
            .unsubscribe(&gossipsub::IdentTopic::new(active.topic));
        Some(TableSessionEvent::RoomClosed {
            table_id: active.table_id.to_string(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tick(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        session_addresses: &[Multiaddr],
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
        now_unix_ms: u64,
        now_monotonic: Instant,
    ) -> Result<Vec<TableSessionEvent>> {
        let Some(active) = self.active.as_mut() else {
            return Ok(Vec::new());
        };
        let mut events = Vec::new();
        if active
            .local_join
            .expires_at_unix_ms()
            .saturating_sub(now_unix_ms)
            <= JOIN_RENEWAL_MARGIN_MS
            && active.local_leave.is_none()
        {
            let renewed_ticket = issue_local_session_ticket(
                swarm,
                active.level.clone(),
                active.local_join.ticket().requested_buy_in(),
                session_addresses.to_vec(),
                device,
                certificate.clone(),
                now_unix_ms,
            )?;
            let renewed = issue_join_intent(
                active.table_id,
                renewed_ticket,
                device,
                certificate.clone(),
                now_unix_ms,
            )?;
            active.local_join = renewed.clone();
            active.joins.insert(renewed.claim_id(), renewed);
        }
        active
            .joins
            .retain(|_, intent| intent.verify_at(now_unix_ms).is_ok());
        active
            .leaves
            .retain(|_, intent| intent.verify_at(now_unix_ms).is_ok());

        maybe_propose_membership(active, device, certificate, now_unix_ms)?;
        events.extend(finalize_membership_if_ready(
            active,
            now_unix_ms,
            now_monotonic,
        )?);
        maybe_propose_roster(active, device, certificate, now_unix_ms, now_monotonic)?;
        events.extend(finalize_roster_if_ready(active, now_unix_ms)?);
        synchronize_explicit_session_peers(active, swarm, now_monotonic)?;
        publish_active_messages(swarm, active, now_monotonic)?;
        Ok(events)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn handle_message(
        &mut self,
        source: Option<PeerId>,
        payload: &[u8],
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
        now_unix_ms: u64,
        now_monotonic: Instant,
    ) -> Result<Vec<TableSessionEvent>> {
        let active = self.active.as_mut().context("尚未进入牌桌房间")?;
        if payload.is_empty() || payload.len() > MAX_MESSAGE_BYTES {
            anyhow::bail!("牌桌房间消息必须为 1 字节到 256 KiB")
        }
        let envelope: RoomWireEnvelope =
            cbor4ii::serde::from_slice(payload).context("牌桌房间消息不是合法 CBOR")?;
        anyhow::ensure!(
            envelope.version == ROOM_WIRE_ENVELOPE_VERSION,
            "不支持的牌桌房间传输信封版本"
        );
        let mut events = Vec::new();
        match envelope.message {
            RoomWireMessage::Session(TableSessionMessage::JoinIntent(intent)) => {
                intent.verify_at(now_unix_ms)?;
                validate_join_scope(active, &intent)?;
                verify_source(source, intent.ticket().session_peer_id())?;
                active.joins.insert(intent.claim_id(), *intent);
            }
            RoomWireMessage::Session(TableSessionMessage::LeaveIntent(intent)) => {
                intent.verify_at(now_unix_ms)?;
                anyhow::ensure!(
                    intent.table_id() == active.table_id,
                    "离桌意图属于另一张牌桌"
                );
                verify_player_source(active, source, intent.player_id())?;
                active.leaves.insert(intent.player_id(), *intent);
            }
            RoomWireMessage::Session(TableSessionMessage::MembershipProposal(proposal)) => {
                if active
                    .membership
                    .as_ref()
                    .is_some_and(|current| current == proposal.as_ref())
                {
                    return Ok(Vec::new());
                }
                proposal.verify_at(now_unix_ms)?;
                validate_membership_scope(active, &proposal)?;
                let coordinator = proposal
                    .proposal()
                    .seat_by_claim_id(proposal.coordinator_claim_id())
                    .context("成员提案缺少协调者声明")?;
                verify_source(source, coordinator.join_intent().ticket().session_peer_id())?;
                remember_embedded_joins(active, proposal.proposal());
                accept_membership_proposal(active, *proposal, device, certificate, now_unix_ms)?;
            }
            RoomWireMessage::Session(TableSessionMessage::MembershipAcceptance(acceptance)) => {
                if active.membership.as_ref().is_some_and(|proposal| {
                    acceptance.proposal_hash() == proposal.proposal().proposal_hash()
                }) {
                    return Ok(Vec::new());
                }
                let proposal = active
                    .pending_membership
                    .as_ref()
                    .context("收到成员确认时尚无待确认提案")?;
                acceptance.verify_at(proposal, now_unix_ms)?;
                let seat = proposal
                    .proposal()
                    .seat_by_claim_id(acceptance.claim_id())
                    .context("成员确认声明不在提案中")?;
                verify_source(source, seat.join_intent().ticket().session_peer_id())?;
                active
                    .membership_acceptances
                    .insert(acceptance.claim_id(), *acceptance);
            }
            RoomWireMessage::CertifiedMembership(certificate) => {
                let proposal = &certificate.proposal;
                if active.membership.as_ref().is_some_and(|current| {
                    current.proposal().proposal_hash() == proposal.proposal().proposal_hash()
                }) {
                    return Ok(Vec::new());
                }
                if active.membership.as_ref().is_some_and(|current| {
                    proposal.proposal().membership_version()
                        <= current.proposal().membership_version()
                }) {
                    return Ok(Vec::new());
                }
                certificate.verify_at(now_unix_ms)?;
                validate_membership_scope(active, proposal)?;
                verify_membership_certificate_source(source, proposal.proposal())?;
                remember_embedded_joins(active, proposal.proposal());
                let required = proposal.proposal().seats().len();
                events.push(TableSessionEvent::MembershipConfirmation {
                    table_id: active.table_id.to_string(),
                    confirmed: u8::try_from(required).unwrap_or(u8::MAX),
                    required: u8::try_from(required).unwrap_or(u8::MAX),
                });
                events.extend(activate_membership(active, *certificate, now_monotonic)?);
            }
            RoomWireMessage::CertifiedRoster(certificate) => {
                let proposal = &certificate.proposal;
                if active.active_roster.as_ref().is_some_and(|current| {
                    current.proposal().proposal_hash() == proposal.proposal().proposal_hash()
                }) {
                    return Ok(Vec::new());
                }
                certificate.verify_at(now_unix_ms)?;
                verify_roster_certificate_source(source, proposal.proposal())?;
                if proposal
                    .proposal()
                    .endpoint_for_player(active.local_join.player_id())
                    .is_none()
                {
                    return Ok(Vec::new());
                }
                validate_roster_scope(active, proposal)?;
                events.extend(activate_roster(active, *certificate)?);
            }
            RoomWireMessage::Roster(HandRosterMessage::Proposal(proposal)) => {
                if active
                    .active_roster
                    .as_ref()
                    .is_some_and(|current| current == proposal.as_ref())
                {
                    return Ok(Vec::new());
                }
                proposal.verify_at(now_unix_ms)?;
                validate_roster_scope(active, &proposal)?;
                let coordinator_id = proposal.coordinator_claim_id();
                let coordinator = proposal
                    .membership()
                    .proposal()
                    .seat_by_claim_id(coordinator_id)
                    .context("逐手协调者不在成员提案中")?;
                verify_source(source, coordinator.join_intent().ticket().session_peer_id())?;
                let synchronizes_local_clock = proposal
                    .proposal()
                    .endpoint_for_player(active.local_join.player_id())
                    .is_some();
                let proposed_hand_number = proposal.proposal().ready_roster().hand_number();
                let previous_receipt_hash =
                    proposal.proposal().ready_roster().previous_receipt_hash();
                accept_roster_proposal(active, *proposal, device, certificate, now_unix_ms)?;
                if synchronizes_local_clock {
                    if !active.hand_clock_known && proposed_hand_number > active.next_hand_number {
                        active.next_hand_number = proposed_hand_number;
                        active.previous_receipt_hash = previous_receipt_hash;
                    }
                    active.hand_clock_known = true;
                }
            }
            RoomWireMessage::Roster(HandRosterMessage::Acceptance(acceptance)) => {
                if active.active_roster.as_ref().is_some_and(|proposal| {
                    acceptance.proposal_hash() == proposal.proposal().proposal_hash()
                }) {
                    return Ok(Vec::new());
                }
                let proposal = active
                    .pending_roster
                    .as_ref()
                    .context("收到逐手名单确认时尚无待确认名单")?;
                acceptance.verify_at(proposal, now_unix_ms)?;
                let endpoint = proposal
                    .proposal()
                    .endpoints()
                    .iter()
                    .find(|endpoint| endpoint.claim_id() == acceptance.claim_id())
                    .context("名单确认声明不在本手名单中")?;
                verify_source(source, endpoint.session_peer_id())?;
                active
                    .roster_acceptances
                    .insert(acceptance.claim_id(), *acceptance);
            }
        }
        events.extend(finalize_membership_if_ready(
            active,
            now_unix_ms,
            now_monotonic,
        )?);
        maybe_propose_roster(active, device, certificate, now_unix_ms, now_monotonic)?;
        events.extend(finalize_roster_if_ready(active, now_unix_ms)?);
        Ok(events)
    }

    pub(crate) fn request_leave(
        &mut self,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
        now_unix_ms: u64,
    ) -> Result<Vec<TableSessionEvent>> {
        let active = self.active.as_mut().context("尚未进入牌桌房间")?;
        if active.local_leave.is_some() {
            return Ok(Vec::new());
        }
        let after_hand_number = active.hand_active.then_some(active.next_hand_number);
        let membership_version = active
            .membership
            .as_ref()
            .map_or(1, |membership| membership.proposal().membership_version());
        let mut nonce = [0_u8; 16];
        OsRng.fill_bytes(&mut nonce);
        let expires_at = now_unix_ms
            .checked_add(JOIN_INTENT_LIFETIME_MS)
            .context("离桌意图有效期溢出")?;
        let intent = LeaveIntent::issue(
            active.table_id,
            after_hand_number,
            membership_version,
            now_unix_ms,
            expires_at,
            nonce,
            device,
            certificate,
        )?;
        active.leaves.insert(intent.player_id(), intent.clone());
        active.local_leave = Some(intent);
        if !active.hand_active {
            active.pending_membership = None;
            active.membership_acceptances.clear();
            active.local_membership_acceptance = None;
            if active.local_is_only_seat() {
                active.leave_completed = true;
            }
        }
        let mut events = vec![
            TableSessionEvent::SafeLeaveRequested {
                table_id: active.table_id.to_string(),
                after_hand_number,
            },
            active.snapshot(Instant::now())?,
        ];
        if active.leave_completed {
            events.push(TableSessionEvent::SafeLeaveCompleted {
                table_id: active.table_id.to_string(),
            });
        }
        Ok(events)
    }

    pub(crate) fn on_hand_boundary(
        &mut self,
        receipt_hash: [u8; 32],
        dealer_seat: PhysicalSeat,
        now_monotonic: Instant,
    ) -> Result<Vec<TableSessionEvent>> {
        let active = self.active.as_mut().context("尚未进入牌桌房间")?;
        anyhow::ensure!(active.hand_active, "当前没有可结束的手牌");
        active.hand_active = false;
        active.hand_clock_known = true;
        active.previous_receipt_hash = Some(receipt_hash);
        active.previous_dealer_seat = Some(dealer_seat);
        active.next_hand_number = active
            .next_hand_number
            .checked_add(1)
            .context("下一手编号溢出")?;
        active.pending_roster = None;
        active.active_roster = None;
        active.roster_certificate = None;
        active.last_roster_certificate_published_at = None;
        active.roster_acceptances.clear();
        active.local_roster_acceptance = None;
        active.pending_membership = None;
        active.membership_acceptances.clear();
        active.local_membership_acceptance = None;
        active.countdown_started_at = Some(now_monotonic);
        let mut events = vec![active.snapshot(now_monotonic)?];
        if active.local_leave.is_some() && active.local_is_only_seat() {
            active.leave_completed = true;
            events.push(TableSessionEvent::SafeLeaveCompleted {
                table_id: active.table_id.to_string(),
            });
        }
        Ok(events)
    }
}

impl ActiveSession {
    fn contains_local_player(&self, proposal: &MembershipProposal) -> bool {
        let local_player = self.local_join.player_id();
        proposal.seat_by_player(local_player).is_some()
            || proposal
                .waiting()
                .iter()
                .any(|intent| intent.player_id() == local_player)
    }

    fn can_converge_singleton(&self) -> bool {
        !self.hand_active
            && self.local_leave.is_none()
            && self.pending_membership.is_none()
            && self.pending_roster.is_none()
            && self.joins.len() == 1
            && self.leaves.is_empty()
            && self.membership.as_ref().is_some_and(|membership| {
                let proposal = membership.proposal();
                proposal.membership_version() == 1
                    && proposal.seats().len() == 1
                    && proposal.waiting().is_empty()
                    && proposal.seats().first().is_some_and(|seat| {
                        seat.join_intent().claim_id() == self.local_join.claim_id()
                    })
            })
    }

    fn local_is_only_seat(&self) -> bool {
        self.membership.as_ref().is_some_and(|membership| {
            membership.proposal().seats().len() == 1
                && membership.proposal().seats().first().is_some_and(|seat| {
                    seat.join_intent().player_id() == self.local_join.player_id()
                })
        })
    }

    fn new(
        table_id: TableId,
        creator_player_id: PlayerId,
        local_join: JoinIntent,
        topic: String,
    ) -> Self {
        let level = local_join.level().clone();
        let joins = BTreeMap::from([(local_join.claim_id(), local_join.clone())]);
        Self {
            table_id,
            creator_player_id,
            level,
            topic,
            local_join,
            joins,
            leaves: BTreeMap::new(),
            membership: None,
            membership_certificate: None,
            last_session_messages_published_at: None,
            last_membership_certificate_published_at: None,
            pending_room_deliveries: HashMap::new(),
            completed_room_deliveries: HashSet::new(),
            consensus_gossip_deliveries: VecDeque::new(),
            explicit_peers: BTreeSet::new(),
            disconnected_explicit_peers: BTreeMap::new(),
            owns_explicit_peers: false,
            pending_membership: None,
            membership_acceptances: BTreeMap::new(),
            local_membership_acceptance: None,
            pending_roster: None,
            active_roster: None,
            roster_certificate: None,
            last_roster_certificate_published_at: None,
            roster_acceptances: BTreeMap::new(),
            local_roster_acceptance: None,
            countdown_started_at: None,
            hand_active: false,
            hand_clock_known: false,
            next_hand_number: 1,
            previous_receipt_hash: None,
            previous_dealer_seat: None,
            ready_table: None,
            local_leave: None,
            leave_completed: false,
        }
    }

    fn local_role(&self) -> LocalRoomRole {
        if self.local_leave.is_some() {
            return LocalRoomRole::Leaving;
        }
        let Some(membership) = self.membership.as_ref() else {
            return LocalRoomRole::Joining;
        };
        let proposal = membership.proposal();
        if proposal
            .seat_by_player(self.local_join.player_id())
            .is_some()
        {
            if self.hand_active {
                LocalRoomRole::Playing
            } else {
                LocalRoomRole::Seated
            }
        } else if proposal
            .waiting()
            .iter()
            .any(|intent| intent.player_id() == self.local_join.player_id())
        {
            LocalRoomRole::Waiting
        } else {
            LocalRoomRole::Joining
        }
    }

    fn snapshot(&self, now_monotonic: Instant) -> Result<TableSessionEvent> {
        let Some(membership) = self.membership.as_ref() else {
            return Ok(TableSessionEvent::RoomSnapshot {
                table_id: self.table_id.to_string(),
                membership_version: 0,
                seats: Vec::new(),
                waiting: Vec::new(),
                capacity: TABLE_CAPACITY,
                local_role: self.local_role(),
                hand_number: self.hand_active.then_some(self.next_hand_number),
                next_hand_countdown_ms: None,
            });
        };
        let proposal = membership.proposal();
        let seats = proposal
            .seats()
            .iter()
            .map(|seat| RoomSeatProjection {
                physical_seat: seat.physical_seat().value(),
                player_id: seat.join_intent().player_id().to_string(),
                buy_in: seat.join_intent().ticket().requested_buy_in().value(),
            })
            .collect();
        let waiting = proposal
            .waiting()
            .iter()
            .map(|intent| intent.player_id().to_string())
            .collect();
        let next_hand_countdown_ms = self.countdown_started_at.map(|started| {
            let elapsed = now_monotonic.saturating_duration_since(started);
            HAND_START_DELAY
                .saturating_sub(elapsed)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        });
        Ok(TableSessionEvent::RoomSnapshot {
            table_id: self.table_id.to_string(),
            membership_version: proposal.membership_version(),
            seats,
            waiting,
            capacity: TABLE_CAPACITY,
            local_role: self.local_role(),
            hand_number: self.hand_active.then_some(self.next_hand_number),
            next_hand_countdown_ms,
        })
    }
}

fn maybe_propose_membership(
    active: &mut ActiveSession,
    device: &DeviceIdentity,
    certificate: &DeviceCertificate,
    now_unix_ms: u64,
) -> Result<()> {
    if active.pending_membership.is_some() {
        return Ok(());
    }
    // A joiner must catch up to the creator's confirmed membership baseline and
    // cannot fork a separate v1 proposal from its partial view.
    if active.membership.is_none() {
        return Ok(());
    }
    let valid_joins = canonical_player_joins(active, now_unix_ms);
    if valid_joins.is_empty() {
        return Ok(());
    }
    let (seats, waiting) = next_membership_layout(active, &valid_joins)?;
    if seats.is_empty() {
        return Ok(());
    }
    let coordinator = seats
        .iter()
        .min_by_key(|seat| seat.physical_seat())
        .map(|seat| seat.join_intent().claim_id())
        .context("成员提案缺少协调者")?;
    let local_claim = seats
        .iter()
        .find(|seat| seat.join_intent().player_id() == active.local_join.player_id())
        .map(|seat| seat.join_intent().claim_id());
    if local_claim != Some(coordinator) {
        return Ok(());
    }
    let previous_hash = active
        .membership
        .as_ref()
        .map(|membership| *membership.proposal().proposal_hash());
    let version = active.membership.as_ref().map_or(1, |membership| {
        membership.proposal().membership_version().saturating_add(1)
    });
    if membership_layout_unchanged(active.membership.as_ref(), &seats, &waiting) {
        return Ok(());
    }
    let proposal = MembershipProposal::assemble(
        active.table_id,
        active.level.clone(),
        version,
        previous_hash,
        seats,
        waiting,
        active.leaves.values().cloned().collect(),
        now_unix_ms,
    )?;
    let signed =
        SignedMembershipProposal::issue(proposal, now_unix_ms, device, certificate.clone())?;
    accept_membership_proposal(active, signed, device, certificate, now_unix_ms)
}

fn canonical_player_joins(active: &ActiveSession, now_unix_ms: u64) -> Vec<JoinIntent> {
    let mut by_player = BTreeMap::<PlayerId, JoinIntent>::new();
    for intent in active.joins.values() {
        if intent.verify_at(now_unix_ms).is_err()
            || intent.table_id() != active.table_id
            || intent.level() != &active.level
        {
            continue;
        }
        match by_player.entry(intent.player_id()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(intent.clone());
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let current = entry.get();
                if intent.created_at_unix_ms() > current.created_at_unix_ms()
                    || (intent.created_at_unix_ms() == current.created_at_unix_ms()
                        && intent.claim_id() < current.claim_id())
                {
                    entry.insert(intent.clone());
                }
            }
        }
    }
    let mut joins = by_player.into_values().collect::<Vec<_>>();
    joins.sort_by_key(JoinIntent::claim_id);
    joins
}

fn next_membership_layout(
    active: &ActiveSession,
    valid_joins: &[JoinIntent],
) -> Result<(Vec<MembershipSeatClaim>, Vec<JoinIntent>)> {
    let joins_by_player = valid_joins
        .iter()
        .map(|intent| (intent.player_id(), intent.clone()))
        .collect::<BTreeMap<_, _>>();
    let leaving = active
        .leaves
        .keys()
        .copied()
        .collect::<BTreeSet<PlayerId>>();
    let mut seats = BTreeMap::<PhysicalSeat, JoinIntent>::new();
    if let Some(current) = &active.membership {
        for seat in current.proposal().seats() {
            let player_id = seat.join_intent().player_id();
            if !active.hand_active && leaving.contains(&player_id) {
                continue;
            }
            if let Some(intent) = joins_by_player.get(&player_id) {
                seats.insert(seat.physical_seat(), intent.clone());
            }
        }
    }
    let mut occupied_players = seats
        .values()
        .map(JoinIntent::player_id)
        .collect::<BTreeSet<_>>();
    let mut candidates = valid_joins
        .iter()
        .filter(|intent| {
            !occupied_players.contains(&intent.player_id())
                && (!leaving.contains(&intent.player_id()) || active.hand_active)
        })
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by_key(JoinIntent::claim_id);
    let mut waiting = Vec::new();
    for intent in candidates {
        if active.hand_active || seats.len() >= usize::from(TABLE_CAPACITY) {
            if waiting.len() < WAITING_CAPACITY {
                waiting.push(intent);
            }
            continue;
        }
        let physical = (1..=TABLE_CAPACITY)
            .filter_map(|value| PhysicalSeat::new(value).ok())
            .find(|seat| !seats.contains_key(seat))
            .context("成员未满但找不到空物理席位")?;
        occupied_players.insert(intent.player_id());
        seats.insert(physical, intent);
    }
    if active.hand_active {
        if let Some(current) = &active.membership {
            for intent in current.proposal().waiting() {
                if !waiting
                    .iter()
                    .any(|candidate| candidate.player_id() == intent.player_id())
                    && joins_by_player.contains_key(&intent.player_id())
                    && waiting.len() < WAITING_CAPACITY
                {
                    waiting.push(
                        joins_by_player
                            .get(&intent.player_id())
                            .context("候补玩家缺少有效入桌声明")?
                            .clone(),
                    );
                }
            }
        }
    }
    waiting.sort_by_key(JoinIntent::claim_id);
    let seats = seats
        .into_iter()
        .map(|(physical, intent)| MembershipSeatClaim::new(physical, intent))
        .collect();
    Ok((seats, waiting))
}

fn membership_layout_unchanged(
    current: Option<&SignedMembershipProposal>,
    seats: &[MembershipSeatClaim],
    waiting: &[JoinIntent],
) -> bool {
    let Some(current) = current else {
        return false;
    };
    current.proposal().seats().len() == seats.len()
        && current.proposal().waiting().len() == waiting.len()
        && current
            .proposal()
            .seats()
            .iter()
            .zip(seats)
            .all(|(left, right)| {
                left.physical_seat() == right.physical_seat()
                    && left.join_intent().claim_id() == right.join_intent().claim_id()
            })
        && current
            .proposal()
            .waiting()
            .iter()
            .zip(waiting)
            .all(|(left, right)| left.claim_id() == right.claim_id())
}

fn accept_membership_proposal(
    active: &mut ActiveSession,
    proposal: SignedMembershipProposal,
    device: &DeviceIdentity,
    certificate: &DeviceCertificate,
    now_unix_ms: u64,
) -> Result<()> {
    let replace = active.pending_membership.as_ref().is_none_or(|current| {
        proposal.proposal().membership_version() > current.proposal().membership_version()
            || (proposal.proposal().membership_version() == current.proposal().membership_version()
                && proposal.proposal().proposal_hash() < current.proposal().proposal_hash())
    });
    if !replace {
        return Ok(());
    }
    let local_seat = proposal
        .proposal()
        .seat_by_player(active.local_join.player_id());
    active.pending_membership = Some(proposal.clone());
    active.membership_acceptances.clear();
    active.local_membership_acceptance = None;
    if let Some(local_seat) = local_seat {
        let acceptance = MembershipAcceptance::issue(
            &proposal,
            local_seat.join_intent().claim_id(),
            now_unix_ms,
            device,
            certificate.clone(),
        )?;
        active
            .membership_acceptances
            .insert(acceptance.claim_id(), acceptance.clone());
        active.local_membership_acceptance = Some(acceptance);
    }
    Ok(())
}

fn finalize_membership_if_ready(
    active: &mut ActiveSession,
    now_unix_ms: u64,
    now_monotonic: Instant,
) -> Result<Vec<TableSessionEvent>> {
    let Some(proposal) = active.pending_membership.as_ref() else {
        return Ok(Vec::new());
    };
    let required = proposal.proposal().seats().len();
    let confirmed = active.membership_acceptances.len();
    let mut events = vec![TableSessionEvent::MembershipConfirmation {
        table_id: active.table_id.to_string(),
        confirmed: u8::try_from(confirmed).unwrap_or(u8::MAX),
        required: u8::try_from(required).unwrap_or(u8::MAX),
    }];
    if confirmed != required
        || verify_membership_acceptances(
            proposal,
            active.membership_acceptances.values(),
            now_unix_ms,
        )
        .is_err()
    {
        return Ok(events);
    }
    let accepted = active
        .pending_membership
        .take()
        .context("待确认成员提案在完成时缺失")?;
    let certificate = CertifiedMembership::assemble(
        accepted,
        active.membership_acceptances.values().cloned(),
        now_unix_ms,
    )?;
    events.extend(activate_membership(active, certificate, now_monotonic)?);
    Ok(events)
}

fn activate_membership(
    active: &mut ActiveSession,
    certificate: CertifiedMembership,
    now_monotonic: Instant,
) -> Result<Vec<TableSessionEvent>> {
    active.membership = Some(certificate.proposal.clone());
    active.membership_certificate = Some(certificate);
    active.last_membership_certificate_published_at = None;
    active.pending_membership = None;
    active.membership_acceptances.clear();
    // Local acceptance is durable signature evidence, not temporary pending
    // state. Other participants need it to reconstruct a missed final
    // certificate. Keep it until the next proposal or hand boundary.
    active.pending_roster = None;
    active.roster_acceptances.clear();
    // A new membership certificate invalidates the old hand roster, so clear
    // only hand-roster acceptances here.
    active.local_roster_acceptance = None;
    if !active.hand_active
        && active
            .membership
            .as_ref()
            .is_some_and(|membership| membership.proposal().seats().len() >= 2)
    {
        active.countdown_started_at.get_or_insert(now_monotonic);
    }
    let mut events = Vec::new();
    let local_present = active.membership.as_ref().is_some_and(|membership| {
        membership
            .proposal()
            .seat_by_player(active.local_join.player_id())
            .is_some()
            || membership
                .proposal()
                .waiting()
                .iter()
                .any(|intent| intent.player_id() == active.local_join.player_id())
    });
    if active.local_leave.is_some() && !local_present {
        active.leave_completed = true;
        events.push(TableSessionEvent::SafeLeaveCompleted {
            table_id: active.table_id.to_string(),
        });
    }
    events.push(active.snapshot(now_monotonic)?);
    Ok(events)
}

fn maybe_propose_roster(
    active: &mut ActiveSession,
    device: &DeviceIdentity,
    certificate: &DeviceCertificate,
    now_unix_ms: u64,
    now_monotonic: Instant,
) -> Result<()> {
    if active.hand_active || active.pending_roster.is_some() || active.pending_membership.is_some()
    {
        return Ok(());
    }
    let Some(membership) = active.membership.as_ref() else {
        return Ok(());
    };
    if membership.proposal().seats().len() < 2 {
        active.countdown_started_at = None;
        return Ok(());
    }
    let started = active.countdown_started_at.get_or_insert(now_monotonic);
    if now_monotonic.saturating_duration_since(*started) < HAND_START_DELAY {
        return Ok(());
    }
    let coordinator = membership
        .proposal()
        .coordinator_claim_id()
        .context("逐手名单缺少协调者")?;
    let local_claim = membership
        .proposal()
        .seat_by_player(active.local_join.player_id())
        .map(|seat| seat.join_intent().claim_id());
    if local_claim != Some(coordinator) {
        return Ok(());
    }
    let domain_membership = membership.proposal().as_membership()?;
    let ready = ReadyHandRoster::from_membership(
        &domain_membership,
        active.next_hand_number,
        active.previous_receipt_hash,
        active.previous_dealer_seat,
    )?;
    let proposal = HandRosterProposal::assemble(ready, membership, now_unix_ms)?;
    let signed = SignedHandRosterProposal::issue(
        proposal,
        membership.clone(),
        now_unix_ms,
        device,
        certificate.clone(),
    )?;
    accept_roster_proposal(active, signed, device, certificate, now_unix_ms)
}

fn accept_roster_proposal(
    active: &mut ActiveSession,
    proposal: SignedHandRosterProposal,
    device: &DeviceIdentity,
    certificate: &DeviceCertificate,
    now_unix_ms: u64,
) -> Result<()> {
    let Some(local_endpoint) = proposal
        .proposal()
        .endpoint_for_player(active.local_join.player_id())
    else {
        return Ok(());
    };
    let replace = active.pending_roster.as_ref().is_none_or(|current| {
        proposal.proposal().ready_roster().hand_number()
            > current.proposal().ready_roster().hand_number()
            || (proposal.proposal().ready_roster().hand_number()
                == current.proposal().ready_roster().hand_number()
                && proposal.proposal().proposal_hash() < current.proposal().proposal_hash())
    });
    if !replace {
        return Ok(());
    }
    let acceptance = HandRosterAcceptance::issue(
        &proposal,
        local_endpoint.claim_id(),
        now_unix_ms,
        device,
        certificate.clone(),
    )?;
    active.pending_roster = Some(proposal);
    active.roster_acceptances.clear();
    active
        .roster_acceptances
        .insert(acceptance.claim_id(), acceptance.clone());
    active.local_roster_acceptance = Some(acceptance);
    Ok(())
}

fn finalize_roster_if_ready(
    active: &mut ActiveSession,
    now_unix_ms: u64,
) -> Result<Vec<TableSessionEvent>> {
    let Some(proposal) = active.pending_roster.as_ref() else {
        return Ok(Vec::new());
    };
    let required = proposal.proposal().endpoints().len();
    let confirmed = active.roster_acceptances.len();
    let hand_number = proposal.proposal().ready_roster().hand_number();
    let events = vec![TableSessionEvent::HandRosterConfirmation {
        table_id: active.table_id.to_string(),
        hand_number,
        confirmed: u8::try_from(confirmed).unwrap_or(u8::MAX),
        required: u8::try_from(required).unwrap_or(u8::MAX),
    }];
    if confirmed != required
        || verify_hand_roster_acceptances(proposal, active.roster_acceptances.values(), now_unix_ms)
            .is_err()
    {
        return Ok(events);
    }
    let accepted = active
        .pending_roster
        .take()
        .context("待确认逐手名单在完成时缺失")?;
    let certificate = CertifiedRoster::assemble(
        accepted,
        active.roster_acceptances.values().cloned(),
        now_unix_ms,
    )?;
    activate_roster(active, certificate)
}

fn activate_roster(
    active: &mut ActiveSession,
    certificate: CertifiedRoster,
) -> Result<Vec<TableSessionEvent>> {
    let proposal = certificate.proposal.clone();
    let required = proposal.proposal().endpoints().len();
    let hand_number = proposal.proposal().ready_roster().hand_number();
    let ready = ready_table_from_proposal(&proposal, active.local_join.player_id())?;
    active.ready_table = Some(ready);
    active.active_roster = Some(proposal);
    active.roster_certificate = Some(certificate);
    active.last_roster_certificate_published_at = None;
    active.pending_roster = None;
    active.roster_acceptances.clear();
    // Like membership acceptance, roster acceptance must be retransmitted after
    // local activation. Otherwise the coordinator can switch state while a peer
    // misses the certificate, permanently leaving consensus at N-1/N.
    active.hand_active = true;
    active.countdown_started_at = None;
    Ok(vec![
        TableSessionEvent::HandRosterConfirmation {
            table_id: active.table_id.to_string(),
            hand_number,
            confirmed: u8::try_from(required).unwrap_or(u8::MAX),
            required: u8::try_from(required).unwrap_or(u8::MAX),
        },
        TableSessionEvent::NextHandReady {
            table_id: active.table_id.to_string(),
            hand_number,
            players: u8::try_from(required).unwrap_or(TABLE_CAPACITY),
        },
    ])
}

fn ready_table_from_proposal(
    proposal: &SignedHandRosterProposal,
    local_player: PlayerId,
) -> Result<ReadyTable> {
    let roster = proposal.proposal().ready_roster();
    let membership = proposal.membership().proposal();
    let mut seats = roster.seats().to_vec();
    seats.sort_by_key(|seat| seat.hand_index());
    let local_seat = seats
        .iter()
        .position(|seat| seat.player_id() == local_player)
        .context("逐手名单缺少本机玩家")?;
    let dealer_index = seats
        .iter()
        .position(|seat| seat.physical_seat() == roster.dealer_seat())
        .context("逐手名单缺少庄家物理席位")?;
    let mut peer_ids = Vec::with_capacity(seats.len());
    let mut peer_addresses = Vec::with_capacity(seats.len());
    for seat in &seats {
        let claim = membership
            .seat_by_player(seat.player_id())
            .context("逐手玩家不在成员提案中")?;
        let peer_id = PeerId::from_bytes(claim.join_intent().ticket().session_peer_id())
            .context("逐手玩家 PeerId 无效")?;
        let address = preferred_dial_address(
            claim
                .join_intent()
                .ticket()
                .session_addresses()
                .iter()
                .map(|raw| peer_dial_address(raw, peer_id))
                .collect::<Result<Vec<_>>>()?,
        )
        .context("逐手玩家没有签名拨号地址")?;
        peer_ids.push(peer_id);
        peer_addresses.push(vec![address]);
    }
    Ok(ReadyTable {
        table_id: *roster.table_id().as_bytes(),
        roster_hash: *proposal.proposal().proposal_hash(),
        hand_number: roster.hand_number(),
        level: membership.level().clone(),
        local_seat,
        dealer_index,
        physical_seats: seats
            .iter()
            .map(|seat| seat.physical_seat().value())
            .collect(),
        players: seats.iter().map(|seat| seat.player_id()).collect(),
        device_public_keys: seats.iter().map(|seat| seat.device_public_key()).collect(),
        peer_ids,
        peer_addresses,
        buy_ins: seats.iter().map(|seat| seat.buy_in()).collect(),
    })
}

fn peer_dial_address(raw: &[u8], expected_peer_id: PeerId) -> Result<Multiaddr> {
    let mut address = Multiaddr::try_from(raw.to_vec()).context("逐手玩家拨号地址无效")?;
    match address.pop() {
        Some(libp2p::multiaddr::Protocol::P2p(peer_id)) if peer_id == expected_peer_id => {}
        _ => anyhow::bail!("逐手玩家拨号地址没有绑定预期 PeerId"),
    }
    anyhow::ensure!(!address.is_empty(), "逐手玩家拨号地址缺少传输端点");
    Ok(address)
}

fn synchronize_explicit_session_peers(
    active: &mut ActiveSession,
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    now_monotonic: Instant,
) -> Result<()> {
    if !active.owns_explicit_peers {
        return Ok(());
    }

    let local_peer_id = *swarm.local_peer_id();
    let mut desired = BTreeMap::<PeerId, BTreeSet<Multiaddr>>::new();
    for proposal in active
        .membership
        .iter()
        .chain(active.pending_membership.iter())
    {
        collect_explicit_recipients(
            &mut desired,
            membership_recipients(proposal.proposal())?,
            local_peer_id,
        );
    }
    // A room of at most six members maintains a small full mesh. Signatures, not
    // topology, determine consensus, so certificates can route around a half-open
    // host connection.

    let desired_peer_ids = desired.keys().copied().collect::<BTreeSet<_>>();
    let stale_peer_ids = active
        .explicit_peers
        .difference(&desired_peer_ids)
        .copied()
        .collect::<Vec<_>>();
    for peer_id in stale_peer_ids {
        swarm
            .behaviour_mut()
            .gossipsub
            .remove_explicit_peer(&peer_id);
        swarm.behaviour_mut().release_peer_connection(peer_id);
        active.explicit_peers.remove(&peer_id);
        active.disconnected_explicit_peers.remove(&peer_id);
    }

    let mut dial_started = false;
    for (peer_id, addresses) in desired {
        let Some(address) = preferred_dial_address(addresses) else {
            continue;
        };
        swarm.add_peer_address(peer_id, address.clone());
        swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
        swarm.behaviour_mut().retain_peer_connection(peer_id);
        active.explicit_peers.insert(peer_id);
        let owns_dial = should_initiate_peer_dial(local_peer_id, peer_id);
        let fallback_due = active
            .disconnected_explicit_peers
            .get(&peer_id)
            .is_some_and(|disconnected_at| {
                now_monotonic.saturating_duration_since(*disconnected_at)
                    >= NON_OWNER_DIAL_FALLBACK_DELAY
            });
        // Normally only one side of a peer pair dials. If the other side observes
        // a half-open connection and the responsible dialer has not recovered in
        // two seconds, it takes over once to avoid a permanent partition.
        if (owns_dial || fallback_due)
            && !dial_started
            && !swarm.is_connected(&peer_id)
            && swarm
                .dial(
                    DialOpts::peer_id(peer_id)
                        .condition(PeerCondition::DisconnectedAndNotDialing)
                        .addresses(vec![address])
                        .build(),
                )
                .is_ok()
        {
            dial_started = true;
        }
    }
    Ok(())
}

fn collect_explicit_recipients(
    desired: &mut BTreeMap<PeerId, BTreeSet<Multiaddr>>,
    recipients: impl IntoIterator<Item = DirectRoomRecipient>,
    local_peer_id: PeerId,
) {
    for recipient in recipients {
        if recipient.peer_id == local_peer_id {
            continue;
        }
        desired
            .entry(recipient.peer_id)
            .or_default()
            .extend(recipient.addresses);
    }
}

fn validate_join_scope(active: &ActiveSession, intent: &JoinIntent) -> Result<()> {
    anyhow::ensure!(
        intent.table_id() == active.table_id,
        "入桌意图属于另一张牌桌"
    );
    anyhow::ensure!(intent.level() == &active.level, "入桌意图牌局级别不一致");
    Ok(())
}

fn validate_membership_scope(
    active: &ActiveSession,
    proposal: &SignedMembershipProposal,
) -> Result<()> {
    anyhow::ensure!(
        proposal.proposal().table_id() == active.table_id,
        "成员提案属于另一张牌桌"
    );
    anyhow::ensure!(
        proposal.proposal().level() == &active.level,
        "成员提案牌局级别不一致"
    );
    if let Some(current) = &active.membership {
        anyhow::ensure!(
            proposal.proposal().membership_version()
                == current.proposal().membership_version().saturating_add(1),
            "成员提案版本不连续"
        );
        anyhow::ensure!(
            proposal.proposal().previous_membership_hash()
                == Some(*current.proposal().proposal_hash()),
            "成员提案没有衔接当前成员摘要"
        );
        if active.hand_active {
            for seat in current.proposal().seats() {
                let next = proposal
                    .proposal()
                    .seat_by_player(seat.join_intent().player_id())
                    .context("手牌进行中的成员提案移除了当前参与者")?;
                anyhow::ensure!(
                    next.physical_seat() == seat.physical_seat(),
                    "手牌进行中的成员提案改变了当前参与者物理席位"
                );
            }
        }
    } else {
        let local_player_id = active.local_join.player_id();
        let contains_local_player = proposal
            .proposal()
            .seat_by_player(local_player_id)
            .is_some()
            || proposal
                .proposal()
                .waiting()
                .iter()
                .any(|intent| intent.player_id() == local_player_id);
        anyhow::ensure!(contains_local_player, "首次同步的成员快照没有包含本地玩家");
        // A late joiner lacks the complete earlier certificate chain and may start
        // from the current confirmed snapshot. This does not trust the host:
        // finalize_membership_if_ready still requires signatures from every
        // current member before the snapshot becomes local truth.
        if proposal.proposal().membership_version() == 1 {
            anyhow::ensure!(
                proposal.proposal().previous_membership_hash().is_none(),
                "首版成员提案不能引用上一版本"
            );
        } else {
            anyhow::ensure!(
                proposal.proposal().previous_membership_hash().is_some(),
                "高版本成员快照必须引用上一版本"
            );
        }
    }
    Ok(())
}

fn validate_roster_scope(
    active: &ActiveSession,
    proposal: &SignedHandRosterProposal,
) -> Result<()> {
    let membership = active
        .membership
        .as_ref()
        .context("逐手名单到达时成员共识尚未完成")?;
    anyhow::ensure!(
        proposal.membership() == membership,
        "逐手名单引用了不同的成员提案"
    );
    let roster = proposal.proposal().ready_roster();
    let hand_number_matches = roster.hand_number() == active.next_hand_number;
    let can_adopt_signed_clock = !active.hand_clock_known
        && roster.hand_number() > active.next_hand_number
        && roster.previous_receipt_hash().is_some()
        && proposal
            .proposal()
            .endpoint_for_player(active.local_join.player_id())
            .is_some();
    anyhow::ensure!(
        hand_number_matches || can_adopt_signed_clock,
        "逐手名单手牌编号与房间状态不一致"
    );
    Ok(())
}

fn remember_embedded_joins(active: &mut ActiveSession, proposal: &MembershipProposal) {
    for intent in proposal
        .seats()
        .iter()
        .map(MembershipSeatClaim::join_intent)
        .chain(proposal.waiting().iter())
    {
        active.joins.insert(intent.claim_id(), intent.clone());
    }
    for intent in proposal.leave_intents() {
        active.leaves.insert(intent.player_id(), intent.clone());
    }
}

fn verify_player_source(
    active: &ActiveSession,
    source: Option<PeerId>,
    player_id: PlayerId,
) -> Result<()> {
    let expected = active
        .joins
        .values()
        .find(|intent| intent.player_id() == player_id)
        .context("无法从入桌声明解析离桌玩家端点")?;
    verify_source(source, expected.ticket().session_peer_id())
}

fn verify_membership_certificate_source(
    source: Option<PeerId>,
    proposal: &MembershipProposal,
) -> Result<()> {
    let source = source.context("成员证书缺少源 PeerId")?;
    let source_is_participant = proposal
        .seats()
        .iter()
        .map(MembershipSeatClaim::join_intent)
        .chain(proposal.waiting().iter())
        .any(|intent| {
            PeerId::from_bytes(intent.ticket().session_peer_id())
                .is_ok_and(|peer_id| peer_id == source)
        });
    anyhow::ensure!(source_is_participant, "成员证书不是由证书内参与者转发");
    Ok(())
}

fn verify_roster_certificate_source(
    source: Option<PeerId>,
    proposal: &HandRosterProposal,
) -> Result<()> {
    let source = source.context("逐手名单证书缺少源 PeerId")?;
    let source_is_participant = proposal.endpoints().iter().any(|endpoint| {
        PeerId::from_bytes(endpoint.session_peer_id()).is_ok_and(|peer_id| peer_id == source)
    });
    anyhow::ensure!(source_is_participant, "逐手名单证书不是由本手参与者转发");
    Ok(())
}

fn verify_source(source: Option<PeerId>, expected: &[u8]) -> Result<()> {
    let source = source.context("严格签名的房间消息缺少源 PeerId")?;
    let expected = PeerId::from_bytes(expected).context("房间消息声明的 PeerId 无效")?;
    anyhow::ensure!(
        source == expected,
        "房间消息源与签名对象的会话 PeerId 不一致"
    );
    Ok(())
}

fn issue_join_intent(
    table_id: TableId,
    ticket: PoolTicket,
    device: &DeviceIdentity,
    certificate: DeviceCertificate,
    now_unix_ms: u64,
) -> Result<JoinIntent> {
    let desired_expiry = now_unix_ms
        .checked_add(JOIN_INTENT_LIFETIME_MS)
        .context("入桌意图有效期溢出")?;
    let expires_at = desired_expiry.min(ticket.expires_at_unix_ms());
    let mut nonce = [0_u8; 16];
    OsRng.fill_bytes(&mut nonce);
    JoinIntent::issue(
        table_id,
        ticket,
        now_unix_ms,
        expires_at,
        nonce,
        device,
        certificate,
    )
    .context("无法签发入桌意图")
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn issue_local_session_ticket(
    swarm: &libp2p::Swarm<NetworkBehaviour>,
    level: StakeLevel,
    buy_in: Chips,
    session_addresses: Vec<Multiaddr>,
    device: &DeviceIdentity,
    certificate: DeviceCertificate,
    now_unix_ms: u64,
) -> Result<PoolTicket> {
    let expires_at_unix_ms = now_unix_ms
        .checked_add(SESSION_TICKET_LIFETIME_MS)
        .context("牌桌会话票据有效期溢出")?;
    let mut nonce = [0_u8; 16];
    OsRng.fill_bytes(&mut nonce);
    PoolTicket::issue(
        swarm.local_peer_id().to_bytes(),
        session_addresses
            .into_iter()
            .map(|address| address.to_vec())
            .collect(),
        level,
        buy_in,
        now_unix_ms,
        expires_at_unix_ms,
        nonce,
        device,
        certificate,
    )
    .context("无法签发牌桌会话票据")
}

fn membership_proposal_for_local_acceptance(
    active: &ActiveSession,
) -> Option<SignedMembershipProposal> {
    let acceptance = active.local_membership_acceptance.as_ref()?;
    [
        active.pending_membership.as_ref(),
        active
            .membership_certificate
            .as_ref()
            .map(|certificate| &certificate.proposal),
    ]
    .into_iter()
    .flatten()
    .find(|proposal| proposal.proposal().proposal_hash() == acceptance.proposal_hash())
    .cloned()
}

fn roster_proposal_for_local_acceptance(
    active: &ActiveSession,
) -> Option<SignedHandRosterProposal> {
    let acceptance = active.local_roster_acceptance.as_ref()?;
    [
        active.pending_roster.as_ref(),
        active
            .roster_certificate
            .as_ref()
            .map(|certificate| &certificate.proposal),
    ]
    .into_iter()
    .flatten()
    .find(|proposal| proposal.proposal().proposal_hash() == acceptance.proposal_hash())
    .cloned()
}

fn publish_active_messages(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    active: &mut ActiveSession,
    now_monotonic: Instant,
) -> Result<()> {
    // The final roster certificate exists only after the coordinator collects all
    // acceptances, so every recipient has already verified membership and the
    // proposal. It must preempt the bounded per-peer request slot; otherwise a
    // lost response for an older certificate can head-of-line block consensus.
    publish_roster_certificate_if_needed(swarm, active, now_monotonic)?;

    // Before a roster certificate exists, retain the strict dependency order:
    // membership certificate -> membership change -> roster.
    publish_membership_certificate_if_needed(swarm, active, now_monotonic)?;
    let should_publish_session_messages =
        active
            .last_session_messages_published_at
            .is_none_or(|last| {
                now_monotonic.saturating_duration_since(last) >= SESSION_MESSAGE_REPUBLISH_INTERVAL
            });
    if should_publish_session_messages {
        publish(
            swarm,
            &active.topic,
            &RoomWireMessage::Session(TableSessionMessage::JoinIntent(Box::new(
                active.local_join.clone(),
            ))),
        )?;
        if let Some(leave) = &active.local_leave {
            publish(
                swarm,
                &active.topic,
                &RoomWireMessage::Session(TableSessionMessage::LeaveIntent(Box::new(
                    leave.clone(),
                ))),
            )?;
        }
        if let Some(proposal) = active.pending_membership.clone() {
            if local_is_membership_coordinator(active, proposal.proposal())? {
                deliver_consensus_message(
                    swarm,
                    active,
                    &RoomWireMessage::Session(TableSessionMessage::MembershipProposal(Box::new(
                        proposal.clone(),
                    ))),
                    membership_recipients(proposal.proposal())?,
                    now_monotonic,
                )?;
            }
        }
        if let (Some(acceptance), Some(proposal)) = (
            active.local_membership_acceptance.clone(),
            membership_proposal_for_local_acceptance(active),
        ) {
            let recipients = if local_is_membership_coordinator(active, proposal.proposal())? {
                Vec::new()
            } else {
                vec![membership_coordinator_recipient(proposal.proposal())?]
            };
            deliver_consensus_message(
                swarm,
                active,
                &RoomWireMessage::Session(TableSessionMessage::MembershipAcceptance(Box::new(
                    acceptance,
                ))),
                recipients,
                now_monotonic,
            )?;
        }
        if let Some(proposal) = active.pending_roster.clone() {
            if local_is_roster_coordinator(active, &proposal)? {
                deliver_consensus_message(
                    swarm,
                    active,
                    &RoomWireMessage::Roster(HandRosterMessage::Proposal(Box::new(
                        proposal.clone(),
                    ))),
                    roster_recipients(proposal.proposal())?,
                    now_monotonic,
                )?;
            }
        }
        if let (Some(acceptance), Some(proposal)) = (
            active.local_roster_acceptance.clone(),
            roster_proposal_for_local_acceptance(active),
        ) {
            let recipients = if local_is_roster_coordinator(active, &proposal)? {
                Vec::new()
            } else {
                vec![roster_coordinator_recipient(&proposal)?]
            };
            deliver_consensus_message(
                swarm,
                active,
                &RoomWireMessage::Roster(HandRosterMessage::Acceptance(Box::new(acceptance))),
                recipients,
                now_monotonic,
            )?;
        }
        active.last_session_messages_published_at = Some(now_monotonic);
    }
    Ok(())
}

fn publish_membership_certificate_if_needed(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    active: &mut ActiveSession,
    now_monotonic: Instant,
) -> Result<bool> {
    let Some(certificate) = active.membership_certificate.clone() else {
        return Ok(true);
    };
    let local_is_coordinator =
        local_is_membership_coordinator(active, certificate.proposal.proposal())?;
    let message = RoomWireMessage::CertifiedMembership(Box::new(certificate.clone()));
    let recipients = if local_is_coordinator {
        membership_recipients(certificate.proposal.proposal())?
    } else {
        Vec::new()
    };
    let direct_complete = consensus_delivery_complete(swarm, active, &message, &recipients)?;
    let periodic_republish_due =
        active
            .last_membership_certificate_published_at
            .is_none_or(|last| {
                now_monotonic.saturating_duration_since(last)
                    >= MEMBERSHIP_CERTIFICATE_REPUBLISH_INTERVAL
            });
    if direct_complete && !periodic_republish_due {
        return Ok(true);
    }
    let complete = deliver_consensus_message(swarm, active, &message, recipients, now_monotonic)?;
    if complete {
        active.last_membership_certificate_published_at = Some(now_monotonic);
    }
    Ok(complete)
}

fn publish_roster_certificate_if_needed(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    active: &mut ActiveSession,
    now_monotonic: Instant,
) -> Result<()> {
    let Some(certificate) = active.roster_certificate.clone() else {
        return Ok(());
    };
    let local_is_coordinator = local_is_roster_coordinator(active, &certificate.proposal)?;
    let message = RoomWireMessage::CertifiedRoster(Box::new(certificate.clone()));
    let recipients = if local_is_coordinator {
        roster_recipients(certificate.proposal.proposal())?
    } else {
        Vec::new()
    };
    let direct_complete = consensus_delivery_complete(swarm, active, &message, &recipients)?;
    let periodic_republish_due = active
        .last_roster_certificate_published_at
        .is_none_or(|last| {
            now_monotonic.saturating_duration_since(last)
                >= MEMBERSHIP_CERTIFICATE_REPUBLISH_INTERVAL
        });
    if direct_complete && !periodic_republish_due {
        return Ok(());
    }
    let complete = deliver_consensus_message(swarm, active, &message, recipients, now_monotonic)?;
    if complete {
        active.last_roster_certificate_published_at = Some(now_monotonic);
    }
    Ok(())
}

fn local_is_membership_coordinator(
    active: &ActiveSession,
    proposal: &MembershipProposal,
) -> Result<bool> {
    let coordinator = proposal
        .coordinator_claim_id()
        .context("成员提案缺少协调者")?;
    Ok(proposal
        .seat_by_player(active.local_join.player_id())
        .is_some_and(|seat| seat.join_intent().claim_id() == coordinator))
}

fn local_is_roster_coordinator(
    active: &ActiveSession,
    proposal: &SignedHandRosterProposal,
) -> Result<bool> {
    let local_claim = proposal
        .proposal()
        .endpoint_for_player(active.local_join.player_id())
        .map(|endpoint| endpoint.claim_id());
    Ok(local_claim == Some(proposal.coordinator_claim_id()))
}

fn consensus_delivery_complete(
    swarm: &libp2p::Swarm<NetworkBehaviour>,
    active: &ActiveSession,
    message: &RoomWireMessage,
    recipients: &[DirectRoomRecipient],
) -> Result<bool> {
    let local_peer_id = *swarm.local_peer_id();
    let message_hash = room_message_hash(message)?;
    Ok(recipients.iter().all(|recipient| {
        recipient.peer_id == local_peer_id
            || active
                .completed_room_deliveries
                .contains(&DirectRoomTarget {
                    peer_id: recipient.peer_id,
                    message_hash,
                })
    }))
}

fn membership_coordinator_recipient(proposal: &MembershipProposal) -> Result<DirectRoomRecipient> {
    let coordinator = proposal
        .coordinator_claim_id()
        .and_then(|claim| proposal.seat_by_claim_id(claim))
        .context("成员提案缺少协调者席位")?;
    direct_room_recipient(
        coordinator.join_intent().ticket().session_peer_id(),
        coordinator.join_intent().ticket().session_addresses(),
    )
}

fn roster_coordinator_recipient(
    proposal: &SignedHandRosterProposal,
) -> Result<DirectRoomRecipient> {
    let endpoint = proposal
        .proposal()
        .endpoints()
        .iter()
        .find(|endpoint| endpoint.claim_id() == proposal.coordinator_claim_id())
        .context("逐手名单缺少协调者端点")?;
    direct_room_recipient(endpoint.session_peer_id(), endpoint.session_addresses())
}

fn publish(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    topic: &str,
    message: &RoomWireMessage,
) -> Result<bool> {
    let payload = encode_room_message(message)?;
    Ok(swarm
        .behaviour_mut()
        .gossipsub
        .publish(gossipsub::IdentTopic::new(topic), payload)
        .is_ok())
}

fn deliver_consensus_message(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    active: &mut ActiveSession,
    message: &RoomWireMessage,
    recipients: Vec<DirectRoomRecipient>,
    now_monotonic: Instant,
) -> Result<bool> {
    let local_peer_id = *swarm.local_peer_id();
    let message_hash = room_message_hash(message)?;
    let payload = encode_room_message(message)?;
    republish_consensus_over_gossip(swarm, active, message_hash, payload.clone(), now_monotonic);

    let mut complete = true;
    for recipient in recipients {
        if recipient.peer_id == local_peer_id {
            continue;
        }
        let target = DirectRoomTarget {
            peer_id: recipient.peer_id,
            message_hash,
        };
        if active.completed_room_deliveries.contains(&target) {
            continue;
        }
        complete = false;
        if active
            .pending_room_deliveries
            .values()
            .any(|pending| pending.peer_id == recipient.peer_id)
        {
            continue;
        }
        for address in recipient.addresses {
            swarm.add_peer_address(recipient.peer_id, address);
        }
        if !swarm.is_connected(&recipient.peer_id) {
            continue;
        }
        let request_id = swarm.behaviour_mut().control.send_request(
            &recipient.peer_id,
            ControlRequest::TableSession(payload.clone()),
        );
        active.pending_room_deliveries.insert(request_id, target);
    }
    Ok(complete)
}

fn republish_consensus_over_gossip(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    active: &mut ActiveSession,
    message_hash: [u8; 32],
    payload: Vec<u8>,
    now_monotonic: Instant,
) {
    let delivery_index = active
        .consensus_gossip_deliveries
        .iter()
        .position(|delivery| delivery.message_hash == message_hash);
    let index = delivery_index.unwrap_or_else(|| {
        if active.consensus_gossip_deliveries.len() >= MAX_GOSSIPED_CONSENSUS_MESSAGES {
            active.consensus_gossip_deliveries.pop_front();
        }
        active
            .consensus_gossip_deliveries
            .push_back(ConsensusGossipDelivery {
                message_hash,
                successful_attempts: 0,
                last_attempted_at: None,
            });
        active.consensus_gossip_deliveries.len() - 1
    });
    let delivery = &mut active.consensus_gossip_deliveries[index];
    if delivery.successful_attempts >= MAX_CONSENSUS_GOSSIP_ATTEMPTS
        || delivery.last_attempted_at.is_some_and(|last| {
            now_monotonic.saturating_duration_since(last) < CONSENSUS_GOSSIP_REPUBLISH_INTERVAL
        })
    {
        return;
    }
    delivery.last_attempted_at = Some(now_monotonic);
    if swarm
        .behaviour_mut()
        .gossipsub
        .publish(gossipsub::IdentTopic::new(&active.topic), payload)
        .is_ok()
    {
        delivery.successful_attempts = delivery.successful_attempts.saturating_add(1);
    }
}

fn room_message_hash(message: &RoomWireMessage) -> Result<[u8; 32]> {
    let message_bytes =
        cbor4ii::serde::to_vec(Vec::new(), message).context("无法摘要牌桌共识消息")?;
    Ok(*blake3::hash(&message_bytes).as_bytes())
}

fn membership_recipients(proposal: &MembershipProposal) -> Result<Vec<DirectRoomRecipient>> {
    proposal
        .seats()
        .iter()
        .map(MembershipSeatClaim::join_intent)
        .chain(proposal.waiting().iter())
        .map(|intent| {
            direct_room_recipient(
                intent.ticket().session_peer_id(),
                intent.ticket().session_addresses(),
            )
        })
        .collect()
}

fn roster_recipients(proposal: &HandRosterProposal) -> Result<Vec<DirectRoomRecipient>> {
    proposal
        .endpoints()
        .iter()
        .map(|endpoint| {
            direct_room_recipient(endpoint.session_peer_id(), endpoint.session_addresses())
        })
        .collect()
}

fn direct_room_recipient(
    raw_peer_id: &[u8],
    raw_addresses: &[Vec<u8>],
) -> Result<DirectRoomRecipient> {
    let peer_id = PeerId::from_bytes(raw_peer_id).context("牌桌共识参与者 PeerId 无效")?;
    let address = preferred_dial_address(
        raw_addresses
            .iter()
            .map(|raw| peer_dial_address(raw, peer_id).context("牌桌共识参与者拨号地址无效"))
            .collect::<Result<Vec<_>>>()?,
    )
    .context("牌桌共识参与者没有可用的拨号地址")?;
    Ok(DirectRoomRecipient {
        peer_id,
        addresses: vec![address],
    })
}

fn encode_room_message(message: &RoomWireMessage) -> Result<Vec<u8>> {
    let mut emission_nonce = [0_u8; 16];
    OsRng.fill_bytes(&mut emission_nonce);
    let envelope = RoomWireEnvelope {
        version: ROOM_WIRE_ENVELOPE_VERSION,
        emission_nonce,
        message: message.clone(),
    };
    cbor4ii::serde::to_vec(Vec::new(), &envelope).context("无法序列化牌桌房间消息")
}

fn subscribe(swarm: &mut libp2p::Swarm<NetworkBehaviour>, topic: &str) -> Result<()> {
    swarm
        .behaviour_mut()
        .gossipsub
        .subscribe(&gossipsub::IdentTopic::new(topic))
        .with_context(|| format!("无法订阅牌桌房间频道：{topic}"))?;
    Ok(())
}

fn session_topic(table_id: TableId) -> String {
    format!("{TABLE_SESSION_TOPIC_PREFIX}{table_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_holdem_domain::{TableMember, TableMembership};

    #[test]
    fn 房间开局规则固定为两人门槛和三秒观察窗() {
        assert_eq!(TABLE_CAPACITY, 6);
        assert_eq!(WAITING_CAPACITY, 6);
        assert_eq!(HAND_START_DELAY, Duration::from_secs(3));
    }

    #[test]
    fn 稳定席位布局在手牌中只把新玩家放入候补() {
        let table = TableId::new([7; 32]);
        let membership = TableMembership::new(
            table,
            1,
            [
                TableMember::new(
                    PlayerId::new([1; 32]),
                    DevicePublicKey::new([1; 32]),
                    Chips::new(100),
                    PhysicalSeat::new(1).expect("测试席位应有效"),
                ),
                TableMember::new(
                    PlayerId::new([2; 32]),
                    DevicePublicKey::new([2; 32]),
                    Chips::new(100),
                    PhysicalSeat::new(4).expect("测试席位应有效"),
                ),
            ],
            [],
        )
        .expect("测试成员应有效");
        assert_eq!(membership.members().count(), 2);
        assert_eq!(membership.waiting().len(), 0);
    }
}
