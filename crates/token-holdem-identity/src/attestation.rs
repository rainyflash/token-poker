use crate::{signature_bytes::SignatureBytes, CertificateError, DeviceCertificate, DeviceIdentity};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const DEVICE_ATTESTATION_DOMAIN: &[u8] = b"token-holdem/device-attestation/v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAttestation {
    certificate: DeviceCertificate,
    payload_hash: [u8; 32],
    signature: SignatureBytes,
}

impl DeviceAttestation {
    pub fn issue(
        domain: &[u8],
        payload: &[u8],
        now_unix_ms: u64,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
    ) -> Result<Self, DeviceAttestationError> {
        certificate.verify_at(now_unix_ms)?;
        if certificate.device_public_key() != device.public_key() {
            return Err(DeviceAttestationError::DeviceKeyMismatch);
        }

        let payload_hash = *blake3::hash(payload).as_bytes();
        let signing_bytes = canonical_signing_bytes(domain, &payload_hash);
        Ok(Self {
            certificate,
            payload_hash,
            signature: device.sign(&signing_bytes),
        })
    }

    pub fn verify(
        &self,
        domain: &[u8],
        payload: &[u8],
        now_unix_ms: u64,
    ) -> Result<(), DeviceAttestationError> {
        self.certificate.verify_at(now_unix_ms)?;
        let actual_hash = *blake3::hash(payload).as_bytes();
        if actual_hash != self.payload_hash {
            return Err(DeviceAttestationError::PayloadHashMismatch);
        }
        self.certificate.verify_device_signature(
            &canonical_signing_bytes(domain, &self.payload_hash),
            &self.signature,
        )?;
        Ok(())
    }

    pub const fn certificate(&self) -> &DeviceCertificate {
        &self.certificate
    }

    pub const fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
}

fn canonical_signing_bytes(domain: &[u8], payload_hash: &[u8; 32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(DEVICE_ATTESTATION_DOMAIN.len() + domain.len() + 36);
    bytes.extend_from_slice(DEVICE_ATTESTATION_DOMAIN);
    bytes.extend_from_slice(&(domain.len() as u32).to_be_bytes());
    bytes.extend_from_slice(domain);
    bytes.extend_from_slice(payload_hash);
    bytes
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DeviceAttestationError {
    #[error(transparent)]
    Certificate(#[from] CertificateError),
    #[error("签名设备与设备证书不匹配")]
    DeviceKeyMismatch,
    #[error("设备证明的载荷摘要不匹配")]
    PayloadHashMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RootIdentity;
    use rand_core::OsRng;

    #[test]
    fn 设备证明绑定业务域和载荷() {
        let root = RootIdentity::generate(&mut OsRng);
        let device = DeviceIdentity::generate(&mut OsRng);
        let certificate = root
            .issue_device_certificate(device.public_key(), "Windows", 1_000, 10_000)
            .expect("证书应签发成功");
        let proof =
            DeviceAttestation::issue(b"match-ticket", b"payload", 2_000, &device, certificate)
                .expect("证明应签发成功");

        assert!(proof.verify(b"match-ticket", b"payload", 3_000).is_ok());
        assert_eq!(
            proof.verify(b"friend-room", b"payload", 3_000),
            Err(DeviceAttestationError::Certificate(
                CertificateError::InvalidDeviceSignature
            ))
        );
        assert_eq!(
            proof.verify(b"match-ticket", b"tampered", 3_000),
            Err(DeviceAttestationError::PayloadHashMismatch)
        );
    }
}
