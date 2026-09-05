use crate::{
    protocol_encoding::{write_addresses, write_bytes, write_level, write_table_id},
    session_endpoint::{validate_session_endpoint, SessionEndpointError},
};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::BTreeMap, fmt::Display};
use thiserror::Error;
use token_holdem_domain::{
    Chips, DevicePublicKey, PlayerId, StakeLevel, TableCandidate, TableId, TableLifecycle,
    TablePoolError, TABLE_CAPACITY, WAITING_CAPACITY,
};
use token_holdem_identity::{
    is_signed_time_window_active, DeviceAttestation, DeviceAttestationError, DeviceCertificate,
    DeviceIdentity,
};

const POOL_TICKET_DOMAIN: &[u8] = b"token-holdem/pool-ticket/v2\0";
const TABLE_ADVERTISEMENT_DOMAIN: &[u8] = b"token-holdem/table-advertisement/v2\0";
const POOL_DOCUMENT_VERSION: u8 = 2;
const MAX_TICKET_LIFETIME_MS: u64 = 60 * 60 * 1_000;
const MAX_ADVERTISEMENT_LIFETIME_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PoolTicketId([u8; 32]);

impl PoolTicketId {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Display for PoolTicketId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolTicket {
    version: u8,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    session_peer_id: Vec<u8>,
    session_addresses: Vec<Vec<u8>>,
    level: StakeLevel,
    requested_buy_in: Chips,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    nonce: [u8; 16],
    attestation: DeviceAttestation,
}

impl PoolTicket {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        session_peer_id: Vec<u8>,
        session_addresses: Vec<Vec<u8>>,
        level: StakeLevel,
        requested_buy_in: Chips,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        nonce: [u8; 16],
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, TablePoolProtocolError> {
        validate_ticket_fields(
            &session_peer_id,
            &session_addresses,
            &level,
            requested_buy_in,
            created_at_unix_ms,
            expires_at_unix_ms,
        )?;
        let version = POOL_DOCUMENT_VERSION;
        let player_id = certificate.player_id();
        let device_public_key = certificate.device_public_key();
        let unsigned = canonical_ticket_bytes(
            version,
            player_id,
            device_public_key,
            &session_peer_id,
            &session_addresses,
            &level,
            requested_buy_in,
            created_at_unix_ms,
            expires_at_unix_ms,
            &nonce,
        );
        let attestation = DeviceAttestation::issue(
            POOL_TICKET_DOMAIN,
            &unsigned,
            created_at_unix_ms,
            device,
            certificate,
        )?;
        Ok(Self {
            version,
            player_id,
            device_public_key,
            session_peer_id,
            session_addresses,
            level,
            requested_buy_in,
            created_at_unix_ms,
            expires_at_unix_ms,
            nonce,
            attestation,
        })
    }

    pub fn verify_at(&self, now_unix_ms: u64) -> Result<(), TablePoolProtocolError> {
        if self.version != POOL_DOCUMENT_VERSION {
            return Err(TablePoolProtocolError::UnsupportedTicketVersion(
                self.version,
            ));
        }
        validate_ticket_fields(
            &self.session_peer_id,
            &self.session_addresses,
            &self.level,
            self.requested_buy_in,
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
        )?;
        validate_active_window(
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            now_unix_ms,
            TablePoolProtocolError::TicketExpired,
        )?;
        let certificate = self.attestation.certificate();
        if certificate.player_id() != self.player_id
            || certificate.device_public_key() != self.device_public_key
        {
            return Err(TablePoolProtocolError::TicketIdentityMismatch);
        }
        self.attestation.verify(
            POOL_TICKET_DOMAIN,
            &self.canonical_unsigned_bytes(),
            now_unix_ms,
        )?;
        Ok(())
    }

    pub fn id(&self) -> PoolTicketId {
        PoolTicketId(*blake3::hash(&self.canonical_unsigned_bytes()).as_bytes())
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

    pub fn level(&self) -> &StakeLevel {
        &self.level
    }

    pub const fn requested_buy_in(&self) -> Chips {
        self.requested_buy_in
    }

    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }

    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }

