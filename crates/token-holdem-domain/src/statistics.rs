use crate::{Chips, HandReceipt, PlayerId, SignedChips};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerStatistics {
    pub completed_hands: u64,
    pub gross_won: Chips,
    pub gross_lost: Chips,
    pub net_chips: SignedChips,
    pub largest_win: Chips,
    pub largest_loss: Chips,
}

impl PlayerStatistics {
    pub fn derive<'a>(
        player_id: PlayerId,
        receipts: impl IntoIterator<Item = &'a HandReceipt>,
    ) -> Result<Self, StatisticsError> {
        let mut statistics = Self {
            completed_hands: 0,
            gross_won: Chips::ZERO,
            gross_lost: Chips::ZERO,
            net_chips: SignedChips::ZERO,
            largest_win: Chips::ZERO,
            largest_loss: Chips::ZERO,
        };

        for receipt in receipts {
            let Some(outcome) = receipt.outcome_for(player_id) else {
                continue;
            };
            let delta = outcome.delta().value();
            statistics.completed_hands = statistics
                .completed_hands
                .checked_add(1)
                .ok_or(StatisticsError::Overflow)?;
            statistics.net_chips = statistics
                .net_chips
                .checked_add(SignedChips::new(delta))
                .ok_or(StatisticsError::Overflow)?;

            if delta >= 0 {
                let won = Chips::new(u64::try_from(delta).map_err(|_| StatisticsError::Overflow)?);
                statistics.gross_won = statistics
                    .gross_won
                    .checked_add(won)
                    .ok_or(StatisticsError::Overflow)?;
                statistics.largest_win = statistics.largest_win.max(won);
            } else {
                let absolute = delta.checked_abs().ok_or(StatisticsError::Overflow)?;
                let lost =
                    Chips::new(u64::try_from(absolute).map_err(|_| StatisticsError::Overflow)?);
                statistics.gross_lost = statistics
                    .gross_lost
                    .checked_add(lost)
                    .ok_or(StatisticsError::Overflow)?;
                statistics.largest_loss = statistics.largest_loss.max(lost);
            }
        }

        Ok(statistics)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StatisticsError {
    #[error("历史战绩聚合溢出")]
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DevicePublicKey, HandOutcome, MatchId, TranscriptHash};

    fn receipt(hand_number: u64, player_delta: i64) -> HandReceipt {
        let player_start = 1_000_000_u64;
        let opponent_start = 1_000_000_u64;
        let player_end = (player_start as i64 + player_delta) as u64;
        let opponent_end = (opponent_start as i64 - player_delta) as u64;
        HandReceipt::settle(
            MatchId::new([1; 16]),
            hand_number,
            "10k-20k",
            TranscriptHash::new([hand_number as u8; 32]),
            hand_number,
            vec![
                HandOutcome {
                    player_id: PlayerId::new([1; 32]),
                    device_public_key: DevicePublicKey::new([2; 32]),
                    starting_stack: Chips::new(player_start),
                    ending_stack: Chips::new(player_end),
                },
                HandOutcome {
                    player_id: PlayerId::new([3; 32]),
                    device_public_key: DevicePublicKey::new([4; 32]),
                    starting_stack: Chips::new(opponent_start),
                    ending_stack: Chips::new(opponent_end),
                },
            ],
        )
        .expect("测试凭证应当有效")
    }

    #[test]
    fn 战绩必须从不可变凭证重新聚合() {
        let receipts = [
            receipt(1, 300_000),
            receipt(2, -120_000),
            receipt(3, 50_000),
        ];
        let statistics = PlayerStatistics::derive(PlayerId::new([1; 32]), receipts.iter())
            .expect("聚合应当成功");

        assert_eq!(statistics.completed_hands, 3);
        assert_eq!(statistics.gross_won, Chips::new(350_000));
        assert_eq!(statistics.gross_lost, Chips::new(120_000));
        assert_eq!(statistics.net_chips, SignedChips::new(230_000));
        assert_eq!(statistics.largest_win, Chips::new(300_000));
        assert_eq!(statistics.largest_loss, Chips::new(120_000));
    }
}
