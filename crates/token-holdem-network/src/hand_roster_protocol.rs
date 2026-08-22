use crate::{
    protocol_encoding::{write_addresses, write_bytes},
    JoinIntent, MembershipSeatClaim, SignedMembershipProposal, TableSessionProtocolError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;
use token_holdem_domain::{
    DevicePublicKey, HandRosterError, JoinClaimId, PlayerId, ReadyHandRoster,
};
use token_holdem_identity::{
    DeviceAttestation, DeviceAttestationError, DeviceCertificate, DeviceIdentity,
};

const HAND_ROSTER_PROPOSAL_DOMAIN: &[u8] = b"token-holdem/hand-roster-proposal/v1\0";
const HAND_ROSTER_ACCEPTANCE_DOMAIN: &[u8] = b"token-holdem/hand-roster-acceptance/v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEndpoint {
    hand_index: u8,
    claim_id: JoinClaimId,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    session_peer_id: Vec<u8>,
    session_addresses: Vec<Vec<u8>>,
}

impl RosterEndpoint {
    fn from_seat(
        hand_index: u8,
        seat: &MembershipSeatClaim,
    ) -> Result<Self, HandRosterProtocolError> {
        let intent = seat.join_intent();
        Ok(Self {
            hand_index,
            claim_id: intent.claim_id(),
            player_id: intent.player_id(),
            device_public_key: intent.device_public_key(),
            session_peer_id: intent.ticket().session_peer_id().to_vec(),
            session_addresses: intent.ticket().session_addresses().to_vec(),
        })
    }

    pub const fn hand_index(&self) -> u8 {
        self.hand_index
    }

    pub const fn claim_id(&self) -> JoinClaimId {
        self.claim_id
    }

    pub const fn player_id(&self) -> PlayerId {
        self.player_id
    }

    pub const fn device_public_key(&self) -> DevicePublicKey {
        self.device_public_key
    }

    pub fn session_peer_id(&self) -> &[u8] {
        &self.session_peer_id
    }

