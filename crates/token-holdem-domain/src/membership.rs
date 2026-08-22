use crate::{Chips, DevicePublicKey, PlayerId, TableId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const TABLE_CAPACITY: u8 = 6;
pub const WAITING_CAPACITY: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PhysicalSeat(u8);

impl PhysicalSeat {
    pub fn new(value: u8) -> Result<Self, MembershipError> {
        if !(1..=TABLE_CAPACITY).contains(&value) {
            return Err(MembershipError::InvalidPhysicalSeat(value));
        }
        Ok(Self(value))
    }

    pub const fn value(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JoinClaimId([u8; 32]);

impl JoinClaimId {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableMember {
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    buy_in: Chips,
    physical_seat: PhysicalSeat,
}

impl TableMember {
    pub const fn new(
        player_id: PlayerId,
        device_public_key: DevicePublicKey,
        buy_in: Chips,
        physical_seat: PhysicalSeat,
    ) -> Self {
        Self {
            player_id,
            device_public_key,
            buy_in,
            physical_seat,
        }
    }

    pub const fn player_id(&self) -> PlayerId {
        self.player_id
    }

    pub const fn device_public_key(&self) -> DevicePublicKey {
        self.device_public_key
    }

    pub const fn buy_in(&self) -> Chips {
        self.buy_in
    }

    pub const fn physical_seat(&self) -> PhysicalSeat {
        self.physical_seat
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinCandidate {
    claim_id: JoinClaimId,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    buy_in: Chips,
}

impl JoinCandidate {
    pub const fn new(
        claim_id: JoinClaimId,
        player_id: PlayerId,
        device_public_key: DevicePublicKey,
        buy_in: Chips,
    ) -> Self {
        Self {
            claim_id,
            player_id,
            device_public_key,
            buy_in,
        }
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

    pub const fn buy_in(&self) -> Chips {
        self.buy_in
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableMembership {
    table_id: TableId,
    version: u64,
    seats: BTreeMap<PhysicalSeat, TableMember>,
    waiting: Vec<JoinCandidate>,
}

impl TableMembership {
    pub fn new(
        table_id: TableId,
        version: u64,
        members: impl IntoIterator<Item = TableMember>,
        waiting: impl IntoIterator<Item = JoinCandidate>,
    ) -> Result<Self, MembershipError> {
        let mut seats = BTreeMap::new();
        for member in members {
            if seats.insert(member.physical_seat(), member).is_some() {
                return Err(MembershipError::DuplicatePhysicalSeat);
            }
        }
        let mut membership = Self {
            table_id,
            version,
            seats,
            waiting: waiting.into_iter().collect(),
        };
        membership.waiting.sort_by_key(JoinCandidate::claim_id);
        membership.validate()?;
        Ok(membership)
    }

    pub const fn table_id(&self) -> TableId {
        self.table_id
    }

    pub const fn version(&self) -> u64 {
        self.version
    }

    pub fn members(&self) -> impl Iterator<Item = &TableMember> {
        self.seats.values()
    }

    pub fn waiting(&self) -> &[JoinCandidate] {
        &self.waiting
    }

    pub fn queue_joins(
        &self,
        joins: impl IntoIterator<Item = JoinCandidate>,
    ) -> Result<Self, MembershipError> {
        let mut next = self.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or(MembershipError::VersionOverflow)?;
        next.waiting.extend(joins);
        next.waiting.sort_by_key(JoinCandidate::claim_id);
        next.deduplicate_waiting();
        next.validate()?;
        Ok(next)
    }

    pub fn reconcile_at_hand_boundary(
        &self,
        leaving_players: &BTreeSet<PlayerId>,
    ) -> Result<Self, MembershipError> {
        let mut next_seats = self
            .seats
            .iter()
            .filter(|(_, member)| !leaving_players.contains(&member.player_id()))
            .map(|(seat, member)| (*seat, member.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut occupied_players = next_seats
            .values()
            .map(TableMember::player_id)
            .collect::<BTreeSet<_>>();
        let mut occupied_devices = next_seats
            .values()
            .map(TableMember::device_public_key)
            .collect::<BTreeSet<_>>();
        let available_seats = (1..=TABLE_CAPACITY)
            .filter_map(|value| PhysicalSeat::new(value).ok())
            .filter(|seat| !next_seats.contains_key(seat))
            .collect::<Vec<_>>();
        let mut available_seats = available_seats.into_iter();
        let mut remaining = Vec::new();

        for candidate in &self.waiting {
            if leaving_players.contains(&candidate.player_id())
                || occupied_players.contains(&candidate.player_id())
                || occupied_devices.contains(&candidate.device_public_key())
            {
                continue;
            }
            if let Some(seat) = available_seats.next() {
                let member = TableMember::new(
                    candidate.player_id(),
                    candidate.device_public_key(),
                    candidate.buy_in(),
                    seat,
                );
                occupied_players.insert(member.player_id());
                occupied_devices.insert(member.device_public_key());
                next_seats.insert(seat, member);
            } else if remaining.len() < WAITING_CAPACITY {
                remaining.push(candidate.clone());
            }
        }

        Self::new(
            self.table_id,
            self.version
                .checked_add(1)
                .ok_or(MembershipError::VersionOverflow)?,
            next_seats.into_values(),
            remaining,
        )
    }

    fn deduplicate_waiting(&mut self) {
        let seated_players = self
            .seats
            .values()
            .map(TableMember::player_id)
            .collect::<BTreeSet<_>>();
        let seated_devices = self
            .seats
            .values()
            .map(TableMember::device_public_key)
            .collect::<BTreeSet<_>>();
        let mut waiting_players = BTreeSet::new();
        let mut waiting_devices = BTreeSet::new();
        self.waiting.retain(|candidate| {
            !seated_players.contains(&candidate.player_id())
                && !seated_devices.contains(&candidate.device_public_key())
                && waiting_players.insert(candidate.player_id())
                && waiting_devices.insert(candidate.device_public_key())
        });
    }

    fn validate(&mut self) -> Result<(), MembershipError> {
        if self.seats.len() > usize::from(TABLE_CAPACITY) {
            return Err(MembershipError::TooManyMembers);
        }
        if self.waiting.len() > WAITING_CAPACITY {
            return Err(MembershipError::WaitingListFull);
        }
        let mut players = BTreeSet::new();
        let mut devices = BTreeSet::new();
        for (seat, member) in &self.seats {
            if member.physical_seat() != *seat {
                return Err(MembershipError::SeatKeyMismatch);
            }
            if !players.insert(member.player_id()) {
                return Err(MembershipError::DuplicatePlayer);
            }
            if !devices.insert(member.device_public_key()) {
                return Err(MembershipError::DuplicateDevice);
            }
        }
        for candidate in &self.waiting {
            if !players.insert(candidate.player_id()) {
                return Err(MembershipError::DuplicatePlayer);
            }
            if !devices.insert(candidate.device_public_key()) {
                return Err(MembershipError::DuplicateDevice);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MembershipError {
    #[error("物理席位必须为 1 到 6，实际为 {0}")]
    InvalidPhysicalSeat(u8),
    #[error("牌桌成员超过 6 人")]
    TooManyMembers,
    #[error("候补连接超过 6 人")]
    WaitingListFull,
    #[error("同一玩家不能重复占用房间成员或候补位置")]
    DuplicatePlayer,
    #[error("同一设备不能重复占用房间成员或候补位置")]
    DuplicateDevice,
    #[error("同一物理席位不能分配给多个成员")]
    DuplicatePhysicalSeat,
    #[error("成员记录的物理席位与映射键不一致")]
    SeatKeyMismatch,
    #[error("成员版本溢出")]
    VersionOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(seed: u8, seat: u8) -> TableMember {
        TableMember::new(
            PlayerId::new([seed; 32]),
            DevicePublicKey::new([seed; 32]),
            Chips::new(u64::from(seed) * 100_000),
            PhysicalSeat::new(seat).expect("测试席位应当有效"),
        )
    }

    fn candidate(seed: u8) -> JoinCandidate {
        JoinCandidate::new(
            JoinClaimId::new([seed; 32]),
            PlayerId::new([seed; 32]),
            DevicePublicKey::new([seed; 32]),
            Chips::new(u64::from(seed) * 100_000),
        )
    }

    #[test]
    fn 手牌边界保留旧席位并按声明顺序填补空席() {
        let membership = TableMembership::new(
            TableId::new([9; 32]),
            3,
            [member(1, 1), member(2, 4)],
            [candidate(4), candidate(3)],
        )
        .expect("测试成员应当有效");
        let leaving = BTreeSet::from([PlayerId::new([1; 32])]);
        let next = membership
            .reconcile_at_hand_boundary(&leaving)
            .expect("边界重排应成功");
        let seats = next
            .members()
            .map(|entry| (entry.player_id(), entry.physical_seat().value()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(seats[&PlayerId::new([2; 32])], 4);
        assert_eq!(seats[&PlayerId::new([3; 32])], 1);
        assert_eq!(seats[&PlayerId::new([4; 32])], 2);
    }

    #[test]
    fn 手中加入只进入候补且去重() {
        let membership =
            TableMembership::new(TableId::new([9; 32]), 1, [member(1, 1), member(2, 2)], [])
                .expect("测试成员应当有效");
        let queued = membership
            .queue_joins([candidate(3), candidate(3)])
            .expect("候补应当去重");
        assert_eq!(queued.members().count(), 2);
        assert_eq!(queued.waiting().len(), 1);
    }

    #[test]
    fn 重复物理席位必须显式失败而不是静默覆盖() {
        let result =
            TableMembership::new(TableId::new([9; 32]), 1, [member(1, 2), member(2, 2)], []);
        assert_eq!(result, Err(MembershipError::DuplicatePhysicalSeat));
    }
}
