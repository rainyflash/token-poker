use crate::{AccountTokenSource, StoreError};
use thiserror::Error;
use token_holdem_domain::{BuyInError, Chips, OfficialTokenSnapshot, StakeLevel, TableStack};

pub struct JoinTable<'a, T: AccountTokenSource> {
    token_source: &'a T,
}

impl<'a, T: AccountTokenSource> JoinTable<'a, T> {
    pub const fn new(token_source: &'a T) -> Self {
        Self { token_source }
    }

    pub async fn execute(
        &self,
        request: JoinTableRequest,
    ) -> Result<JoinTableResult, JoinTableError> {
        let snapshot = self.token_source.latest_snapshot().await?;
        let stack = TableStack::open(&request.level, &snapshot, request.buy_in)?;
        Ok(JoinTableResult {
            level: request.level,
            snapshot,
            stack,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinTableRequest {
    pub level: StakeLevel,
    pub buy_in: Chips,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinTableResult {
    pub level: StakeLevel,
    pub snapshot: OfficialTokenSnapshot,
    pub stack: TableStack,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum JoinTableError {
    #[error(transparent)]
    TokenSource(#[from] StoreError),
    #[error(transparent)]
    BuyIn(#[from] BuyInError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use token_holdem_domain::AccountFingerprint;

    struct FixedTokenSource(OfficialTokenSnapshot);

    #[async_trait]
    impl AccountTokenSource for FixedTokenSource {
        async fn latest_snapshot(&self) -> Result<OfficialTokenSnapshot, StoreError> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn 加入牌桌时必须重新读取官方token快照() {
        let source = FixedTokenSource(OfficialTokenSnapshot {
            account: AccountFingerprint::new([1; 32]),
            lifetime_tokens: Chips::new(1_000_000),
            observed_at_unix_ms: 10,
        });
        let level = StakeLevel::new(
            "10k-20k",
            Chips::new(10_000),
            Chips::new(20_000),
            Chips::new(800_000),
            Chips::new(2_000_000),
            2,
            6,
        )
        .expect("牌桌级别应当有效");
        let result = JoinTable::new(&source)
            .execute(JoinTableRequest {
                level,
                buy_in: Chips::new(1_200_000),
            })
            .await;

        assert!(matches!(
            result,
            Err(JoinTableError::BuyIn(
                BuyInError::ExceedsOfficialTokens { .. }
            ))
        ));
    }
}
