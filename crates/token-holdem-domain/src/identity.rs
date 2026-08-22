use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

macro_rules! bytes32_identifier {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name([u8; 32]);

        impl $name {
            pub const fn new(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&hex::encode(self.0))
            }
        }
    };
}

bytes32_identifier!(PlayerId);
bytes32_identifier!(DevicePublicKey);
bytes32_identifier!(AccountFingerprint);
