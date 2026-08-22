use crate::{ContentAddress, RemoteEventStore, ReplicatedContent, StoreError};
use thiserror::Error;
use token_holdem_identity::{CoSignedReceipt, SignedReceiptError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplicationPolicy {
    pub minimum_confirmed_replicas: u16,
}

impl ReplicationPolicy {
    pub fn new(minimum_confirmed_replicas: u16) -> Result<Self, PublishReceiptError> {
        if minimum_confirmed_replicas == 0 {
            return Err(PublishReceiptError::InvalidReplicationPolicy);
        }
        Ok(Self {
            minimum_confirmed_replicas,
        })
    }
}

pub struct PublishReceipt<'a, S: RemoteEventStore> {
    store: &'a S,
    policy: ReplicationPolicy,
}

impl<'a, S: RemoteEventStore> PublishReceipt<'a, S> {
    pub const fn new(store: &'a S, policy: ReplicationPolicy) -> Self {
        Self { store, policy }
    }

    pub async fn execute(
        &self,
        receipt: &CoSignedReceipt,
        now_unix_ms: u64,
    ) -> Result<ReplicatedContent, PublishReceiptError> {
        receipt.verify(now_unix_ms)?;
        let content = serde_json::to_vec(receipt)
            .map_err(|error| PublishReceiptError::Serialization(error.to_string()))?;
        let expected_address = ContentAddress::from_content(&content);
        let replicated = self.store.put(content).await?;
        if replicated.address != expected_address {
            return Err(PublishReceiptError::Store(StoreError::IntegrityMismatch));
        }
        if replicated.confirmed_replicas < self.policy.minimum_confirmed_replicas {
            return Err(PublishReceiptError::InsufficientReplicas {
                required: self.policy.minimum_confirmed_replicas,
                confirmed: replicated.confirmed_replicas,
            });
        }
        Ok(replicated)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PublishReceiptError {
    #[error("至少需要一个远端副本")]
    InvalidReplicationPolicy,
    #[error(transparent)]
    InvalidReceipt(#[from] SignedReceiptError),
    #[error("结算凭证序列化失败：{0}")]
    Serialization(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("远端副本不足：需要 {required}，已确认 {confirmed}")]
    InsufficientReplicas { required: u16, confirmed: u16 },
}
