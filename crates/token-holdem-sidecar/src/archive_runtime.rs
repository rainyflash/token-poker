use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use libp2p::{request_response::OutboundRequestId, PeerId};
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use token_holdem_application::ContentAddress;
use token_holdem_domain::PlayerId;
use token_holdem_identity::CoSignedReceipt;
use token_holdem_network::{
    ArchiveFetchResponse, ArchiveListResponse, ArchiveRequest, ArchiveResponse, ControlRequest,
    ControlResponse, NetworkBehaviour, RecoveryFetchResponse, RecoveryStoreRequest,
    RecoveryStoreResponse,
};

const MAX_ARCHIVE_CONTENT_BYTES: usize = 256 * 1_024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_PLAYER_ARCHIVES: usize = 20_000;
const MAX_RECOVERY_ENVELOPE_BYTES: usize = 64 * 1_024;
const MAX_RECOVERY_ENTRIES: usize = 100_000;
const MAX_REPLICATION_SECONDS: u64 = 365 * 24 * 60 * 60;
const DEFAULT_REPLICATION_SECONDS: u64 = 30 * 24 * 60 * 60;
const ARCHIVE_ACCEPT_DOMAIN: &[u8] = b"token-holdem/archive-accept/v1\0";
const ARCHIVE_FETCH_DOMAIN: &[u8] = b"token-holdem/archive-fetch/v1\0";
const ARCHIVE_LIST_DOMAIN: &[u8] = b"token-holdem/archive-list/v1\0";
const RECOVERY_ACCEPT_DOMAIN: &[u8] = b"token-holdem/recovery-accept/v1\0";
const RECOVERY_FETCH_DOMAIN: &[u8] = b"token-holdem/recovery-fetch/v1\0";

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ArchiveEvent {
    ArchiveNodeReady {
        public_key: String,
    },
    ArchivePeersConfigured {
        peers: Vec<String>,
        minimum_confirmed_replicas: u16,
    },
    ReceiptArchivePending {
        address: String,
        required: u16,
    },
    ReceiptArchived {
        address: String,
        confirmed_replicas: u16,
    },
    ReceiptArchiveFailed {
        address: String,
        reason: String,
    },
    ReceiptFetched {
        address: String,
        receipt_id: String,
        hand_number: u64,
    },
    ArchiveIndexReceived {
        player_id: String,
        addresses: u32,
    },
    RecoveryBackupPending {
        locator: String,
        required: u16,
    },
    RecoveryBackupStored {
        locator: String,
        confirmed_replicas: u16,
    },
    RecoveryBackupFailed {
        locator: String,
        reason: String,
    },
    RecoveryBackupFetched {
        locator: String,
    },
    Warning {
        message: String,
    },
}

enum PendingRequest {
    Put { address: ContentAddress },
    Fetch { address: ContentAddress },
    List { player_id: PlayerId },
    RecoveryPut { locator: [u8; 32] },
    RecoveryFetch { locator: [u8; 32] },
}

struct PendingUpload {
    remaining: usize,
    required: u16,
    confirmed: BTreeSet<PeerId>,
    finished: bool,
}

struct PendingFetch {
    remaining: usize,
    finished: bool,
}

struct PendingList {
    remaining: usize,
    received: BTreeSet<ContentAddress>,
}

pub(crate) struct ArchiveRuntime {
    node: Option<VolunteerArchive>,
    peers: Vec<PeerId>,
    minimum_confirmed_replicas: u16,
    pending_requests: HashMap<OutboundRequestId, PendingRequest>,
    uploads: BTreeMap<ContentAddress, PendingUpload>,
    fetches: BTreeMap<ContentAddress, PendingFetch>,
    lists: BTreeMap<PlayerId, PendingList>,
    recovery_uploads: BTreeMap<[u8; 32], PendingUpload>,
    recovery_fetches: BTreeMap<[u8; 32], PendingFetch>,
    recovered_envelopes: BTreeMap<[u8; 32], Vec<Vec<u8>>>,
    pinned_keys: HashMap<PeerId, [u8; 32]>,
    verified_receipts: BTreeMap<ContentAddress, CoSignedReceipt>,
    archived_addresses: BTreeSet<ContentAddress>,
}

