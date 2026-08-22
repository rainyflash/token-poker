use libp2p::{
    core::{transport::PortUse, upgrade::DeniedUpgrade, Endpoint},
    swarm::{
        behaviour::{FromSwarm, NotifyHandler, ToSwarm},
        ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent, ConnectionId,
        NetworkBehaviour, SubstreamProtocol, THandler, THandlerInEvent, THandlerOutEvent,
    },
    Multiaddr, PeerId,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::Infallible,
    task::{Context, Poll},
};

#[derive(Debug, Clone, Copy)]
pub enum LeaseCommand {
    SetRetained(bool),
}

#[derive(Default)]
pub struct ConnectionLeaseBehaviour {
    retained_peers: HashSet<PeerId>,
    connections: HashMap<PeerId, HashSet<ConnectionId>>,
    pending_events: VecDeque<ToSwarm<Infallible, LeaseCommand>>,
}

impl ConnectionLeaseBehaviour {
    pub(crate) fn retain_peer(&mut self, peer_id: PeerId) {
        if self.retained_peers.insert(peer_id) {
            self.notify_peer(peer_id, true);
        }
    }

    pub(crate) fn release_peer(&mut self, peer_id: PeerId) {
        if self.retained_peers.remove(&peer_id) {
            self.notify_peer(peer_id, false);
        }
    }

    pub(crate) fn is_peer_retained(&self, peer_id: &PeerId) -> bool {
        self.retained_peers.contains(peer_id)
    }

    fn notify_peer(&mut self, peer_id: PeerId, retained: bool) {
        let connection_ids = self
            .connections
            .get(&peer_id)
            .into_iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        self.pending_events
            .extend(
                connection_ids
                    .into_iter()
                    .map(|connection_id| ToSwarm::NotifyHandler {
                        peer_id,
                        handler: NotifyHandler::One(connection_id),
                        event: LeaseCommand::SetRetained(retained),
                    }),
            );
    }
}

impl NetworkBehaviour for ConnectionLeaseBehaviour {
    type ConnectionHandler = ConnectionLeaseHandler;
    type ToSwarm = Infallible;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer_id: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(ConnectionLeaseHandler::new(
            self.retained_peers.contains(&peer_id),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer_id: PeerId,
        _addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(ConnectionLeaseHandler::new(
            self.retained_peers.contains(&peer_id),
        ))
    }

    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        match event {
            FromSwarm::ConnectionEstablished(event) => {
                self.connections
                    .entry(event.peer_id)
                    .or_default()
                    .insert(event.connection_id);
            }
            FromSwarm::ConnectionClosed(event) => {
                if event.remaining_established == 0 {
                    self.connections.remove(&event.peer_id);
                } else if let Some(connection_ids) = self.connections.get_mut(&event.peer_id) {
                    connection_ids.remove(&event.connection_id);
                }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match event {}
    }

    fn poll(
        &mut self,
        _context: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        self.pending_events
            .pop_front()
            .map_or(Poll::Pending, Poll::Ready)
    }
}

#[derive(Clone)]
pub struct ConnectionLeaseHandler {
    retained: bool,
}

impl ConnectionLeaseHandler {
    fn new(retained: bool) -> Self {
        Self { retained }
    }
}

impl ConnectionHandler for ConnectionLeaseHandler {
    type FromBehaviour = LeaseCommand;
    type ToBehaviour = Infallible;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            LeaseCommand::SetRetained(retained) => self.retained = retained,
        }
    }

    fn connection_keep_alive(&self) -> bool {
        self.retained
    }

    fn poll(
        &mut self,
        _context: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        Poll::Pending
    }

    fn on_connection_event(
        &mut self,
        _event: libp2p::swarm::handler::ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
        >,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 连接租约只在显式持有期间保活() {
        let mut handler = ConnectionLeaseHandler::new(false);
        assert!(!handler.connection_keep_alive());
        handler.on_behaviour_event(LeaseCommand::SetRetained(true));
        assert!(handler.connection_keep_alive());
        handler.on_behaviour_event(LeaseCommand::SetRetained(false));
        assert!(!handler.connection_keep_alive());
    }
}
