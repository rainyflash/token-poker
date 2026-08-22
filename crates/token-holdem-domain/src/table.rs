use crate::{AccountFingerprint, Chips};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StakeLevel {
    id: String,
    small_blind: Chips,
    big_blind: Chips,
    minimum_buy_in: Chips,
    maximum_buy_in: Chips,
    minimum_players: u8,
    maximum_players: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StakeLevelDefinition {
    id: &'static str,
    small_blind: Chips,
    big_blind: Chips,
    minimum_buy_in: Chips,
    maximum_buy_in: Chips,
    minimum_players: u8,
    maximum_players: u8,
}

impl StakeLevelDefinition {
    const fn new(
        id: &'static str,
        small_blind: u64,
        big_blind: u64,
        minimum_buy_in: u64,
        maximum_buy_in: u64,
        minimum_players: u8,
        maximum_players: u8,
    ) -> Self {
        Self {
            id,
            small_blind: Chips::new(small_blind),
            big_blind: Chips::new(big_blind),
            minimum_buy_in: Chips::new(minimum_buy_in),
            maximum_buy_in: Chips::new(maximum_buy_in),
            minimum_players,
            maximum_players,
        }
    }

    pub const fn id(self) -> &'static str {
        self.id
    }

    pub fn build(self) -> Result<StakeLevel, StakeLevelError> {
        StakeLevel::new(
            self.id,
            self.small_blind,
            self.big_blind,
            self.minimum_buy_in,
            self.maximum_buy_in,
            self.minimum_players,
            self.maximum_players,
        )
    }
}

pub const DEFAULT_PUBLIC_STAKE_LEVEL_ID: &str = "1m-2m";

pub const PUBLIC_STAKE_LEVELS: [StakeLevelDefinition; 4] = [
    StakeLevelDefinition::new("100k-200k", 100_000, 200_000, 8_000_000, 20_000_000, 2, 6),
    StakeLevelDefinition::new("1m-2m", 1_000_000, 2_000_000, 80_000_000, 200_000_000, 2, 6),
    StakeLevelDefinition::new(
        "10m-20m",
        10_000_000,
        20_000_000,
        800_000_000,
        2_000_000_000,
        2,
        6,
    ),
    StakeLevelDefinition::new(
        "100m-200m",
        100_000_000,
        200_000_000,
        8_000_000_000,
        20_000_000_000,
        2,
        6,
    ),
];

pub fn public_stake_level(id: &str) -> Result<Option<StakeLevel>, StakeLevelError> {
    PUBLIC_STAKE_LEVELS
        .iter()
        .copied()
        .find(|definition| definition.id() == id)
        .map(StakeLevelDefinition::build)
        .transpose()
}

