use crate::{ArchiveFetchResponse, ArchiveRequest, ArchiveResponse};
use async_trait::async_trait;
use futures::future::join_all;
use libp2p::PeerId;
use token_holdem_application::{ContentAddress, RemoteEventStore, ReplicatedContent, StoreError};

#[async_trait]
pub trait ArchiveReplicaClient: Send + Sync {
    async fn put_replica(
        &self,
        peer: PeerId,
        request: ArchiveRequest,
    ) -> Result<ArchiveResponse, StoreError>;

    async fn get_replica(
        &self,
        peer: PeerId,
        address: ContentAddress,
    ) -> Result<ArchiveFetchResponse, StoreError>;
}

pub struct P2pRemoteEventStore<C> {
    client: C,
    archive_peers: Vec<PeerId>,
    requested_replication_seconds: u64,
}

impl<C> P2pRemoteEventStore<C> {
    pub fn new(
        client: C,
        archive_peers: Vec<PeerId>,
        requested_replication_seconds: u64,
    ) -> Result<Self, StoreError> {
        if archive_peers.is_empty() {
            return Err(StoreError::Unavailable("没有配置志愿归档节点".to_owned()));
        }
        if requested_replication_seconds == 0 {
            return Err(StoreError::Rejected("副本保留时间不能为零".to_owned()));
        }
        Ok(Self {
            client,
            archive_peers,
            requested_replication_seconds,
        })
    }
}

#[async_trait]
impl<C: ArchiveReplicaClient> RemoteEventStore for P2pRemoteEventStore<C> {
    async fn put(&self, content: Vec<u8>) -> Result<ReplicatedContent, StoreError> {
        let address = ContentAddress::from_content(&content);
        let responses = join_all(self.archive_peers.iter().copied().map(|peer| {
            self.client.put_replica(
                peer,
                ArchiveRequest {
                    address,
                    content: content.clone(),
                    requested_replication_seconds: self.requested_replication_seconds,
                },
            )
        }))
        .await;
        let mut confirmed_replicas = 0_u16;
        let mut last_error = None;
        for response in responses {
            match response {
                Ok(acceptance) if acceptance.address == address => {
                    confirmed_replicas = confirmed_replicas.saturating_add(1);
                }
                Ok(_) => last_error = Some(StoreError::IntegrityMismatch),
                Err(error) => last_error = Some(error),
            }
        }
        if confirmed_replicas == 0 {
            return Err(last_error.unwrap_or_else(|| {
                StoreError::Unavailable("所有志愿归档节点均未确认副本".to_owned())
            }));
        }
        Ok(ReplicatedContent {
            address,
            confirmed_replicas,
        })
    }

    async fn get(&self, address: ContentAddress) -> Result<Option<Vec<u8>>, StoreError> {
        let responses = join_all(
            self.archive_peers
                .iter()
                .copied()
                .map(|peer| self.client.get_replica(peer, address)),
        )
        .await;
        let mut saw_integrity_failure = false;
        let mut saw_successful_absence = false;
        let mut last_error = None;
        for response in responses {
            match response {
                Ok(response) if response.address != address => saw_integrity_failure = true,
                Ok(response) => match response.content {
                    Some(content) if ContentAddress::from_content(&content) == address => {
                        return Ok(Some(content));
                    }
                    Some(_) => saw_integrity_failure = true,
                    None => saw_successful_absence = true,
                },
                Err(error) => last_error = Some(error),
            }
        }
        if saw_integrity_failure {
            return Err(StoreError::IntegrityMismatch);
        }
        if saw_successful_absence {
            return Ok(None);
        }
        Err(last_error
            .unwrap_or_else(|| StoreError::Unavailable("所有志愿归档节点均不可用".to_owned())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::Mutex};

    struct MemoryReplicaClient {
        content: Mutex<BTreeMap<PeerId, Vec<u8>>>,
    }

    #[async_trait]
    impl ArchiveReplicaClient for MemoryReplicaClient {
        async fn put_replica(
            &self,
            peer: PeerId,
            request: ArchiveRequest,
        ) -> Result<ArchiveResponse, StoreError> {
            self.content.lock().unwrap().insert(peer, request.content);
            Ok(ArchiveResponse {
                address: request.address,
                accepted_until_unix_ms: u64::MAX,
                archive_node_public_key: vec![2; 32],
                archive_node_signature: vec![1],
            })
        }

        async fn get_replica(
            &self,
            peer: PeerId,
            address: ContentAddress,
        ) -> Result<ArchiveFetchResponse, StoreError> {
            Ok(ArchiveFetchResponse {
                address,
                content: self.content.lock().unwrap().get(&peer).cloned(),
                archive_node_public_key: vec![2; 32],
                archive_node_signature: vec![1],
            })
        }
    }

    #[tokio::test]
    async fn 内容必须获得多个远端节点确认且可按地址读取() {
        let peers = vec![PeerId::random(), PeerId::random()];
        let store = P2pRemoteEventStore::new(
            MemoryReplicaClient {
                content: Mutex::new(BTreeMap::new()),
            },
            peers,
            86_400,
        )
        .unwrap();
        let content = b"co-signed receipt".to_vec();
        let replicated = store.put(content.clone()).await.unwrap();
        assert_eq!(replicated.confirmed_replicas, 2);
        assert_eq!(store.get(replicated.address).await.unwrap(), Some(content));
    }
}