impl ArchiveRuntime {
    pub(crate) fn new(directory: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            node: directory.map(VolunteerArchive::open).transpose()?,
            peers: Vec::new(),
            minimum_confirmed_replicas: 1,
            pending_requests: HashMap::new(),
            uploads: BTreeMap::new(),
            fetches: BTreeMap::new(),
            lists: BTreeMap::new(),
            recovery_uploads: BTreeMap::new(),
            recovery_fetches: BTreeMap::new(),
            recovered_envelopes: BTreeMap::new(),
            pinned_keys: HashMap::new(),
            verified_receipts: BTreeMap::new(),
            archived_addresses: BTreeSet::new(),
        })
    }

    pub(crate) fn node_ready_event(&self) -> Option<ArchiveEvent> {
        self.node
            .as_ref()
            .map(|node| ArchiveEvent::ArchiveNodeReady {
                public_key: hex::encode(node.public_key()),
            })
    }

    pub(crate) fn is_configured_peer(&self, peer_id: PeerId) -> bool {
        self.peers.contains(&peer_id)
    }

    pub(crate) fn configure_peers(
        &mut self,
        peers: Vec<PeerId>,
        minimum_confirmed_replicas: u16,
    ) -> Result<ArchiveEvent> {
        let peers = peers
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if peers.is_empty() {
            anyhow::bail!("至少需要一个志愿归档节点")
        }
        if minimum_confirmed_replicas == 0 || usize::from(minimum_confirmed_replicas) > peers.len()
        {
            anyhow::bail!("归档确认副本数必须在 1 到节点总数之间")
        }
        self.peers = peers;
        self.minimum_confirmed_replicas = minimum_confirmed_replicas;
        self.pending_requests.clear();
        self.uploads.clear();
        self.fetches.clear();
        self.lists.clear();
        self.recovery_uploads.clear();
        self.recovery_fetches.clear();
        self.recovered_envelopes.clear();
        self.pinned_keys.clear();
        Ok(ArchiveEvent::ArchivePeersConfigured {
            peers: self.peers.iter().map(ToString::to_string).collect(),
            minimum_confirmed_replicas,
        })
    }

    pub(crate) fn publish_receipt(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        receipt: CoSignedReceipt,
        now_unix_ms: u64,
    ) -> Result<Vec<ArchiveEvent>> {
        receipt
            .verify(now_unix_ms)
            .context("拒绝归档未通过联合签名验证的凭证")?;
        let content = serde_json::to_vec(&receipt).context("无法序列化联合签名凭证")?;
        if content.len() > MAX_ARCHIVE_CONTENT_BYTES {
            anyhow::bail!("联合签名凭证超过归档大小上限")
        }
        let address = ContentAddress::from_content(&content);
        self.verified_receipts.insert(address, receipt);
        if self.peers.is_empty() {
            return Ok(vec![ArchiveEvent::ReceiptArchiveFailed {
                address: address.to_string(),
                reason: "未配置志愿归档节点；凭证仅保留在当前进程内存中".to_owned(),
            }]);
        }

        let request = ArchiveRequest {
            address,
            content,
            requested_replication_seconds: DEFAULT_REPLICATION_SECONDS,
        };
        for peer in &self.peers {
            let request_id = swarm
                .behaviour_mut()
                .control
                .send_request(peer, ControlRequest::Archive(request.clone()));
            self.pending_requests
                .insert(request_id, PendingRequest::Put { address });
        }
        self.uploads.insert(
            address,
            PendingUpload {
                remaining: self.peers.len(),
                required: self.minimum_confirmed_replicas,
                confirmed: BTreeSet::new(),
                finished: false,
            },
        );
        Ok(vec![ArchiveEvent::ReceiptArchivePending {
            address: address.to_string(),
            required: self.minimum_confirmed_replicas,
        }])
    }

    pub(crate) fn sync_player(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        player_id: PlayerId,
    ) -> Result<Vec<ArchiveEvent>> {
        if self.peers.is_empty() {
            return Ok(vec![ArchiveEvent::Warning {
                message: "未配置志愿归档节点，无法同步历史战绩".to_owned(),
            }]);
        }
        for peer in &self.peers {
            let request_id = swarm
                .behaviour_mut()
                .control
                .send_request(peer, ControlRequest::ListPlayerArchives { player_id });
            self.pending_requests
                .insert(request_id, PendingRequest::List { player_id });
        }
        self.lists.insert(
            player_id,
            PendingList {
                remaining: self.peers.len(),
                received: BTreeSet::new(),
            },
        );
        Ok(Vec::new())
    }

    pub(crate) fn fetch_receipt(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        address: ContentAddress,
    ) -> Result<Vec<ArchiveEvent>> {
        if self.verified_receipts.contains_key(&address) {
            return Ok(Vec::new());
        }
        if self.fetches.contains_key(&address) {
            return Ok(Vec::new());
        }
        if self.peers.is_empty() {
            return Ok(vec![ArchiveEvent::Warning {
                message: format!("未配置志愿归档节点，无法读取凭证 {address}"),
            }]);
        }
        self.start_fetch(swarm, address);
        Ok(Vec::new())
    }

    pub(crate) fn publish_recovery(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        locator: [u8; 32],
        encrypted_envelope: Vec<u8>,
    ) -> Result<Vec<ArchiveEvent>> {
        validate_recovery_object(locator, &encrypted_envelope)?;
        if self.peers.is_empty() {
            return Ok(vec![ArchiveEvent::RecoveryBackupFailed {
                locator: hex::encode(locator),
                reason: "未配置志愿归档节点；加密身份恢复包仅存在于当前进程内存中".to_owned(),
            }]);
        }

        self.pending_requests.retain(|_, pending| {
            !matches!(pending, PendingRequest::RecoveryPut { locator: pending_locator } if *pending_locator == locator)
        });
        let request = RecoveryStoreRequest {
            locator,
            encrypted_envelope,
            requested_replication_seconds: MAX_REPLICATION_SECONDS,
        };
        for peer in &self.peers {
            let request_id = swarm
                .behaviour_mut()
                .control
                .send_request(peer, ControlRequest::StoreRecovery(request.clone()));
            self.pending_requests
                .insert(request_id, PendingRequest::RecoveryPut { locator });
        }
        self.recovery_uploads.insert(
            locator,
            PendingUpload {
                remaining: self.peers.len(),
                required: self.minimum_confirmed_replicas,
                confirmed: BTreeSet::new(),
                finished: false,
            },
        );
        Ok(vec![ArchiveEvent::RecoveryBackupPending {
            locator: hex::encode(locator),
            required: self.minimum_confirmed_replicas,
        }])
    }

    pub(crate) fn fetch_recovery(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        locator: [u8; 32],
    ) -> Result<Vec<ArchiveEvent>> {
        validate_recovery_locator(locator)?;
        if self.peers.is_empty() {
            return Ok(vec![ArchiveEvent::RecoveryBackupFailed {
                locator: hex::encode(locator),
                reason: "未配置志愿归档节点，无法远端恢复身份".to_owned(),
            }]);
        }
        if self
            .recovery_fetches
            .get(&locator)
            .is_some_and(|fetch| !fetch.finished)
        {
            return Ok(Vec::new());
        }
        self.recovery_fetches.remove(&locator);
        self.recovered_envelopes.remove(&locator);
        for peer in &self.peers {
            let request_id = swarm
                .behaviour_mut()
                .control
                .send_request(peer, ControlRequest::FetchRecovery { locator });
            self.pending_requests
                .insert(request_id, PendingRequest::RecoveryFetch { locator });
        }
        self.recovery_fetches.insert(
            locator,
            PendingFetch {
                remaining: self.peers.len(),
                finished: false,
            },
        );
        Ok(Vec::new())
    }

    pub(crate) fn take_recovered_envelope(&mut self, locator: [u8; 32]) -> Option<Vec<u8>> {
        let candidates = self.recovered_envelopes.get_mut(&locator)?;
        if candidates.is_empty() {
            return None;
        }
        let envelope = candidates.remove(0);
        if candidates.is_empty() {
            self.recovered_envelopes.remove(&locator);
        }
        Some(envelope)
    }

    pub(crate) fn recovery_fetch_is_exhausted(&self, locator: [u8; 32]) -> bool {
        self.recovery_fetches
            .get(&locator)
            .is_none_or(|fetch| fetch.remaining == 0)
    }

    pub(crate) fn cancel_recovery_fetch(&mut self, locator: [u8; 32]) {
        self.pending_requests.retain(|_, pending| {
            !matches!(pending, PendingRequest::RecoveryFetch { locator: pending_locator } if *pending_locator == locator)
        });
        self.recovery_fetches.remove(&locator);
        self.recovered_envelopes.remove(&locator);
    }

    pub(crate) fn serve_request(
        &self,
        request: ControlRequest,
        now_unix_ms: u64,
    ) -> Result<ControlResponse> {
        let node = self
            .node
            .as_ref()
            .context("当前 sidecar 未启用志愿归档模式")?;
        match request {
            ControlRequest::Archive(request) => {
                node.put(request, now_unix_ms).map(ControlResponse::Archive)
            }
            ControlRequest::FetchArchive { address } => {
                node.fetch(address).map(ControlResponse::ArchiveFetch)
            }
            ControlRequest::ListPlayerArchives { player_id } => {
                node.list(player_id).map(ControlResponse::ArchiveList)
            }
            ControlRequest::StoreRecovery(request) => node
                .put_recovery(request, now_unix_ms)
                .map(ControlResponse::RecoveryStored),
            ControlRequest::FetchRecovery { locator } => node
                .fetch_recovery(locator)
                .map(ControlResponse::RecoveryFetch),
            _ => anyhow::bail!("请求不是归档协议消息"),
        }
    }

    pub(crate) fn handle_response(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        request_id: OutboundRequestId,
        peer: PeerId,
        response: ControlResponse,
        now_unix_ms: u64,
    ) -> Result<Option<Vec<ArchiveEvent>>> {
        let Some(pending) = self.pending_requests.remove(&request_id) else {
            return Ok(None);
        };
        let events = match (pending, response) {
            (PendingRequest::Put { address }, ControlResponse::Archive(response)) => {
                self.handle_put_response(peer, address, response, now_unix_ms)?
            }
            (PendingRequest::Fetch { address }, ControlResponse::ArchiveFetch(response)) => {
                self.handle_fetch_response(peer, address, response, now_unix_ms)?
            }
            (PendingRequest::List { player_id }, ControlResponse::ArchiveList(response)) => {
                self.handle_list_response(swarm, peer, player_id, response)?
            }
            (
                PendingRequest::RecoveryPut { locator },
                ControlResponse::RecoveryStored(response),
            ) => self.handle_recovery_put_response(peer, locator, response, now_unix_ms)?,
            (
                PendingRequest::RecoveryFetch { locator },
                ControlResponse::RecoveryFetch(response),
            ) => self.handle_recovery_fetch_response(peer, locator, response)?,
            (PendingRequest::Put { address }, ControlResponse::Rejected { reason }) => {
                self.finish_put_attempt(address, None, Some(reason))?
            }
            (PendingRequest::Fetch { address }, ControlResponse::Rejected { reason }) => {
                self.finish_fetch_attempt(address, Some(reason))?
            }
            (PendingRequest::List { player_id }, ControlResponse::Rejected { reason }) => {
                self.finish_list_attempt(player_id, Some(reason))?
            }
            (PendingRequest::RecoveryPut { locator }, ControlResponse::Rejected { reason }) => {
                self.finish_recovery_put_attempt(locator, None, Some(reason))?
            }
            (PendingRequest::RecoveryFetch { locator }, ControlResponse::Rejected { reason }) => {
                self.finish_recovery_fetch_attempt(locator, Some(reason))?
            }
            (pending, _) => self.finish_mismatched_response(pending)?,
        };
        Ok(Some(events))
    }

    pub(crate) fn handle_failure(
        &mut self,
        request_id: OutboundRequestId,
        error: impl ToString,
    ) -> Result<Option<Vec<ArchiveEvent>>> {
        let Some(pending) = self.pending_requests.remove(&request_id) else {
            return Ok(None);
        };
        let reason = error.to_string();
        let events = match pending {
            PendingRequest::Put { address } => {
                self.finish_put_attempt(address, None, Some(reason))?
            }
            PendingRequest::Fetch { address } => {
                self.finish_fetch_attempt(address, Some(reason))?
            }
            PendingRequest::List { player_id } => {
                self.finish_list_attempt(player_id, Some(reason))?
            }
            PendingRequest::RecoveryPut { locator } => {
                self.finish_recovery_put_attempt(locator, None, Some(reason))?
            }
            PendingRequest::RecoveryFetch { locator } => {
                self.finish_recovery_fetch_attempt(locator, Some(reason))?
            }
        };
        Ok(Some(events))
    }

    pub(crate) fn verified_receipts(&self) -> &BTreeMap<ContentAddress, CoSignedReceipt> {
        &self.verified_receipts
    }

    pub(crate) fn is_archived(&self, address: ContentAddress) -> bool {
        self.archived_addresses.contains(&address)
    }

    fn handle_put_response(
        &mut self,
        peer: PeerId,
        address: ContentAddress,
        response: ArchiveResponse,
        now_unix_ms: u64,
    ) -> Result<Vec<ArchiveEvent>> {
        if response.address != address || response.accepted_until_unix_ms <= now_unix_ms {
            return self.finish_put_attempt(
                address,
                None,
                Some("归档确认的内容地址或保留期限无效".to_owned()),
            );
        }
        if let Err(error) = self.verify_node_signature(
            peer,
            &response.archive_node_public_key,
            &acceptance_signing_bytes(response.address, response.accepted_until_unix_ms),
            &response.archive_node_signature,
        ) {
            return self.finish_put_attempt(address, None, Some(error.to_string()));
        }
        self.finish_put_attempt(address, Some(peer), None)
    }

    fn handle_fetch_response(
        &mut self,
        peer: PeerId,
        address: ContentAddress,
        response: ArchiveFetchResponse,
        now_unix_ms: u64,
    ) -> Result<Vec<ArchiveEvent>> {
        if response.address != address {
            return self.finish_fetch_attempt(address, Some("归档读取响应地址不匹配".to_owned()));
        }
        if let Err(error) = self.verify_node_signature(
            peer,
            &response.archive_node_public_key,
            &fetch_signing_bytes(response.address, response.content.as_deref()),
            &response.archive_node_signature,
        ) {
            return self.finish_fetch_attempt(address, Some(error.to_string()));
        }
        let Some(content) = response.content else {
            return self.finish_fetch_attempt(address, None);
        };
        if content.len() > MAX_ARCHIVE_CONTENT_BYTES
            || ContentAddress::from_content(&content) != address
        {
            return self
                .finish_fetch_attempt(address, Some("远端凭证没有通过内容哈希校验".to_owned()));
        }
        let receipt: CoSignedReceipt = match serde_json::from_slice(&content) {
            Ok(receipt) => receipt,
            Err(error) => {
                return self.finish_fetch_attempt(
                    address,
                    Some(format!("远端内容不是联合签名凭证：{error}")),
                )
            }
        };
        if let Err(error) = receipt.verify(now_unix_ms) {
            return self
                .finish_fetch_attempt(address, Some(format!("远端联合签名凭证验证失败：{error}")));
        }
        let receipt_id = hex::encode(receipt.receipt.id().as_bytes());
        let hand_number = receipt.receipt.hand_number();
        self.verified_receipts.insert(address, receipt);
        self.archived_addresses.insert(address);
        if let Some(fetch) = self.fetches.get_mut(&address) {
            fetch.finished = true;
        }
        Ok(vec![ArchiveEvent::ReceiptFetched {
            address: address.to_string(),
            receipt_id,
            hand_number,
        }])
    }

    fn handle_list_response(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        peer: PeerId,
        player_id: PlayerId,
        mut response: ArchiveListResponse,
    ) -> Result<Vec<ArchiveEvent>> {
        if response.player_id != player_id || response.addresses.len() > MAX_PLAYER_ARCHIVES {
            return self
                .finish_list_attempt(player_id, Some("归档索引响应的玩家或条目数无效".to_owned()));
        }
        response.addresses.sort_unstable();
        response.addresses.dedup();
        if let Err(error) = self.verify_node_signature(
            peer,
            &response.archive_node_public_key,
            &list_signing_bytes(player_id, &response.addresses),
            &response.archive_node_signature,
        ) {
            return self.finish_list_attempt(player_id, Some(error.to_string()));
        }
        let mut new_addresses = Vec::new();
        if let Some(list) = self.lists.get_mut(&player_id) {
            list.remaining = list.remaining.saturating_sub(1);
            for address in response.addresses {
                if list.received.insert(address)
                    && !self.verified_receipts.contains_key(&address)
                    && !self.fetches.contains_key(&address)
                {
                    new_addresses.push(address);
                }
            }
        }
        for address in new_addresses {
            self.start_fetch(swarm, address);
        }
        let count = self
            .lists
            .get(&player_id)
            .map_or(0, |list| list.received.len());
        Ok(vec![ArchiveEvent::ArchiveIndexReceived {
            player_id: player_id.to_string(),
            addresses: u32::try_from(count).unwrap_or(u32::MAX),
        }])
    }

    fn handle_recovery_put_response(
        &mut self,
        peer: PeerId,
        locator: [u8; 32],
        response: RecoveryStoreResponse,
        now_unix_ms: u64,
    ) -> Result<Vec<ArchiveEvent>> {
        if response.locator != locator || response.accepted_until_unix_ms <= now_unix_ms {
            return self.finish_recovery_put_attempt(
                locator,
                None,
                Some("恢复包确认的定位符或保留期限无效".to_owned()),
            );
        }
        if let Err(error) = self.verify_node_signature(
            peer,
            &response.archive_node_public_key,
            &recovery_acceptance_signing_bytes(locator, response.accepted_until_unix_ms),
            &response.archive_node_signature,
        ) {
            return self.finish_recovery_put_attempt(locator, None, Some(error.to_string()));
        }
        self.finish_recovery_put_attempt(locator, Some(peer), None)
    }

    fn handle_recovery_fetch_response(
        &mut self,
        peer: PeerId,
        locator: [u8; 32],
        response: RecoveryFetchResponse,
    ) -> Result<Vec<ArchiveEvent>> {
        if response.locator != locator {
            return self.finish_recovery_fetch_attempt(
                locator,
                Some("恢复包读取响应的定位符不匹配".to_owned()),
            );
        }
        if let Err(error) = self.verify_node_signature(
            peer,
            &response.archive_node_public_key,
            &recovery_fetch_signing_bytes(locator, response.encrypted_envelope.as_deref()),
            &response.archive_node_signature,
        ) {
            return self.finish_recovery_fetch_attempt(locator, Some(error.to_string()));
        }
        let Some(encrypted_envelope) = response.encrypted_envelope else {
            return self.finish_recovery_fetch_attempt(locator, None);
        };
        if let Err(error) = validate_recovery_object(locator, &encrypted_envelope) {
            return self.finish_recovery_fetch_attempt(locator, Some(error.to_string()));
        }
        self.recovered_envelopes
            .entry(locator)
            .or_default()
            .push(encrypted_envelope);
        let fetch = self
            .recovery_fetches
            .get_mut(&locator)
            .context("恢复包读取状态缺失")?;
        fetch.remaining = fetch.remaining.saturating_sub(1);
        fetch.finished = fetch.remaining == 0;
        Ok(vec![ArchiveEvent::RecoveryBackupFetched {
            locator: hex::encode(locator),
        }])
    }

    fn finish_put_attempt(
        &mut self,
        address: ContentAddress,
        confirmed_peer: Option<PeerId>,
        reason: Option<String>,
    ) -> Result<Vec<ArchiveEvent>> {
        let upload = self.uploads.get_mut(&address).context("归档上传状态缺失")?;
        upload.remaining = upload.remaining.saturating_sub(1);
        if let Some(peer) = confirmed_peer {
            upload.confirmed.insert(peer);
        }
        if !upload.finished && upload.confirmed.len() >= usize::from(upload.required) {
            upload.finished = true;
            self.archived_addresses.insert(address);
            return Ok(vec![ArchiveEvent::ReceiptArchived {
                address: address.to_string(),
                confirmed_replicas: u16::try_from(upload.confirmed.len()).unwrap_or(u16::MAX),
            }]);
        }
        if !upload.finished && upload.remaining == 0 {
            upload.finished = true;
            return Ok(vec![ArchiveEvent::ReceiptArchiveFailed {
                address: address.to_string(),
                reason: reason.unwrap_or_else(|| {
                    format!(
                        "远端确认不足：需要 {}，实际 {}",
                        upload.required,
                        upload.confirmed.len()
                    )
                }),
            }]);
        }
        Ok(Vec::new())
    }

    fn finish_fetch_attempt(
        &mut self,
        address: ContentAddress,
        reason: Option<String>,
    ) -> Result<Vec<ArchiveEvent>> {
        let fetch = self.fetches.get_mut(&address).context("归档读取状态缺失")?;
        fetch.remaining = fetch.remaining.saturating_sub(1);
        if !fetch.finished && fetch.remaining == 0 {
            fetch.finished = true;
            return Ok(vec![ArchiveEvent::Warning {
                message: reason.unwrap_or_else(|| format!("所有归档节点均未找到凭证 {address}")),
            }]);
        }
        Ok(Vec::new())
    }

    fn finish_list_attempt(
        &mut self,
        player_id: PlayerId,
        reason: Option<String>,
    ) -> Result<Vec<ArchiveEvent>> {
        let list = self.lists.get_mut(&player_id).context("归档索引状态缺失")?;
        list.remaining = list.remaining.saturating_sub(1);
        if list.remaining == 0 && list.received.is_empty() {
            return Ok(vec![ArchiveEvent::Warning {
                message: reason.unwrap_or_else(|| "远端没有该玩家的历史凭证索引".to_owned()),
            }]);
        }
        Ok(Vec::new())
    }

    fn finish_recovery_put_attempt(
        &mut self,
        locator: [u8; 32],
        confirmed_peer: Option<PeerId>,
        reason: Option<String>,
    ) -> Result<Vec<ArchiveEvent>> {
        let upload = self
            .recovery_uploads
            .get_mut(&locator)
            .context("恢复包上传状态缺失")?;
        upload.remaining = upload.remaining.saturating_sub(1);
        if let Some(peer) = confirmed_peer {
            upload.confirmed.insert(peer);
        }
        if upload.confirmed.len() >= usize::from(upload.required) && confirmed_peer.is_some() {
            upload.finished = true;
            return Ok(vec![ArchiveEvent::RecoveryBackupStored {
                locator: hex::encode(locator),
                confirmed_replicas: u16::try_from(upload.confirmed.len()).unwrap_or(u16::MAX),
            }]);
        }
        if !upload.finished && upload.remaining == 0 {
            upload.finished = true;
            return Ok(vec![ArchiveEvent::RecoveryBackupFailed {
                locator: hex::encode(locator),
                reason: reason.unwrap_or_else(|| {
                    format!(
                        "远端恢复包确认不足：需要 {}，实际 {}",
                        upload.required,
                        upload.confirmed.len()
                    )
                }),
            }]);
        }
        Ok(Vec::new())
    }

    fn finish_recovery_fetch_attempt(
        &mut self,
        locator: [u8; 32],
        reason: Option<String>,
    ) -> Result<Vec<ArchiveEvent>> {
        let fetch = self
            .recovery_fetches
            .get_mut(&locator)
            .context("恢复包读取状态缺失")?;
        fetch.remaining = fetch.remaining.saturating_sub(1);
        if !fetch.finished && fetch.remaining == 0 {
            fetch.finished = true;
            if self
                .recovered_envelopes
                .get(&locator)
                .is_some_and(|candidates| !candidates.is_empty())
            {
                return Ok(Vec::new());
            }
            return Ok(vec![ArchiveEvent::RecoveryBackupFailed {
                locator: hex::encode(locator),
                reason: reason.unwrap_or_else(|| "所有归档节点均未找到加密身份恢复包".to_owned()),
            }]);
        }
        Ok(Vec::new())
    }

    fn finish_mismatched_response(&mut self, pending: PendingRequest) -> Result<Vec<ArchiveEvent>> {
        let reason = "归档节点返回了与请求类型不匹配的响应".to_owned();
        match pending {
            PendingRequest::Put { address } => self.finish_put_attempt(address, None, Some(reason)),
            PendingRequest::Fetch { address } => self.finish_fetch_attempt(address, Some(reason)),
            PendingRequest::List { player_id } => self.finish_list_attempt(player_id, Some(reason)),
            PendingRequest::RecoveryPut { locator } => {
                self.finish_recovery_put_attempt(locator, None, Some(reason))
            }
            PendingRequest::RecoveryFetch { locator } => {
                self.finish_recovery_fetch_attempt(locator, Some(reason))
            }
        }
    }

    fn start_fetch(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        address: ContentAddress,
    ) {
        for peer in &self.peers {
            let request_id = swarm
                .behaviour_mut()
                .control
                .send_request(peer, ControlRequest::FetchArchive { address });
            self.pending_requests
                .insert(request_id, PendingRequest::Fetch { address });
        }
        self.fetches.insert(
            address,
            PendingFetch {
                remaining: self.peers.len(),
                finished: false,
            },
        );
    }

    fn verify_node_signature(
        &mut self,
        peer: PeerId,
        public_key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<()> {
        let public_key: [u8; 32] = public_key
            .try_into()
            .map_err(|_| anyhow::anyhow!("归档节点公钥长度无效"))?;
        if self
            .pinned_keys
            .get(&peer)
            .is_some_and(|pinned| pinned != &public_key)
        {
            anyhow::bail!("同一归档 PeerId 在会话中更换了签名公钥")
        }
        let signature: [u8; 64] = signature
            .try_into()
            .map_err(|_| anyhow::anyhow!("归档节点签名长度无效"))?;
        VerifyingKey::from_bytes(&public_key)
            .context("归档节点公钥无效")?
            .verify(message, &Signature::from_bytes(&signature))
            .context("归档节点响应签名无效")?;
        self.pinned_keys.insert(peer, public_key);
        Ok(())
    }
}

