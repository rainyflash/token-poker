use crate::{
    protocol_encoding::{write_level, write_optional_hash, write_table_id},
    PoolTicket, PoolTicketId, TablePoolProtocolError,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt::Display};
use thiserror::Error;
use token_holdem_domain::{
    DevicePublicKey, JoinCandidate, JoinClaimId, MembershipError, PhysicalSeat, PlayerId,
    StakeLevel, TableId, TableMember, TableMembership, TABLE_CAPACITY, WAITING_CAPACITY,
};
use token_holdem_identity::{
    is_signed_time_before_expiry, is_signed_time_not_future, is_signed_time_window_active,
    DeviceAttestation, DeviceAttestationError, DeviceCertificate, DeviceIdentity,
};

const JOIN_INTENT_DOMAIN: &[u8] = b"token-holdem/join-intent/v1\0";
const LEAVE_INTENT_DOMAIN: &[u8] = b"token-holdem/leave-intent/v1\0";
const MEMBERSHIP_PROPOSAL_DOMAIN: &[u8] = b"token-holdem/membership-proposal/v1\0";
const MEMBERSHIP_ACCEPTANCE_DOMAIN: &[u8] = b"token-holdem/membership-acceptance/v1\0";
const MAX_INTENT_LIFETIME_MS: u64 = 60 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinIntent {
    version: u8,
    table_id: TableId,
    ticket: PoolTicket,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    nonce: [u8; 16],
    attestation: DeviceAttestation,
}

impl JoinIntent {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        table_id: TableId,
        ticket: PoolTicket,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        nonce: [u8; 16],
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, TableSessionProtocolError> {
        ticket.verify_at(created_at_unix_ms)?;
        validate_join_window(&ticket, created_at_unix_ms, expires_at_unix_ms)?;
        validate_ticket_signer(&ticket, device, &certificate)?;
        let version = 1;
        let unsigned = canonical_join_intent_bytes(
            version,
            table_id,
            ticket.id(),
            created_at_unix_ms,
            expires_at_unix_ms,
            &nonce,
        );
        let attestation = DeviceAttestation::issue(
            JOIN_INTENT_DOMAIN,
            &unsigned,
            created_at_unix_ms,
            device,
            certificate,
        )?;
        Ok(Self {
            version,
            table_id,
            ticket,
            created_at_unix_ms,
            expires_at_unix_ms,
            nonce,
            attestation,
        })
    }

    pub fn verify_at(&self, now_unix_ms: u64) -> Result<(), TableSessionProtocolError> {
        if self.version != 1 {
            return Err(TableSessionProtocolError::UnsupportedJoinIntentVersion(
                self.version,
            ));
        }
        self.ticket.verify_at(now_unix_ms)?;
        validate_join_window(
            &self.ticket,
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
        )?;
        validate_active_window(
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            now_unix_ms,
            TableSessionProtocolError::JoinIntentExpired,
        )?;
        let certificate = self.attestation.certificate();
        if certificate.player_id() != self.ticket.player_id()
            || certificate.device_public_key() != self.ticket.device_public_key()
        {
            return Err(TableSessionProtocolError::JoinIntentIdentityMismatch);
        }
        self.attestation.verify(
            JOIN_INTENT_DOMAIN,
            &self.canonical_unsigned_bytes(),
            now_unix_ms,
        )?;
        Ok(())
    }

    pub fn claim_id(&self) -> JoinClaimId {
        JoinClaimId::new(*blake3::hash(&self.canonical_unsigned_bytes()).as_bytes())
    }

    pub const fn table_id(&self) -> TableId {
        self.table_id
    }

    pub const fn player_id(&self) -> PlayerId {
        self.ticket.player_id()
    }

    pub const fn device_public_key(&self) -> DevicePublicKey {
        self.ticket.device_public_key()
    }

    pub fn level(&self) -> &StakeLevel {
        self.ticket.level()
    }

    pub const fn ticket(&self) -> &PoolTicket {
        &self.ticket
    }

    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }

    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }

    pub fn as_candidate(&self) -> JoinCandidate {
        JoinCandidate::new(
            self.claim_id(),
            self.player_id(),
            self.device_public_key(),
            self.ticket.requested_buy_in(),
        )
    }

    pub fn canonical_unsigned_bytes(&self) -> Vec<u8> {
        canonical_join_intent_bytes(
            self.version,
            self.table_id,
            self.ticket.id(),
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            &self.nonce,
        )
    }
}

