use crate::{
    protocol_encoding::{write_addresses, write_bytes, write_level, write_table_id},
    session_endpoint::{validate_session_endpoint, SessionEndpointError},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use token_holdem_domain::{DevicePublicKey, PlayerId, StakeLevel, TableId, TABLE_CAPACITY};
use token_holdem_identity::{
    is_signed_time_window_active, DeviceAttestation, DeviceAttestationError, DeviceCertificate,
    DeviceIdentity,
};

const FRIEND_ROOM_DOMAIN: &[u8] = b"token-holdem/friend-room-invite/v3\0";
const ROOM_ID_DOMAIN: &[u8] = b"token-holdem/friend-room-id/v3\0";
const FRIEND_ROOM_VERSION: u8 = 3;

pub type RoomId = TableId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendRoomInvite {
    version: u8,
    room_id: RoomId,
    room_secret: [u8; 32],
    host_player_id: PlayerId,
    host_device_public_key: DevicePublicKey,
    host_session_peer_id: Vec<u8>,
    host_session_addresses: Vec<Vec<u8>>,
    level: StakeLevel,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    attestation: DeviceAttestation,
}

impl FriendRoomInvite {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        room_secret: [u8; 32],
        host_session_peer_id: Vec<u8>,
        host_session_addresses: Vec<Vec<u8>>,
        level: StakeLevel,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, FriendRoomInviteError> {
        validate_fields(
            &room_secret,
            &host_session_peer_id,
            &host_session_addresses,
            &level,
            created_at_unix_ms,
            expires_at_unix_ms,
        )?;
        let version = FRIEND_ROOM_VERSION;
        let room_id = derive_room_id(&room_secret);
        let host_player_id = certificate.player_id();
        let host_device_public_key = certificate.device_public_key();
        let unsigned = canonical_invite_bytes(
            version,
            room_id,
            &room_secret,
            host_player_id,
            host_device_public_key,
            &host_session_peer_id,
            &host_session_addresses,
            &level,
            created_at_unix_ms,
            expires_at_unix_ms,
        );
        let attestation = DeviceAttestation::issue(
            FRIEND_ROOM_DOMAIN,
            &unsigned,
            created_at_unix_ms,
            device,
            certificate,
        )?;
        Ok(Self {
            version,
            room_id,
            room_secret,
            host_player_id,
            host_device_public_key,
            host_session_peer_id,
            host_session_addresses,
            level,
            created_at_unix_ms,
            expires_at_unix_ms,
            attestation,
        })
    }

    pub fn verify_at(&self, now_unix_ms: u64) -> Result<(), FriendRoomInviteError> {
        if self.version != FRIEND_ROOM_VERSION {
            return Err(FriendRoomInviteError::UnsupportedVersion(self.version));
        }
        validate_fields(
            &self.room_secret,
            &self.host_session_peer_id,
            &self.host_session_addresses,
            &self.level,
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
        )?;
        if !is_signed_time_window_active(
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            now_unix_ms,
        ) {
            return Err(FriendRoomInviteError::Expired);
        }
        if derive_room_id(&self.room_secret) != self.room_id {
            return Err(FriendRoomInviteError::RoomIdMismatch);
        }
        let certificate = self.attestation.certificate();
        if certificate.player_id() != self.host_player_id
            || certificate.device_public_key() != self.host_device_public_key
        {
            return Err(FriendRoomInviteError::IdentityMismatch);
        }
        self.attestation.verify(
            FRIEND_ROOM_DOMAIN,
            &self.canonical_unsigned_bytes(),
            now_unix_ms,
        )?;
        Ok(())
    }

    pub const fn room_id(&self) -> RoomId {
        self.room_id
    }

    pub const fn room_secret(&self) -> &[u8; 32] {
        &self.room_secret
    }

    pub const fn host_player_id(&self) -> PlayerId {
        self.host_player_id
    }

    pub fn level(&self) -> &StakeLevel {
        &self.level
    }

    pub fn host_session_peer_id(&self) -> &[u8] {
        &self.host_session_peer_id
    }

    pub fn host_session_addresses(&self) -> &[Vec<u8>] {
        &self.host_session_addresses
    }

    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }

    fn canonical_unsigned_bytes(&self) -> Vec<u8> {
        canonical_invite_bytes(
            self.version,
            self.room_id,
            &self.room_secret,
            self.host_player_id,
            self.host_device_public_key,
            &self.host_session_peer_id,
            &self.host_session_addresses,
            &self.level,
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn canonical_invite_bytes(
    version: u8,
    room_id: RoomId,
    room_secret: &[u8; 32],
    host_player_id: PlayerId,
    host_device_public_key: DevicePublicKey,
    host_session_peer_id: &[u8],
    host_session_addresses: &[Vec<u8>],
    level: &StakeLevel,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(384);
    bytes.extend_from_slice(FRIEND_ROOM_DOMAIN);
    bytes.push(version);
    write_table_id(&mut bytes, room_id);
    bytes.extend_from_slice(room_secret);
    bytes.extend_from_slice(host_player_id.as_bytes());
    bytes.extend_from_slice(host_device_public_key.as_bytes());
    write_bytes(&mut bytes, host_session_peer_id);
    write_addresses(&mut bytes, host_session_addresses);
    write_level(&mut bytes, level);
    bytes.extend_from_slice(&created_at_unix_ms.to_be_bytes());
    bytes.extend_from_slice(&expires_at_unix_ms.to_be_bytes());
    bytes
}

fn derive_room_id(secret: &[u8; 32]) -> RoomId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(ROOM_ID_DOMAIN);
    hasher.update(secret);
    TableId::new(*hasher.finalize().as_bytes())
}

fn validate_fields(
    room_secret: &[u8; 32],
    peer_id: &[u8],
    addresses: &[Vec<u8>],
    level: &StakeLevel,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<(), FriendRoomInviteError> {
    if room_secret.iter().all(|byte| *byte == 0) {
        return Err(FriendRoomInviteError::InvalidSecret);
    }
    match validate_session_endpoint(peer_id, addresses) {
        Ok(_) => {}
        Err(SessionEndpointError::AddressPeerIdMismatch) => {
            return Err(FriendRoomInviteError::AddressPeerIdMismatch);
        }
        Err(SessionEndpointError::InvalidPeerId) => {
            return Err(FriendRoomInviteError::InvalidPeerId);
        }
        Err(SessionEndpointError::InvalidAddresses) => {
            return Err(FriendRoomInviteError::InvalidAddresses);
        }
    }
    if level.minimum_players() != 2 || level.maximum_players() != TABLE_CAPACITY {
        return Err(FriendRoomInviteError::InvalidPlayerRange);
    }
    let Some(lifetime) = expires_at_unix_ms.checked_sub(created_at_unix_ms) else {
        return Err(FriendRoomInviteError::InvalidValidityWindow);
    };
    if lifetime == 0 || lifetime > 86_400_000 {
        return Err(FriendRoomInviteError::InvalidValidityWindow);
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FriendRoomInviteError {
    #[error(transparent)]
    Attestation(#[from] DeviceAttestationError),
    #[error("不支持的好友房邀请版本 {0}")]
    UnsupportedVersion(u8),
    #[error("好友房密钥不能全为零")]
    InvalidSecret,
    #[error("房主会话 PeerId 无效")]
    InvalidPeerId,
    #[error("好友房邀请必须包含 1 到 8 个合法且不重复的房主地址")]
    InvalidAddresses,
    #[error("好友房地址中的 PeerId 与房主会话 PeerId 不一致")]
    AddressPeerIdMismatch,
    #[error("好友房牌桌级别必须允许 2 到 6 人动态入座")]
    InvalidPlayerRange,
    #[error("好友房邀请有效期必须为 1 毫秒到 24 小时")]
    InvalidValidityWindow,
    #[error("好友房邀请尚未生效或已经过期")]
    Expired,
    #[error("好友房编号与房间密钥不匹配")]
    RoomIdMismatch,
    #[error("好友房房主身份与设备证书不一致")]
    IdentityMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::{Multiaddr, PeerId};
    use rand_core::OsRng;
    use token_holdem_domain::Chips;
    use token_holdem_identity::RootIdentity;

    #[test]
    fn 好友房邀请只携带动态桌访问能力而不固定人数和买入() {
        let root = RootIdentity::generate(&mut OsRng);
        let device = DeviceIdentity::generate(&mut OsRng);
        let certificate = root
            .issue_device_certificate(device.public_key(), "主机", 1_000, 100_000)
            .expect("证书应签发成功");
        let level = StakeLevel::new(
            "1k-2k",
            Chips::new(1_000),
            Chips::new(2_000),
            Chips::new(80_000),
            Chips::new(200_000),
            2,
            6,
        )
        .expect("级别应有效");
        let peer_id = PeerId::random();
        let address = format!("/ip4/127.0.0.1/tcp/30123/p2p/{peer_id}")
            .parse::<Multiaddr>()
            .expect("测试地址应有效")
            .to_vec();
        let invite = FriendRoomInvite::issue(
            [9; 32],
            peer_id.to_bytes(),
            vec![address],
            level,
            2_000,
            90_000,
            &device,
            certificate,
        )
        .expect("邀请应签发成功");

        assert!(invite.verify_at(3_000).is_ok());
        let encoded = cbor4ii::serde::to_vec(Vec::new(), &invite).expect("邀请应编码为 CBOR");
        let decoded: FriendRoomInvite =
            cbor4ii::serde::from_slice(&encoded).expect("另一台设备应能解码 CBOR 邀请");
        assert_eq!(decoded, invite);
        assert!(decoded.verify_at(3_000).is_ok());
        let mut old_rules = decoded;
        old_rules.version = 2;
        assert!(matches!(
            old_rules.verify_at(3_000),
            Err(FriendRoomInviteError::UnsupportedVersion(2))
        ));
    }
}