struct VolunteerArchive {
    root: PathBuf,
    signer: SigningKey,
}

impl VolunteerArchive {
    fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)
            .with_context(|| format!("无法创建志愿归档目录：{}", root.display()))?;
        let root = root
            .canonicalize()
            .with_context(|| format!("无法解析志愿归档目录：{}", root.display()))?;
        if !root.is_dir() {
            anyhow::bail!("志愿归档路径不是目录：{}", root.display())
        }
        fs::create_dir_all(root.join("objects")).context("无法创建归档对象目录")?;
        fs::create_dir_all(root.join("players")).context("无法创建归档玩家索引目录")?;
        fs::create_dir_all(root.join("recovery")).context("无法创建身份恢复包目录")?;
        let key_path = root.join("archive-signing-key");
        let signer = if key_path.exists() {
            let bytes = fs::read(&key_path).context("无法读取归档节点签名密钥")?;
            let seed: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("归档节点签名密钥长度无效"))?;
            SigningKey::from_bytes(&seed)
        } else {
            let signer = SigningKey::generate(&mut OsRng);
            atomic_write_if_absent(&key_path, signer.as_bytes())?;
            signer
        };
        Ok(Self { root, signer })
    }

    fn public_key(&self) -> [u8; 32] {
        self.signer.verifying_key().to_bytes()
    }

    fn put(&self, request: ArchiveRequest, now_unix_ms: u64) -> Result<ArchiveResponse> {
        if request.content.is_empty() || request.content.len() > MAX_ARCHIVE_CONTENT_BYTES {
            anyhow::bail!("归档内容必须为 1 字节到 256 KiB")
        }
        if request.requested_replication_seconds == 0
            || request.requested_replication_seconds > MAX_REPLICATION_SECONDS
        {
            anyhow::bail!("请求的归档保留期超出允许范围")
        }
        if ContentAddress::from_content(&request.content) != request.address {
            anyhow::bail!("归档请求的内容地址不匹配")
        }
        let receipt: CoSignedReceipt =
            serde_json::from_slice(&request.content).context("归档内容不是联合签名凭证")?;
        receipt
            .verify(now_unix_ms)
            .context("归档内容没有通过联合签名验证")?;

        let object_path = self.object_path(request.address);
        if !object_path.exists()
            && fs::read_dir(self.root.join("objects"))
                .context("无法统计归档对象")?
                .take(MAX_ARCHIVE_ENTRIES + 1)
                .count()
                >= MAX_ARCHIVE_ENTRIES
        {
            anyhow::bail!("归档节点已达到最大条目数")
        }
        atomic_write_if_absent(&object_path, &request.content)?;
        for outcome in receipt.receipt.outcomes() {
            let player_directory = self
                .root
                .join("players")
                .join(outcome.player_id.to_string());
            fs::create_dir_all(&player_directory).context("无法创建玩家归档索引目录")?;
            atomic_write_if_absent(&player_directory.join(request.address.to_string()), b"1")?;
        }

        let accepted_until_unix_ms = now_unix_ms
            .checked_add(request.requested_replication_seconds.saturating_mul(1_000))
            .context("归档保留截止时间溢出")?;
        let signature = self
            .signer
            .sign(&acceptance_signing_bytes(
                request.address,
                accepted_until_unix_ms,
            ))
            .to_bytes()
            .to_vec();
        Ok(ArchiveResponse {
            address: request.address,
            accepted_until_unix_ms,
            archive_node_public_key: self.public_key().to_vec(),
            archive_node_signature: signature,
        })
    }

    fn fetch(&self, address: ContentAddress) -> Result<ArchiveFetchResponse> {
        let path = self.object_path(address);
        let content = if path.exists() {
            let content = fs::read(path).context("无法读取归档对象")?;
            if content.len() > MAX_ARCHIVE_CONTENT_BYTES
                || ContentAddress::from_content(&content) != address
            {
                anyhow::bail!("归档对象未通过内容哈希校验")
            }
            Some(content)
        } else {
            None
        };
        let signature = self
            .signer
            .sign(&fetch_signing_bytes(address, content.as_deref()))
            .to_bytes()
            .to_vec();
        Ok(ArchiveFetchResponse {
            address,
            content,
            archive_node_public_key: self.public_key().to_vec(),
            archive_node_signature: signature,
        })
    }

    fn list(&self, player_id: PlayerId) -> Result<ArchiveListResponse> {
        let directory = self.root.join("players").join(player_id.to_string());
        let mut addresses = Vec::new();
        if directory.exists() {
            for entry in fs::read_dir(directory)
                .context("无法读取玩家归档索引")?
                .take(MAX_PLAYER_ARCHIVES + 1)
            {
                if addresses.len() >= MAX_PLAYER_ARCHIVES {
                    anyhow::bail!("玩家归档索引超过安全上限")
                }
                let name = entry
                    .context("无法读取玩家归档索引条目")?
                    .file_name()
                    .to_string_lossy()
                    .into_owned();
                addresses.push(parse_content_address(&name)?);
            }
        }
        addresses.sort_unstable();
        addresses.dedup();
        let signature = self
            .signer
            .sign(&list_signing_bytes(player_id, &addresses))
            .to_bytes()
            .to_vec();
        Ok(ArchiveListResponse {
            player_id,
            addresses,
            archive_node_public_key: self.public_key().to_vec(),
            archive_node_signature: signature,
        })
    }

    fn put_recovery(
        &self,
        request: RecoveryStoreRequest,
        now_unix_ms: u64,
    ) -> Result<RecoveryStoreResponse> {
        validate_recovery_object(request.locator, &request.encrypted_envelope)?;
        if request.requested_replication_seconds == 0
            || request.requested_replication_seconds > MAX_REPLICATION_SECONDS
        {
            anyhow::bail!("请求的身份恢复包保留期超出允许范围")
        }
        let path = self.recovery_path(request.locator);
        if !path.exists()
            && fs::read_dir(self.root.join("recovery"))
                .context("无法统计身份恢复包")?
                .take(MAX_RECOVERY_ENTRIES + 1)
                .count()
                >= MAX_RECOVERY_ENTRIES
        {
            anyhow::bail!("归档节点已达到身份恢复包最大条目数")
        }
        atomic_write_if_absent(&path, &request.encrypted_envelope)?;
        let accepted_until_unix_ms = now_unix_ms
            .checked_add(request.requested_replication_seconds.saturating_mul(1_000))
            .context("身份恢复包保留截止时间溢出")?;
        let signature = self
            .signer
            .sign(&recovery_acceptance_signing_bytes(
                request.locator,
                accepted_until_unix_ms,
            ))
            .to_bytes()
            .to_vec();
        Ok(RecoveryStoreResponse {
            locator: request.locator,
            accepted_until_unix_ms,
            archive_node_public_key: self.public_key().to_vec(),
            archive_node_signature: signature,
        })
    }

    fn fetch_recovery(&self, locator: [u8; 32]) -> Result<RecoveryFetchResponse> {
        validate_recovery_locator(locator)?;
        let path = self.recovery_path(locator);
        let encrypted_envelope = if path.exists() {
            let content = fs::read(path).context("无法读取加密身份恢复包")?;
            validate_recovery_object(locator, &content)?;
            Some(content)
        } else {
            None
        };
        let signature = self
            .signer
            .sign(&recovery_fetch_signing_bytes(
                locator,
                encrypted_envelope.as_deref(),
            ))
            .to_bytes()
            .to_vec();
        Ok(RecoveryFetchResponse {
            locator,
            encrypted_envelope,
            archive_node_public_key: self.public_key().to_vec(),
            archive_node_signature: signature,
        })
    }

    fn object_path(&self, address: ContentAddress) -> PathBuf {
        self.root.join("objects").join(address.to_string())
    }

    fn recovery_path(&self, locator: [u8; 32]) -> PathBuf {
        self.root.join("recovery").join(hex::encode(locator))
    }
}

