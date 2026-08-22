use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentAddress([u8; 32]);

impl ContentAddress {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_content(content: &[u8]) -> Self {
        Self(*blake3::hash(content).as_bytes())
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Display for ContentAddress {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicatedContent {
    pub address: ContentAddress,
    pub confirmed_replicas: u16,
}
