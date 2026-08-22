use crate::RootIdentity;
use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroizing;

const RECOVERY_AAD_V1: &[u8] = b"token-holdem/recovery-envelope/v1";
const RECOVERY_AAD_V2_DOMAIN: &[u8] = b"token-holdem/recovery-envelope/v2\0";
const RECOVERY_LOCATOR_DOMAIN: &[u8] = b"token-holdem/recovery-locator/v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryEnvelope {
    pub version: u8,
    pub salt: [u8; 16],
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

pub fn derive_recovery_locator(
    account_fingerprint: &str,
    recovery_secret: &str,
) -> Result<[u8; 32], RecoveryError> {
    validate_recovery_secret(recovery_secret)?;
    validate_account_fingerprint(account_fingerprint)?;
    let mut salt_hasher = blake3::Hasher::new();
    salt_hasher.update(RECOVERY_LOCATOR_DOMAIN);
    salt_hasher.update(account_fingerprint.as_bytes());
    let salt_hash = salt_hasher.finalize();
    let mut locator = [0_u8; 32];
    Argon2::default()
        .hash_password_into(
            recovery_secret.as_bytes(),
            &salt_hash.as_bytes()[..16],
            &mut locator,
        )
        .map_err(|_| RecoveryError::KeyDerivationFailed)?;
    Ok(locator)
}

impl RecoveryEnvelope {
    pub fn seal(identity: &RootIdentity, recovery_secret: &str) -> Result<Self, RecoveryError> {
        Self::seal_with_aad(identity, recovery_secret, 1, RECOVERY_AAD_V1)
    }

    pub fn seal_for_account(
        identity: &RootIdentity,
        recovery_secret: &str,
        account_fingerprint: &str,
    ) -> Result<Self, RecoveryError> {
        let aad = account_bound_aad(account_fingerprint)?;
        Self::seal_with_aad(identity, recovery_secret, 2, &aad)
    }

    fn seal_with_aad(
        identity: &RootIdentity,
        recovery_secret: &str,
        version: u8,
        aad: &[u8],
    ) -> Result<Self, RecoveryError> {
        validate_recovery_secret(recovery_secret)?;
        let mut salt = [0_u8; 16];
        OsRng.fill_bytes(&mut salt);
        let mut key = Zeroizing::new([0_u8; 32]);
        Argon2::default()
            .hash_password_into(recovery_secret.as_bytes(), &salt, key.as_mut())
            .map_err(|_| RecoveryError::KeyDerivationFailed)?;

        let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
            .map_err(|_| RecoveryError::EncryptionFailed)?;
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let seed = identity.seed();
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: seed.as_ref(),
                    aad,
                },
            )
            .map_err(|_| RecoveryError::EncryptionFailed)?;

        Ok(Self {
            version,
            salt,
            nonce: nonce.into(),
            ciphertext,
        })
    }

    pub fn open(&self, recovery_secret: &str) -> Result<RootIdentity, RecoveryError> {
        if self.version != 1 {
            return Err(RecoveryError::UnsupportedVersion(self.version));
        }
        self.open_with_aad(recovery_secret, RECOVERY_AAD_V1)
    }

    pub fn open_for_account(
        &self,
        recovery_secret: &str,
        account_fingerprint: &str,
    ) -> Result<RootIdentity, RecoveryError> {
        if self.version != 2 {
            return Err(RecoveryError::UnsupportedVersion(self.version));
        }
        let aad = account_bound_aad(account_fingerprint)?;
        self.open_with_aad(recovery_secret, &aad)
    }

    fn open_with_aad(
        &self,
        recovery_secret: &str,
        aad: &[u8],
    ) -> Result<RootIdentity, RecoveryError> {
        validate_recovery_secret(recovery_secret)?;
        let mut key = Zeroizing::new([0_u8; 32]);
        Argon2::default()
            .hash_password_into(recovery_secret.as_bytes(), &self.salt, key.as_mut())
            .map_err(|_| RecoveryError::KeyDerivationFailed)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
            .map_err(|_| RecoveryError::DecryptionFailed)?;
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    XNonce::from_slice(&self.nonce),
                    Payload {
                        msg: &self.ciphertext,
                        aad,
                    },
                )
                .map_err(|_| RecoveryError::DecryptionFailed)?,
        );
        let seed: [u8; 32] = plaintext
            .as_slice()
            .try_into()
            .map_err(|_| RecoveryError::InvalidPlaintext)?;
        Ok(RootIdentity::from_seed(seed))
    }
}