pub(crate) fn parse_content_address(value: &str) -> Result<ContentAddress> {
    let decoded = hex::decode(value.trim()).context("内容地址不是合法十六进制")?;
    let bytes: [u8; 32] = decoded
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("内容地址必须为 32 字节"))?;
    Ok(ContentAddress::new(bytes))
}

fn atomic_write_if_absent(path: &Path, content: &[u8]) -> Result<()> {
    if path.exists() {
        let existing =
            fs::read(path).with_context(|| format!("无法读取现有对象：{}", path.display()))?;
        if existing != content {
            anyhow::bail!("内容地址发生不可接受的磁盘冲突：{}", path.display())
        }
        return Ok(());
    }
    let parent = path.parent().context("原子写入目标缺少父目录")?;
    fs::create_dir_all(parent).context("无法创建原子写入父目录")?;
    let mut nonce = [0_u8; 8];
    OsRng.fill_bytes(&mut nonce);
    let temporary = parent.join(format!(".tmp-{}", hex::encode(nonce)));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("无法创建临时归档对象：{}", temporary.display()))?;
    file.write_all(content).context("无法写入临时归档对象")?;
    file.sync_all().context("无法同步临时归档对象")?;
    drop(file);
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) if path.exists() => {
            let existing = fs::read(path).context("无法读取并发写入的归档对象")?;
            fs::remove_file(&temporary).context("无法清理临时归档对象")?;
            if existing == content {
                Ok(())
            } else {
                anyhow::bail!("并发归档写入发生内容冲突：{error}")
            }
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error).context("无法提交原子归档写入")
        }
    }
}

