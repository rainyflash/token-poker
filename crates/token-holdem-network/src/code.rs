use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolCodeError {
    #[error("无法将协议载荷序列化为 CBOR")]
    EncodingFailed,
    #[error("协议代码超过长度上限：最大 {maximum} 字节，实际 {actual} 字节")]
    CodeTooLong { maximum: usize, actual: usize },
    #[error("协议代码前缀无效")]
    InvalidPrefix,
    #[error("协议代码不是合法 Base64URL")]
    InvalidBase64,
    #[error("协议代码的 CBOR 载荷无效")]
    InvalidPayload,
}

pub fn encode_code<T: Serialize>(prefix: &str, value: &T) -> Result<String, ProtocolCodeError> {
    let payload = encode_payload(value)?;
    Ok(encode_payload_code(prefix, &payload))
}

pub fn encode_payload<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolCodeError> {
    cbor4ii::serde::to_vec(Vec::new(), value).map_err(|_| ProtocolCodeError::EncodingFailed)
}

pub fn encode_payload_code(prefix: &str, payload: &[u8]) -> String {
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(payload))
}

pub fn decode_code<T: DeserializeOwned>(
    prefix: &str,
    code: &str,
    maximum_code_bytes: usize,
) -> Result<T, ProtocolCodeError> {
    decode_code_with_payload(prefix, code, maximum_code_bytes).map(|(value, _)| value)
}

pub fn decode_code_with_payload<T: DeserializeOwned>(
    prefix: &str,
    code: &str,
    maximum_code_bytes: usize,
) -> Result<(T, Vec<u8>), ProtocolCodeError> {
    let normalized = code.trim();
    if normalized.len() > maximum_code_bytes {
        return Err(ProtocolCodeError::CodeTooLong {
            maximum: maximum_code_bytes,
            actual: normalized.len(),
        });
    }
    let encoded = normalized
        .strip_prefix(prefix)
        .ok_or(ProtocolCodeError::InvalidPrefix)?;
    let payload = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| ProtocolCodeError::InvalidBase64)?;
    let value = decode_payload(&payload)?;
    Ok((value, payload))
}

pub fn decode_payload<T: DeserializeOwned>(payload: &[u8]) -> Result<T, ProtocolCodeError> {
    cbor4ii::serde::from_slice(payload).map_err(|_| ProtocolCodeError::InvalidPayload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Example {
        sequence: u64,
        label: String,
    }

    #[test]
    fn 协议代码可以无损往返并返回原始载荷() {
        let expected = Example {
            sequence: 8,
            label: "牌桌".to_owned(),
        };
        let code = encode_code("TH1-", &expected).expect("编码应成功");
        let (actual, payload) =
            decode_code_with_payload::<Example>("TH1-", &code, 1_024).expect("解码应成功");

        assert_eq!(actual, expected);
        assert_eq!(encode_payload(&expected).expect("编码应成功"), payload);
    }

    #[test]
    fn 协议代码拒绝错误前缀超长输入和畸形载荷() {
        assert_eq!(
            decode_code::<Example>("TH1-", "THR1-AAAA", 1_024),
            Err(ProtocolCodeError::InvalidPrefix)
        );
        assert_eq!(
            decode_code::<Example>("TH1-", "TH1-AAAA", 3),
            Err(ProtocolCodeError::CodeTooLong {
                maximum: 3,
                actual: 8,
            })
        );
        assert_eq!(
            decode_code::<Example>("TH1-", "TH1-*", 1_024),
            Err(ProtocolCodeError::InvalidBase64)
        );
        assert_eq!(
            decode_code::<Example>("TH1-", "TH1-AA", 1_024),
            Err(ProtocolCodeError::InvalidPayload)
        );
    }
}