fn canonical_join_intent_bytes(
    version: u8,
    table_id: TableId,
    ticket_id: PoolTicketId,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    nonce: &[u8; 16],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(160);
    bytes.extend_from_slice(JOIN_INTENT_DOMAIN);
    bytes.push(version);
    write_table_id(&mut bytes, table_id);
    bytes.extend_from_slice(ticket_id.as_bytes());
    bytes.extend_from_slice(&created_at_unix_ms.to_be_bytes());
    bytes.extend_from_slice(&expires_at_unix_ms.to_be_bytes());
    bytes.extend_from_slice(nonce);
    bytes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LeaveIntentId([u8; 32]);

impl LeaveIntentId {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Display for LeaveIntentId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaveIntent {
    version: u8,
    table_id: TableId,
    after_hand_number: Option<u64>,
    membership_version: u64,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    nonce: [u8; 16],
    attestation: DeviceAttestation,
}

impl LeaveIntent {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        table_id: TableId,
        after_hand_number: Option<u64>,
        membership_version: u64,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        nonce: [u8; 16],
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, TableSessionProtocolError> {
        validate_leave_fields(after_hand_number, created_at_unix_ms, expires_at_unix_ms)?;
        if certificate.device_public_key() != device.public_key() {
            return Err(TableSessionProtocolError::LeaveIntentIdentityMismatch);
        }
        let version = 1;
        let player_id = certificate.player_id();
        let device_public_key = certificate.device_public_key();
        let unsigned = canonical_leave_intent_bytes(
            version,
            table_id,
            after_hand_number,
            membership_version,
            player_id,
            device_public_key,
            created_at_unix_ms,
            expires_at_unix_ms,
            &nonce,
        );
        let attestation = DeviceAttestation::issue(
            LEAVE_INTENT_DOMAIN,
            &unsigned,
            created_at_unix_ms,
            device,
            certificate,
        )?;
        Ok(Self {
            version,
            table_id,
            after_hand_number,
            membership_version,
            player_id,
            device_public_key,
            created_at_unix_ms,
            expires_at_unix_ms,
            nonce,
            attestation,
        })
    }

    pub fn verify_at(&self, now_unix_ms: u64) -> Result<(), TableSessionProtocolError> {
        if self.version != 1 {
            return Err(TableSessionProtocolError::UnsupportedLeaveIntentVersion(
                self.version,
            ));
        }
        validate_leave_fields(
            self.after_hand_number,
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
        )?;
        validate_active_window(
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            now_unix_ms,
            TableSessionProtocolError::LeaveIntentExpired,
        )?;
        let certificate = self.attestation.certificate();
        if certificate.player_id() != self.player_id
            || certificate.device_public_key() != self.device_public_key
        {
            return Err(TableSessionProtocolError::LeaveIntentIdentityMismatch);
        }
        self.attestation.verify(
            LEAVE_INTENT_DOMAIN,
            &self.canonical_unsigned_bytes(),
            now_unix_ms,
        )?;
        Ok(())
    }

    pub fn id(&self) -> LeaveIntentId {
        LeaveIntentId(*blake3::hash(&self.canonical_unsigned_bytes()).as_bytes())
    }

    pub const fn table_id(&self) -> TableId {
        self.table_id
    }

    pub const fn after_hand_number(&self) -> Option<u64> {
        self.after_hand_number
    }

    pub const fn membership_version(&self) -> u64 {
        self.membership_version
    }

    pub const fn player_id(&self) -> PlayerId {
        self.player_id
    }

    pub const fn device_public_key(&self) -> DevicePublicKey {
        self.device_public_key
    }

    pub fn canonical_unsigned_bytes(&self) -> Vec<u8> {
        canonical_leave_intent_bytes(
            self.version,
            self.table_id,
            self.after_hand_number,
            self.membership_version,
            self.player_id,
            self.device_public_key,
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            &self.nonce,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn canonical_leave_intent_bytes(
    version: u8,
    table_id: TableId,
    after_hand_number: Option<u64>,
    membership_version: u64,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    nonce: &[u8; 16],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(192);
    bytes.extend_from_slice(LEAVE_INTENT_DOMAIN);
    bytes.push(version);
    write_table_id(&mut bytes, table_id);
    match after_hand_number {
        Some(hand_number) => {
            bytes.push(1);
            bytes.extend_from_slice(&hand_number.to_be_bytes());
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(&membership_version.to_be_bytes());
    bytes.extend_from_slice(player_id.as_bytes());
    bytes.extend_from_slice(device_public_key.as_bytes());
    bytes.extend_from_slice(&created_at_unix_ms.to_be_bytes());
    bytes.extend_from_slice(&expires_at_unix_ms.to_be_bytes());
    bytes.extend_from_slice(nonce);
    bytes
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipSeatClaim {
    physical_seat: PhysicalSeat,
    join_intent: JoinIntent,
}

impl MembershipSeatClaim {
    pub const fn new(physical_seat: PhysicalSeat, join_intent: JoinIntent) -> Self {
        Self {
            physical_seat,
            join_intent,
        }
    }

    pub const fn physical_seat(&self) -> PhysicalSeat {
        self.physical_seat
    }

    pub const fn join_intent(&self) -> &JoinIntent {
        &self.join_intent
    }

    pub fn as_member(&self) -> TableMember {
        TableMember::new(
            self.join_intent.player_id(),
            self.join_intent.device_public_key(),
            self.join_intent.ticket().requested_buy_in(),
            self.physical_seat,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipProposal {
    version: u8,
    table_id: TableId,
    level: StakeLevel,
    membership_version: u64,
    previous_membership_hash: Option<[u8; 32]>,
    seats: Vec<MembershipSeatClaim>,
    waiting: Vec<JoinIntent>,
    leave_intents: Vec<LeaveIntent>,
    proposal_hash: [u8; 32],
}

impl MembershipProposal {
    #[allow(clippy::too_many_arguments)]
    pub fn assemble(
        table_id: TableId,
        level: StakeLevel,
        membership_version: u64,
        previous_membership_hash: Option<[u8; 32]>,
        mut seats: Vec<MembershipSeatClaim>,
        mut waiting: Vec<JoinIntent>,
        mut leave_intents: Vec<LeaveIntent>,
        now_unix_ms: u64,
    ) -> Result<Self, TableSessionProtocolError> {
        seats.sort_by_key(MembershipSeatClaim::physical_seat);
        waiting.sort_by_key(JoinIntent::claim_id);
        leave_intents.sort_by_key(LeaveIntent::id);
        let mut proposal = Self {
            version: 1,
            table_id,
            level,
            membership_version,
            previous_membership_hash,
            seats,
            waiting,
            leave_intents,
            proposal_hash: [0; 32],
        };
        proposal.validate_contents(now_unix_ms)?;
        proposal.proposal_hash = *blake3::hash(&proposal.canonical_unsigned_bytes()).as_bytes();
        Ok(proposal)
    }

    pub fn verify_at(&self, now_unix_ms: u64) -> Result<(), TableSessionProtocolError> {
        if self.version != 1 {
            return Err(
                TableSessionProtocolError::UnsupportedMembershipProposalVersion(self.version),
            );
        }
        self.validate_contents(now_unix_ms)?;
        let expected = *blake3::hash(&self.canonical_unsigned_bytes()).as_bytes();
        if expected != self.proposal_hash {
            return Err(TableSessionProtocolError::MembershipProposalHashMismatch);
        }
        Ok(())
    }

    pub const fn table_id(&self) -> TableId {
        self.table_id
    }

    pub fn level(&self) -> &StakeLevel {
        &self.level
    }

    pub const fn membership_version(&self) -> u64 {
        self.membership_version
    }

    pub const fn previous_membership_hash(&self) -> Option<[u8; 32]> {
        self.previous_membership_hash
    }

    pub fn seats(&self) -> &[MembershipSeatClaim] {
        &self.seats
    }

    pub fn waiting(&self) -> &[JoinIntent] {
        &self.waiting
    }

    pub fn leave_intents(&self) -> &[LeaveIntent] {
        &self.leave_intents
    }

    pub const fn proposal_hash(&self) -> &[u8; 32] {
        &self.proposal_hash
    }

    pub fn coordinator_claim_id(&self) -> Option<JoinClaimId> {
        self.seats
            .iter()
            .min_by_key(|seat| seat.physical_seat())
            .map(|seat| seat.join_intent().claim_id())
    }

    pub fn seat_by_claim_id(&self, claim_id: JoinClaimId) -> Option<&MembershipSeatClaim> {
        self.seats
            .iter()
            .find(|seat| seat.join_intent().claim_id() == claim_id)
    }

    pub fn seat_by_player(&self, player_id: PlayerId) -> Option<&MembershipSeatClaim> {
        self.seats
            .iter()
            .find(|seat| seat.join_intent().player_id() == player_id)
    }

    pub fn as_membership(&self) -> Result<TableMembership, TableSessionProtocolError> {
        Ok(TableMembership::new(
            self.table_id,
            self.membership_version,
            self.seats.iter().map(MembershipSeatClaim::as_member),
            self.waiting.iter().map(JoinIntent::as_candidate),
        )?)
    }

    fn validate_contents(&self, now_unix_ms: u64) -> Result<(), TableSessionProtocolError> {
        if self.membership_version == 0 {
            return Err(TableSessionProtocolError::InvalidMembershipVersion);
        }
        if self.seats.is_empty() || self.seats.len() > usize::from(TABLE_CAPACITY) {
            return Err(TableSessionProtocolError::InvalidMembershipSize(
                self.seats.len(),
            ));
        }
        if self.waiting.len() > WAITING_CAPACITY {
            return Err(TableSessionProtocolError::WaitingListFull);
        }
        if self.leave_intents.len() > usize::from(TABLE_CAPACITY) + WAITING_CAPACITY {
            return Err(TableSessionProtocolError::TooManyLeaveIntents);
        }
        if !self
            .seats
            .windows(2)
            .all(|pair| pair[0].physical_seat() < pair[1].physical_seat())
            || !self
                .waiting
                .windows(2)
                .all(|pair| pair[0].claim_id() < pair[1].claim_id())
            || !self
                .leave_intents
                .windows(2)
                .all(|pair| pair[0].id() < pair[1].id())
        {
            return Err(TableSessionProtocolError::NonCanonicalOrdering);
        }
        for seat in &self.seats {
            self.validate_join_intent(seat.join_intent(), now_unix_ms)?;
        }
        for intent in &self.waiting {
            self.validate_join_intent(intent, now_unix_ms)?;
        }
        let mut leaving_players = BTreeSet::new();
        for intent in &self.leave_intents {
            intent.verify_at(now_unix_ms)?;
            if intent.table_id() != self.table_id {
                return Err(TableSessionProtocolError::WrongTable);
            }
            if !leaving_players.insert(intent.player_id()) {
                return Err(TableSessionProtocolError::DuplicateLeaveIntent);
            }
        }
        self.as_membership()?;
        Ok(())
    }

    fn validate_join_intent(
        &self,
        intent: &JoinIntent,
        now_unix_ms: u64,
    ) -> Result<(), TableSessionProtocolError> {
        intent.verify_at(now_unix_ms)?;
        if intent.table_id() != self.table_id {
            return Err(TableSessionProtocolError::WrongTable);
        }
        if intent.level() != &self.level {
            return Err(TableSessionProtocolError::WrongStakeLevel);
        }
        Ok(())
    }

    fn canonical_unsigned_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(768);
        bytes.extend_from_slice(MEMBERSHIP_PROPOSAL_DOMAIN);
        bytes.push(self.version);
        write_table_id(&mut bytes, self.table_id);
        write_level(&mut bytes, &self.level);
        bytes.extend_from_slice(&self.membership_version.to_be_bytes());
        write_optional_hash(&mut bytes, self.previous_membership_hash);
        bytes.push(u8::try_from(self.seats.len()).expect("成员最多六人"));
        for seat in &self.seats {
            bytes.push(seat.physical_seat().value());
            bytes.extend_from_slice(seat.join_intent().claim_id().as_bytes());
        }
        bytes.push(u8::try_from(self.waiting.len()).expect("候补最多六人"));
        for intent in &self.waiting {
            bytes.extend_from_slice(intent.claim_id().as_bytes());
        }
        bytes.push(u8::try_from(self.leave_intents.len()).expect("离桌意图最多十二个"));
        for intent in &self.leave_intents {
            bytes.extend_from_slice(intent.id().as_bytes());
        }
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedMembershipProposal {
    version: u8,
    proposal: MembershipProposal,
    coordinator_claim_id: JoinClaimId,
    attestation: DeviceAttestation,
}

impl SignedMembershipProposal {
    pub fn issue(
        proposal: MembershipProposal,
        now_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, TableSessionProtocolError> {
        proposal.verify_at(now_unix_ms)?;
        let coordinator_claim_id = proposal
            .coordinator_claim_id()
            .ok_or(TableSessionProtocolError::MissingCoordinator)?;
        let coordinator = proposal
            .seat_by_claim_id(coordinator_claim_id)
            .ok_or(TableSessionProtocolError::MissingCoordinator)?;
        validate_join_signer(coordinator.join_intent(), device, &certificate)
            .map_err(|_| TableSessionProtocolError::UnauthorizedCoordinator)?;
        let version = 1;
        let unsigned = canonical_signed_membership_bytes(
            version,
            proposal.proposal_hash(),
            coordinator_claim_id,
        );
        let attestation = DeviceAttestation::issue(
            MEMBERSHIP_PROPOSAL_DOMAIN,
            &unsigned,
            now_unix_ms,
            device,
            certificate,
        )?;
        Ok(Self {
            version,
            proposal,
            coordinator_claim_id,
            attestation,
        })
    }

    pub fn verify_at(&self, now_unix_ms: u64) -> Result<(), TableSessionProtocolError> {
        if self.version != 1 {
            return Err(
                TableSessionProtocolError::UnsupportedSignedMembershipProposalVersion(self.version),
            );
        }
        self.proposal.verify_at(now_unix_ms)?;
        if self.proposal.coordinator_claim_id() != Some(self.coordinator_claim_id) {
            return Err(TableSessionProtocolError::UnauthorizedCoordinator);
        }
        let coordinator = self
            .proposal
            .seat_by_claim_id(self.coordinator_claim_id)
            .ok_or(TableSessionProtocolError::MissingCoordinator)?;
        let certificate = self.attestation.certificate();
        if certificate.player_id() != coordinator.join_intent().player_id()
            || certificate.device_public_key() != coordinator.join_intent().device_public_key()
        {
            return Err(TableSessionProtocolError::UnauthorizedCoordinator);
        }
        self.attestation.verify(
            MEMBERSHIP_PROPOSAL_DOMAIN,
            &canonical_signed_membership_bytes(
                self.version,
                self.proposal.proposal_hash(),
                self.coordinator_claim_id,
            ),
            now_unix_ms,
        )?;
        Ok(())
    }

    pub const fn proposal(&self) -> &MembershipProposal {
        &self.proposal
    }

    pub const fn coordinator_claim_id(&self) -> JoinClaimId {
        self.coordinator_claim_id
    }
}

fn canonical_signed_membership_bytes(
    version: u8,
    proposal_hash: &[u8; 32],
    coordinator_claim_id: JoinClaimId,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(96);
    bytes.extend_from_slice(MEMBERSHIP_PROPOSAL_DOMAIN);
    bytes.push(version);
    bytes.extend_from_slice(proposal_hash);
    bytes.extend_from_slice(coordinator_claim_id.as_bytes());
    bytes
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipAcceptance {
    version: u8,
    proposal_hash: [u8; 32],
    claim_id: JoinClaimId,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    accepted_at_unix_ms: u64,
    attestation: DeviceAttestation,
}

impl MembershipAcceptance {
    pub fn issue(
        proposal: &SignedMembershipProposal,
        claim_id: JoinClaimId,
        accepted_at_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, TableSessionProtocolError> {
        proposal.verify_at(accepted_at_unix_ms)?;
        let seat = proposal
            .proposal()
            .seat_by_claim_id(claim_id)
            .ok_or(TableSessionProtocolError::ClaimNotInMembership(claim_id))?;
        validate_join_signer(seat.join_intent(), device, &certificate)
            .map_err(|_| TableSessionProtocolError::AcceptanceIdentityMismatch)?;
        let version = 1;
        let proposal_hash = *proposal.proposal().proposal_hash();
        let player_id = seat.join_intent().player_id();
        let device_public_key = seat.join_intent().device_public_key();
        let unsigned = canonical_membership_acceptance_bytes(
            version,
            &proposal_hash,
            claim_id,
            player_id,
            device_public_key,
            accepted_at_unix_ms,
        );
        let attestation = DeviceAttestation::issue(
            MEMBERSHIP_ACCEPTANCE_DOMAIN,
            &unsigned,
            accepted_at_unix_ms,
            device,
            certificate,
        )?;
        Ok(Self {
            version,
            proposal_hash,
            claim_id,
            player_id,
            device_public_key,
            accepted_at_unix_ms,
            attestation,
        })
    }

    pub fn verify_at(
        &self,
        proposal: &SignedMembershipProposal,
        now_unix_ms: u64,
    ) -> Result<(), TableSessionProtocolError> {
        if self.version != 1 {
            return Err(
                TableSessionProtocolError::UnsupportedMembershipAcceptanceVersion(self.version),
            );
        }
        proposal.verify_at(now_unix_ms)?;
        if &self.proposal_hash != proposal.proposal().proposal_hash() {
            return Err(TableSessionProtocolError::AcceptanceProposalMismatch);
        }
        let seat = proposal.proposal().seat_by_claim_id(self.claim_id).ok_or(
            TableSessionProtocolError::ClaimNotInMembership(self.claim_id),
        )?;
        if self.player_id != seat.join_intent().player_id()
            || self.device_public_key != seat.join_intent().device_public_key()
            || self.attestation.certificate().player_id() != self.player_id
            || self.attestation.certificate().device_public_key() != self.device_public_key
        {
            return Err(TableSessionProtocolError::AcceptanceIdentityMismatch);
        }
        if !is_signed_time_not_future(self.accepted_at_unix_ms, now_unix_ms)
            || !is_signed_time_before_expiry(
                self.accepted_at_unix_ms,
                seat.join_intent().expires_at_unix_ms(),
            )
        {
            return Err(TableSessionProtocolError::AcceptanceOutsideValidityWindow);
        }
        self.attestation.verify(
            MEMBERSHIP_ACCEPTANCE_DOMAIN,
            &canonical_membership_acceptance_bytes(
                self.version,
                &self.proposal_hash,
                self.claim_id,
                self.player_id,
                self.device_public_key,
                self.accepted_at_unix_ms,
            ),
            now_unix_ms,
        )?;
        Ok(())
    }

    pub const fn claim_id(&self) -> JoinClaimId {
        self.claim_id
    }

    pub const fn proposal_hash(&self) -> &[u8; 32] {
        &self.proposal_hash
    }
}

pub fn verify_membership_acceptances<'a>(
    proposal: &SignedMembershipProposal,
    acceptances: impl IntoIterator<Item = &'a MembershipAcceptance>,
    now_unix_ms: u64,
) -> Result<(), TableSessionProtocolError> {
    proposal.verify_at(now_unix_ms)?;
    let expected = proposal
        .proposal()
        .seats()
        .iter()
        .map(|seat| seat.join_intent().claim_id())
        .collect::<BTreeSet<_>>();
    let mut observed = BTreeSet::new();
    for acceptance in acceptances {
        acceptance.verify_at(proposal, now_unix_ms)?;
        if !observed.insert(acceptance.claim_id()) {
            return Err(TableSessionProtocolError::DuplicateAcceptance);
        }
    }
    if observed != expected {
        return Err(TableSessionProtocolError::IncompleteAcceptances);
    }
    Ok(())
}

fn canonical_membership_acceptance_bytes(
    version: u8,
    proposal_hash: &[u8; 32],
    claim_id: JoinClaimId,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    accepted_at_unix_ms: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(176);
    bytes.extend_from_slice(MEMBERSHIP_ACCEPTANCE_DOMAIN);
    bytes.push(version);
    bytes.extend_from_slice(proposal_hash);
    bytes.extend_from_slice(claim_id.as_bytes());
    bytes.extend_from_slice(player_id.as_bytes());
    bytes.extend_from_slice(device_public_key.as_bytes());
    bytes.extend_from_slice(&accepted_at_unix_ms.to_be_bytes());
    bytes
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum TableSessionMessage {
    JoinIntent(Box<JoinIntent>),
    LeaveIntent(Box<LeaveIntent>),
    MembershipProposal(Box<SignedMembershipProposal>),
    MembershipAcceptance(Box<MembershipAcceptance>),
}

fn validate_ticket_signer(
    ticket: &PoolTicket,
    device: &DeviceIdentity,
    certificate: &DeviceCertificate,
) -> Result<(), TableSessionProtocolError> {
    if ticket.player_id() != certificate.player_id()
        || ticket.device_public_key() != certificate.device_public_key()
        || ticket.device_public_key() != device.public_key()
    {
        return Err(TableSessionProtocolError::JoinIntentIdentityMismatch);
    }
    Ok(())
}

fn validate_join_signer(
    intent: &JoinIntent,
    device: &DeviceIdentity,
    certificate: &DeviceCertificate,
) -> Result<(), TableSessionProtocolError> {
    if intent.player_id() != certificate.player_id()
        || intent.device_public_key() != certificate.device_public_key()
        || intent.device_public_key() != device.public_key()
    {
        return Err(TableSessionProtocolError::JoinIntentIdentityMismatch);
    }
    Ok(())
}

fn validate_join_window(
    ticket: &PoolTicket,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<(), TableSessionProtocolError> {
    validate_window(created_at_unix_ms, expires_at_unix_ms)?;
    if !is_signed_time_not_future(ticket.created_at_unix_ms(), created_at_unix_ms)
        || !is_signed_time_before_expiry(expires_at_unix_ms, ticket.expires_at_unix_ms())
    {
        return Err(TableSessionProtocolError::JoinIntentOutsideTicketWindow);
    }
    Ok(())
}

fn validate_leave_fields(
    after_hand_number: Option<u64>,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<(), TableSessionProtocolError> {
    if after_hand_number == Some(0) {
        return Err(TableSessionProtocolError::InvalidHandNumber);
    }
    validate_window(created_at_unix_ms, expires_at_unix_ms)
}

fn validate_window(
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<(), TableSessionProtocolError> {
    let Some(lifetime) = expires_at_unix_ms.checked_sub(created_at_unix_ms) else {
        return Err(TableSessionProtocolError::InvalidIntentValidityWindow);
    };
    if lifetime == 0 || lifetime > MAX_INTENT_LIFETIME_MS {
        return Err(TableSessionProtocolError::InvalidIntentValidityWindow);
    }
    Ok(())
}

fn validate_active_window(
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    now_unix_ms: u64,
    error: TableSessionProtocolError,
) -> Result<(), TableSessionProtocolError> {
    if !is_signed_time_window_active(created_at_unix_ms, expires_at_unix_ms, now_unix_ms) {
        return Err(error);
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TableSessionProtocolError {
    #[error("不支持的入桌意图版本 {0}")]
    UnsupportedJoinIntentVersion(u8),
    #[error("不支持的离桌意图版本 {0}")]
    UnsupportedLeaveIntentVersion(u8),
    #[error("不支持的成员提案版本 {0}")]
    UnsupportedMembershipProposalVersion(u8),
    #[error("不支持的签名成员提案版本 {0}")]
    UnsupportedSignedMembershipProposalVersion(u8),
    #[error("不支持的成员确认版本 {0}")]
    UnsupportedMembershipAcceptanceVersion(u8),
    #[error("意图有效期必须为 1 毫秒到 120 秒")]
    InvalidIntentValidityWindow,
    #[error("入桌意图有效期必须完全位于公开池票据有效期内")]
    JoinIntentOutsideTicketWindow,
    #[error("入桌意图尚未生效或已经过期")]
    JoinIntentExpired,
    #[error("离桌意图尚未生效或已经过期")]
    LeaveIntentExpired,
    #[error("入桌意图与票据设备身份不一致")]
    JoinIntentIdentityMismatch,
    #[error("离桌意图与设备证书身份不一致")]
    LeaveIntentIdentityMismatch,
    #[error("离桌意图的手牌编号必须从 1 开始")]
    InvalidHandNumber,
    #[error("成员版本必须从 1 开始")]
    InvalidMembershipVersion,
    #[error("成员提案必须包含 1 到 6 个成员，实际为 {0}")]
    InvalidMembershipSize(usize),
    #[error("成员提案候补队列已满")]
    WaitingListFull,
    #[error("成员提案中的离桌意图超过房间成员与候补总上限")]
    TooManyLeaveIntents,
    #[error("签名对象属于另一张牌桌")]
    WrongTable,
    #[error("入桌意图的牌局级别与房间不一致")]
    WrongStakeLevel,
    #[error("成员提案中的对象未按规范顺序排列")]
    NonCanonicalOrdering,
    #[error("同一玩家出现重复离桌意图")]
    DuplicateLeaveIntent,
    #[error("成员提案摘要不匹配")]
    MembershipProposalHashMismatch,
    #[error("成员提案缺少协调者")]
    MissingCoordinator,
    #[error("成员提案不是由规范顺序中的协调者签发")]
    UnauthorizedCoordinator,
    #[error("入桌声明 {0:?} 不在成员提案中")]
    ClaimNotInMembership(JoinClaimId),
    #[error("成员确认不属于当前成员提案")]
    AcceptanceProposalMismatch,
    #[error("成员确认与入桌声明身份不一致")]
    AcceptanceIdentityMismatch,
    #[error("成员确认时间不在入桌声明有效期内")]
    AcceptanceOutsideValidityWindow,
    #[error("同一入桌声明出现重复成员确认")]
    DuplicateAcceptance,
    #[error("尚未收齐所有入座成员的确认")]
    IncompleteAcceptances,
    #[error(transparent)]
    Pool(#[from] TablePoolProtocolError),
    #[error(transparent)]
    Attestation(#[from] DeviceAttestationError),
    #[error(transparent)]
    Membership(#[from] MembershipError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PoolTicket;
    use libp2p::{Multiaddr, PeerId};
    use rand_core::OsRng;
    use token_holdem_domain::Chips;
    use token_holdem_identity::RootIdentity;

    struct Fixture {
        join: JoinIntent,
        device: DeviceIdentity,
        certificate: DeviceCertificate,
    }

    fn level() -> StakeLevel {
        StakeLevel::new(
            "10k-20k",
            Chips::new(10_000),
            Chips::new(20_000),
            Chips::new(800_000),
            Chips::new(2_000_000),
            2,
            6,
        )
        .expect("测试级别应当有效")
    }

    fn fixture(seed: u8, table_id: TableId) -> Fixture {
        let root = RootIdentity::generate(&mut OsRng);
        let device = DeviceIdentity::generate(&mut OsRng);
        let certificate = root
            .issue_device_certificate(device.public_key(), format!("设备 {seed}"), 1_000, 20_000)
            .expect("测试证书应当签发成功");
        let peer_id = PeerId::random();
        let address = format!(
            "/ip4/127.0.0.1/tcp/{}/p2p/{peer_id}",
            32_000 + u16::from(seed)
        )
        .parse::<Multiaddr>()
        .expect("测试地址应当有效")
        .to_vec();
        let ticket = PoolTicket::issue(
            peer_id.to_bytes(),
            vec![address],
            level(),
            Chips::new(900_000 + u64::from(seed) * 100_000),
            2_000,
            12_000,
            [seed; 16],
            &device,
            certificate.clone(),
        )
        .expect("测试票据应当签发成功");
        let join = JoinIntent::issue(
            table_id,
            ticket,
            2_500,
            11_000,
            [seed.wrapping_add(10); 16],
            &device,
            certificate.clone(),
        )
        .expect("测试入桌意图应当签发成功");
        Fixture {
            join,
            device,
            certificate,
        }
    }

    fn signed_proposal(fixtures: &[Fixture]) -> SignedMembershipProposal {
        let table_id = fixtures[0].join.table_id();
        let seats = fixtures
            .iter()
            .enumerate()
            .map(|(index, fixture)| {
                MembershipSeatClaim::new(
                    PhysicalSeat::new(u8::try_from(index + 1).expect("测试席位应可转换"))
                        .expect("测试席位应当有效"),
                    fixture.join.clone(),
                )
            })
            .collect::<Vec<_>>();
        let proposal = MembershipProposal::assemble(
            table_id,
            level(),
            1,
            None,
            seats,
            Vec::new(),
            Vec::new(),
            3_000,
        )
        .expect("成员提案应当有效");
        let coordinator_claim = proposal.coordinator_claim_id().expect("提案应有协调者");
        let coordinator = fixtures
            .iter()
            .find(|fixture| fixture.join.claim_id() == coordinator_claim)
            .expect("应找到协调者设备");
        SignedMembershipProposal::issue(
            proposal,
            3_100,
            &coordinator.device,
            coordinator.certificate.clone(),
        )
        .expect("成员提案应当签发成功")
    }

    #[test]
    fn 入桌意图绑定稳定桌号和原始票据() {
        let table_id = TableId::new([7; 32]);
        let fixture = fixture(1, table_id);
        assert_eq!(fixture.join.table_id(), table_id);
        assert!(fixture.join.verify_at(0).is_ok());
        assert!(fixture.join.verify_at(3_000).is_ok());
        assert_ne!(fixture.join.claim_id().as_bytes(), &[0; 32]);
    }

    #[test]
    fn 入桌意图签发允许票据来自时钟略快的设备() {
        let table_id = TableId::new([7; 32]);
        let fixture = fixture(1, table_id);
        let join = JoinIntent::issue(
            table_id,
            fixture.join.ticket().clone(),
            0,
            1_000,
            [42; 16],
            &fixture.device,
            fixture.certificate,
        )
        .expect("有限时钟偏差不应阻断入桌意图签发");

        assert!(join.verify_at(0).is_ok());
    }

    #[test]
    fn 只有最小物理席位的成员能签发规范提案() {
        let fixtures = [
            fixture(1, TableId::new([7; 32])),
            fixture(2, TableId::new([7; 32])),
        ];
        let signed = signed_proposal(&fixtures);
        assert!(signed.verify_at(3_200).is_ok());
        assert_eq!(signed.coordinator_claim_id(), fixtures[0].join.claim_id());
    }

    #[test]
    fn 成员提案必须收齐每个入座成员的设备确认() {
        let fixtures = [
            fixture(1, TableId::new([7; 32])),
            fixture(2, TableId::new([7; 32])),
        ];
        let proposal = signed_proposal(&fixtures);
        let acceptances = fixtures
            .iter()
            .map(|fixture| {
                MembershipAcceptance::issue(
                    &proposal,
                    fixture.join.claim_id(),
                    3_300,
                    &fixture.device,
                    fixture.certificate.clone(),
                )
                .expect("成员确认应当签发成功")
            })
            .collect::<Vec<_>>();
        assert!(verify_membership_acceptances(&proposal, &acceptances, 0).is_ok());
        assert!(verify_membership_acceptances(&proposal, &acceptances, 3_400).is_ok());
        assert_eq!(
            verify_membership_acceptances(&proposal, &acceptances[..1], 3_400),
            Err(TableSessionProtocolError::IncompleteAcceptances)
        );
    }

    #[test]
    fn 离桌意图绑定当前手与成员版本() {
        let fixture = fixture(1, TableId::new([7; 32]));
        let leave = LeaveIntent::issue(
            fixture.join.table_id(),
            Some(8),
            3,
            3_000,
            9_000,
            [90; 16],
            &fixture.device,
            fixture.certificate,
        )
        .expect("离桌意图应当签发成功");
        assert_eq!(leave.after_hand_number(), Some(8));
        assert_eq!(leave.membership_version(), 3);
        assert!(leave.verify_at(4_000).is_ok());
    }
}
