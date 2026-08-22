use anyhow::{Context, Result};
use libp2p::{core::transport::ListenerId, multiaddr::Protocol, relay, Multiaddr, PeerId, Swarm};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use token_holdem_network::NetworkBehaviour;

use crate::network_address::is_public_direct_address;

const MAX_RELAY_NODES: usize = 4;
const MAX_ADDRESSES_PER_IDENTIFIED_PEER: usize = 8;
const RESERVATION_RETRY_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum RelayEvent {
    #[serde(rename = "relay_candidate_added")]
    CandidateAdded {
        peer_id: String,
        address: String,
        source: &'static str,
    },
    #[serde(rename = "relay_reservation_requested")]
    ReservationRequested {
        peer_id: String,
        address: String,
    },
    #[serde(rename = "relay_reservation_accepted")]
    ReservationAccepted {
        peer_id: String,
        address: String,
        renewal: bool,
        duration_seconds: Option<u64>,
        data_bytes: Option<u64>,
    },
    #[serde(rename = "relay_circuit_established")]
    CircuitEstablished {
        peer_id: String,
        direction: &'static str,
        duration_seconds: Option<u64>,
        data_bytes: Option<u64>,
    },
    Warning {
        message: String,
    },
}

struct RelayCandidate {
    address: Multiaddr,
    listener_id: Option<ListenerId>,
    reservation_requested_at: Instant,
    reservation_accepted: bool,
}

#[derive(Default)]
pub(crate) struct RelayRuntime {
    candidates: BTreeMap<PeerId, RelayCandidate>,
}

impl RelayRuntime {
    pub(crate) fn add_explicit(
        &mut self,
        swarm: &mut Swarm<NetworkBehaviour>,
        raw: &str,
    ) -> Result<Vec<RelayEvent>> {
        let address = raw
            .parse::<Multiaddr>()
            .with_context(|| format!("Circuit Relay 地址无效：{raw}"))?;
        let peer_id = peer_id_from_address(&address)
            .with_context(|| format!("Circuit Relay 地址缺少 /p2p/<PeerId>：{raw}"))?;
        self.add_candidate(swarm, peer_id, address, "explicit")
    }

    pub(crate) fn observe_identified(
        &mut self,
        swarm: &mut Swarm<NetworkBehaviour>,
        peer_id: PeerId,
        addresses: &[Multiaddr],
    ) -> Result<Vec<RelayEvent>> {
        if self.candidates.contains_key(&peer_id) || self.candidates.len() >= MAX_RELAY_NODES {
            return Ok(Vec::new());
        }
        let Some(address) = addresses
            .iter()
            .take(MAX_ADDRESSES_PER_IDENTIFIED_PEER)
            .filter_map(|address| with_peer_id(address.clone(), peer_id))
            .find(is_public_direct_address)
        else {
            return Ok(Vec::new());
        };
        self.add_candidate(swarm, peer_id, address, "identify")
    }

    pub(crate) fn handle_client_event(&mut self, event: relay::client::Event) -> RelayEvent {
        match event {
            relay::client::Event::ReservationReqAccepted {
                relay_peer_id,
                renewal,
                limit,
            } => {
                let address = self
                    .candidates
                    .get_mut(&relay_peer_id)
                    .map(|candidate| {
                        candidate.reservation_accepted = true;
                        candidate.address.to_string()
                    })
                    .unwrap_or_default();
                RelayEvent::ReservationAccepted {
                    peer_id: relay_peer_id.to_string(),
                    address,
                    renewal,
                    duration_seconds: limit
                        .and_then(|value| value.duration())
                        .map(|v| v.as_secs()),
                    data_bytes: limit.and_then(|value| value.data_in_bytes()),
                }
            }
            relay::client::Event::OutboundCircuitEstablished {
                relay_peer_id,
                limit,
            } => RelayEvent::CircuitEstablished {
                peer_id: relay_peer_id.to_string(),
                direction: "outbound",
                duration_seconds: limit
                    .and_then(|value| value.duration())
                    .map(|v| v.as_secs()),
                data_bytes: limit.and_then(|value| value.data_in_bytes()),
            },
            relay::client::Event::InboundCircuitEstablished { src_peer_id, limit } => {
                RelayEvent::CircuitEstablished {
                    peer_id: src_peer_id.to_string(),
                    direction: "inbound",
                    duration_seconds: limit
                        .and_then(|value| value.duration())
                        .map(|v| v.as_secs()),
                    data_bytes: limit.and_then(|value| value.data_in_bytes()),
                }
            }
        }
    }

    pub(crate) fn on_connected(
        &mut self,
        _swarm: &mut Swarm<NetworkBehaviour>,
        _peer_id: PeerId,
    ) -> Result<Vec<RelayEvent>> {
        Ok(Vec::new())
    }