fn validate_recovery_locator(locator: [u8; 32]) -> Result<()> {
    if locator == [0_u8; 32] {
        anyhow::bail!("身份恢复定位符不能全为零")
    }
    Ok(())
}

fn validate_recovery_object(locator: [u8; 32], encrypted_envelope: &[u8]) -> Result<()> {
    validate_recovery_locator(locator)?;
    if encrypted_envelope.is_empty() || encrypted_envelope.len() > MAX_RECOVERY_ENVELOPE_BYTES {
        anyhow::bail!("加密身份恢复包必须为 1 字节到 64 KiB")
    }
    Ok(())
}

fn acceptance_signing_bytes(address: ContentAddress, accepted_until_unix_ms: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(ARCHIVE_ACCEPT_DOMAIN.len() + 40);
    bytes.extend_from_slice(ARCHIVE_ACCEPT_DOMAIN);
    bytes.extend_from_slice(address.as_bytes());
    bytes.extend_from_slice(&accepted_until_unix_ms.to_be_bytes());
    bytes
}

fn fetch_signing_bytes(address: ContentAddress, content: Option<&[u8]>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(ARCHIVE_FETCH_DOMAIN.len() + 65);
    bytes.extend_from_slice(ARCHIVE_FETCH_DOMAIN);
    bytes.extend_from_slice(address.as_bytes());
    match content {
        Some(content) => {
            bytes.push(1);
            bytes.extend_from_slice(blake3::hash(content).as_bytes());
        }
        None => bytes.push(0),
    }
    bytes
}