    pub fn canonical_unsigned_bytes(&self) -> Vec<u8> {
        canonical_ticket_bytes(
            self.version,
            self.player_id,
            self.device_public_key,
            &self.session_peer_id,
            &self.session_addresses,
            &self.level,
            self.requested_buy_in,
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            &self.nonce,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn canonical_ticket_bytes(
    version: u8,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    session_peer_id: &[u8],
    session_addresses: &[Vec<u8>],
    level: &StakeLevel,
    requested_buy_in: Chips,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    nonce: &[u8; 16],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(384);
    bytes.extend_from_slice(POOL_TICKET_DOMAIN);
    bytes.push(version);
    bytes.extend_from_slice(player_id.as_bytes());
    bytes.extend_from_slice(device_public_key.as_bytes());
    write_bytes(&mut bytes, session_peer_id);
    write_addresses(&mut bytes, session_addresses);
    write_level(&mut bytes, level);
    bytes.extend_from_slice(&requested_buy_in.value().to_be_bytes());
    bytes.extend_from_slice(&created_at_unix_ms.to_be_bytes());
    bytes.extend_from_slice(&expires_at_unix_ms.to_be_bytes());
    bytes.extend_from_slice(nonce);
    bytes
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableAdvertisement {
    version: u8,
    table_id: TableId,
    level: StakeLevel,
    member_count: u8,
    waiting_count: u8,
    lifecycle: TableLifecycle,
    membership_version: u64,
    membership_hash: [u8; 32],
    creator_player_id: PlayerId,
    signer_player_id: PlayerId,
    signer_device_public_key: DevicePublicKey,
    admission_peer_id: Vec<u8>,
    admission_addresses: Vec<Vec<u8>>,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    nonce: [u8; 16],
    attestation: DeviceAttestation,
}

impl TableAdvertisement {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        table_id: TableId,
        level: StakeLevel,
        member_count: u8,
        waiting_count: u8,
        lifecycle: TableLifecycle,
        membership_version: u64,
        membership_hash: [u8; 32],
        creator_player_id: PlayerId,
        admission_peer_id: Vec<u8>,
        admission_addresses: Vec<Vec<u8>>,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        nonce: [u8; 16],
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, TablePoolProtocolError> {
        validate_advertisement_fields(
            &level,
            member_count,
            waiting_count,
            &admission_peer_id,
            &admission_addresses,
            created_at_unix_ms,
            expires_at_unix_ms,
        )?;
        let version = POOL_DOCUMENT_VERSION;
        let signer_player_id = certificate.player_id();
        let signer_device_public_key = certificate.device_public_key();
        let unsigned = canonical_advertisement_bytes(
            version,
            table_id,
            &level,
            member_count,
            waiting_count,
            lifecycle,
            membership_version,
            &membership_hash,
            creator_player_id,
            signer_player_id,
            signer_device_public_key,
            &admission_peer_id,
            &admission_addresses,
            created_at_unix_ms,
            expires_at_unix_ms,
            &nonce,
        );
        let attestation = DeviceAttestation::issue(
            TABLE_ADVERTISEMENT_DOMAIN,
            &unsigned,
            created_at_unix_ms,
            device,
            certificate,
        )?;
        Ok(Self {
            version,
            table_id,
            level,
            member_count,
            waiting_count,
            lifecycle,
            membership_version,
            membership_hash,
            creator_player_id,
            signer_player_id,
            signer_device_public_key,
            admission_peer_id,
            admission_addresses,
            created_at_unix_ms,
            expires_at_unix_ms,
            nonce,
            attestation,
        })
    }

    pub fn verify_at(&self, now_unix_ms: u64) -> Result<(), TablePoolProtocolError> {
        if self.version != POOL_DOCUMENT_VERSION {
            return Err(TablePoolProtocolError::UnsupportedAdvertisementVersion(
                self.version,
            ));
        }
        validate_advertisement_fields(
            &self.level,
            self.member_count,
            self.waiting_count,
            &self.admission_peer_id,
            &self.admission_addresses,
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
        )?;
        validate_active_window(
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            now_unix_ms,
            TablePoolProtocolError::AdvertisementExpired,
        )?;
        let certificate = self.attestation.certificate();
        if certificate.player_id() != self.signer_player_id
            || certificate.device_public_key() != self.signer_device_public_key
        {
            return Err(TablePoolProtocolError::AdvertisementIdentityMismatch);
        }
        self.attestation.verify(
            TABLE_ADVERTISEMENT_DOMAIN,
            &self.canonical_unsigned_bytes(),
            now_unix_ms,
        )?;
        Ok(())
    }

    pub const fn table_id(&self) -> TableId {
        self.table_id
    }

    pub fn level(&self) -> &StakeLevel {
        &self.level
    }

    pub const fn member_count(&self) -> u8 {
        self.member_count
    }

    pub const fn waiting_count(&self) -> u8 {
        self.waiting_count
    }

    pub const fn lifecycle(&self) -> TableLifecycle {
        self.lifecycle
    }

    pub const fn membership_version(&self) -> u64 {
        self.membership_version
    }

    pub const fn membership_hash(&self) -> &[u8; 32] {
        &self.membership_hash
    }

    pub const fn creator_player_id(&self) -> PlayerId {
        self.creator_player_id
    }

    pub const fn signer_player_id(&self) -> PlayerId {
        self.signer_player_id
    }

    pub fn admission_peer_id(&self) -> &[u8] {
        &self.admission_peer_id
    }

    pub fn admission_addresses(&self) -> &[Vec<u8>] {
        &self.admission_addresses
    }

    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }

    pub fn as_candidate(&self) -> Result<TableCandidate, TablePoolProtocolError> {
        Ok(TableCandidate::new(
            self.table_id,
            self.level.id(),
            self.member_count,
            self.waiting_count,
            TABLE_CAPACITY,
            self.lifecycle,
        )?)
    }

    pub fn canonical_unsigned_bytes(&self) -> Vec<u8> {
        canonical_advertisement_bytes(
            self.version,
            self.table_id,
            &self.level,
            self.member_count,
            self.waiting_count,
            self.lifecycle,
            self.membership_version,
            &self.membership_hash,
            self.creator_player_id,
            self.signer_player_id,
            self.signer_device_public_key,
            &self.admission_peer_id,
            &self.admission_addresses,
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            &self.nonce,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn canonical_advertisement_bytes(
    version: u8,
    table_id: TableId,
    level: &StakeLevel,
    member_count: u8,
    waiting_count: u8,
    lifecycle: TableLifecycle,
    membership_version: u64,
    membership_hash: &[u8; 32],
    creator_player_id: PlayerId,
    signer_player_id: PlayerId,
    signer_device_public_key: DevicePublicKey,
    admission_peer_id: &[u8],
    admission_addresses: &[Vec<u8>],
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    nonce: &[u8; 16],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(512);
    bytes.extend_from_slice(TABLE_ADVERTISEMENT_DOMAIN);
    bytes.push(version);
    write_table_id(&mut bytes, table_id);
    write_level(&mut bytes, level);
    bytes.push(member_count);
    bytes.push(waiting_count);
    bytes.push(match lifecycle {
        TableLifecycle::Waiting => 0,
        TableLifecycle::HandInProgress => 1,
        TableLifecycle::Closing => 2,
    });
    bytes.extend_from_slice(&membership_version.to_be_bytes());
    bytes.extend_from_slice(membership_hash);
    bytes.extend_from_slice(creator_player_id.as_bytes());
    bytes.extend_from_slice(signer_player_id.as_bytes());
    bytes.extend_from_slice(signer_device_public_key.as_bytes());
    write_bytes(&mut bytes, admission_peer_id);
    write_addresses(&mut bytes, admission_addresses);
    bytes.extend_from_slice(&created_at_unix_ms.to_be_bytes());
    bytes.extend_from_slice(&expires_at_unix_ms.to_be_bytes());
    bytes.extend_from_slice(nonce);
    bytes
}

pub fn select_table_advertisement<'a>(
    advertisements: &'a [TableAdvertisement],
    level: &StakeLevel,
    now_unix_ms: u64,
) -> Option<&'a TableAdvertisement> {
    advertisements
        .iter()
        .filter(|advertisement| {
            advertisement.verify_at(now_unix_ms).is_ok()
                && advertisement.level == *level
                && advertisement.lifecycle != TableLifecycle::Closing
                && advertisement.member_count < TABLE_CAPACITY
                && usize::from(advertisement.waiting_count) < WAITING_CAPACITY
        })
        .min_by(|left, right| compare_advertisements(left, right))
}

pub fn rank_pool_tickets<'a>(
    tickets: impl IntoIterator<Item = &'a PoolTicket>,
    level: &StakeLevel,
    now_unix_ms: u64,
) -> Vec<&'a PoolTicket> {
    let mut by_player = BTreeMap::<PlayerId, &PoolTicket>::new();
    for ticket in tickets {
        if ticket.verify_at(now_unix_ms).is_err() || ticket.level != *level {
            continue;
        }
        match by_player.entry(ticket.player_id()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(ticket);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if ticket.id() < entry.get().id() {
                    entry.insert(ticket);
                }
            }
        }
    }
    let mut ranked = by_player.into_values().collect::<Vec<_>>();
    ranked.sort_by_key(|ticket| ticket.id());
    ranked
}

fn compare_advertisements(left: &TableAdvertisement, right: &TableAdvertisement) -> Ordering {
    right
        .member_count
        .cmp(&left.member_count)
        .then_with(|| left.waiting_count.cmp(&right.waiting_count))
        .then_with(|| left.table_id.cmp(&right.table_id))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum TablePoolMessage {
    Ticket(PoolTicket),
    Advertisement(TableAdvertisement),
}

fn validate_ticket_fields(
    session_peer_id: &[u8],
    session_addresses: &[Vec<u8>],
    level: &StakeLevel,
    requested_buy_in: Chips,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<(), TablePoolProtocolError> {
    validate_endpoint(session_peer_id, session_addresses)?;
    validate_public_level(level)?;
    if requested_buy_in < level.minimum_buy_in() || requested_buy_in > level.maximum_buy_in() {
        return Err(TablePoolProtocolError::InvalidBuyIn);
    }
    validate_window(
        created_at_unix_ms,
        expires_at_unix_ms,
        MAX_TICKET_LIFETIME_MS,
        TablePoolProtocolError::InvalidTicketValidityWindow,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_advertisement_fields(
    level: &StakeLevel,
    member_count: u8,
    waiting_count: u8,
    admission_peer_id: &[u8],
    admission_addresses: &[Vec<u8>],
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<(), TablePoolProtocolError> {
    validate_public_level(level)?;
    if member_count > TABLE_CAPACITY {
        return Err(TablePoolProtocolError::InvalidMemberCount(member_count));
    }
    if usize::from(waiting_count) > WAITING_CAPACITY {
        return Err(TablePoolProtocolError::InvalidWaitingCount(waiting_count));
    }
    validate_endpoint(admission_peer_id, admission_addresses)?;
    validate_window(
        created_at_unix_ms,
        expires_at_unix_ms,
        MAX_ADVERTISEMENT_LIFETIME_MS,
        TablePoolProtocolError::InvalidAdvertisementValidityWindow,
    )
}

fn validate_public_level(level: &StakeLevel) -> Result<(), TablePoolProtocolError> {
    if level.minimum_players() != 2 || level.maximum_players() != TABLE_CAPACITY {
        return Err(TablePoolProtocolError::InvalidPublicLevelPlayerRange);
    }
    Ok(())
}

fn validate_window(
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    maximum_lifetime_ms: u64,
    error: TablePoolProtocolError,
) -> Result<(), TablePoolProtocolError> {
    let Some(lifetime) = expires_at_unix_ms.checked_sub(created_at_unix_ms) else {
        return Err(error);
    };
    if lifetime == 0 || lifetime > maximum_lifetime_ms {
        return Err(error);
    }
    Ok(())
}

fn validate_active_window(
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    now_unix_ms: u64,
    error: TablePoolProtocolError,
) -> Result<(), TablePoolProtocolError> {
    if !is_signed_time_window_active(created_at_unix_ms, expires_at_unix_ms, now_unix_ms) {
        return Err(error);
    }
    Ok(())
}

fn validate_endpoint(peer_id: &[u8], addresses: &[Vec<u8>]) -> Result<(), TablePoolProtocolError> {
    validate_session_endpoint(peer_id, addresses)
        .map(|_| ())
        .map_err(|error| match error {
            SessionEndpointError::InvalidPeerId => TablePoolProtocolError::InvalidPeerId,
            SessionEndpointError::InvalidAddresses => TablePoolProtocolError::InvalidAddresses,
            SessionEndpointError::AddressPeerIdMismatch => {
                TablePoolProtocolError::AddressPeerIdMismatch
            }
        })
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TablePoolProtocolError {
    #[error("不支持的公开池票据版本 {0}")]
    UnsupportedTicketVersion(u8),
    #[error("不支持的牌桌广告版本 {0}")]
    UnsupportedAdvertisementVersion(u8),
    #[error("会话 PeerId 无效")]
    InvalidPeerId,
    #[error("会话端点必须包含 1 到 8 个合法且不重复的地址")]
    InvalidAddresses,
    #[error("会话地址中的 PeerId 与声明的 PeerId 不一致")]
    AddressPeerIdMismatch,
    #[error("公开牌桌级别必须固定允许 2 到 6 人")]
    InvalidPublicLevelPlayerRange,
    #[error("买入额不在牌桌级别允许范围内")]
    InvalidBuyIn,
    #[error("公开池票据有效期必须为 1 毫秒到 120 秒")]
    InvalidTicketValidityWindow,
    #[error("牌桌广告有效期必须为 1 毫秒到 30 秒")]
    InvalidAdvertisementValidityWindow,
    #[error("公开池票据尚未生效或已经过期")]
    TicketExpired,
    #[error("牌桌广告尚未生效或已经过期")]
    AdvertisementExpired,
    #[error("公开池票据身份与设备证书不一致")]
    TicketIdentityMismatch,
    #[error("牌桌广告签发者与设备证书不一致")]
    AdvertisementIdentityMismatch,
    #[error("牌桌广告成员数无效：{0}")]
    InvalidMemberCount(u8),
    #[error("牌桌广告候补数无效：{0}")]
    InvalidWaitingCount(u8),
    #[error(transparent)]
    Attestation(#[from] DeviceAttestationError),
    #[error(transparent)]
    Domain(#[from] TablePoolError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::{Multiaddr, PeerId};
    use rand_core::OsRng;
    use token_holdem_identity::RootIdentity;

    struct Fixture {
        ticket: PoolTicket,
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

    fn endpoint(seed: u8) -> (Vec<u8>, Vec<Vec<u8>>) {
        let peer_id = PeerId::random();
        let address = format!(
            "/ip4/127.0.0.1/tcp/{}/p2p/{peer_id}",
            31_000 + u16::from(seed)
        )
        .parse::<Multiaddr>()
        .expect("测试地址应当有效")
        .to_vec();
        (peer_id.to_bytes(), vec![address])
    }

    fn fixture(seed: u8, buy_in: u64) -> Fixture {
        let root = RootIdentity::generate(&mut OsRng);
        let device = DeviceIdentity::generate(&mut OsRng);
        let certificate = root
            .issue_device_certificate(device.public_key(), format!("设备 {seed}"), 1_000, 20_000)
            .expect("测试证书应当签发成功");
        let (peer_id, addresses) = endpoint(seed);
        let ticket = PoolTicket::issue(
            peer_id,
            addresses,
            level(),
            Chips::new(buy_in),
            2_000,
            10_000,
            [seed; 16],
            &device,
            certificate.clone(),
        )
        .expect("测试票据应当签发成功");
        Fixture {
            ticket,
            device,
            certificate,
        }
    }

    fn advertisement(
        fixture: &Fixture,
        table_seed: u8,
        members: u8,
        waiting: u8,
    ) -> TableAdvertisement {
        TableAdvertisement::issue(
            TableId::new([table_seed; 32]),
            level(),
            members,
            waiting,
            TableLifecycle::HandInProgress,
            7,
            [4; 32],
            fixture.ticket.player_id(),
            fixture.ticket.session_peer_id().to_vec(),
            fixture.ticket.session_addresses().to_vec(),
            2_500,
            9_000,
            [table_seed; 16],
            &fixture.device,
            fixture.certificate.clone(),
        )
        .expect("测试广告应当签发成功")
    }

    #[test]
    fn 旧牌规的票据与广告不得进入新牌局() {
        let mut fixture = fixture(1, 1_000_000);
        let mut ad = advertisement(&fixture, 1, 2, 0);
        assert!(fixture.ticket.verify_at(3_000).is_ok());
        assert!(ad.verify_at(3_000).is_ok());
        fixture.ticket.version = 1;
        ad.version = 1;
        assert!(matches!(
            fixture.ticket.verify_at(3_000),
            Err(TablePoolProtocolError::UnsupportedTicketVersion(1))
        ));
        assert!(matches!(
            ad.verify_at(3_000),
            Err(TablePoolProtocolError::UnsupportedAdvertisementVersion(1))
        ));
    }

    #[test]
    fn 同一级别不同合法买入额进入同一公开池() {
        let first = fixture(1, 900_000);
        let second = fixture(2, 1_800_000);
        let ranked = rank_pool_tickets([&second.ticket, &first.ticket], &level(), 3_000);
        assert_eq!(ranked.len(), 2);
        assert_ne!(ranked[0].requested_buy_in(), ranked[1].requested_buy_in());
    }

    #[test]
    fn 广告排序优先填充人数更多且候补更少的桌() {
        let fixture = fixture(1, 1_000_000);
        let ads = vec![
            advertisement(&fixture, 3, 2, 0),
            advertisement(&fixture, 2, 4, 1),
            advertisement(&fixture, 1, 4, 0),
        ];
        let selected =
            select_table_advertisement(&ads, &level(), 3_000).expect("应当选中一个兼容广告");
        assert_eq!(selected.table_id(), TableId::new([1; 32]));
    }

    #[test]
    fn 篡改后的广告无法通过设备签名验证() {
        let fixture = fixture(1, 1_000_000);
        let mut ad = advertisement(&fixture, 1, 2, 0);
        ad.member_count = 5;
        assert!(matches!(
            ad.verify_at(3_000),
            Err(TablePoolProtocolError::Attestation(
                DeviceAttestationError::PayloadHashMismatch
            ))
        ));
    }

    #[test]
    fn 票据可跨进程编码且不含目标人数() {
        let fixture = fixture(1, 1_000_000);
        let encoded =
            cbor4ii::serde::to_vec(Vec::new(), &fixture.ticket).expect("票据应当编码成功");
        let decoded: PoolTicket = cbor4ii::serde::from_slice(&encoded).expect("票据应当解码成功");
        assert_eq!(decoded, fixture.ticket);
        assert!(decoded.verify_at(3_000).is_ok());
    }

    #[test]
    fn 公开池消息允许有限跨设备时钟偏差() {
        let fixture = fixture(1, 1_000_000);
        let advertisement = advertisement(&fixture, 1, 1, 0);

        assert!(fixture.ticket.verify_at(0).is_ok());
        assert!(advertisement.verify_at(0).is_ok());
        assert_eq!(
            fixture.ticket.verify_at(
                fixture
                    .ticket
                    .expires_at_unix_ms()
                    .saturating_add(token_holdem_identity::MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS)
            ),
            Err(TablePoolProtocolError::TicketExpired)
        );
    }
}
