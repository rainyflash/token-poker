#![forbid(unsafe_code)]

mod attestation;
mod certificate;
mod receipt_signature;
mod recovery;
mod signature_bytes;
mod signed_time;

pub use attestation::{DeviceAttestation, DeviceAttestationError};
pub use certificate::{
    CertificateError, DeviceCertificate, DeviceIdentity, RootIdentity, DEVICE_CERTIFICATE_VERSION,
};
pub use receipt_signature::{CoSignedReceipt, ParticipantSignature, SignedReceiptError};
pub use recovery::{derive_recovery_locator, RecoveryEnvelope, RecoveryError};
pub use signed_time::{
    is_signed_time_before_expiry, is_signed_time_not_future, is_signed_time_window_active,
    is_signed_time_window_expired, MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS,
};
