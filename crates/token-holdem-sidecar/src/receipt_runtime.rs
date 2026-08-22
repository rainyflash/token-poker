use anyhow::{Context, Result};
use std::collections::BTreeMap;
use token_holdem_domain::{HandReceipt, PlayerId};
use token_holdem_identity::{
    CoSignedReceipt, DeviceCertificate, DeviceIdentity, ParticipantSignature,
};

pub(crate) struct ReceiptConsensus {
    receipt: HandReceipt,
    signatures: BTreeMap<PlayerId, ParticipantSignature>,
    finalized: bool,
}

impl ReceiptConsensus {
    pub(crate) fn start(
        receipt: HandReceipt,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
        now_unix_ms: u64,
    ) -> Result<(Self, ParticipantSignature)> {
        let signature = ParticipantSignature::create(&receipt, device, certificate, now_unix_ms)
            .context("无法签署本手牌结算凭证")?;
        let signatures = BTreeMap::from([(signature.player_id(), signature.clone())]);
        Ok((
            Self {
                receipt,
                signatures,
                finalized: false,
            },
            signature,
        ))
    }

    pub(crate) fn accept(
        &mut self,
        receipt: &HandReceipt,
        signature: ParticipantSignature,
        now_unix_ms: u64,
    ) -> Result<bool> {
        if receipt != &self.receipt {
            anyhow::bail!("收到与本地结算不一致的冲突凭证")
        }
        signature
            .verify_for(receipt, now_unix_ms)
            .context("参与者结算签名无效")?;
        let player_id = signature.player_id();
        if let Some(existing) = self.signatures.get(&player_id) {
            if existing == &signature {
                return Ok(false);
            }
            anyhow::bail!("玩家 {player_id} 提交了冲突结算签名")
        }
        self.signatures.insert(player_id, signature);
        Ok(true)
    }

    pub(crate) fn try_finalize(&mut self, now_unix_ms: u64) -> Result<Option<CoSignedReceipt>> {
        if self.finalized || self.signatures.len() != self.receipt.outcomes().len() {
            return Ok(None);
        }
        let receipt = CoSignedReceipt {
            receipt: self.receipt.clone(),
            signatures: self.signatures.values().cloned().collect(),
        };
        receipt
            .verify(now_unix_ms)
            .context("全桌联合签名凭证验证失败")?;
        self.finalized = true;
        Ok(Some(receipt))
    }

    pub(crate) fn signature_count(&self) -> usize {
        self.signatures.len()
    }

    pub(crate) fn is_finalized(&self) -> bool {
        self.finalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use token_holdem_domain::{Chips, HandOutcome, MatchId, PlayerId, TranscriptHash};
    use token_holdem_identity::RootIdentity;

    #[test]
    fn 冲突凭证不能混入同一签名集合() {
        let root = RootIdentity::generate(&mut OsRng);
        let device = DeviceIdentity::generate(&mut OsRng);
        let certificate = root
            .issue_device_certificate(device.public_key(), "A", 1, 10_000)
            .expect("证书应有效");
        let other_player = PlayerId::new([8; 32]);
        let receipt = HandReceipt::settle(
            MatchId::new([1; 16]),
            1,
            "1k-2k",
            TranscriptHash::new([2; 32]),
            3_000,
            vec![
                HandOutcome {
                    player_id: root.player_id(),
                    device_public_key: device.public_key(),
                    starting_stack: Chips::new(100_000),
                    ending_stack: Chips::new(110_000),
                },
                HandOutcome {
                    player_id: other_player,
                    device_public_key: token_holdem_domain::DevicePublicKey::new([9; 32]),
                    starting_stack: Chips::new(100_000),
                    ending_stack: Chips::new(90_000),
                },
            ],
        )
        .expect("凭证应有效");
        let (mut consensus, signature) =
            ReceiptConsensus::start(receipt.clone(), &device, certificate, 3_000)
                .expect("共识应启动");
        let conflicting = HandReceipt::settle(
            MatchId::new([1; 16]),
            1,
            "1k-2k",
            TranscriptHash::new([3; 32]),
            3_000,
            receipt.outcomes().to_vec(),
        )
        .expect("冲突凭证本身仍可合法");

        assert!(consensus.accept(&conflicting, signature, 3_000).is_err());
    }
}
