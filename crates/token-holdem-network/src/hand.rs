use serde::{Deserialize, Serialize};
use thiserror::Error;
use token_holdem_domain::{DevicePublicKey, HandReceipt, PlayerAction, PlayerId};
use token_holdem_identity::{
    DeviceAttestation, DeviceAttestationError, DeviceCertificate, DeviceIdentity,
    ParticipantSignature,
};
use token_holdem_mental_poker::{KeyAnnouncement, RevealSharePacket, ShufflePacket};

pub const TABLE_TOPIC_PREFIX: &str = "/token-holdem/table-hand/2/";
const HAND_ACTION_DOMAIN: &[u8] = b"token-holdem/hand-action/v3\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandPublicMessage {
    KeyAnnouncement {
        table_id: [u8; 32],
        hand_number: u64,
        roster_hash: [u8; 32],
        seat: u8,
        announcement: KeyAnnouncement,
    },
    Shuffle {
        table_id: [u8; 32],
        hand_number: u64,
        roster_hash: [u8; 32],
        seat: u8,
        packet: ShufflePacket,
    },
    CommunityRevealShare {
        table_id: [u8; 32],
        hand_number: u64,
        roster_hash: [u8; 32],
        seat: u8,
        card_index: u8,
        packet: RevealSharePacket,
    },
    DealReady {
        table_id: [u8; 32],
        hand_number: u64,
        roster_hash: [u8; 32],
        seat: u8,
    },
    ReceiptSignature {
        table_id: [u8; 32],
        hand_number: u64,
        roster_hash: [u8; 32],
        receipt: HandReceipt,
        signature: ParticipantSignature,
    },
    ActionCommitted {
        table_id: [u8; 32],
        hand_number: u64,
        roster_hash: [u8; 32],
        sequence: u64,
        action: SignedHandAction,
    },
}

impl HandPublicMessage {
    pub const fn table_id(&self) -> &[u8; 32] {
        match self {
            Self::KeyAnnouncement { table_id, .. }
            | Self::Shuffle { table_id, .. }
            | Self::CommunityRevealShare { table_id, .. }
            | Self::DealReady { table_id, .. }
            | Self::ReceiptSignature { table_id, .. }
            | Self::ActionCommitted { table_id, .. } => table_id,
        }
    }

    pub const fn hand_number(&self) -> u64 {
        match self {
            Self::KeyAnnouncement { hand_number, .. }
            | Self::Shuffle { hand_number, .. }
            | Self::CommunityRevealShare { hand_number, .. }
            | Self::DealReady { hand_number, .. }
            | Self::ReceiptSignature { hand_number, .. }
            | Self::ActionCommitted { hand_number, .. } => *hand_number,
        }
    }

