#![forbid(unsafe_code)]

mod attestation;
mod certificate;
mod receipt_signature;
mod recovery;
mod signature_bytes;

pub use attestation::{DeviceAttestation, DeviceAttestationError};
pub use certificate::{
    CertificateError, DeviceCertificate, DeviceIdentity, RootIdentity, DEVICE_CERTIFICATE_VERSION,
};
pub use receipt_signature::{CoSignedReceipt, ParticipantSignature, SignedReceiptError};
pub use recovery::{derive_recovery_locator, RecoveryEnvelope, RecoveryError};
