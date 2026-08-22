use crate::network_address::{
    is_public_direct_address, is_publishable_address, should_initiate_peer_dial,
};
use anyhow::{Context, Result};
use libp2p::{
    multiaddr::Protocol,
    rendezvous::{self, Cookie, Namespace},
    swarm::dial_opts::{DialOpts, PeerCondition},
    Multiaddr, PeerId, Swarm,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
use token_holdem_network::{
    add_bootstrap_address, NetworkBehaviour, RENDEZVOUS_REGISTRATION_TTL_SECONDS,
};

const MAX_RENDEZVOUS_NODES: usize = 8;
const MAX_DISCOVERED_PEERS: usize = 256;
const MAX_ADDRESSES_PER_PEER: usize = 8;
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(20);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum DiscoveryEvent {
    DiscoveryConfigured {
        nodes: Vec<String>,
        namespace: String,
    },
    AdvertisedAddressAdded {
        address: String,
    },
    RendezvousRegistered {
        node: String,
        address: String,
        namespace: String,
        ttl_seconds: u64,
    },
    RendezvousCandidateAdded {
        node: String,
        address: String,
        source: &'static str,
    },
    PeersDiscovered {
        node: String,
        peers: u16,
    },
    Warning {
        message: String,
    },
}

struct RendezvousNode {
    address: Multiaddr,
    namespace: Namespace,
    cookie: Option<Cookie>,
    last_discovery: Option<Instant>,
    registration_pending: bool,
    register_again_at: Option<Instant>,
    last_dial_attempt: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct DiscoveryRuntime {
    nodes: BTreeMap<PeerId, RendezvousNode>,
    discovered_peers: BTreeSet<PeerId>,
    namespace: Option<Namespace>,
}

impl DiscoveryRuntime {
    pub(crate) fn configure(
        &mut self,
        swarm: &mut Swarm<NetworkBehaviour>,
        addresses: Vec<String>,
        namespace: String,
    ) -> Result<Vec<DiscoveryEvent>> {
        if addresses.is_empty() || addresses.len() > MAX_RENDEZVOUS_NODES {
            anyhow::bail!("社区发现节点数量必须为 1 到 {MAX_RENDEZVOUS_NODES}")
        }
        let namespace = normalize_namespace(namespace)?;
        let namespace = Namespace::new(namespace).context("发现命名空间过长")?;
        self.namespace = Some(namespace.clone());
        for peer_id in &self.discovered_peers {
            swarm
                .behaviour_mut()
                .gossipsub
                .remove_explicit_peer(peer_id);
        }
        self.discovered_peers.clear();
        let mut parsed = BTreeMap::new();
        for raw in addresses {
            let address = raw
                .parse::<Multiaddr>()
                .with_context(|| format!("社区发现节点地址无效：{raw}"))?;
            let peer_id = peer_id_from_address(&address)
                .with_context(|| format!("社区发现节点地址缺少 /p2p/<PeerId>：{raw}"))?;
            parsed.entry(peer_id).or_insert(address);
        }
        self.nodes.clear();
        for (peer_id, address) in &parsed {
            add_bootstrap_address(swarm, *peer_id, address.clone());
            self.nodes.insert(
                *peer_id,
                RendezvousNode {
                    address: address.clone(),
                    namespace: namespace.clone(),
                    cookie: None,
                    last_discovery: None,
                    registration_pending: false,
                    register_again_at: None,
                    last_dial_attempt: None,
                },
            );
            if !swarm.is_connected(peer_id) {
                self.try_dial(swarm, *peer_id);
            }
        }
        let _ = swarm.behaviour_mut().kademlia.bootstrap();
        let nodes = parsed.keys().map(ToString::to_string).collect::<Vec<_>>();
        for peer_id in parsed.keys().copied().collect::<Vec<_>>() {
            if swarm.is_connected(&peer_id) {
                self.request_discovery(swarm, peer_id);
                self.try_register(swarm, peer_id);
            }
        }
        Ok(vec![DiscoveryEvent::DiscoveryConfigured {
            nodes,
            namespace: namespace.to_string(),
        }])
    }

    pub(crate) fn observe_rendezvous_service(
        &mut self,
        swarm: &mut Swarm<NetworkBehaviour>,
        peer_id: PeerId,
        addresses: &[Multiaddr],
    ) -> Vec<DiscoveryEvent> {
        if peer_id == *swarm.local_peer_id()
            || self.nodes.contains_key(&peer_id)
            || self.nodes.len() >= MAX_RENDEZVOUS_NODES
        {
            return Vec::new();
        }
        let Some(namespace) = self.namespace.clone() else {
            return Vec::new();
        };
        let Some(address) = addresses
            .iter()
            .take(MAX_ADDRESSES_PER_PEER)
            .filter_map(|address| peer_service_address(address.clone(), peer_id))
            .next()
        else {
            return Vec::new();
        };
        add_bootstrap_address(swarm, peer_id, address.clone());
        self.nodes.insert(
            peer_id,
            RendezvousNode {
                address: address.clone(),
                namespace,
                cookie: None,
                last_discovery: None,
                registration_pending: false,
                register_again_at: None,
                last_dial_attempt: None,
            },
        );
        if swarm.is_connected(&peer_id) {
            self.request_discovery(swarm, peer_id);
            self.try_register(swarm, peer_id);
        }
        vec![DiscoveryEvent::RendezvousCandidateAdded {
            node: peer_id.to_string(),
            address: address.to_string(),
            source: "identify",
        }]
    }

    pub(crate) fn add_external_address(
        &mut self,
        swarm: &mut Swarm<NetworkBehaviour>,
        mut address: Multiaddr,
    ) -> Result<Vec<DiscoveryEvent>> {
        if let Some(Protocol::P2p(peer_id)) = address.iter().last() {
            if peer_id != *swarm.local_peer_id() {
                anyhow::bail!("公开地址尾部 PeerId 不是当前 sidecar")
            }
            address.pop();
        }
        if address.is_empty()
            || address
                .iter()
                .any(|protocol| matches!(protocol, Protocol::P2p(_)))
        {
            anyhow::bail!("公开地址格式无效")
        }
        if !is_publishable_address(&address) {
            anyhow::bail!("公开地址必须是公网 DNS/IP 或 Circuit Relay 地址")
        }
        swarm.add_external_address(address.clone());
        self.register_all(swarm);
        Ok(vec![DiscoveryEvent::AdvertisedAddressAdded {
            address: address.to_string(),
        }])
    }

    pub(crate) fn on_connected(&mut self, swarm: &mut Swarm<NetworkBehaviour>, peer_id: PeerId) {
        if self.nodes.contains_key(&peer_id) {
            self.request_discovery(swarm, peer_id);
            self.try_register(swarm, peer_id);
        }
        if self.discovered_peers.contains(&peer_id)
            && should_initiate_peer_dial(*swarm.local_peer_id(), peer_id)
        {
            swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
        }
    }

    pub(crate) fn on_external_address_confirmed(&mut self, swarm: &mut Swarm<NetworkBehaviour>) {
        self.register_all(swarm);
    }

    pub(crate) fn on_publishable_address_added(&mut self, swarm: &mut Swarm<NetworkBehaviour>) {
        self.register_all(swarm);
    }

    pub(crate) fn is_configured_node(&self, peer_id: PeerId) -> bool {
        self.nodes.contains_key(&peer_id)
    }

    pub(crate) fn forget_unreachable_peer(
        &mut self,
        swarm: &mut Swarm<NetworkBehaviour>,
        peer_id: PeerId,
    ) -> bool {
        if !self.discovered_peers.remove(&peer_id) {
            return false;
        }
        swarm
            .behaviour_mut()
            .gossipsub
            .remove_explicit_peer(&peer_id);
        true
    }

    pub(crate) fn on_disconnected(&mut self, peer_id: PeerId) {
        if let Some(node) = self.nodes.get_mut(&peer_id) {
            node.registration_pending = false;
            node.register_again_at = None;
        }
    }

    pub(crate) fn tick(&mut self, swarm: &mut Swarm<NetworkBehaviour>) {
        let now = Instant::now();
        let reconnect_nodes = self
            .nodes
            .iter()
            .filter_map(|(peer_id, node)| {
                let due = node
                    .last_dial_attempt
                    .is_none_or(|last| now.duration_since(last) >= RECONNECT_INTERVAL);
                (due && !swarm.is_connected(peer_id)).then_some(*peer_id)
            })
            .collect::<Vec<_>>();
        for peer_id in reconnect_nodes {
            self.try_dial(swarm, peer_id);
        }
        let due_nodes = self
            .nodes
            .iter()
            .filter_map(|(peer_id, node)| {
                let due = node
                    .last_discovery
                    .is_none_or(|last| now.duration_since(last) >= DISCOVERY_INTERVAL);
                (due && swarm.is_connected(peer_id)).then_some(*peer_id)
            })
            .collect::<Vec<_>>();
        for peer_id in due_nodes {
            self.request_discovery(swarm, peer_id);
        }
        let due_registrations = self
            .nodes
            .iter()
            .filter_map(|(peer_id, node)| {
                let due = !node.registration_pending
                    && node
                        .register_again_at
                        .is_none_or(|deadline| now >= deadline);
                (due && swarm.is_connected(peer_id)).then_some(*peer_id)
            })
            .collect::<Vec<_>>();
        for peer_id in due_registrations {
            self.try_register(swarm, peer_id);
        }
    }

    pub(crate) fn handle_event(
        &mut self,
        swarm: &mut Swarm<NetworkBehaviour>,
        event: rendezvous::client::Event,
    ) -> Vec<DiscoveryEvent> {
        match event {
            rendezvous::client::Event::Discovered {
                rendezvous_node,
                registrations,
                cookie,
            } => {
                let Some(node) = self.nodes.get_mut(&rendezvous_node) else {
                    return vec![DiscoveryEvent::Warning {
                        message: format!("忽略未配置节点 {rendezvous_node} 的发现响应"),
                    }];
                };
                node.cookie = Some(cookie);
                node.last_discovery = Some(Instant::now());
                let expected_namespace = node.namespace.clone();
                let local_peer_id = *swarm.local_peer_id();
                let mut discovered = 0_u16;
                for registration in registrations.into_iter().take(MAX_DISCOVERED_PEERS) {
                    if registration.namespace != expected_namespace {
                        continue;
                    }
                    let peer_id = registration.record.peer_id();
                    if peer_id == local_peer_id || peer_id == rendezvous_node {
                        continue;
                    }
                    let addresses = registration
                        .record
                        .addresses()
                        .iter()
                        .take(MAX_ADDRESSES_PER_PEER)
                        .filter_map(|address| normalized_peer_address(address.clone(), peer_id))
                        .filter(is_publishable_address)
                        .collect::<Vec<_>>();
                    if addresses.is_empty() {
                        continue;
                    }
                    if !self.discovered_peers.contains(&peer_id)
                        && self.discovered_peers.len() >= MAX_DISCOVERED_PEERS
                    {
                        continue;
                    }
                    for address in &addresses {
                        add_bootstrap_address(swarm, peer_id, address.clone());
                    }
                    // An explicit Gossipsub peer triggers dialing, so only the
                    // deterministic dialing side may hold it. Otherwise both
                    // behavior instances dial despite manual-dial deduplication.
                    let should_dial = should_initiate_peer_dial(local_peer_id, peer_id);
                    if swarm.is_connected(&peer_id) && should_dial {
                        swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                    } else if should_dial {
                        let _ = swarm.dial(
                            DialOpts::peer_id(peer_id)
                                .condition(PeerCondition::DisconnectedAndNotDialing)
                                .addresses(addresses)
                                .build(),
                        );
                    }
                    self.discovered_peers.insert(peer_id);
                    discovered = discovered.saturating_add(1);
                }
                vec![DiscoveryEvent::PeersDiscovered {
                    node: rendezvous_node.to_string(),
                    peers: discovered,
                }]
            }
            rendezvous::client::Event::DiscoverFailed {
                rendezvous_node,
                error,
                ..
            } => {
                if let Some(node) = self.nodes.get_mut(&rendezvous_node) {
                    node.cookie = None;
                    node.last_discovery = None;
                }
                vec![DiscoveryEvent::Warning {
                    message: format!("社区节点 {rendezvous_node} 发现失败：{error:?}"),
                }]
            }
            rendezvous::client::Event::Registered {
                rendezvous_node,
                ttl,
                namespace,
            } => {
                let address = if let Some(node) = self.nodes.get_mut(&rendezvous_node) {
                    node.registration_pending = false;
                    node.register_again_at =
                        Some(Instant::now() + Duration::from_secs(ttl.saturating_div(2).max(30)));
                    node.address.to_string()
                } else {
                    String::new()
                };
                vec![DiscoveryEvent::RendezvousRegistered {
                    node: rendezvous_node.to_string(),
                    address,
                    namespace: namespace.to_string(),
                    ttl_seconds: ttl,
                }]
            }
            rendezvous::client::Event::RegisterFailed {
                rendezvous_node,
                namespace,
                error,
            } => {
                if let Some(node) = self.nodes.get_mut(&rendezvous_node) {
                    node.registration_pending = false;
                    node.register_again_at = Some(Instant::now() + Duration::from_secs(60));
                }
                vec![DiscoveryEvent::Warning {
                    message: format!(
                        "社区节点 {rendezvous_node} 拒绝命名空间 {namespace} 注册：{error:?}"
                    ),
                }]
            }
            rendezvous::client::Event::Expired { peer } => {
                self.discovered_peers.remove(&peer);
                swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer);
                Vec::new()
            }
        }
    }

    fn request_discovery(&mut self, swarm: &mut Swarm<NetworkBehaviour>, peer_id: PeerId) {
        let Some(node) = self.nodes.get_mut(&peer_id) else {
            return;
        };
        swarm.behaviour_mut().rendezvous_client.discover(
            Some(node.namespace.clone()),
            node.cookie.clone(),
            Some(MAX_DISCOVERED_PEERS as u64),
            peer_id,
        );
        node.last_discovery = Some(Instant::now());
    }

    fn try_register(&mut self, swarm: &mut Swarm<NetworkBehaviour>, peer_id: PeerId) {
        let Some(node) = self.nodes.get_mut(&peer_id) else {
            return;
        };
        if node.registration_pending {
            return;
        }
        if swarm
            .behaviour_mut()
            .rendezvous_client
            .register(
                node.namespace.clone(),
                peer_id,
                Some(RENDEZVOUS_REGISTRATION_TTL_SECONDS),
            )
            .is_ok()
        {
            node.registration_pending = true;
        }
    }

    fn register_all(&mut self, swarm: &mut Swarm<NetworkBehaviour>) {
        for peer_id in self.nodes.keys().copied().collect::<Vec<_>>() {
            if swarm.is_connected(&peer_id) {
                self.try_register(swarm, peer_id);
            }
        }
    }

    fn try_dial(&mut self, swarm: &mut Swarm<NetworkBehaviour>, peer_id: PeerId) {
        let Some(node) = self.nodes.get_mut(&peer_id) else {
            return;
        };
        node.last_dial_attempt = Some(Instant::now());
        let Some(address) = direct_dial_address(node.address.clone(), peer_id) else {
            return;
        };
        let _ = swarm.dial(
            DialOpts::peer_id(peer_id)
                .condition(PeerCondition::DisconnectedAndNotDialing)
                .addresses(vec![address])
                .build(),
        );
    }
}

fn normalize_namespace(value: String) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.len() > 64
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/'))
    {
        anyhow::bail!("发现命名空间只能包含 1 到 64 个 ASCII 字母、数字、短横线、下划线或斜线")
    }
    Ok(normalized)
}