    pub(crate) fn on_disconnected(&mut self, peer_id: PeerId) {
        if let Some(candidate) = self.candidates.get_mut(&peer_id) {
            candidate.reservation_accepted = false;
        }
    }

    pub(crate) fn on_dial_failure(&mut self, peer_id: PeerId) {
        self.on_disconnected(peer_id);
    }

    pub(crate) fn is_candidate(&self, peer_id: PeerId) -> bool {
        self.candidates.contains_key(&peer_id)
    }

    pub(crate) fn tick(&mut self, swarm: &mut Swarm<NetworkBehaviour>) -> Vec<RelayEvent> {
        let now = Instant::now();
        let due = self
            .candidates
            .iter()
            .filter_map(|(peer_id, candidate)| {
                (!candidate.reservation_accepted
                    && now.duration_since(candidate.reservation_requested_at)
                        >= RESERVATION_RETRY_INTERVAL)
                    .then_some(*peer_id)
            })
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for peer_id in due {
            let Some(candidate) = self.candidates.get_mut(&peer_id) else {
                continue;
            };
            if let Some(listener_id) = candidate.listener_id.take() {
                swarm.remove_listener(listener_id);
            }
            candidate.reservation_requested_at = now;
            match request_reservation(swarm, &candidate.address) {
                Ok(listener_id) => {
                    candidate.listener_id = Some(listener_id);
                    events.push(RelayEvent::ReservationRequested {
                        peer_id: peer_id.to_string(),
                        address: candidate.address.to_string(),
                    });
                }
                Err(error) => events.push(RelayEvent::Warning {
                    message: format!("Circuit Relay 保留重试失败：{error:#}"),
                }),
            }
        }
        events
    }

    fn add_candidate(
        &mut self,
        swarm: &mut Swarm<NetworkBehaviour>,
        peer_id: PeerId,
        address: Multiaddr,
        source: &'static str,
    ) -> Result<Vec<RelayEvent>> {
        if peer_id == *swarm.local_peer_id() {
            anyhow::bail!("不能把当前 sidecar 配置成自己的 Circuit Relay")
        }
        if let Some(existing) = self.candidates.get(&peer_id) {
            let _ = existing;
            return Ok(Vec::new());
        }
        if self.candidates.len() >= MAX_RELAY_NODES {
            anyhow::bail!("Circuit Relay 候选最多 {MAX_RELAY_NODES} 个")
        }
        let address_text = address.to_string();
        let listener_id = request_reservation(swarm, &address)
            .with_context(|| format!("无法向中继申请保留：{address_text}"))?;
        self.candidates.insert(
            peer_id,
            RelayCandidate {
                address,
                listener_id: Some(listener_id),
                reservation_requested_at: Instant::now(),
                reservation_accepted: false,
            },
        );
        Ok(vec![
            RelayEvent::CandidateAdded {
                peer_id: peer_id.to_string(),
                address: address_text.clone(),
                source,
            },
            RelayEvent::ReservationRequested {
                peer_id: peer_id.to_string(),
                address: address_text,
            },
        ])
    }
}

fn request_reservation(
    swarm: &mut Swarm<NetworkBehaviour>,
    address: &Multiaddr,
) -> Result<ListenerId> {
    let mut relay_address = address.clone();
    relay_address.push(Protocol::P2pCircuit);
    swarm
        .listen_on(relay_address.clone())
        .with_context(|| format!("无法监听中继地址：{relay_address}"))
}

fn peer_id_from_address(address: &Multiaddr) -> Option<PeerId> {
    address.iter().find_map(|protocol| match protocol {
        Protocol::P2p(peer_id) => Some(peer_id),
        _ => None,
    })
}

fn with_peer_id(mut address: Multiaddr, peer_id: PeerId) -> Option<Multiaddr> {
    if address
        .iter()
        .any(|protocol| matches!(protocol, Protocol::P2pCircuit))
    {
        return None;
    }
    match address.iter().last() {
        Some(Protocol::P2p(found)) if found == peer_id => {}
        Some(Protocol::P2p(_)) => return None,
        _ => address.push(Protocol::P2p(peer_id)),
    }
    Some(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn 跨会话候选拒绝本地地址并接受公网地址() {
        let local = Multiaddr::from_str(
            "/ip4/192.168.1.2/tcp/4001/p2p/12D3KooWCQrKJT9mKBdRS33rQaADSw2Y3aQTp7wGBDciPu61YPbC",
        )
        .unwrap();
        let public = Multiaddr::from_str(
            "/dns4/relay.example/tcp/4001/p2p/12D3KooWCQrKJT9mKBdRS33rQaADSw2Y3aQTp7wGBDciPu61YPbC",
        )
        .unwrap();
        assert!(!is_public_direct_address(&local));
        assert!(is_public_direct_address(&public));
    }
}
