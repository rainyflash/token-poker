use crate::network_address::is_public_direct_address;
use libp2p::{autonat, relay, upnp, Multiaddr, PeerId};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use token_holdem_network::RelayServerLimits;
use token_holdem_sidecar::{
    HostNetworkCost, PowerSource, VolunteerBlockReason, VolunteerConsent, VolunteerDecision,
    VolunteerInputs,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reachability {
    Unknown,
    Private,
    Public,
}

impl Reachability {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Private => "private",
            Self::Public => "public",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum VolunteerEvent {
    VolunteerStatus {
        consent: &'static str,
        network_cost: &'static str,
        power_source: &'static str,
        policy_reason: &'static str,
        reachability: &'static str,
        reachability_evidence: &'static str,
        role: &'static str,
        discovery_server_enabled: bool,
        relay_server_enabled: bool,
        upnp_enabled: bool,
        active_reservations: u16,
        active_circuits: u16,
        max_reservations: u16,
        max_circuits: u16,
        max_circuit_duration_seconds: u64,
        max_circuit_bytes: u64,
    },
    RelayServerReservation {
        peer_id: String,
        action: &'static str,
    },
    RelayServerCircuit {
        source_peer_id: String,
        destination_peer_id: String,
        action: &'static str,
    },
}

pub(crate) struct VolunteerRuntime {
    inputs: VolunteerInputs,
    decision: VolunteerDecision,
    discovery_server_enabled: bool,
    relay_server_enabled: bool,
    upnp_enabled: bool,
    assume_public: bool,
    autonat_status: Reachability,
    confirmed_public_addresses: HashSet<String>,
    active_reservations: HashSet<PeerId>,
    active_circuits: HashMap<(PeerId, PeerId), usize>,
    limits: RelayServerLimits,
}

impl VolunteerRuntime {
    pub(crate) fn new(
        inputs: VolunteerInputs,
        decision: VolunteerDecision,
        discovery_server_enabled: bool,
        relay_server_enabled: bool,
        upnp_enabled: bool,
        assume_public: bool,
        limits: RelayServerLimits,
    ) -> Self {
        Self {
            inputs,
            decision,
            discovery_server_enabled,
            relay_server_enabled,
            upnp_enabled,
            assume_public,
            autonat_status: Reachability::Unknown,
            confirmed_public_addresses: HashSet::new(),
            active_reservations: HashSet::new(),
            active_circuits: HashMap::new(),
            limits,
        }
    }

    pub(crate) fn status(&self) -> VolunteerEvent {
        VolunteerEvent::VolunteerStatus {
            consent: consent_name(self.inputs.consent),
            network_cost: network_cost_name(self.inputs.network_cost),
            power_source: power_source_name(self.inputs.power_source),
            policy_reason: policy_reason_name(self.decision.reason),
            reachability: self.reachability().as_str(),
            reachability_evidence: self.reachability_evidence(),
            role: self.role(),
            discovery_server_enabled: self.discovery_server_enabled,
            relay_server_enabled: self.relay_server_enabled,
            upnp_enabled: self.upnp_enabled,
            active_reservations: saturating_u16(self.active_reservations.len()),
            active_circuits: saturating_u16(self.active_circuit_count()),
            max_reservations: saturating_u16(self.limits.max_reservations),
            max_circuits: saturating_u16(self.limits.max_circuits),
            max_circuit_duration_seconds: self.limits.max_circuit_duration.as_secs(),
            max_circuit_bytes: self.limits.max_circuit_bytes,
        }
    }

    pub(crate) fn on_autonat(&mut self, event: &autonat::Event) -> Option<VolunteerEvent> {
        let autonat::Event::StatusChanged { new, .. } = event else {
            return None;
        };
        self.autonat_status = match new {
            autonat::NatStatus::Public(_) => Reachability::Public,
            autonat::NatStatus::Private => Reachability::Private,
            autonat::NatStatus::Unknown => Reachability::Unknown,
        };
        Some(self.status())
    }

    pub(crate) fn on_upnp(&mut self, event: &upnp::Event) -> VolunteerEvent {
        match event {
            upnp::Event::NewExternalAddr { external_addr, .. } => {
                self.remember_confirmed_address(external_addr);
            }
            upnp::Event::ExpiredExternalAddr { external_addr, .. } => {
                self.forget_confirmed_address(external_addr);
            }
            upnp::Event::GatewayNotFound | upnp::Event::NonRoutableGateway => {}
        }
        self.status()
    }

    pub(crate) fn on_external_address_confirmed(&mut self, address: &Multiaddr) -> VolunteerEvent {
        self.remember_confirmed_address(address);
        self.status()
    }

    pub(crate) fn on_external_address_expired(&mut self, address: &Multiaddr) -> VolunteerEvent {
        self.forget_confirmed_address(address);
        self.status()
    }

    pub(crate) fn on_relay_server(&mut self, event: relay::Event) -> Vec<VolunteerEvent> {
        let detail = match event {
            relay::Event::ReservationReqAccepted {
                src_peer_id,
                renewed,
            } => {
                self.active_reservations.insert(src_peer_id);
                VolunteerEvent::RelayServerReservation {
                    peer_id: src_peer_id.to_string(),
                    action: if renewed { "renewed" } else { "accepted" },
                }
            }
            relay::Event::ReservationReqDenied { src_peer_id, .. } => {
                VolunteerEvent::RelayServerReservation {
                    peer_id: src_peer_id.to_string(),
                    action: "denied",
                }
            }
            relay::Event::ReservationClosed { src_peer_id } => {
                self.active_reservations.remove(&src_peer_id);
                VolunteerEvent::RelayServerReservation {
                    peer_id: src_peer_id.to_string(),
                    action: "closed",
                }
            }
            relay::Event::ReservationTimedOut { src_peer_id } => {
                self.active_reservations.remove(&src_peer_id);
                VolunteerEvent::RelayServerReservation {
                    peer_id: src_peer_id.to_string(),
                    action: "timed_out",
                }
            }
            relay::Event::CircuitReqAccepted {
                src_peer_id,
                dst_peer_id,
            } => {
                *self
                    .active_circuits
                    .entry((src_peer_id, dst_peer_id))
                    .or_default() += 1;
                VolunteerEvent::RelayServerCircuit {
                    source_peer_id: src_peer_id.to_string(),
                    destination_peer_id: dst_peer_id.to_string(),
                    action: "accepted",
                }
            }
            relay::Event::CircuitReqDenied {
                src_peer_id,
                dst_peer_id,
                ..
            } => VolunteerEvent::RelayServerCircuit {
                source_peer_id: src_peer_id.to_string(),
                destination_peer_id: dst_peer_id.to_string(),
                action: "denied",
            },
            relay::Event::CircuitClosed {
                src_peer_id,
                dst_peer_id,
                ..
            } => {
                let key = (src_peer_id, dst_peer_id);
                if let Some(count) = self.active_circuits.get_mut(&key) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        self.active_circuits.remove(&key);
                    }
                }
                VolunteerEvent::RelayServerCircuit {
                    source_peer_id: src_peer_id.to_string(),
                    destination_peer_id: dst_peer_id.to_string(),
                    action: "closed",
                }
            }
            #[allow(deprecated)]
            relay::Event::ReservationReqAcceptFailed { src_peer_id, .. }
            | relay::Event::ReservationReqDenyFailed { src_peer_id, .. } => {
                VolunteerEvent::RelayServerReservation {
                    peer_id: src_peer_id.to_string(),
                    action: "failed",
                }
            }
            #[allow(deprecated)]
            relay::Event::CircuitReqOutboundConnectFailed {
                src_peer_id,
                dst_peer_id,
                ..
            }
            | relay::Event::CircuitReqAcceptFailed {
                src_peer_id,
                dst_peer_id,
                ..
            }
            | relay::Event::CircuitReqDenyFailed {
                src_peer_id,
                dst_peer_id,
                ..
            } => VolunteerEvent::RelayServerCircuit {
                source_peer_id: src_peer_id.to_string(),
                destination_peer_id: dst_peer_id.to_string(),
                action: "failed",
            },
            relay::Event::StatusChanged { .. } => return vec![self.status()],
        };
        vec![detail, self.status()]
    }

    fn reachability(&self) -> Reachability {
        if self.assume_public
            || matches!(self.autonat_status, Reachability::Public)
            || !self.confirmed_public_addresses.is_empty()
        {
            return Reachability::Public;
        }
        self.autonat_status
    }

    fn reachability_evidence(&self) -> &'static str {
        if self.assume_public {
            return "dedicated_node";
        }
        if !self.confirmed_public_addresses.is_empty() {
            return "confirmed_external_address";
        }
        match self.autonat_status {
            Reachability::Public => "autonat",
            Reachability::Private => "autonat_private",
            Reachability::Unknown => "none",
        }
    }

    fn role(&self) -> &'static str {
        if !self.discovery_server_enabled && !self.relay_server_enabled {
            return "disabled";
        }
        match (
            self.reachability(),
            self.discovery_server_enabled,
            self.relay_server_enabled,
        ) {
            (Reachability::Public, true, true) => "active_discovery_relay",
            (Reachability::Public, true, false) => "active_discovery",
            (_, _, true) => "relay_candidate",
            _ => "discovery_candidate",
        }
    }

    fn remember_confirmed_address(&mut self, address: &Multiaddr) {
        if is_public_direct_address(address) {
            self.confirmed_public_addresses.insert(address.to_string());
        }
    }

    fn forget_confirmed_address(&mut self, address: &Multiaddr) {
        self.confirmed_public_addresses.remove(&address.to_string());
    }

    fn active_circuit_count(&self) -> usize {
        self.active_circuits.values().copied().sum()
    }
}

