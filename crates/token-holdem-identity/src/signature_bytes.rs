use serde::{
    de::{Error as _, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SignatureBytes([u8; 64]);

impl SignatureBytes {
    pub const fn zeroed() -> Self {
        Self([0; 64])
    }

    pub const fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    pub const fn as_array(&self) -> &[u8; 64] {
        &self.0
    }
}

impl Serialize for SignatureBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for SignatureBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(SignatureBytesVisitor)
    }
}

struct SignatureBytesVisitor;

impl<'de> Visitor<'de> for SignatureBytesVisitor {
    type Value = SignatureBytes;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("恰好 64 字节的签名")
    }

    fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        signature_from_slice(value)
    }

    fn visit_borrowed_bytes<E>(self, value: &'de [u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        signature_from_slice(value)
    }

    fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        signature_from_slice(&value)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut bytes = Vec::with_capacity(64);
        while let Some(byte) = sequence.next_element::<u8>()? {
            bytes.push(byte);
            if bytes.len() > 64 {
                return Err(A::Error::custom("签名长度超过 64 字节"));
            }
        }
        signature_from_slice(&bytes)
    }
}

fn signature_from_slice<E>(value: &[u8]) -> Result<SignatureBytes, E>
where
    E: serde::de::Error,
{
    let bytes: [u8; 64] = value
        .try_into()
        .map_err(|_| E::custom(format!("签名长度必须为 64，实际为 {}", value.len())))?;
    Ok(SignatureBytes(bytes))
}