    pub const fn roster_hash(&self) -> &[u8; 32] {
        match self {
            Self::KeyAnnouncement { roster_hash, .. }
            | Self::Shuffle { roster_hash, .. }
            | Self::CommunityRevealShare { roster_hash, .. }
            | Self::DealReady { roster_hash, .. }
            | Self::ReceiptSignature { roster_hash, .. }
            | Self::ActionCommitted { roster_hash, .. } => roster_hash,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandPrivateMessage {
    HoleRevealShare {
        table_id: [u8; 32],
        hand_number: u64,
        roster_hash: [u8; 32],
        from_seat: u8,
        to_seat: u8,
        card_index: u8,
        packet: RevealSharePacket,
    },
}

impl HandPrivateMessage {
    pub const fn table_id(&self) -> &[u8; 32] {
        match self {
            Self::HoleRevealShare { table_id, .. } => table_id,
        }
    }

    pub const fn hand_number(&self) -> u64 {
        match self {
            Self::HoleRevealShare { hand_number, .. } => *hand_number,
        }
    }

    pub const fn roster_hash(&self) -> &[u8; 32] {
        match self {
            Self::HoleRevealShare { roster_hash, .. } => roster_hash,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedHandAction {
    version: u8,
    table_id: [u8; 32],
    hand_number: u64,
    roster_hash: [u8; 32],
    expected_sequence: u64,
    expected_public_state_hash: [u8; 32],
    seat: u8,
    player_id: PlayerId,
    action: PlayerAction,
    issued_at_unix_ms: u64,
    attestation: DeviceAttestation,
}

impl SignedHandAction {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        table_id: [u8; 32],
        hand_number: u64,
        roster_hash: [u8; 32],
        expected_sequence: u64,
        expected_public_state_hash: [u8; 32],
        seat: u8,
        action: PlayerAction,
        issued_at_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, SignedHandActionError> {
        if seat == 0 {
            return Err(SignedHandActionError::InvalidSeat);
        }
        let version = 3;
        let player_id = certificate.player_id();
        let unsigned = canonical_action_bytes(
            version,
            &table_id,
            hand_number,
            &roster_hash,
            expected_sequence,
            &expected_public_state_hash,
            seat,
            player_id,
            action,
            issued_at_unix_ms,
        );
        let attestation = DeviceAttestation::issue(
            HAND_ACTION_DOMAIN,
            &unsigned,
            issued_at_unix_ms,
            device,
            certificate,
        )?;
        Ok(Self {
            version,
            table_id,
            hand_number,
            roster_hash,
            expected_sequence,
            expected_public_state_hash,
            seat,
            player_id,
            action,
            issued_at_unix_ms,
            attestation,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn verify_for(
        &self,
        table_id: &[u8; 32],
        hand_number: u64,
        roster_hash: &[u8; 32],
        expected_sequence: u64,
        expected_public_state_hash: &[u8; 32],
        seat: u8,
        player_id: PlayerId,
        device_public_key: DevicePublicKey,
        now_unix_ms: u64,
    ) -> Result<(), SignedHandActionError> {
        if self.version != 3 {
            return Err(SignedHandActionError::UnsupportedVersion(self.version));
        }
        if self.table_id != *table_id || self.hand_number != hand_number {
            return Err(SignedHandActionError::WrongHand);
        }
        if &self.roster_hash != roster_hash {
            return Err(SignedHandActionError::WrongRoster);
        }
        if self.expected_sequence != expected_sequence {
            return Err(SignedHandActionError::WrongSequence {
                expected: expected_sequence,
                actual: self.expected_sequence,
            });
        }
        if &self.expected_public_state_hash != expected_public_state_hash {
            return Err(SignedHandActionError::WrongPublicStateHash);
        }
        if self.seat == 0 || self.seat != seat {
            return Err(SignedHandActionError::InvalidSeat);
        }
        let certificate = self.attestation.certificate();
        if self.player_id != player_id
            || certificate.player_id() != player_id
            || certificate.device_public_key() != device_public_key
        {
            return Err(SignedHandActionError::IdentityMismatch);
        }
        self.attestation.verify(
            HAND_ACTION_DOMAIN,
            &canonical_action_bytes(
                self.version,
                &self.table_id,
                self.hand_number,
                &self.roster_hash,
                self.expected_sequence,
                &self.expected_public_state_hash,
                self.seat,
                self.player_id,
                self.action,
                self.issued_at_unix_ms,
            ),
            now_unix_ms,
        )?;
        Ok(())
    }

    pub const fn table_id(&self) -> &[u8; 32] {
        &self.table_id
    }

    pub const fn hand_number(&self) -> u64 {
        self.hand_number
    }

    pub const fn expected_sequence(&self) -> u64 {
        self.expected_sequence
    }

    pub const fn roster_hash(&self) -> &[u8; 32] {
        &self.roster_hash
    }

    pub const fn expected_public_state_hash(&self) -> &[u8; 32] {
        &self.expected_public_state_hash
    }

    pub const fn seat(&self) -> u8 {
        self.seat
    }

    pub const fn player_id(&self) -> PlayerId {
        self.player_id
    }

    pub const fn action(&self) -> PlayerAction {
        self.action
    }

    pub const fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }

    pub const fn payload_hash(&self) -> &[u8; 32] {
        self.attestation.payload_hash()
    }

    pub fn canonical_unsigned_bytes(&self) -> Vec<u8> {
        canonical_action_bytes(
            self.version,
            &self.table_id,
            self.hand_number,
            &self.roster_hash,
            self.expected_sequence,
            &self.expected_public_state_hash,
            self.seat,
            self.player_id,
            self.action,
            self.issued_at_unix_ms,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn canonical_action_bytes(
    version: u8,
    table_id: &[u8; 32],
    hand_number: u64,
    roster_hash: &[u8; 32],
    expected_sequence: u64,
    expected_public_state_hash: &[u8; 32],
    seat: u8,
    player_id: PlayerId,
    action: PlayerAction,
    issued_at_unix_ms: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(160);
    bytes.extend_from_slice(HAND_ACTION_DOMAIN);
    bytes.push(version);
    bytes.extend_from_slice(table_id);
    bytes.extend_from_slice(&hand_number.to_be_bytes());
    bytes.extend_from_slice(roster_hash);
    bytes.extend_from_slice(&expected_sequence.to_be_bytes());
    bytes.extend_from_slice(expected_public_state_hash);
    bytes.push(seat);
    bytes.extend_from_slice(player_id.as_bytes());
    match action {
        PlayerAction::Fold => bytes.push(0),
        PlayerAction::Check => bytes.push(1),
        PlayerAction::Call => bytes.push(2),
        PlayerAction::RaiseTo(target) => {
            bytes.push(3);
            bytes.extend_from_slice(&target.value().to_be_bytes());
        }
    }
    bytes.extend_from_slice(&issued_at_unix_ms.to_be_bytes());
    bytes
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SignedHandActionError {
    #[error(transparent)]
    Attestation(#[from] DeviceAttestationError),
    #[error("不支持的手牌动作版本 {0}")]
    UnsupportedVersion(u8),
    #[error("手牌动作不属于当前牌桌或手牌")]
    WrongHand,
    #[error("手牌动作不属于当前冻结参与者名单")]
    WrongRoster,
    #[error("手牌动作序号不一致：预期 {expected}，实际 {actual}")]
    WrongSequence { expected: u64, actual: u64 },
    #[error("手牌动作绑定的公共前置状态摘要不一致")]
    WrongPublicStateHash,
    #[error("手牌动作座位无效")]
    InvalidSeat,
    #[error("手牌动作的玩家或设备身份不匹配")]
    IdentityMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use token_holdem_domain::Chips;
    use token_holdem_identity::RootIdentity;

    #[test]
    fn 动作签名绑定牌桌手数序号座位和筹码() {
        let root = RootIdentity::generate(&mut OsRng);
        let device = DeviceIdentity::generate(&mut OsRng);
        let certificate = root
            .issue_device_certificate(device.public_key(), "Windows", 1_000, 20_000)
            .expect("设备证书应签发");
        let action = SignedHandAction::issue(
            [7; 32],
            1,
            [6; 32],
            3,
            [8; 32],
            2,
            PlayerAction::RaiseTo(Chips::new(80_000)),
            2_000,
            &device,
            certificate,
        )
        .expect("动作应签发");

        assert!(action
            .verify_for(
                &[7; 32],
                1,
                &[6; 32],
                3,
                &[8; 32],
                2,
                root.player_id(),
                device.public_key(),
                3_000,
            )
            .is_ok());
        assert!(action
            .verify_for(
                &[7; 32],
                1,
                &[6; 32],
                4,
                &[8; 32],
                2,
                root.player_id(),
                device.public_key(),
                3_000,
            )
            .is_err());
        assert_eq!(
            action.verify_for(
                &[7; 32],
                1,
                &[6; 32],
                3,
                &[9; 32],
                2,
                root.player_id(),
                device.public_key(),
                3_000,
            ),
            Err(SignedHandActionError::WrongPublicStateHash)
        );
    }

    #[test]
    fn 规范动作摘要忽略证书外壳但区分动作内容() {
        let root = RootIdentity::generate(&mut OsRng);
        let device = DeviceIdentity::generate(&mut OsRng);
        let first_certificate = root
            .issue_device_certificate(device.public_key(), "设备 A", 1_000, 20_000)
            .unwrap();
        let second_certificate = root
            .issue_device_certificate(device.public_key(), "设备别名", 1_000, 20_000)
            .unwrap();
        let first = SignedHandAction::issue(
            [7; 32],
            1,
            [6; 32],
            1,
            [8; 32],
            1,
            PlayerAction::Call,
            2_000,
            &device,
            first_certificate,
        )
        .unwrap();
        let same_payload = SignedHandAction::issue(
            [7; 32],
            1,
            [6; 32],
            1,
            [8; 32],
            1,
            PlayerAction::Call,
            2_000,
            &device,
            second_certificate.clone(),
        )
        .unwrap();
        let conflicting = SignedHandAction::issue(
            [7; 32],
            1,
            [6; 32],
            1,
            [8; 32],
            1,
            PlayerAction::Fold,
            2_000,
            &device,
            second_certificate,
        )
        .unwrap();

        assert_ne!(first, same_payload);
        assert_eq!(first.payload_hash(), same_payload.payload_hash());
        assert_ne!(first.payload_hash(), conflicting.payload_hash());
    }
}