fn consent_name(value: VolunteerConsent) -> &'static str {
    value.as_str()
}

fn network_cost_name(value: HostNetworkCost) -> &'static str {
    value.as_str()
}

fn power_source_name(value: PowerSource) -> &'static str {
    value.as_str()
}

fn policy_reason_name(value: VolunteerBlockReason) -> &'static str {
    value.as_str()
}

fn saturating_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn runtime() -> VolunteerRuntime {
        VolunteerRuntime::new(
            VolunteerInputs {
                consent: VolunteerConsent::Granted,
                network_cost: HostNetworkCost::Unmetered,
                power_source: PowerSource::Ac,
            },
            VolunteerDecision {
                enable_discovery_server: true,
                enable_relay_server: true,
                enable_upnp: true,
                reason: VolunteerBlockReason::Eligible,
            },
            true,
            true,
            true,
            false,
            RelayServerLimits::default(),
        )
    }

    #[test]
    fn 只有公网直连地址能激活志愿角色() {
        let mut subject = runtime();
        let private = Multiaddr::from_str("/ip4/192.168.1.8/tcp/4001").unwrap();
        subject.on_external_address_confirmed(&private);
        assert_eq!(subject.reachability(), Reachability::Unknown);

        let relayed = Multiaddr::from_str(
            "/ip4/203.0.113.8/tcp/4001/p2p/12D3KooWJ6vY5Zr4oV2A6zVsrHmQmzM9dV2cgF6p6wAVYM3u5j9X/p2p-circuit",
        )
        .unwrap();
        subject.on_external_address_confirmed(&relayed);
        assert_eq!(subject.reachability(), Reachability::Unknown);

        let public = Multiaddr::from_str("/ip4/8.8.8.8/tcp/4001").unwrap();
        subject.on_external_address_confirmed(&public);
        assert_eq!(subject.reachability(), Reachability::Public);
    }
}