fn peer_id_from_address(address: &Multiaddr) -> Option<PeerId> {
    address.iter().find_map(|protocol| match protocol {
        Protocol::P2p(peer_id) => Some(peer_id),
        _ => None,
    })
}

fn normalized_peer_address(mut address: Multiaddr, expected_peer_id: PeerId) -> Option<Multiaddr> {
    match address.iter().last() {
        Some(Protocol::P2p(peer_id)) if peer_id == expected_peer_id => {
            address.pop();
        }
        Some(Protocol::P2p(_)) => return None,
        _ => {}
    }
    (!address.is_empty()).then_some(address)
}

fn peer_service_address(mut address: Multiaddr, expected_peer_id: PeerId) -> Option<Multiaddr> {
    if address
        .iter()
        .any(|protocol| matches!(protocol, Protocol::P2pCircuit))
    {
        return None;
    }
    match address.iter().last() {
        Some(Protocol::P2p(peer_id)) if peer_id == expected_peer_id => {}
        Some(Protocol::P2p(_)) => return None,
        _ => address.push(Protocol::P2p(expected_peer_id)),
    }
    is_public_direct_address(&address).then_some(address)
}

fn direct_dial_address(mut address: Multiaddr, expected_peer_id: PeerId) -> Option<Multiaddr> {
    match address.iter().last() {
        Some(Protocol::P2p(peer_id)) if peer_id == expected_peer_id => {
            address.pop();
        }
        Some(Protocol::P2p(_)) => return None,
        _ => {}
    }
    is_public_direct_address(&address).then_some(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 发现命名空间拒绝路径逃逸和非_ascii() {
        assert_eq!(
            normalize_namespace(" Token-Holdem/V1 ".to_owned()).unwrap(),
            "token-holdem/v1"
        );
        assert!(normalize_namespace("../escape".to_owned()).is_err());
        assert!(normalize_namespace("德州扑克".to_owned()).is_err());
    }

    #[test]
    fn 发现记录只接受公网直连或中继地址() {
        let peer_id: PeerId = "12D3KooWCQrKJT9mKBdRS33rQaADSw2Y3aQTp7wGBDciPu61YPbC"
            .parse()
            .unwrap();
        let private: Multiaddr = "/ip4/192.168.49.145/tcp/5255".parse().unwrap();
        let public: Multiaddr = "/ip4/172.104.187.120/tcp/4001".parse().unwrap();
        let relay: Multiaddr =
            format!("/ip4/172.104.187.120/tcp/4001/p2p/{peer_id}/p2p-circuit/p2p/{peer_id}")
                .parse()
                .unwrap();

        assert!(normalized_peer_address(private, peer_id)
            .is_some_and(|address| !is_publishable_address(&address)));
        assert!(normalized_peer_address(public, peer_id)
            .is_some_and(|address| is_publishable_address(&address)));
        assert!(normalized_peer_address(relay, peer_id)
            .is_some_and(|address| is_publishable_address(&address)));
    }
}
