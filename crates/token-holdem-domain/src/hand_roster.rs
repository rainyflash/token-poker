use crate::{
    Chips, DevicePublicKey, MembershipError, PhysicalSeat, PlayerId, TableId, TableMembership,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

const HAND_ROSTER_DOMAIN: &[u8] = b"token-holdem/hand-roster/v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandRosterSeat {
    physical_seat: PhysicalSeat,
    hand_index: u8,
    player_id: PlayerId,
    device_public_key: DevicePublicKey,
    buy_in: Chips,
}

impl HandRosterSeat {
    pub const fn physical_seat(&self) -> PhysicalSeat {
        self.physical_seat
    }

    pub const fn hand_index(&self) -> u8 {
        self.hand_index
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
pub struct ReadyHandRoster {
    table_id: TableId,
    hand_number: u64,
    membership_version: u64,
    previous_receipt_hash: Option<[u8; 32]>,
    dealer_seat: PhysicalSeat,
    seats: Vec<HandRosterSeat>,
    roster_hash: [u8; 32],
}

impl ReadyHandRoster {
    pub fn from_membership(
        membership: &TableMembership,
        hand_number: u64,
        previous_receipt_hash: Option<[u8; 32]>,
        previous_dealer_seat: Option<PhysicalSeat>,
    ) -> Result<Self, HandRosterError> {
        if hand_number == 0 {
            return Err(HandRosterError::InvalidHandNumber);
        }
        let members = membership.members().collect::<Vec<_>>();
        if !(2..=6).contains(&members.len()) {
            return Err(HandRosterError::InvalidPlayerCount(members.len()));
        }
        let occupied = members
            .iter()
            .map(|member| member.physical_seat())
            .collect::<BTreeSet<_>>();
        let dealer_seat = next_dealer(previous_dealer_seat, &occupied)
            .ok_or(HandRosterError::DealerSeatMissing)?;
        let seats = members
            .into_iter()
            .enumerate()
            .map(|(index, member)| {
                Ok(HandRosterSeat {
                    physical_seat: member.physical_seat(),
                    hand_index: u8::try_from(index)
                        .map_err(|_| HandRosterError::InvalidPlayerCount(index))?,
                    player_id: member.player_id(),
                    device_public_key: member.device_public_key(),
                    buy_in: member.buy_in(),
                })
            })
            .collect::<Result<Vec<_>, HandRosterError>>()?;
        let mut roster = Self {
            table_id: membership.table_id(),
            hand_number,
            membership_version: membership.version(),
            previous_receipt_hash,
            dealer_seat,
            seats,
            roster_hash: [0; 32],
        };
        roster.validate()?;
        roster.roster_hash = *blake3::hash(&roster.canonical_bytes()).as_bytes();
        Ok(roster)
    }

    pub const fn table_id(&self) -> TableId {
        self.table_id
    }

    pub const fn hand_number(&self) -> u64 {
        self.hand_number
    }

    pub const fn membership_version(&self) -> u64 {
        self.membership_version
    }

    pub const fn dealer_seat(&self) -> PhysicalSeat {
        self.dealer_seat
    }

    pub const fn previous_receipt_hash(&self) -> Option<[u8; 32]> {
        self.previous_receipt_hash
    }

    pub fn seats(&self) -> &[HandRosterSeat] {
        &self.seats
    }

    pub const fn roster_hash(&self) -> &[u8; 32] {
        &self.roster_hash
    }

    pub fn verify(&self) -> Result<(), HandRosterError> {
        self.validate()?;
        let expected = *blake3::hash(&self.canonical_bytes()).as_bytes();
        if expected != self.roster_hash {
            return Err(HandRosterError::RosterHashMismatch);
        }
        Ok(())
    }

    pub fn hand_index_for_player(&self, player_id: PlayerId) -> Option<u8> {
        self.seats
            .iter()
            .find(|seat| seat.player_id() == player_id)
            .map(HandRosterSeat::hand_index)
    }

    fn validate(&self) -> Result<(), HandRosterError> {
        if !(2..=6).contains(&self.seats.len()) {
            return Err(HandRosterError::InvalidPlayerCount(self.seats.len()));
        }
        let mut physical_seats = BTreeSet::new();
        let mut hand_indices = BTreeSet::new();
        let mut players = BTreeSet::new();
        let mut devices = BTreeSet::new();
        for seat in &self.seats {
            if !physical_seats.insert(seat.physical_seat()) {
                return Err(HandRosterError::DuplicatePhysicalSeat);
            }
            if !hand_indices.insert(seat.hand_index()) {
                return Err(HandRosterError::DuplicateHandIndex);
            }
            if !players.insert(seat.player_id()) {
                return Err(HandRosterError::DuplicatePlayer);
            }
            if !devices.insert(seat.device_public_key()) {
                return Err(HandRosterError::DuplicateDevice);
            }
        }
        if !physical_seats.contains(&self.dealer_seat) {
            return Err(HandRosterError::DealerSeatMissing);
        }
        let expected = (0..self.seats.len())
            .map(|index| u8::try_from(index).expect("最多六个手牌索引"))
            .collect::<BTreeSet<_>>();
        if hand_indices != expected {
            return Err(HandRosterError::NonContiguousHandIndices);
        }
        Ok(())
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(512);
        bytes.extend_from_slice(HAND_ROSTER_DOMAIN);
        bytes.extend_from_slice(self.table_id.as_bytes());
        bytes.extend_from_slice(&self.hand_number.to_be_bytes());
        bytes.extend_from_slice(&self.membership_version.to_be_bytes());
        match self.previous_receipt_hash {
            Some(hash) => {
                bytes.push(1);
                bytes.extend_from_slice(&hash);
            }
            None => bytes.push(0),
        }
        bytes.push(self.dealer_seat.value());
        bytes.push(u8::try_from(self.seats.len()).expect("最多六个席位"));
        for seat in &self.seats {
            bytes.push(seat.physical_seat().value());
            bytes.push(seat.hand_index());
            bytes.extend_from_slice(seat.player_id().as_bytes());
            bytes.extend_from_slice(seat.device_public_key().as_bytes());
            bytes.extend_from_slice(&seat.buy_in().value().to_be_bytes());
        }
        bytes
    }
}

fn next_dealer(
    previous: Option<PhysicalSeat>,
    occupied: &BTreeSet<PhysicalSeat>,
) -> Option<PhysicalSeat> {
    let Some(previous) = previous else {
        return occupied.iter().next().copied();
    };
    (1..=6).find_map(|offset| {
        let value = ((previous.value() - 1 + offset) % 6) + 1;
        PhysicalSeat::new(value)
            .ok()
            .filter(|seat| occupied.contains(seat))
    })
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HandRosterError {
    #[error("手牌编号必须从 1 开始")]
    InvalidHandNumber,
    #[error("逐手名单必须包含 2 到 6 人，实际为 {0}")]
    InvalidPlayerCount(usize),
    #[error("逐手名单包含重复物理席位")]
    DuplicatePhysicalSeat,
    #[error("逐手名单包含重复手牌索引")]
    DuplicateHandIndex,
    #[error("逐手名单包含重复玩家")]
    DuplicatePlayer,
    #[error("逐手名单包含重复设备")]
    DuplicateDevice,
    #[error("逐手名单的连续索引不完整")]
    NonContiguousHandIndices,
    #[error("庄家物理席位不在逐手名单中")]
    DealerSeatMissing,
    #[error("逐手名单摘要与名单内容不一致")]
    RosterHashMismatch,
    #[error(transparent)]
    Membership(#[from] MembershipError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JoinCandidate, JoinClaimId, TableMember};

    fn member(seed: u8, seat: u8) -> TableMember {
        TableMember::new(
            PlayerId::new([seed; 32]),
            DevicePublicKey::new([seed; 32]),
            Chips::new(u64::from(seed) * 1_000_000),
            PhysicalSeat::new(seat).expect("测试席位应当有效"),
        )
    }

    #[test]
    fn 稀疏物理席位映射为连续手牌索引() {
        let membership = TableMembership::new(
            TableId::new([7; 32]),
            4,
            [member(1, 2), member(2, 5), member(3, 6)],
            Vec::<JoinCandidate>::new(),
        )
        .expect("测试成员应当有效");
        let roster = ReadyHandRoster::from_membership(&membership, 8, Some([4; 32]), None)
            .expect("逐手名单应当有效");
        assert_eq!(roster.dealer_seat().value(), 2);
        assert_eq!(
            roster
                .seats()
                .iter()
                .map(HandRosterSeat::hand_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn 庄家跳过退出后的空席() {
        let membership = TableMembership::new(
            TableId::new([7; 32]),
            5,
            [member(1, 2), member(3, 6)],
            Vec::<JoinCandidate>::new(),
        )
        .expect("测试成员应当有效");
        let roster = ReadyHandRoster::from_membership(
            &membership,
            9,
            Some([8; 32]),
            Some(PhysicalSeat::new(2).expect("测试席位应当有效")),
        )
        .expect("下一手名单应当有效");
        assert_eq!(roster.dealer_seat().value(), 6);
    }

    #[test]
    fn 名单摘要绑定成员版本和上一手凭证() {
        let membership = TableMembership::new(
            TableId::new([7; 32]),
            5,
            [member(1, 1), member(2, 2)],
            [JoinCandidate::new(
                JoinClaimId::new([3; 32]),
                PlayerId::new([3; 32]),
                DevicePublicKey::new([3; 32]),
                Chips::new(1_000_000),
            )],
        )
        .expect("测试成员应当有效");
        let first = ReadyHandRoster::from_membership(&membership, 2, Some([1; 32]), None)
            .expect("名单应当有效");
        let second = ReadyHandRoster::from_membership(&membership, 2, Some([2; 32]), None)
            .expect("名单应当有效");
        assert_ne!(first.roster_hash(), second.roster_hash());
    }
}