fn account_bound_aad(account_fingerprint: &str) -> Result<Vec<u8>, RecoveryError> {
    validate_account_fingerprint(account_fingerprint)?;
    let mut aad = Vec::with_capacity(RECOVERY_AAD_V2_DOMAIN.len() + account_fingerprint.len());
    aad.extend_from_slice(RECOVERY_AAD_V2_DOMAIN);
    aad.extend(
        account_fingerprint
            .bytes()
            .map(|byte| byte.to_ascii_lowercase()),
    );
    Ok(aad)
}

fn validate_account_fingerprint(account_fingerprint: &str) -> Result<(), RecoveryError> {
    if account_fingerprint.len() != 64
        || !account_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RecoveryError::InvalidAccountFingerprint);
    }
    Ok(())
}

fn validate_recovery_secret(value: &str) -> Result<(), RecoveryError> {
    if value.chars().count() < 12 {
        return Err(RecoveryError::RecoverySecretTooShort);
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RecoveryError {
    #[error("恢复密语至少需要 12 个字符")]
    RecoverySecretTooShort,
    #[error("不支持的恢复包版本 {0}")]
    UnsupportedVersion(u8),
    #[error("恢复密钥派生失败")]
    KeyDerivationFailed,
    #[error("根密钥加密失败")]
    EncryptionFailed,
    #[error("根密钥解密失败；恢复密语错误或数据已损坏")]
    DecryptionFailed,
    #[error("恢复包中的根密钥长度无效")]
    InvalidPlaintext,
    #[error("Codex 账户指纹格式无效")]
    InvalidAccountFingerprint,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    #[test]
    fn 加密恢复包可以在另一台设备还原同一玩家身份() {
        let root = RootIdentity::generate(&mut OsRng);
        let player_id = root.player_id();
        let envelope =
            RecoveryEnvelope::seal(&root, "correct horse battery staple").expect("加密应当成功");
        let restored = envelope
            .open("correct horse battery staple")
            .expect("解密应当成功");

        assert_eq!(restored.player_id(), player_id);
        assert!(envelope.open("this is the wrong secret").is_err());
    }

    #[test]
    fn 恢复定位符同时绑定账户指纹和恢复密语() {
        let account = "ab".repeat(32);
        let first = derive_recovery_locator(&account, "correct horse battery staple").unwrap();
        let second = derive_recovery_locator(&account, "another sufficiently long secret").unwrap();
        let other_account =
            derive_recovery_locator(&"cd".repeat(32), "correct horse battery staple").unwrap();

        assert_ne!(first, second);
        assert_ne!(first, other_account);
    }

    #[test]
    fn 账户绑定恢复包不能被归档节点调包到其他账户() {
        let root = RootIdentity::generate(&mut OsRng);
        let account = "ab".repeat(32);
        let other_account = "cd".repeat(32);
        let envelope =
            RecoveryEnvelope::seal_for_account(&root, "correct horse battery staple", &account)
                .expect("账户绑定加密应当成功");

        let restored = envelope
            .open_for_account("correct horse battery staple", &account)
            .expect("同一账户应当恢复");
        assert_eq!(restored.player_id(), root.player_id());
        assert!(envelope
            .open_for_account("correct horse battery staple", &other_account)
            .is_err());
        assert!(envelope.open("correct horse battery staple").is_err());
    }
}