impl StakeLevel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        small_blind: Chips,
        big_blind: Chips,
        minimum_buy_in: Chips,
        maximum_buy_in: Chips,
        minimum_players: u8,
        maximum_players: u8,
    ) -> Result<Self, StakeLevelError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(StakeLevelError::EmptyId);
        }
        if small_blind == Chips::ZERO || big_blind <= small_blind {
            return Err(StakeLevelError::InvalidBlinds);
        }
        if minimum_buy_in < big_blind || maximum_buy_in < minimum_buy_in {
            return Err(StakeLevelError::InvalidBuyInRange);
        }
        if !(2..=9).contains(&minimum_players) || !(minimum_players..=9).contains(&maximum_players)
        {
            return Err(StakeLevelError::InvalidPlayerRange);
        }

        Ok(Self {
            id,
            small_blind,
            big_blind,
            minimum_buy_in,
            maximum_buy_in,
            minimum_players,
            maximum_players,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub const fn small_blind(&self) -> Chips {
        self.small_blind
    }

    pub const fn big_blind(&self) -> Chips {
        self.big_blind
    }

    pub const fn minimum_buy_in(&self) -> Chips {
        self.minimum_buy_in
    }

    pub const fn maximum_buy_in(&self) -> Chips {
        self.maximum_buy_in
    }

    pub const fn minimum_players(&self) -> u8 {
        self.minimum_players
    }

    pub const fn maximum_players(&self) -> u8 {
        self.maximum_players
    }

    pub fn validate_buy_in(
        &self,
        snapshot: &OfficialTokenSnapshot,
        requested: Chips,
    ) -> Result<(), BuyInError> {
        if requested < self.minimum_buy_in {
            return Err(BuyInError::BelowMinimum {
                minimum: self.minimum_buy_in,
                requested,
            });
        }
        if requested > self.maximum_buy_in {
            return Err(BuyInError::AboveMaximum {
                maximum: self.maximum_buy_in,
                requested,
            });
        }
        if requested > snapshot.lifetime_tokens {
            return Err(BuyInError::ExceedsOfficialTokens {
                available: snapshot.lifetime_tokens,
                requested,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StakeLevelError {
    #[error("牌桌级别编号不能为空")]
    EmptyId,
    #[error("盲注必须为正数，并且大盲必须大于小盲")]
    InvalidBlinds,
    #[error("买入下限不得低于大盲，买入上限不得低于买入下限")]
    InvalidBuyInRange,
    #[error("牌桌人数必须在 2 到 9 人之间")]
    InvalidPlayerRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficialTokenSnapshot {
    pub account: AccountFingerprint,
    pub lifetime_tokens: Chips,
    pub observed_at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TablePhase {
    BetweenHands,
    HandInProgress,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableStack {
    current: Chips,
    total_top_ups: Chips,
}

impl TableStack {
    pub fn open(
        level: &StakeLevel,
        snapshot: &OfficialTokenSnapshot,
        buy_in: Chips,
    ) -> Result<Self, BuyInError> {
        level.validate_buy_in(snapshot, buy_in)?;
        Ok(Self {
            current: buy_in,
            total_top_ups: Chips::ZERO,
        })
    }

    pub const fn current(&self) -> Chips {
        self.current
    }

    pub const fn total_top_ups(&self) -> Chips {
        self.total_top_ups
    }

    pub fn top_up(
        &mut self,
        level: &StakeLevel,
        phase: TablePhase,
        amount: Chips,
    ) -> Result<(), BuyInError> {
        if phase != TablePhase::BetweenHands {
            return Err(BuyInError::TopUpOutsideHandBoundary);
        }
        if amount == Chips::ZERO {
            return Err(BuyInError::ZeroTopUp);
        }
        let next = self
            .current
            .checked_add(amount)
            .ok_or(BuyInError::Overflow)?;
        if next > level.maximum_buy_in {
            return Err(BuyInError::TopUpExceedsMaximum {
                maximum: level.maximum_buy_in,
                resulting_stack: next,
            });
        }
        self.current = next;
        self.total_top_ups = self
            .total_top_ups
            .checked_add(amount)
            .ok_or(BuyInError::Overflow)?;
        Ok(())
    }

    pub fn apply_hand_delta(&mut self, delta: i128) -> Result<(), BuyInError> {
        let next = i128::from(self.current.value())
            .checked_add(delta)
            .ok_or(BuyInError::Overflow)?;
        if next < 0 || next > i128::from(u64::MAX) {
            return Err(BuyInError::InvalidResultingStack);
        }
        self.current = Chips::new(next as u64);
        Ok(())
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BuyInError {
    #[error("买入额 {requested} 低于下限 {minimum}")]
    BelowMinimum { minimum: Chips, requested: Chips },
    #[error("买入额 {requested} 高于上限 {maximum}")]
    AboveMaximum { maximum: Chips, requested: Chips },
    #[error("买入额 {requested} 超过官方累计 Token {available}")]
    ExceedsOfficialTokens { available: Chips, requested: Chips },
    #[error("只能在两手牌之间补充筹码")]
    TopUpOutsideHandBoundary,
    #[error("补充筹码不能为零")]
    ZeroTopUp,
    #[error("补充后的筹码 {resulting_stack} 超过牌桌上限 {maximum}")]
    TopUpExceedsMaximum {
        maximum: Chips,
        resulting_stack: Chips,
    },
    #[error("筹码计算溢出")]
    Overflow,
    #[error("手牌结算会产生无效筹码余额")]
    InvalidResultingStack,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn snapshot(tokens: u64) -> OfficialTokenSnapshot {
        OfficialTokenSnapshot {
            account: AccountFingerprint::new([7; 32]),
            lifetime_tokens: Chips::new(tokens),
            observed_at_unix_ms: 1,
        }
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
        .expect("测试牌桌应当有效")
    }

    #[test]
    fn 买入额必须同时满足牌桌范围和官方token余额() {
        let level = level();
        assert!(level
            .validate_buy_in(&snapshot(35_500_000_000), Chips::new(1_200_000))
            .is_ok());
        assert!(matches!(
            level.validate_buy_in(&snapshot(1_000_000), Chips::new(1_200_000)),
            Err(BuyInError::ExceedsOfficialTokens { .. })
        ));
    }

    #[test]
    fn 补充筹码只能发生在手牌之间且不得突破上限() {
        let level = level();
        let mut stack = TableStack::open(&level, &snapshot(10_000_000), Chips::new(1_200_000))
            .expect("初始买入应当成功");

        assert_eq!(
            stack.top_up(&level, TablePhase::HandInProgress, Chips::new(100_000)),
            Err(BuyInError::TopUpOutsideHandBoundary)
        );
        stack
            .top_up(&level, TablePhase::BetweenHands, Chips::new(800_000))
            .expect("补充到上限应当成功");
        assert_eq!(stack.current(), Chips::new(2_000_000));
    }

    #[test]
    fn 公共牌桌目录保持唯一且遵循四十到一百个大盲买入() {
        let mut ids = HashSet::new();
        for definition in PUBLIC_STAKE_LEVELS {
            assert!(ids.insert(definition.id()), "牌桌级别编号不得重复");
            let level = definition.build().expect("内置牌桌级别必须有效");
            assert_eq!(
                level.minimum_buy_in().value(),
                level.big_blind().value() * 40
            );
            assert_eq!(
                level.maximum_buy_in().value(),
                level.big_blind().value() * 100
            );
            assert_eq!((level.minimum_players(), level.maximum_players()), (2, 6));
        }
        assert!(ids.contains(DEFAULT_PUBLIC_STAKE_LEVEL_ID));
    }

    #[test]
    fn 四档公开牌局使用统一放大一百倍后的筹码范围() {
        let expected = [
            ("100k-200k", 100_000, 200_000, 8_000_000, 20_000_000),
            ("1m-2m", 1_000_000, 2_000_000, 80_000_000, 200_000_000),
            (
                "10m-20m",
                10_000_000,
                20_000_000,
                800_000_000,
                2_000_000_000,
            ),
            (
                "100m-200m",
                100_000_000,
                200_000_000,
                8_000_000_000,
                20_000_000_000,
            ),
        ];

        for (definition, expected) in PUBLIC_STAKE_LEVELS.into_iter().zip(expected) {
            let level = definition.build().expect("内置牌桌级别必须有效");
            assert_eq!(
                (
                    level.id(),
                    level.small_blind().value(),
                    level.big_blind().value(),
                    level.minimum_buy_in().value(),
                    level.maximum_buy_in().value(),
                ),
                expected
            );
        }
    }

    #[test]
    fn 极限推理级别提供亿级盲注和八十亿到两百亿买入() {
        let level = public_stake_level("100m-200m")
            .expect("内置配置必须有效")
            .expect("极限推理级别必须存在");

        assert_eq!(level.small_blind(), Chips::new(100_000_000));
        assert_eq!(level.big_blind(), Chips::new(200_000_000));
        assert_eq!(level.minimum_buy_in(), Chips::new(8_000_000_000));
        assert_eq!(level.maximum_buy_in(), Chips::new(20_000_000_000));
    }
}