    pub fn session_addresses(&self) -> &[Vec<u8>] {
        &self.session_addresses
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandRosterProposal {
    version: u8,
    ready_roster: ReadyHandRoster,
    membership_proposal_hash: [u8; 32],
    endpoints: Vec<RosterEndpoint>,
    proposal_hash: [u8; 32],
}

impl HandRosterProposal {
    pub fn assemble(
        ready_roster: ReadyHandRoster,
        membership: &SignedMembershipProposal,
        now_unix_ms: u64,
    ) -> Result<Self, HandRosterProtocolError> {
        membership.verify_at(now_unix_ms)?;
        ready_roster.verify()?;
        validate_roster_membership(&ready_roster, membership)?;
        let endpoints = ready_roster
            .seats()
            .iter()
            .map(|roster_seat| {
                let membership_seat = membership
                    .proposal()
                    .seat_by_player(roster_seat.player_id())
                    .ok_or(HandRosterProtocolError::RosterPlayerMissingFromMembership)?;
                RosterEndpoint::from_seat(roster_seat.hand_index(), membership_seat)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut proposal = Self {
            version: 1,
            ready_roster,
            membership_proposal_hash: *membership.proposal().proposal_hash(),
            endpoints,
            proposal_hash: [0; 32],
        };
        proposal.validate_contents(membership)?;
        proposal.proposal_hash = *blake3::hash(&proposal.canonical_unsigned_bytes()).as_bytes();
        Ok(proposal)
    }

    pub fn verify(
        &self,
        membership: &SignedMembershipProposal,
    ) -> Result<(), HandRosterProtocolError> {
        if self.version != 1 {
            return Err(HandRosterProtocolError::UnsupportedRosterProposalVersion(
                self.version,
            ));
        }
        self.ready_roster.verify()?;
        self.validate_contents(membership)?;
        let expected = *blake3::hash(&self.canonical_unsigned_bytes()).as_bytes();
        if expected != self.proposal_hash {
            return Err(HandRosterProtocolError::RosterProposalHashMismatch);
        }
        Ok(())
    }

    pub const fn ready_roster(&self) -> &ReadyHandRoster {
        &self.ready_roster
    }

    pub const fn membership_proposal_hash(&self) -> &[u8; 32] {
        &self.membership_proposal_hash
    }

    pub fn endpoints(&self) -> &[RosterEndpoint] {
        &self.endpoints
    }

    pub const fn proposal_hash(&self) -> &[u8; 32] {
        &self.proposal_hash
    }

    pub fn endpoint_for_player(&self, player_id: PlayerId) -> Option<&RosterEndpoint> {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.player_id() == player_id)
    }

    fn validate_contents(
        &self,
        membership: &SignedMembershipProposal,
    ) -> Result<(), HandRosterProtocolError> {
        if self.membership_proposal_hash != *membership.proposal().proposal_hash() {
            return Err(HandRosterProtocolError::WrongMembershipProposal);
        }
        validate_roster_membership(&self.ready_roster, membership)?;
        if self.endpoints.len() != self.ready_roster.seats().len() {
            return Err(HandRosterProtocolError::EndpointCountMismatch);
        }
        for (roster_seat, endpoint) in self.ready_roster.seats().iter().zip(self.endpoints.iter()) {
            if endpoint.hand_index() != roster_seat.hand_index()
                || endpoint.player_id() != roster_seat.player_id()
                || endpoint.device_public_key() != roster_seat.device_public_key()
            {
                return Err(HandRosterProtocolError::EndpointIdentityMismatch);
            }
            let membership_seat = membership
                .proposal()
                .seat_by_player(endpoint.player_id())
                .ok_or(HandRosterProtocolError::RosterPlayerMissingFromMembership)?;
            let intent = membership_seat.join_intent();
            if endpoint.claim_id() != intent.claim_id()
                || endpoint.session_peer_id() != intent.ticket().session_peer_id()
                || endpoint.session_addresses() != intent.ticket().session_addresses()
            {
                return Err(HandRosterProtocolError::EndpointIdentityMismatch);
            }
        }
        Ok(())
    }

    fn canonical_unsigned_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(768);
        bytes.extend_from_slice(HAND_ROSTER_PROPOSAL_DOMAIN);
        bytes.push(self.version);
        bytes.extend_from_slice(self.ready_roster.roster_hash());
        bytes.extend_from_slice(&self.membership_proposal_hash);
        bytes.push(u8::try_from(self.endpoints.len()).expect("本手最多六人"));
        for endpoint in &self.endpoints {
            bytes.push(endpoint.hand_index());
            bytes.extend_from_slice(endpoint.claim_id().as_bytes());
            bytes.extend_from_slice(endpoint.player_id().as_bytes());
            bytes.extend_from_slice(endpoint.device_public_key().as_bytes());
            write_bytes(&mut bytes, endpoint.session_peer_id());
            write_addresses(&mut bytes, endpoint.session_addresses());
        }
        bytes
    }
}

fn validate_roster_membership(
    roster: &ReadyHandRoster,
    membership: &SignedMembershipProposal,
) -> Result<(), HandRosterProtocolError> {
    let proposal = membership.proposal();
    if roster.table_id() != proposal.table_id()
        || roster.membership_version() != proposal.membership_version()
    {
        return Err(HandRosterProtocolError::WrongMembershipProposal);
    }
    if roster.seats().len() != proposal.seats().len() {
        return Err(HandRosterProtocolError::RosterDoesNotFreezeAllMembers);
    }
    for roster_seat in roster.seats() {
        let membership_seat = proposal
            .seat_by_player(roster_seat.player_id())
            .ok_or(HandRosterProtocolError::RosterPlayerMissingFromMembership)?;
        let member = membership_seat.as_member();
        if roster_seat.physical_seat() != member.physical_seat()
            || roster_seat.device_public_key() != member.device_public_key()
            || roster_seat.buy_in() != member.buy_in()
        {
            return Err(HandRosterProtocolError::RosterMemberMismatch);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedHandRosterProposal {
    version: u8,
    proposal: HandRosterProposal,
    membership: SignedMembershipProposal,
    coordinator_claim_id: JoinClaimId,
    attestation: DeviceAttestation,
}

impl SignedHandRosterProposal {
    pub fn issue(
        proposal: HandRosterProposal,
        membership: SignedMembershipProposal,
        now_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, HandRosterProtocolError> {
        membership.verify_at(now_unix_ms)?;
        proposal.verify(&membership)?;
        let coordinator_claim_id = membership
            .proposal()
            .coordinator_claim_id()
            .ok_or(HandRosterProtocolError::MissingCoordinator)?;
        let coordinator = membership
            .proposal()
            .seat_by_claim_id(coordinator_claim_id)
            .ok_or(HandRosterProtocolError::MissingCoordinator)?;
        validate_intent_signer(coordinator.join_intent(), device, &certificate)?;
        let version = 1;
        let unsigned =
            canonical_signed_roster_bytes(version, proposal.proposal_hash(), coordinator_claim_id);
        let attestation = DeviceAttestation::issue(
            HAND_ROSTER_PROPOSAL_DOMAIN,
            &unsigned,
            now_unix_ms,
            device,
            certificate,
        )?;
        Ok(Self {
            version,
            proposal,
            membership,
            coordinator_claim_id,
            attestation,
        })
    }

    pub fn verify_at(&self, now_unix_ms: u64) -> Result<(), HandRosterProtocolError> {
        if self.version != 1 {
            return Err(
                HandRosterProtocolError::UnsupportedSignedRosterProposalVersion(self.version),
            );
        }
        self.membership.verify_at(now_unix_ms)?;
        self.proposal.verify(&self.membership)?;
        if self.membership.proposal().coordinator_claim_id() != Some(self.coordinator_claim_id) {
            return Err(HandRosterProtocolError::UnauthorizedCoordinator);
        }
        let coordinator = self
            .membership
            .proposal()
            .seat_by_claim_id(self.coordinator_claim_id)
            .ok_or(HandRosterProtocolError::MissingCoordinator)?;
        let certificate = self.attestation.certificate();
        if certificate.player_id() != coordinator.join_intent().player_id()
            || certificate.device_public_key() != coordinator.join_intent().device_public_key()
        {
            return Err(HandRosterProtocolError::UnauthorizedCoordinator);
        }
        self.attestation.verify(
            HAND_ROSTER_PROPOSAL_DOMAIN,
            &canonical_signed_roster_bytes(
                self.version,
                self.proposal.proposal_hash(),
                self.coordinator_claim_id,
            ),
            now_unix_ms,
        )?;
        Ok(())
    }

    pub const fn proposal(&self) -> &HandRosterProposal {
        &self.proposal
    }

    pub const fn membership(&self) -> &SignedMembershipProposal {
        &self.membership
    }

    pub const fn coordinator_claim_id(&self) -> JoinClaimId {
        self.coordinator_claim_id
    }
}

fn canonical_signed_roster_bytes(
    version: u8,
    proposal_hash: &[u8; 32],
    coordinator_claim_id: JoinClaimId,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(96);
    bytes.extend_from_slice(HAND_ROSTER_PROPOSAL_DOMAIN);
    bytes.push(version);
    bytes.extend_from_slice(proposal_hash);
    bytes.extend_from_slice(coordinator_claim_id.as_bytes());
    bytes
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandRosterAcceptance {
    version: u8,
    proposal_hash: [u8; 32],
    claim_id: JoinClaimId,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    accepted_at_unix_ms: u64,
    attestation: DeviceAttestation,
}

impl HandRosterAcceptance {
    pub fn issue(
        proposal: &SignedHandRosterProposal,
        claim_id: JoinClaimId,
        accepted_at_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, HandRosterProtocolError> {
        proposal.verify_at(accepted_at_unix_ms)?;
        let endpoint = proposal
            .proposal()
            .endpoints()
            .iter()
            .find(|endpoint| endpoint.claim_id() == claim_id)
            .ok_or(HandRosterProtocolError::ClaimNotInRoster(claim_id))?;
        let intent = intent_for_endpoint(proposal, endpoint)?;
        validate_intent_signer(intent, device, &certificate)?;
        let version = 1;
        let proposal_hash = *proposal.proposal().proposal_hash();
        let player_id = endpoint.player_id();
        let device_public_key = endpoint.device_public_key();
        let unsigned = canonical_roster_acceptance_bytes(
            version,
            &proposal_hash,
            claim_id,
            player_id,
            device_public_key,
            accepted_at_unix_ms,
        );
        let attestation = DeviceAttestation::issue(
            HAND_ROSTER_ACCEPTANCE_DOMAIN,
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
        proposal: &SignedHandRosterProposal,
        now_unix_ms: u64,
    ) -> Result<(), HandRosterProtocolError> {
        if self.version != 1 {
            return Err(HandRosterProtocolError::UnsupportedRosterAcceptanceVersion(
                self.version,
            ));
        }
        proposal.verify_at(now_unix_ms)?;
        if &self.proposal_hash != proposal.proposal().proposal_hash() {
            return Err(HandRosterProtocolError::AcceptanceProposalMismatch);
        }
        let endpoint = proposal
            .proposal()
            .endpoints()
            .iter()
            .find(|endpoint| endpoint.claim_id() == self.claim_id)
            .ok_or(HandRosterProtocolError::ClaimNotInRoster(self.claim_id))?;
        let intent = intent_for_endpoint(proposal, endpoint)?;
        if self.player_id != endpoint.player_id() {
            return Err(HandRosterProtocolError::AcceptancePlayerMismatch);
        }
        if self.device_public_key != endpoint.device_public_key() {
            return Err(HandRosterProtocolError::AcceptanceDeviceMismatch);
        }
        if self.attestation.certificate().player_id() != self.player_id
            || self.attestation.certificate().device_public_key() != self.device_public_key
        {
            return Err(HandRosterProtocolError::AcceptanceCertificateMismatch);
        }
        if self.accepted_at_unix_ms > now_unix_ms
            || self.accepted_at_unix_ms >= intent.expires_at_unix_ms()
        {
            return Err(HandRosterProtocolError::AcceptanceOutsideValidityWindow);
        }
        self.attestation.verify(
            HAND_ROSTER_ACCEPTANCE_DOMAIN,
            &canonical_roster_acceptance_bytes(
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

pub fn verify_hand_roster_acceptances<'a>(
    proposal: &SignedHandRosterProposal,
    acceptances: impl IntoIterator<Item = &'a HandRosterAcceptance>,
    now_unix_ms: u64,
) -> Result<(), HandRosterProtocolError> {
    proposal.verify_at(now_unix_ms)?;
    let expected = proposal
        .proposal()
        .endpoints()
        .iter()
        .map(RosterEndpoint::claim_id)
        .collect::<BTreeSet<_>>();
    let mut observed = BTreeSet::new();
    for acceptance in acceptances {
        acceptance.verify_at(proposal, now_unix_ms)?;
        if !observed.insert(acceptance.claim_id()) {
            return Err(HandRosterProtocolError::DuplicateAcceptance);
        }
    }
    if expected != observed {
        return Err(HandRosterProtocolError::IncompleteAcceptances);
    }
    Ok(())
}

fn canonical_roster_acceptance_bytes(
    version: u8,
    proposal_hash: &[u8; 32],
    claim_id: JoinClaimId,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    accepted_at_unix_ms: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(176);
    bytes.extend_from_slice(HAND_ROSTER_ACCEPTANCE_DOMAIN);
    bytes.push(version);
    bytes.extend_from_slice(proposal_hash);
    bytes.extend_from_slice(claim_id.as_bytes());
    bytes.extend_from_slice(player_id.as_bytes());
    bytes.extend_from_slice(device_public_key.as_bytes());
    bytes.extend_from_slice(&accepted_at_unix_ms.to_be_bytes());
    bytes
}

fn intent_for_endpoint<'a>(
    proposal: &'a SignedHandRosterProposal,
    endpoint: &RosterEndpoint,
) -> Result<&'a JoinIntent, HandRosterProtocolError> {
    proposal
        .membership()
        .proposal()
        .seat_by_claim_id(endpoint.claim_id())
        .map(MembershipSeatClaim::join_intent)
        .ok_or(HandRosterProtocolError::ClaimNotInRoster(
            endpoint.claim_id(),
        ))
}

fn validate_intent_signer(
    intent: &JoinIntent,
    device: &DeviceIdentity,
    certificate: &DeviceCertificate,
) -> Result<(), HandRosterProtocolError> {
    if intent.player_id() != certificate.player_id()
        || intent.device_public_key() != certificate.device_public_key()
        || intent.device_public_key() != device.public_key()
    {
        return Err(HandRosterProtocolError::AcceptanceSignerMismatch);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum HandRosterMessage {
    Proposal(Box<SignedHandRosterProposal>),
    Acceptance(Box<HandRosterAcceptance>),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HandRosterProtocolError {
    #[error("不支持的逐手名单提案版本 {0}")]
    UnsupportedRosterProposalVersion(u8),
    #[error("不支持的签名逐手名单提案版本 {0}")]
    UnsupportedSignedRosterProposalVersion(u8),
    #[error("不支持的逐手名单确认版本 {0}")]
    UnsupportedRosterAcceptanceVersion(u8),
    #[error("逐手名单不属于当前成员提案")]
    WrongMembershipProposal,
    #[error("逐手名单必须冻结全部当前入座成员")]
    RosterDoesNotFreezeAllMembers,
    #[error("逐手名单包含不在成员提案中的玩家")]
    RosterPlayerMissingFromMembership,
    #[error("逐手名单玩家属性与成员提案不一致")]
    RosterMemberMismatch,
    #[error("逐手名单端点数量与参与者数量不一致")]
    EndpointCountMismatch,
    #[error("逐手名单端点与入桌声明身份不一致")]
    EndpointIdentityMismatch,
    #[error("逐手名单提案摘要不匹配")]
    RosterProposalHashMismatch,
    #[error("逐手名单缺少协调者")]
    MissingCoordinator,
    #[error("逐手名单不是由规范顺序中的协调者签发")]
    UnauthorizedCoordinator,
    #[error("入桌声明 {0:?} 不在逐手名单中")]
    ClaimNotInRoster(JoinClaimId),
    #[error("逐手名单确认不属于当前提案")]
    AcceptanceProposalMismatch,
    #[error("逐手名单确认的玩家与入桌声明不一致")]
    AcceptancePlayerMismatch,
    #[error("逐手名单确认的设备与入桌声明不一致")]
    AcceptanceDeviceMismatch,
    #[error("逐手名单确认的证书与确认载荷不一致")]
    AcceptanceCertificateMismatch,
    #[error("本机签名设备不拥有逐手名单中的入桌声明")]
    AcceptanceSignerMismatch,
    #[error("逐手名单确认时间不在入桌声明有效期内")]
    AcceptanceOutsideValidityWindow,
    #[error("同一入桌声明出现重复逐手名单确认")]
    DuplicateAcceptance,
    #[error("尚未收齐所有本手参与者的名单确认")]
    IncompleteAcceptances,
    #[error(transparent)]
    Session(#[from] TableSessionProtocolError),
    #[error(transparent)]
    Domain(#[from] HandRosterError),
    #[error(transparent)]
    Attestation(#[from] DeviceAttestationError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        JoinIntent, MembershipProposal, MembershipSeatClaim, PoolTicket, SignedMembershipProposal,
    };
    use libp2p::{Multiaddr, PeerId};
    use rand_core::OsRng;
    use token_holdem_domain::{Chips, PhysicalSeat, StakeLevel, TableId, TableMembership};
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
            33_000 + u16::from(seed)
        )
        .parse::<Multiaddr>()
        .expect("测试地址应当有效")
        .to_vec();
        let ticket = PoolTicket::issue(
            peer_id.to_bytes(),
            vec![address],
            level(),
            Chips::new(900_000),
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
            [seed.wrapping_add(20); 16],
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

    fn membership(fixtures: &[Fixture]) -> SignedMembershipProposal {
        let seats = fixtures
            .iter()
            .enumerate()
            .map(|(index, fixture)| {
                MembershipSeatClaim::new(
                    PhysicalSeat::new(u8::try_from(index + 1).expect("测试席位可转换"))
                        .expect("测试席位应当有效"),
                    fixture.join.clone(),
                )
            })
            .collect::<Vec<_>>();
        let proposal = MembershipProposal::assemble(
            fixtures[0].join.table_id(),
            level(),
            1,
            None,
            seats,
            Vec::new(),
            Vec::new(),
            3_000,
        )
        .expect("成员提案应当有效");
        let coordinator_id = proposal.coordinator_claim_id().expect("成员提案应有协调者");
        let coordinator = fixtures
            .iter()
            .find(|fixture| fixture.join.claim_id() == coordinator_id)
            .expect("应找到协调者设备");
        SignedMembershipProposal::issue(
            proposal,
            3_100,
            &coordinator.device,
            coordinator.certificate.clone(),
        )
        .expect("成员提案应当签发成功")
    }

    fn signed_roster(fixtures: &[Fixture]) -> SignedHandRosterProposal {
        let membership = membership(fixtures);
        let domain_membership: TableMembership = membership
            .proposal()
            .as_membership()
            .expect("领域成员应当有效");
        let ready = ReadyHandRoster::from_membership(&domain_membership, 1, None, None)
            .expect("逐手名单应当有效");
        let proposal =
            HandRosterProposal::assemble(ready, &membership, 3_200).expect("逐手名单提案应当有效");
        let coordinator_id = membership
            .proposal()
            .coordinator_claim_id()
            .expect("逐手名单应有协调者");
        let coordinator = fixtures
            .iter()
            .find(|fixture| fixture.join.claim_id() == coordinator_id)
            .expect("应找到逐手协调者");
        SignedHandRosterProposal::issue(
            proposal,
            membership,
            3_300,
            &coordinator.device,
            coordinator.certificate.clone(),
        )
        .expect("逐手名单应当签发成功")
    }

    #[test]
    fn 逐手名单绑定成员摘要和每位玩家会话端点() {
        let fixtures = [
            fixture(1, TableId::new([8; 32])),
            fixture(2, TableId::new([8; 32])),
        ];
        let roster = signed_roster(&fixtures);
        assert!(roster.verify_at(3_400).is_ok());
        assert_eq!(roster.proposal().endpoints().len(), 2);
        assert_ne!(roster.proposal().proposal_hash(), &[0; 32]);
    }

    #[test]
    fn 逐手协调者必须继承最小物理席位而不是随机声明顺序() {
        let table_id = TableId::new([9; 32]);
        let first = fixture(1, table_id);
        let second = (2..=u8::MAX)
            .map(|seed| fixture(seed, table_id))
            .find(|candidate| candidate.join.claim_id() < first.join.claim_id())
            .expect("应能构造声明编号小于首席玩家的测试成员");
        let fixtures = [first, second];
        let membership = membership(&fixtures);
        let domain_membership = membership
            .proposal()
            .as_membership()
            .expect("领域成员应当有效");
        let ready = ReadyHandRoster::from_membership(&domain_membership, 1, None, None)
            .expect("逐手名单应当有效");
        let proposal =
            HandRosterProposal::assemble(ready, &membership, 3_200).expect("逐手名单提案应当有效");

        let signed = SignedHandRosterProposal::issue(
            proposal,
            membership.clone(),
            3_300,
            &fixtures[0].device,
            fixtures[0].certificate.clone(),
        )
        .expect("最小物理席位玩家应当始终能签发逐手名单");

        assert_eq!(
            signed.coordinator_claim_id(),
            membership
                .proposal()
                .coordinator_claim_id()
                .expect("成员提案应有协调者")
        );
        assert_ne!(
            signed.coordinator_claim_id(),
            fixtures[1].join.claim_id(),
            "更小的随机声明编号不得夺走物理首席的协调权"
        );
    }

    #[test]
    fn 必须收齐本手所有参与者的名单确认() {
        let fixtures = [
            fixture(1, TableId::new([8; 32])),
            fixture(2, TableId::new([8; 32])),
        ];
        let roster = signed_roster(&fixtures);
        let acceptances = fixtures
            .iter()
            .map(|fixture| {
                HandRosterAcceptance::issue(
                    &roster,
                    fixture.join.claim_id(),
                    3_500,
                    &fixture.device,
                    fixture.certificate.clone(),
                )
                .expect("名单确认应当签发成功")
            })
            .collect::<Vec<_>>();
        assert!(verify_hand_roster_acceptances(&roster, &acceptances, 3_600).is_ok());
        assert_eq!(
            verify_hand_roster_acceptances(&roster, &acceptances[..1], 3_600),
            Err(HandRosterProtocolError::IncompleteAcceptances)
        );
    }

    #[test]
    fn 篡改会话端点会破坏逐手名单摘要() {
        let fixtures = [
            fixture(1, TableId::new([8; 32])),
            fixture(2, TableId::new([8; 32])),
        ];
        let mut roster = signed_roster(&fixtures);
        roster.proposal.endpoints[0].session_peer_id.push(0);
        assert!(matches!(
            roster.verify_at(3_400),
            Err(HandRosterProtocolError::EndpointIdentityMismatch)
        ));
    }
}
