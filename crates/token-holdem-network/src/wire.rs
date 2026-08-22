use crate::{FriendRoomInvite, HandPrivateMessage, HandPublicMessage};
use serde::{Deserialize, Serialize};
use token_holdem_application::ContentAddress;
use token_holdem_domain::PlayerId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlRequest {
    FriendRoom(FriendRoomInvite),
    TableSession(Vec<u8>),
    HandPublic(HandPublicMessage),
    HandPrivate(HandPrivateMessage),
    Archive(ArchiveRequest),
    FetchArchive { address: ContentAddress },
    ListPlayerArchives { player_id: PlayerId },
    StoreRecovery(RecoveryStoreRequest),
    FetchRecovery { locator: [u8; 32] },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlResponse {
    Accepted,
    Rejected { reason: String },
    Archive(ArchiveResponse),
    ArchiveFetch(ArchiveFetchResponse),
    ArchiveList(ArchiveListResponse),
    RecoveryStored(RecoveryStoreResponse),
    RecoveryFetch(RecoveryFetchResponse),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveRequest {
    pub address: ContentAddress,
    pub content: Vec<u8>,
    pub requested_replication_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveResponse {
    pub address: ContentAddress,
    pub accepted_until_unix_ms: u64,
    pub archive_node_public_key: Vec<u8>,
    pub archive_node_signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveFetchResponse {
    pub address: ContentAddress,
    pub content: Option<Vec<u8>>,
    pub archive_node_public_key: Vec<u8>,
    pub archive_node_signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveListResponse {
    pub player_id: PlayerId,
    pub addresses: Vec<ContentAddress>,
    pub archive_node_public_key: Vec<u8>,
    pub archive_node_signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryStoreRequest {
    pub locator: [u8; 32],
    pub encrypted_envelope: Vec<u8>,
    pub requested_replication_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryStoreResponse {
    pub locator: [u8; 32],
    pub accepted_until_unix_ms: u64,
    pub archive_node_public_key: Vec<u8>,
    pub archive_node_signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryFetchResponse {
    pub locator: [u8; 32],
    pub encrypted_envelope: Option<Vec<u8>>,
    pub archive_node_public_key: Vec<u8>,
    pub archive_node_signature: Vec<u8>,
}
