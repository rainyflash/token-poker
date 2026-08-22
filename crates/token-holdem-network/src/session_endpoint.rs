use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionEndpointError {
    InvalidPeerId,
    InvalidAddresses,
    AddressPeerIdMismatch,
}

pub(crate) fn validate_session_endpoint(
    peer_id: &[u8],
    addresses: &[Vec<u8>],
) -> Result<PeerId, SessionEndpointError> {
    let expected_peer_id =
        PeerId::from_bytes(peer_id).map_err(|_| SessionEndpointError::InvalidPeerId)?;
    if addresses.is_empty() || addresses.len() > 8 {
        return Err(SessionEndpointError::InvalidAddresses);
    }

    let mut unique = BTreeSet::new();
    for raw_address in addresses {
        if raw_address.is_empty()
            || raw_address.len() > 512
            || !unique.insert(raw_address.as_slice())
        {
            return Err(SessionEndpointError::InvalidAddresses);
        }
        let address = Multiaddr::try_from(raw_address.clone())
            .map_err(|_| SessionEndpointError::InvalidAddresses)?;
        let actual_peer_id = address.iter().filter_map(|protocol| match protocol {
            Protocol::P2p(peer_id) => Some(peer_id),
            _ => None,
        });
        if actual_peer_id.last() != Some(expected_peer_id) {
            return Err(SessionEndpointError::AddressPeerIdMismatch);
        }
    }
    Ok(expected_peer_id)
}
