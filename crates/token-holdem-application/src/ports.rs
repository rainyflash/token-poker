use crate::{ContentAddress, ReplicatedContent};
use async_trait::async_trait;
use thiserror::Error;
use token_holdem_domain::OfficialTokenSnapshot;

#[async_trait]
pub trait AccountTokenSource: Send + Sync {
    async fn latest_snapshot(&self) -> Result<OfficialTokenSnapshot, StoreError>;
}

#[async_trait]
pub trait RemoteEventStore: Send + Sync {
    async fn put(&self, content: Vec<u8>) -> Result<ReplicatedContent, StoreError>;

    async fn get(&self, address: ContentAddress) -> Result<Option<Vec<u8>>, StoreError>;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StoreError {
    #[error("远端节点暂时不可用：{0}")]
    Unavailable(String),
    #[error("远端节点拒绝了内容：{0}")]
    Rejected(String),
    #[error("远端内容未通过哈希校验")]
    IntegrityMismatch,
}
