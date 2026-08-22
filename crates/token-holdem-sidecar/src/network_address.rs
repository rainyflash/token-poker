use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};

pub(crate) fn should_initiate_peer_dial(local_peer_id: PeerId, remote_peer_id: PeerId) -> bool {
    local_peer_id < remote_peer_id
}

pub(crate) fn is_public_direct_address(address: &Multiaddr) -> bool {
    !contains_relay_hop(address) && address.iter().any(is_public_host_protocol)
}

pub(crate) fn is_publishable_address(address: &Multiaddr) -> bool {
    contains_relay_hop(address) || address.iter().any(is_public_host_protocol)
}

pub(crate) fn preferred_dial_address(
    addresses: impl IntoIterator<Item = Multiaddr>,
) -> Option<Multiaddr> {
    addresses
        .into_iter()
        .enumerate()
        .min_by_key(|(index, address)| {
            let (reachability, transport) = dial_priority(address);
            (reachability, transport, *index)
        })
        .map(|(_, address)| address)
}

fn dial_priority(address: &Multiaddr) -> (u8, u8) {
    let reachability = if contains_relay_hop(address) {
        0
    } else if is_public_direct_address(address) {
        1
    } else if contains_private_host(address) {
        2
    } else if contains_loopback_host(address) {
        3
    } else {
        4
    };
    let transport = if address
        .iter()
        .any(|protocol| matches!(protocol, Protocol::Tcp(_)))
    {
        0
    } else {
        1
    };
    (reachability, transport)
}

fn contains_private_host(address: &Multiaddr) -> bool {
    address.iter().any(|protocol| match protocol {
        Protocol::Ip4(ip) => ip.is_private(),
        Protocol::Ip6(ip) => (ip.segments()[0] & 0xfe00) == 0xfc00,
        _ => false,
    })
}

fn contains_loopback_host(address: &Multiaddr) -> bool {
    address.iter().any(|protocol| match protocol {
        Protocol::Ip4(ip) => ip.is_loopback(),
        Protocol::Ip6(ip) => ip.is_loopback(),
        _ => false,
    })
}

fn contains_relay_hop(address: &Multiaddr) -> bool {
    address
        .iter()
        .any(|protocol| matches!(protocol, Protocol::P2pCircuit))
}

fn is_public_host_protocol(protocol: Protocol<'_>) -> bool {
    match protocol {
        Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) | Protocol::Dnsaddr(_) => true,
        Protocol::Ip4(ip) => is_public_ipv4(ip),
        Protocol::Ip6(ip) => is_public_ipv6(ip),
        _ => false,
    }
}

fn is_public_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let [first, second, third, _] = ip.octets();
    !ip.is_unspecified()
        && !ip.is_loopback()
        && !ip.is_private()
        && !ip.is_link_local()
        && !ip.is_multicast()
        && !ip.is_broadcast()
        && !(first == 100 && (64..=127).contains(&second))
        && !(first == 192 && second == 0 && third == 0)
        && !(first == 192 && second == 0 && third == 2)
        && !(first == 198 && (18..=19).contains(&second))
        && !(first == 198 && second == 51 && third == 100)
        && !(first == 203 && second == 0 && third == 113)
        && first < 224
}

fn is_public_ipv6(ip: std::net::Ipv6Addr) -> bool {
    let segments = ip.segments();
    !ip.is_unspecified()
        && !ip.is_loopback()
        && !ip.is_multicast()
        && (segments[0] & 0xfe00) != 0xfc00
        && (segments[0] & 0xffc0) != 0xfe80
        && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn 只发布公网直连或中继地址() {
        for raw in [
            "/ip4/127.0.0.1/tcp/4001",
            "/ip4/192.168.49.145/tcp/4001",
            "/ip4/172.29.80.1/tcp/4001",
            "/ip4/198.18.0.1/tcp/4001",
            "/ip4/203.0.113.10/tcp/4001",
            "/ip6/2001:db8::1/tcp/4001",
        ] {
            let address = Multiaddr::from_str(raw).expect("测试地址应有效");
            assert!(!is_publishable_address(&address), "不应发布 {raw}");
        }

        let public = Multiaddr::from_str("/ip4/172.104.187.120/tcp/4001").unwrap();
        let relay = Multiaddr::from_str(
            "/ip4/172.104.187.120/tcp/4001/p2p/12D3KooWCQrKJT9mKBdRS33rQaADSw2Y3aQTp7wGBDciPu61YPbC/p2p-circuit",
        )
        .unwrap();
        assert!(is_public_direct_address(&public));
        assert!(is_publishable_address(&public));
        assert!(!is_public_direct_address(&relay));
        assert!(is_publishable_address(&relay));
    }

    #[test]
    fn 同一对玩家只会有一端主动拨号() {
        let left: PeerId = "12D3KooWCQrKJT9mKBdRS33rQaADSw2Y3aQTp7wGBDciPu61YPbC"
            .parse()
            .unwrap();
        let right: PeerId = "12D3KooWJ5qFSaGVcMzdLX8hKQ8fKXSmND4JkJ2v9o7xyH4YzQ7A"
            .parse()
            .unwrap();

        assert_ne!(
            should_initiate_peer_dial(left, right),
            should_initiate_peer_dial(right, left)
        );
        assert!(!should_initiate_peer_dial(left, left));
    }

    #[test]
    fn 拨号只选择中继或公网或首个本地地址中的最优一条() {
        let tunnel = Multiaddr::from_str("/ip4/198.18.0.1/tcp/4001").unwrap();
        let private_quic = Multiaddr::from_str("/ip4/192.168.1.10/udp/4001/quic-v1").unwrap();
        let private_tcp = Multiaddr::from_str("/ip4/192.168.1.10/tcp/4001").unwrap();
        let loopback = Multiaddr::from_str("/ip4/127.0.0.1/tcp/4001").unwrap();
        let public = Multiaddr::from_str("/ip4/172.104.187.120/tcp/4001").unwrap();
        let relay = Multiaddr::from_str(
            "/ip4/172.104.187.120/tcp/4001/p2p/12D3KooWCQrKJT9mKBdRS33rQaADSw2Y3aQTp7wGBDciPu61YPbC/p2p-circuit",
        )
        .unwrap();

        assert_eq!(
            preferred_dial_address([private_tcp.clone(), public.clone(), relay.clone()]),
            Some(relay)
        );
        assert_eq!(
            preferred_dial_address([private_tcp.clone(), public.clone()]),
            Some(public)
        );
        assert_eq!(
            preferred_dial_address([tunnel.clone(), private_quic, loopback, private_tcp.clone(),]),
            Some(private_tcp)
        );
        assert_eq!(preferred_dial_address([tunnel.clone()]), Some(tunnel));
        assert_eq!(preferred_dial_address(Vec::<Multiaddr>::new()), None);
    }
}
