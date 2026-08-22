use crate::{
    connection_lease::ConnectionLeaseBehaviour, ControlRequest, ControlResponse, CONTROL_PROTOCOL,
    IDENTIFY_PROTOCOL, RENDEZVOUS_SERVER_MAX_TTL_SECONDS, RENDEZVOUS_SERVER_MIN_TTL_SECONDS,
};
use libp2p::{
    autonat, dcutr, gossipsub, identify,
    identity::Keypair,
    kad::{self, store::MemoryStore},
    noise, ping, relay, rendezvous, request_response,
    swarm::{behaviour::toggle::Toggle, NetworkBehaviour as NetworkBehaviourTrait, Swarm},
    tcp, upnp, yamux, Multiaddr, PeerId, StreamProtocol, SwarmBuilder,
};
use std::time::Duration;
use thiserror::Error;

// A waiting player may need an entire hand before receiving a seat. A 90-second
// timeout would kill healthy connections before expansion consensus begins.
// Reclamation remains bounded so discovery peers cannot retain history forever.
const ACTIVE_CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
// The receiver verifies Mental Poker proofs synchronously before responding.
// With six local processes, three seconds falsely times out healthy requests and
// repeatedly resets substreams. Thirty seconds stays bounded on slow Windows hosts.
const CONTROL_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(NetworkBehaviourTrait)]
pub struct NetworkBehaviour {
    pub(crate) connection_lease: ConnectionLeaseBehaviour,
    pub relay_client: relay::client::Behaviour,
    pub relay_server: Toggle<relay::Behaviour>,
    pub dcutr: dcutr::Behaviour,
    pub identify: identify::Behaviour,
    pub ping: ping::Behaviour,
    pub autonat: autonat::Behaviour,
    pub rendezvous_client: rendezvous::client::Behaviour,
    pub rendezvous_server: Toggle<rendezvous::server::Behaviour>,
    pub upnp: Toggle<upnp::tokio::Behaviour>,
    pub kademlia: kad::Behaviour<MemoryStore>,
    pub gossipsub: gossipsub::Behaviour,
    pub control: request_response::cbor::Behaviour<ControlRequest, ControlResponse>,
}

pub type NetworkEvent = NetworkBehaviourEvent;

impl NetworkBehaviour {
    pub fn retain_peer_connection(&mut self, peer_id: PeerId) {
        self.connection_lease.retain_peer(peer_id);
    }

    pub fn release_peer_connection(&mut self, peer_id: PeerId) {
        self.connection_lease.release_peer(peer_id);
    }

