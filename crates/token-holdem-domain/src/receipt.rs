use crate::{Chips, DevicePublicKey, PlayerId, SignedChips};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MatchId([u8; 16]);

impl MatchId {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HandId([u8; 32]);

impl HandId {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TranscriptHash([u8; 32]);

impl TranscriptHash {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandOutcome {
    pub player_id: PlayerId,
    pub device_public_key: DevicePublicKey,
    pub starting_stack: Chips,
    pub ending_stack: Chips,
}

impl HandOutcome {
    pub fn delta(&self) -> SignedChips {
        SignedChips::new(
            i128::from(self.ending_stack.value()) - i128::from(self.starting_stack.value()),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandReceipt {
    version: u8,
    match_id: MatchId,
    hand_number: u64,
    stake_level_id: String,
    transcript_hash: TranscriptHash,
    settled_at_unix_ms: u64,
    outcomes: Vec<HandOutcome>,
}

impl HandReceipt {
    pub fn settle(
        match_id: MatchId,
        hand_number: u64,
        stake_level_id: impl Into<String>,
        transcript_hash: TranscriptHash,
        settled_at_unix_ms: u64,
        mut outcomes: Vec<HandOutcome>,
    ) -> Result<Self, ReceiptError> {
        let stake_level_id = stake_level_id.into();
        if stake_level_id.trim().is_empty() {
            return Err(ReceiptError::EmptyStakeLevelId);
        }
        if outcomes.len() < 2 {
            return Err(ReceiptError::NotEnoughPlayers);
        }

        outcomes.sort_by_key(|outcome| outcome.player_id);
        let receipt = Self {
            version: 1,
            match_id,
            hand_number,
            stake_level_id,
            transcript_hash,
            settled_at_unix_ms,
            outcomes,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), ReceiptError> {
        if self.version != 1 {
            return Err(ReceiptError::UnsupportedVersion(self.version));
        }
        if self.stake_level_id.trim().is_empty() {
            return Err(ReceiptError::EmptyStakeLevelId);
        }
        if self.outcomes.len() < 2 {
            return Err(ReceiptError::NotEnoughPlayers);
        }

        for pair in self.outcomes.windows(2) {
            if pair[0].player_id == pair[1].player_id {
                return Err(ReceiptError::DuplicatePlayer(pair[0].player_id));
            }
            if pair[0].player_id > pair[1].player_id {
                return Err(ReceiptError::NonCanonicalPlayerOrder);
            }
        }
        let mut devices = BTreeSet::new();
        let mut total_start = 0_u128;
        let mut total_end = 0_u128;
        for outcome in &self.outcomes {
            if !devices.insert(outcome.device_public_key) {
                return Err(ReceiptError::DuplicateDevice(outcome.device_public_key));
            }
            total_start = total_start
                .checked_add(u128::from(outcome.starting_stack.value()))
                .ok_or(ReceiptError::Overflow)?;
            total_end = total_end
                .checked_add(u128::from(outcome.ending_stack.value()))
                .ok_or(ReceiptError::Overflow)?;
        }
        if total_start != total_end {
            return Err(ReceiptError::NotZeroSum {
                total_start,
                total_end,
            });
        }
        Ok(())
    }

    pub fn id(&self) -> HandId {
        HandId::new(*blake3::hash(&self.canonical_bytes()).as_bytes())
    }

    pub const fn match_id(&self) -> MatchId {
        self.match_id
    }

    pub const fn hand_number(&self) -> u64 {
        self.hand_number
    }

    pub fn stake_level_id(&self) -> &str {
        &self.stake_level_id
    }

    pub const fn transcript_hash(&self) -> TranscriptHash {
        self.transcript_hash
    }

    pub const fn settled_at_unix_ms(&self) -> u64 {
        self.settled_at_unix_ms
    }

    pub fn outcomes(&self) -> &[HandOutcome] {
        &self.outcomes
    }

    pub fn outcome_for(&self, player_id: PlayerId) -> Option<&HandOutcome> {
        self.outcomes
            .binary_search_by_key(&player_id, |outcome| outcome.player_id)
            .ok()
            .map(|index| &self.outcomes[index])
    }

    /// Signed bytes use explicit length-prefix encoding so JSON field ordering
    /// cannot invalidate signatures.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(256);
        bytes.extend_from_slice(b"token-holdem/hand-receipt/v1\0");
        bytes.push(self.version);
        bytes.extend_from_slice(self.match_id.as_bytes());
        bytes.extend_from_slice(&self.hand_number.to_be_bytes());
        append_bytes(&mut bytes, self.stake_level_id.as_bytes());
        bytes.extend_from_slice(self.transcript_hash.as_bytes());
        bytes.extend_from_slice(&self.settled_at_unix_ms.to_be_bytes());
        bytes.extend_from_slice(&(self.outcomes.len() as u32).to_be_bytes());
        for outcome in &self.outcomes {
            bytes.extend_from_slice(outcome.player_id.as_bytes());
            bytes.extend_from_slice(outcome.device_public_key.as_bytes());
            bytes.extend_from_slice(&outcome.starting_stack.value().to_be_bytes());
            bytes.extend_from_slice(&outcome.ending_stack.value().to_be_bytes());
        }
        bytes
    }
}

fn append_bytes(target: &mut Vec<u8>, value: &[u8]) {
    target.extend_from_slice(&(value.len() as u32).to_be_bytes());
    target.extend_from_slice(value);
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ReceiptError {
    #[error("不支持的结算凭证版本 {0}")]
    UnsupportedVersion(u8),
    #[error("牌桌级别编号不能为空")]
    EmptyStakeLevelId,
    #[error("正常结算至少需要两名玩家")]
    NotEnoughPlayers,
    #[error("结算凭证包含重复玩家 {0}")]
    DuplicatePlayer(PlayerId),
    #[error("结算凭证包含重复设备 {0}")]
    DuplicateDevice(DevicePublicKey),
    #[error("结算凭证玩家必须按玩家编号严格排序")]
    NonCanonicalPlayerOrder,
    #[error("结算不是零和：开始筹码 {total_start}，结束筹码 {total_end}")]
    NotZeroSum { total_start: u128, total_end: u128 },
    #[error("结算计算溢出")]
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 正常结算必须保持零和() {
        let result = HandReceipt::settle(
            MatchId::new([1; 16]),
            9,
            "10k-20k",
            TranscriptHash::new([2; 32]),
            100,
            vec![
                HandOutcome {
                    player_id: PlayerId::new([3; 32]),
                    device_public_key: DevicePublicKey::new([4; 32]),
                    starting_stack: Chips::new(1_000_000),
                    ending_stack: Chips::new(1_300_000),
                },
                HandOutcome {
                    player_id: PlayerId::new([5; 32]),
                    device_public_key: DevicePublicKey::new([6; 32]),
                    starting_stack: Chips::new(1_000_000),
                    ending_stack: Chips::new(700_000),
                },
            ],
        );

        assert!(result.is_ok());
    }

    #[test]
    fn 非零和结算会被拒绝() {
        let result = HandReceipt::settle(
            MatchId::new([1; 16]),
            9,
            "10k-20k",
            TranscriptHash::new([2; 32]),
            100,
            vec![
                HandOutcome {
                    player_id: PlayerId::new([3; 32]),
                    device_public_key: DevicePublicKey::new([4; 32]),
                    starting_stack: Chips::new(1_000_000),
                    ending_stack: Chips::new(1_300_000),
                },
                HandOutcome {
                    player_id: PlayerId::new([5; 32]),
                    device_public_key: DevicePublicKey::new([6; 32]),
                    starting_stack: Chips::new(1_000_000),
                    ending_stack: Chips::new(800_000),
                },
            ],
        );

        assert!(matches!(result, Err(ReceiptError::NotZeroSum { .. })));
    }

    #[test]
    fn 反序列化后的凭证仍必须重新验证不变量() {
        let mut valid = HandReceipt::settle(
            MatchId::new([1; 16]),
            1,
            "10k-20k",
            TranscriptHash::new([2; 32]),
            100,
            vec![
                HandOutcome {
                    player_id: PlayerId::new([3; 32]),
                    device_public_key: DevicePublicKey::new([4; 32]),
                    starting_stack: Chips::new(1_000),
                    ending_stack: Chips::new(1_100),
                },
                HandOutcome {
                    player_id: PlayerId::new([5; 32]),
                    device_public_key: DevicePublicKey::new([6; 32]),
                    starting_stack: Chips::new(1_000),
                    ending_stack: Chips::new(900),
                },
            ],
        )
        .expect("测试凭证应合法");
        valid.outcomes.reverse();

        assert_eq!(valid.validate(), Err(ReceiptError::NonCanonicalPlayerOrder));
    }
}
