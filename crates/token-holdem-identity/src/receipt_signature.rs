use crate::{signature_bytes::SignatureBytes, CertificateError, DeviceCertificate, DeviceIdentity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use token_holdem_domain::{DevicePublicKey, HandReceipt, PlayerId, ReceiptError};

const RECEIPT_SIGNATURE_DOMAIN: &[u8] = b"token-holdem/receipt-signature/v1\0";
const MAX_RECEIPT_FUTURE_SKEW_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantSignature {
    pub certificate: DeviceCertificate,
    signature: SignatureBytes,
}

impl ParticipantSignature {
    pub fn create(
        receipt: &HandReceipt,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
        now_unix_ms: u64,
    ) -> Result<Self, SignedReceiptError> {
        receipt.validate()?;
        certificate.verify_at(now_unix_ms)?;
        validate_receipt_time(receipt, now_unix_ms)?;
        certificate.verify_at(receipt.settled_at_unix_ms())?;
        if certificate.device_public_key() != device.public_key() {
            return Err(SignedReceiptError::CertificateDoesNotMatchDevice);
        }
        let outcome = receipt
            .outcome_for(certificate.player_id())
            .ok_or(SignedReceiptError::PlayerMissingFromReceipt)?;
        if outcome.device_public_key != certificate.device_public_key() {
            return Err(SignedReceiptError::ReceiptUsesDifferentDevice);
        }
        let signature = device.sign(&signature_message(receipt));
        Ok(Self {
            certificate,
            signature,
        })
    }

    pub const fn player_id(&self) -> PlayerId {
        self.certificate.player_id()
    }

    pub const fn device_public_key(&self) -> DevicePublicKey {
        self.certificate.device_public_key()
    }

    pub fn verify_for(
        &self,
        receipt: &HandReceipt,
        now_unix_ms: u64,
    ) -> Result<(), SignedReceiptError> {
        receipt.validate()?;
        validate_receipt_time(receipt, now_unix_ms)?;
        self.certificate.verify_at(receipt.settled_at_unix_ms())?;
        let player_id = self.certificate.player_id();
        let outcome = receipt
            .outcome_for(player_id)
            .ok_or(SignedReceiptError::UnexpectedSigner(player_id))?;
        if outcome.device_public_key != self.certificate.device_public_key() {
            return Err(SignedReceiptError::ReceiptUsesDifferentDevice);
        }
        self.certificate
            .verify_device_signature(&signature_message(receipt), &self.signature)?;
        Ok(())
    }
}

fn validate_receipt_time(
    receipt: &HandReceipt,
    now_unix_ms: u64,
) -> Result<(), SignedReceiptError> {
    if receipt.settled_at_unix_ms() > now_unix_ms.saturating_add(MAX_RECEIPT_FUTURE_SKEW_MS) {
        return Err(SignedReceiptError::ReceiptFromFuture {
            settled_at: receipt.settled_at_unix_ms(),
            now: now_unix_ms,
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoSignedReceipt {
    pub receipt: HandReceipt,
    pub signatures: Vec<ParticipantSignature>,
}

impl CoSignedReceipt {
    pub fn verify(&self, now_unix_ms: u64) -> Result<(), SignedReceiptError> {
        self.receipt.validate()?;
        let expected: BTreeMap<PlayerId, DevicePublicKey> = self
            .receipt
            .outcomes()
            .iter()
            .map(|outcome| (outcome.player_id, outcome.device_public_key))
            .collect();
        let mut signed_players = BTreeSet::new();
        for participant in &self.signatures {
            participant.verify_for(&self.receipt, now_unix_ms)?;
            let player_id = participant.certificate.player_id();
            if !signed_players.insert(player_id) {
                return Err(SignedReceiptError::DuplicatePlayerSignature(player_id));
            }
            let expected_device = expected
                .get(&player_id)
                .ok_or(SignedReceiptError::UnexpectedSigner(player_id))?;
            if *expected_device != participant.certificate.device_public_key() {
                return Err(SignedReceiptError::ReceiptUsesDifferentDevice);
            }
        }

        if signed_players.len() != expected.len() {
            return Err(SignedReceiptError::MissingSignatures {
                expected: expected.len(),
                actual: signed_players.len(),
            });
        }
        Ok(())
    }
}

fn signature_message(receipt: &HandReceipt) -> Vec<u8> {
    let canonical = receipt.canonical_bytes();
    let mut message = Vec::with_capacity(RECEIPT_SIGNATURE_DOMAIN.len() + canonical.len());
    message.extend_from_slice(RECEIPT_SIGNATURE_DOMAIN);
    message.extend_from_slice(&canonical);
    message
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SignedReceiptError {
    #[error(transparent)]
    Receipt(#[from] ReceiptError),
    #[error(transparent)]
    Certificate(#[from] CertificateError),
    #[error("设备证书与当前设备密钥不匹配")]
    CertificateDoesNotMatchDevice,
    #[error("签名玩家不在结算凭证中")]
    PlayerMissingFromReceipt,
    #[error("结算凭证声明了另一台设备")]
    ReceiptUsesDifferentDevice,
    #[error("玩家 {0} 重复签名")]
    DuplicatePlayerSignature(PlayerId),
    #[error("玩家 {0} 不是本手牌参与者")]
    UnexpectedSigner(PlayerId),
    #[error("缺少参与方签名：需要 {expected}，实际 {actual}")]
    MissingSignatures { expected: usize, actual: usize },
    #[error("结算凭证来自未来：结算时间 {settled_at}，当前时间 {now}")]
    ReceiptFromFuture { settled_at: u64, now: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RootIdentity;
    use rand_core::OsRng;
    use token_holdem_domain::{Chips, HandOutcome, MatchId, TranscriptHash};

    #[test]
    fn 所有参与设备联合签名后凭证才有效() {
        let root_a = RootIdentity::generate(&mut OsRng);
        let root_b = RootIdentity::generate(&mut OsRng);
        let device_a = DeviceIdentity::generate(&mut OsRng);
        let device_b = DeviceIdentity::generate(&mut OsRng);
        let certificate_a = root_a
            .issue_device_certificate(device_a.public_key(), "A", 1, 10_000)
            .expect("证书 A 应当有效");
        let certificate_b = root_b
            .issue_device_certificate(device_b.public_key(), "B", 1, 10_000)
            .expect("证书 B 应当有效");
        let receipt = HandReceipt::settle(
            MatchId::new([1; 16]),
            1,
            "10k-20k",
            TranscriptHash::new([2; 32]),
            5_000,
            vec![
                HandOutcome {
                    player_id: root_a.player_id(),
                    device_public_key: device_a.public_key(),
                    starting_stack: Chips::new(1_000_000),
                    ending_stack: Chips::new(1_300_000),
                },
                HandOutcome {
                    player_id: root_b.player_id(),
                    device_public_key: device_b.public_key(),
                    starting_stack: Chips::new(1_000_000),
                    ending_stack: Chips::new(700_000),
                },
            ],
        )
        .expect("凭证应当有效");

        let signed = CoSignedReceipt {
            receipt: receipt.clone(),
            signatures: vec![
                ParticipantSignature::create(&receipt, &device_a, certificate_a, 5_000)
                    .expect("A 应当签名成功"),
                ParticipantSignature::create(&receipt, &device_b, certificate_b, 5_000)
                    .expect("B 应当签名成功"),
            ],
        };

        assert!(signed.verify(5_000).is_ok());
        assert!(validate_receipt_time(&receipt, 4_992).is_ok());

        let far_future_receipt = HandReceipt::settle(
            MatchId::new([3; 16]),
            2,
            "10k-20k",
            TranscriptHash::new([4; 32]),
            65_001,
            vec![
                HandOutcome {
                    player_id: root_a.player_id(),
                    device_public_key: device_a.public_key(),
                    starting_stack: Chips::new(1_000_000),
                    ending_stack: Chips::new(1_300_000),
                },
                HandOutcome {
                    player_id: root_b.player_id(),
                    device_public_key: device_b.public_key(),
                    starting_stack: Chips::new(1_000_000),
                    ending_stack: Chips::new(700_000),
                },
            ],
        )
        .expect("未来凭证结构本身应当有效");
        assert!(matches!(
            validate_receipt_time(&far_future_receipt, 5_000),
            Err(SignedReceiptError::ReceiptFromFuture { .. })
        ));
    }
}
