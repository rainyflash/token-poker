use std::collections::BTreeMap;
use thiserror::Error;
use token_holdem_domain::{
    DevicePublicKey, HandOutcome, HandReceipt, HoldemSettlement, MatchId, PlayerId, ReceiptError,
    TranscriptHash,
};

pub struct CreateHandReceiptRequest {
    pub match_id: MatchId,
    pub hand_number: u64,
    pub stake_level_id: String,
    pub transcript_hash: TranscriptHash,
    pub settled_at_unix_ms: u64,
    pub participant_devices: BTreeMap<PlayerId, DevicePublicKey>,
    pub settlement: HoldemSettlement,
}

pub struct CreateHandReceipt;

impl CreateHandReceipt {
    pub fn execute(request: CreateHandReceiptRequest) -> Result<HandReceipt, CreateReceiptError> {
        if request.participant_devices.len() != request.settlement.players.len() {
            return Err(CreateReceiptError::ParticipantSetMismatch);
        }
        let outcomes = request
            .settlement
            .players
            .into_iter()
            .map(|player| {
                let device_public_key = request
                    .participant_devices
                    .get(&player.player_id)
                    .copied()
                    .ok_or(CreateReceiptError::MissingDevice(player.player_id))?;
                Ok(HandOutcome {
                    player_id: player.player_id,
                    device_public_key,
                    starting_stack: player.starting_stack,
                    ending_stack: player.ending_stack,
                })
            })
            .collect::<Result<Vec<_>, CreateReceiptError>>()?;

        HandReceipt::settle(
            request.match_id,
            request.hand_number,
            request.stake_level_id,
            request.transcript_hash,
            request.settled_at_unix_ms,
            outcomes,
        )
        .map_err(CreateReceiptError::Receipt)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CreateReceiptError {
    #[error("设备映射与结算玩家集合不一致")]
    ParticipantSetMismatch,
    #[error("玩家 {0} 缺少签名设备")]
    MissingDevice(PlayerId),
    #[error(transparent)]
    Receipt(#[from] ReceiptError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_holdem_domain::{Chips, PlayerSettlement, SignedChips};

    #[test]
    fn 牌局结算可以映射为待联合签名凭证() {
        let first = PlayerId::new([1; 32]);
        let second = PlayerId::new([2; 32]);
        let receipt = CreateHandReceipt::execute(CreateHandReceiptRequest {
            match_id: MatchId::new([3; 16]),
            hand_number: 1,
            stake_level_id: "1k-2k".to_owned(),
            transcript_hash: TranscriptHash::new([4; 32]),
            settled_at_unix_ms: 10_000,
            participant_devices: BTreeMap::from([
                (first, DevicePublicKey::new([5; 32])),
                (second, DevicePublicKey::new([6; 32])),
            ]),
            settlement: HoldemSettlement {
                players: vec![
                    PlayerSettlement {
                        player_id: first,
                        starting_stack: Chips::new(100),
                        ending_stack: Chips::new(130),
                        delta: SignedChips::new(30),
                    },
                    PlayerSettlement {
                        player_id: second,
                        starting_stack: Chips::new(100),
                        ending_stack: Chips::new(70),
                        delta: SignedChips::new(-30),
                    },
                ],
            },
        })
        .expect("凭证应创建成功");

        assert_eq!(receipt.outcomes().len(), 2);
    }
}