    pub fn is_peer_connection_retained(&self, peer_id: &PeerId) -> bool {
        self.connection_lease.is_peer_retained(peer_id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct NetworkConfig {
    pub enable_rendezvous_server: bool,
    pub enable_relay_server: bool,
    pub enable_upnp: bool,
    pub relay_limits: RelayServerLimits,
    pub identity: Option<Keypair>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayServerLimits {
    pub max_reservations: usize,
    pub max_reservations_per_peer: usize,
    pub reservation_duration: Duration,
    pub max_circuits: usize,
    pub max_circuits_per_peer: usize,
    pub max_circuit_duration: Duration,
    pub max_circuit_bytes: u64,
}

impl Default for RelayServerLimits {
    fn default() -> Self {
        Self {
            max_reservations: 64,
            max_reservations_per_peer: 1,
            reservation_duration: Duration::from_secs(60 * 60),
            max_circuits: 16,
            max_circuits_per_peer: 2,
            max_circuit_duration: Duration::from_secs(2 * 60 * 60),
            max_circuit_bytes: 64 * 1_024 * 1_024,
        }
    }
}

impl RelayServerLimits {
    fn into_config(self) -> relay::Config {
        relay::Config {
            max_reservations: self.max_reservations,
            max_reservations_per_peer: self.max_reservations_per_peer,
            reservation_duration: self.reservation_duration,
            max_circuits: self.max_circuits,
            max_circuits_per_peer: self.max_circuits_per_peer,
            max_circuit_duration: self.max_circuit_duration,
            max_circuit_bytes: self.max_circuit_bytes,
            ..relay::Config::default()
        }
    }
}

pub fn build_swarm(config: NetworkConfig) -> Result<Swarm<NetworkBehaviour>, NetworkBuildError> {
    let keypair = config.identity.unwrap_or_else(Keypair::generate_ed25519);
    let peer_id = keypair.public().to_peer_id();
    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(2))
        .validation_mode(gossipsub::ValidationMode::Strict)
        .build()
        .map_err(|error| NetworkBuildError::Gossipsub(error.to_string()))?;
    let gossipsub = gossipsub::Behaviour::new(
        gossipsub::MessageAuthenticity::Signed(keypair.clone()),
        gossipsub_config,
    )
    .map_err(|error| NetworkBuildError::Gossipsub(error.to_string()))?;
    let mut kademlia = kad::Behaviour::new(peer_id, MemoryStore::new(peer_id));
    kademlia.set_mode(Some(if config.enable_rendezvous_server {
        kad::Mode::Server
    } else {
        kad::Mode::Client
    }));
    let rendezvous_client = rendezvous::client::Behaviour::new(keypair.clone());
    let rendezvous_server = config.enable_rendezvous_server.then(|| {
        rendezvous::server::Behaviour::new(
            rendezvous::server::Config::default()
                .with_min_ttl(RENDEZVOUS_SERVER_MIN_TTL_SECONDS)
                .with_max_ttl(RENDEZVOUS_SERVER_MAX_TTL_SECONDS),
        )
    });
    let relay_server = config
        .enable_relay_server
        .then(|| relay::Behaviour::new(peer_id, config.relay_limits.into_config()));
    let upnp = config.enable_upnp.then(upnp::tokio::Behaviour::default);
    let control = request_response::cbor::Behaviour::new(
        [(
            StreamProtocol::new(CONTROL_PROTOCOL),
            request_response::ProtocolSupport::Full,
        )],
        request_response::Config::default().with_request_timeout(CONTROL_REQUEST_TIMEOUT),
    );

    SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )
        .map_err(|error| NetworkBuildError::Transport(error.to_string()))?
        .with_quic()
        .with_dns()
        .map_err(|error| NetworkBuildError::Transport(error.to_string()))?
        .with_relay_client(noise::Config::new, yamux::Config::default)
        .map_err(|error| NetworkBuildError::Transport(error.to_string()))?
        .with_behaviour(move |identity, relay_client| NetworkBehaviour {
            connection_lease: ConnectionLeaseBehaviour::default(),
            relay_client,
            relay_server: relay_server.into(),
            dcutr: dcutr::Behaviour::new(identity.public().to_peer_id()),
            identify: identify::Behaviour::new(identify::Config::new(
                IDENTIFY_PROTOCOL.to_owned(),
                identity.public(),
            )),
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(20))),
            autonat: autonat::Behaviour::new(
                identity.public().to_peer_id(),
                autonat::Config::default(),
            ),
            rendezvous_client,
            rendezvous_server: rendezvous_server.into(),
            upnp: upnp.into(),
            kademlia,
            gossipsub,
            control,
        })
        .map_err(|error| NetworkBuildError::Behaviour(error.to_string()))
        .map(|builder| {
            builder
                .with_swarm_config(|config| {
                    config.with_idle_connection_timeout(ACTIVE_CONNECTION_IDLE_TIMEOUT)
                })
                .build()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn 志愿网络行为只在显式启用时装配() {
        let disabled = build_swarm(NetworkConfig::default()).expect("默认网络应创建成功");
        assert!(!disabled.behaviour().relay_server.is_enabled());
        assert!(!disabled.behaviour().rendezvous_server.is_enabled());
        assert!(!disabled.behaviour().upnp.is_enabled());

        let enabled = build_swarm(NetworkConfig {
            enable_rendezvous_server: true,
            enable_relay_server: true,
            enable_upnp: true,
            ..NetworkConfig::default()
        })
        .expect("志愿网络应创建成功");
        assert!(enabled.behaviour().relay_server.is_enabled());
        assert!(enabled.behaviour().rendezvous_server.is_enabled());
        assert!(enabled.behaviour().upnp.is_enabled());
    }

    #[test]
    fn relay_默认预算全部有界且不可为零() {
        let limits = RelayServerLimits::default();
        assert!(limits.max_reservations > 0);
        assert!(limits.max_reservations_per_peer > 0);
        assert!(limits.max_circuits > 0);
        assert!(limits.max_circuits_per_peer > 0);
        assert!(!limits.reservation_duration.is_zero());
        assert!(!limits.max_circuit_duration.is_zero());
        assert!(limits.max_circuit_bytes > 0);
    }
}

pub fn listen(swarm: &mut Swarm<NetworkBehaviour>) -> Result<(), NetworkBuildError> {
    let quic: Multiaddr = "/ip4/0.0.0.0/udp/0/quic-v1"
        .parse::<Multiaddr>()
        .map_err(|error| NetworkBuildError::Address(error.to_string()))?;
    let tcp: Multiaddr = "/ip4/0.0.0.0/tcp/0"
        .parse::<Multiaddr>()
        .map_err(|error| NetworkBuildError::Address(error.to_string()))?;
    swarm
        .listen_on(quic)
        .map_err(|error| NetworkBuildError::Transport(error.to_string()))?;
    swarm
        .listen_on(tcp)
        .map_err(|error| NetworkBuildError::Transport(error.to_string()))?;
    Ok(())
}

pub fn add_bootstrap_address(
    swarm: &mut Swarm<NetworkBehaviour>,
    peer_id: PeerId,
    address: Multiaddr,
) {
    swarm
        .behaviour_mut()
        .kademlia
        .add_address(&peer_id, address);
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NetworkBuildError {
    #[error("传输层初始化失败：{0}")]
    Transport(String),
    #[error("网络行为初始化失败：{0}")]
    Behaviour(String),
    #[error("Gossipsub 初始化失败：{0}")]
    Gossipsub(String),
    #[error("网络地址无效：{0}")]
    Address(String),
}
