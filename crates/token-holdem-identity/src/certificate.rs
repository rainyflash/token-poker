use crate::{is_signed_time_window_active, signature_bytes::SignatureBytes};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use token_holdem_domain::{DevicePublicKey, PlayerId};
use zeroize::Zeroizing;

pub const DEVICE_CERTIFICATE_VERSION: u8 = 1;
const PLAYER_ID_DOMAIN: &[u8] = b"token-holdem/player-id/v1\0";
const DEVICE_CERTIFICATE_DOMAIN: &[u8] = b"token-holdem/device-certificate/v1\0";

pub struct RootIdentity {
    signing_key: SigningKey,
}

impl RootIdentity {
    pub fn generate(rng: &mut (impl CryptoRng + RngCore)) -> Self {
        Self {
            signing_key: SigningKey::generate(rng),
        }
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&seed),
        }
    }

    pub fn seed(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.signing_key.to_bytes())
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn player_id(&self) -> PlayerId {
        player_id_from_root_key(&self.verifying_key())
    }

    pub fn issue_device_certificate(
        &self,
        device_public_key: DevicePublicKey,
        label: impl Into<String>,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<DeviceCertificate, CertificateError> {
        if expires_at_unix_ms <= issued_at_unix_ms {
            return Err(CertificateError::InvalidValidityWindow);
        }
        let label = label.into();
        if label.trim().is_empty() || label.len() > 80 {
            return Err(CertificateError::InvalidLabel);
        }

        let mut certificate = DeviceCertificate {
            version: DEVICE_CERTIFICATE_VERSION,
            player_id: self.player_id(),
            root_public_key: self.verifying_key().to_bytes(),
            device_public_key,
            label,
            issued_at_unix_ms,
            expires_at_unix_ms,
            root_signature: SignatureBytes::zeroed(),
        };
        certificate.root_signature = SignatureBytes::new(
            self.signing_key
                .sign(&certificate.canonical_unsigned_bytes())
                .to_bytes(),
        );
        Ok(certificate)
    }
}

pub struct DeviceIdentity {
    signing_key: SigningKey,
}

impl DeviceIdentity {
    pub fn generate(rng: &mut (impl CryptoRng + RngCore)) -> Self {
        Self {
            signing_key: SigningKey::generate(rng),
        }
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&seed),
        }
    }

    pub fn public_key(&self) -> DevicePublicKey {
        DevicePublicKey::new(self.signing_key.verifying_key().to_bytes())
    }

    pub(crate) fn sign(&self, message: &[u8]) -> SignatureBytes {
        SignatureBytes::new(self.signing_key.sign(message).to_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCertificate {
    version: u8,
    player_id: PlayerId,
    root_public_key: [u8; 32],
    device_public_key: DevicePublicKey,
    label: String,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    root_signature: SignatureBytes,
}

impl DeviceCertificate {
    pub const fn player_id(&self) -> PlayerId {
        self.player_id
    }

    pub const fn device_public_key(&self) -> DevicePublicKey {
        self.device_public_key
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub const fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }

    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }

    pub fn verify_at(&self, now_unix_ms: u64) -> Result<(), CertificateError> {
        if self.version != DEVICE_CERTIFICATE_VERSION {
            return Err(CertificateError::UnsupportedVersion(self.version));
        }
        let root_key = VerifyingKey::from_bytes(&self.root_public_key)
            .map_err(|_| CertificateError::InvalidRootPublicKey)?;
        if player_id_from_root_key(&root_key) != self.player_id {
            return Err(CertificateError::PlayerIdMismatch);
        }
        let signature = Signature::from_bytes(self.root_signature.as_array());
        root_key
            .verify(&self.canonical_unsigned_bytes(), &signature)
            .map_err(|_| CertificateError::InvalidRootSignature)?;
        if !is_signed_time_window_active(
            self.issued_at_unix_ms,
            self.expires_at_unix_ms,
            now_unix_ms,
        ) {
            return Err(CertificateError::OutsideValidityWindow);
        }
        Ok(())
    }

    pub(crate) fn verify_device_signature(
        &self,
        message: &[u8],
        signature: &SignatureBytes,
    ) -> Result<(), CertificateError> {
        let key = VerifyingKey::from_bytes(self.device_public_key.as_bytes())
            .map_err(|_| CertificateError::InvalidDevicePublicKey)?;
        key.verify(message, &Signature::from_bytes(signature.as_array()))
            .map_err(|_| CertificateError::InvalidDeviceSignature)
    }

    fn canonical_unsigned_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(192);
        bytes.extend_from_slice(DEVICE_CERTIFICATE_DOMAIN);
        bytes.push(self.version);
        bytes.extend_from_slice(self.player_id.as_bytes());
        bytes.extend_from_slice(&self.root_public_key);
        bytes.extend_from_slice(self.device_public_key.as_bytes());
        bytes.extend_from_slice(&(self.label.len() as u32).to_be_bytes());
        bytes.extend_from_slice(self.label.as_bytes());
        bytes.extend_from_slice(&self.issued_at_unix_ms.to_be_bytes());
        bytes.extend_from_slice(&self.expires_at_unix_ms.to_be_bytes());
        bytes
    }
}

fn player_id_from_root_key(root_key: &VerifyingKey) -> PlayerId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(PLAYER_ID_DOMAIN);
    hasher.update(root_key.as_bytes());
    PlayerId::new(*hasher.finalize().as_bytes())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CertificateError {
    #[error("设备证书有效期无效")]
    InvalidValidityWindow,
    #[error("设备名称必须为 1 到 80 个字节")]
    InvalidLabel,
    #[error("不支持的设备证书版本 {0}")]
    UnsupportedVersion(u8),
    #[error("根公钥无效")]
    InvalidRootPublicKey,
    #[error("设备公钥无效")]
    InvalidDevicePublicKey,
    #[error("玩家编号与根公钥不匹配")]
    PlayerIdMismatch,
    #[error("根签名无效")]
    InvalidRootSignature,
    #[error("设备签名无效")]
    InvalidDeviceSignature,
    #[error("设备证书不在有效期内")]
    OutsideValidityWindow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    fn assert_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}

    #[test]
    fn 签名密钥必须在析构时清零() {
        assert_zeroize_on_drop::<SigningKey>();
    }

    #[test]
    fn 根身份可以签发并验证设备证书() {
        let root = RootIdentity::generate(&mut OsRng);
        let device = DeviceIdentity::generate(&mut OsRng);
        let certificate = root
            .issue_device_certificate(device.public_key(), "主力电脑", 1_000, 10_000)
            .expect("签发应当成功");

        assert_eq!(certificate.player_id(), root.player_id());
        assert!(certificate.verify_at(5_000).is_ok());
        assert!(certificate.verify_at(10_000).is_ok());
        assert_eq!(
            certificate
                .verify_at(10_000_u64.saturating_add(crate::MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS)),
            Err(CertificateError::OutsideValidityWindow)
        );
    }
}
