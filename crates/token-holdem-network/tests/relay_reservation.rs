use futures::StreamExt;
use libp2p::{multiaddr::Protocol, relay, swarm::SwarmEvent, Multiaddr};
use std::time::Duration;
use token_holdem_network::{build_swarm, NetworkConfig, NetworkEvent, RelayServerLimits};

#[tokio::test]
async fn relay_reservation_is_confirmed_by_both_peers() {
    let limits = RelayServerLimits {
        max_reservations: 3,
        max_circuits: 2,
        max_circuit_duration: Duration::from_secs(180),
        max_circuit_bytes: 1_048_576,
        ..RelayServerLimits::default()
    };
    let mut server = build_swarm(NetworkConfig {
        enable_relay_server: true,
        relay_limits: limits,
        ..NetworkConfig::default()
    })
    .expect("relay server swarm should build");
    let mut client =
        build_swarm(NetworkConfig::default()).expect("relay client swarm should build");

    let listen_address: Multiaddr = "/ip4/127.0.0.1/tcp/0"
        .parse()
        .expect("loopback address should parse");
    server
        .listen_on(listen_address)
        .expect("relay server should listen");

    let server_address = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let SwarmEvent::NewListenAddr { address, .. } = server.select_next_some().await {
                break address.with(Protocol::P2p(*server.local_peer_id()));
            }
        }
    })
    .await
    .expect("relay server should publish a listen address");
    let tcp_port = server_address
        .iter()
        .find_map(|protocol| match protocol {
            Protocol::Tcp(port) => Some(port),
            _ => None,
        })
        .expect("relay server address should contain a TCP port");
    server.add_external_address(
        Multiaddr::empty()
            .with(Protocol::Dns4("localhost".into()))
            .with(Protocol::Tcp(tcp_port)),
    );

    client
        .listen_on(server_address.with(Protocol::P2pCircuit))
        .expect("relay reservation listener should start");

    let (client_accepted, server_accepted) = tokio::time::timeout(Duration::from_secs(10), async {
        let mut client_accepted = false;
        let mut server_accepted = false;
        while !client_accepted || !server_accepted {
            tokio::select! {
                event = client.select_next_some() => {
                    if matches!(
                        event,
                        SwarmEvent::Behaviour(NetworkEvent::RelayClient(
                            relay::client::Event::ReservationReqAccepted { .. }
                        ))
                    ) {
                        client_accepted = true;
                    }
                }
                event = server.select_next_some() => {
                    if matches!(
                        event,
                        SwarmEvent::Behaviour(NetworkEvent::RelayServer(
                            relay::Event::ReservationReqAccepted { .. }
                        ))
                    ) {
                        server_accepted = true;
                    }
                }
            }
        }
        (client_accepted, server_accepted)
    })
    .await
    .expect("relay reservation should complete on both peers");

    assert!(client_accepted);
    assert!(server_accepted);
}
