use crate::{DevicePublicKey, PlayerId};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, fmt::Display};
use thiserror::Error;

const TABLE_ID_DOMAIN: &[u8] = b"token-holdem/table-id/v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TableId([u8; 32]);

impl TableId {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn derive(creator: PlayerId, device_public_key: DevicePublicKey, nonce: [u8; 32]) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(TABLE_ID_DOMAIN);
        hasher.update(creator.as_bytes());
        hasher.update(device_public_key.as_bytes());
        hasher.update(&nonce);
        Self(*hasher.finalize().as_bytes())
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Display for TableId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableLifecycle {
    Waiting,
    HandInProgress,
    Closing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCandidate {
    table_id: TableId,
    level_id: String,
    member_count: u8,
    waiting_count: u8,
    capacity: u8,
    lifecycle: TableLifecycle,
}

impl TableCandidate {
    pub fn new(
        table_id: TableId,
        level_id: impl Into<String>,
        member_count: u8,
        waiting_count: u8,
        capacity: u8,
        lifecycle: TableLifecycle,
    ) -> Result<Self, TablePoolError> {
        let level_id = level_id.into();
        if level_id.trim().is_empty() {
            return Err(TablePoolError::EmptyLevelId);
        }
        if !(2..=6).contains(&capacity) {
            return Err(TablePoolError::InvalidCapacity(capacity));
        }
        if member_count > capacity {
            return Err(TablePoolError::MemberCountExceedsCapacity);
        }
        if waiting_count > 6 {
            return Err(TablePoolError::WaitingListFull);
        }
        Ok(Self {
            table_id,
            level_id,
            member_count,
            waiting_count,
            capacity,
            lifecycle,
        })
    }

    pub const fn table_id(&self) -> TableId {
        self.table_id
    }

    pub fn level_id(&self) -> &str {
        &self.level_id
    }

    pub const fn member_count(&self) -> u8 {
        self.member_count
    }

    pub const fn waiting_count(&self) -> u8 {
        self.waiting_count
    }

    pub const fn capacity(&self) -> u8 {
        self.capacity
    }

    pub const fn lifecycle(&self) -> TableLifecycle {
        self.lifecycle
    }

    pub fn is_joinable_for(&self, level_id: &str) -> bool {
        self.level_id == level_id
            && self.lifecycle != TableLifecycle::Closing
            && self.member_count < self.capacity
            && self.waiting_count < 6
    }
}

pub fn select_compatible_table<'a>(
    candidates: &'a [TableCandidate],
    level_id: &str,
) -> Option<&'a TableCandidate> {
    candidates
        .iter()
        .filter(|candidate| candidate.is_joinable_for(level_id))
        .min_by(|left, right| compare_candidates(left, right))
}

fn compare_candidates(left: &TableCandidate, right: &TableCandidate) -> Ordering {
    right
        .member_count
        .cmp(&left.member_count)
        .then_with(|| left.waiting_count.cmp(&right.waiting_count))
        .then_with(|| left.table_id.cmp(&right.table_id))
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TablePoolError {
    #[error("牌桌级别编号不能为空")]
    EmptyLevelId,
    #[error("牌桌容量必须为 2 到 6，实际为 {0}")]
    InvalidCapacity(u8),
    #[error("牌桌成员数超过容量")]
    MemberCountExceedsCapacity,
    #[error("牌桌候补队列已满")]
    WaitingListFull,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: u8, members: u8, waiting: u8) -> TableCandidate {
        TableCandidate::new(
            TableId::new([id; 32]),
            "10k-20k",
            members,
            waiting,
            6,
            TableLifecycle::HandInProgress,
        )
        .expect("测试候选桌应当有效")
    }

    #[test]
    fn 兼容桌优先填充成员最多且候补最少的牌桌() {
        let candidates = vec![candidate(3, 2, 0), candidate(2, 4, 1), candidate(1, 4, 0)];
        let selected = select_compatible_table(&candidates, "10k-20k").expect("应当找到牌桌");
        assert_eq!(selected.table_id(), TableId::new([1; 32]));
    }

    #[test]
    fn 满桌和关闭桌不会被选择() {
        let full = TableCandidate::new(
            TableId::new([1; 32]),
            "10k-20k",
            6,
            0,
            6,
            TableLifecycle::HandInProgress,
        )
        .expect("满桌候选应可建模");
        let closing = TableCandidate::new(
            TableId::new([2; 32]),
            "10k-20k",
            2,
            0,
            6,
            TableLifecycle::Closing,
        )
        .expect("关闭桌候选应可建模");
        assert!(select_compatible_table(&[full, closing], "10k-20k").is_none());
    }
}