fn list_signing_bytes(player_id: PlayerId, addresses: &[ContentAddress]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(ARCHIVE_LIST_DOMAIN.len() + 36 + addresses.len() * 32);
    bytes.extend_from_slice(ARCHIVE_LIST_DOMAIN);
    bytes.extend_from_slice(player_id.as_bytes());
    bytes.extend_from_slice(&(addresses.len() as u32).to_be_bytes());
    for address in addresses {
        bytes.extend_from_slice(address.as_bytes());
    }
    bytes
}

fn recovery_acceptance_signing_bytes(locator: [u8; 32], accepted_until_unix_ms: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(RECOVERY_ACCEPT_DOMAIN.len() + 40);
    bytes.extend_from_slice(RECOVERY_ACCEPT_DOMAIN);
    bytes.extend_from_slice(&locator);
    bytes.extend_from_slice(&accepted_until_unix_ms.to_be_bytes());
    bytes
}

fn recovery_fetch_signing_bytes(locator: [u8; 32], content: Option<&[u8]>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(RECOVERY_FETCH_DOMAIN.len() + 65);
    bytes.extend_from_slice(RECOVERY_FETCH_DOMAIN);
    bytes.extend_from_slice(&locator);
    match content {
        Some(content) => {
            bytes.push(1);
            bytes.extend_from_slice(blake3::hash(content).as_bytes());
        }
        None => bytes.push(0),
    }
    bytes
}
