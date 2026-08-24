#![forbid(unsafe_code)]

mod archive_store;
mod behaviour;
mod code;
#[doc(hidden)]
pub mod connection_lease;
mod hand;
mod hand_roster_protocol;
mod protocol_encoding;
mod room;
mod session_endpoint;
mod table_pool;
mod table_session;
mod wire;

pub use behaviour::{
    add_bootstrap_address, build_swarm, listen, NetworkBehaviour, NetworkBuildError, NetworkConfig,
    NetworkEvent, RelayServerLimits,
};
pub use code::{
    decode_code, decode_code_with_payload, decode_payload, encode_code, encode_payload,
    encode_payload_code, ProtocolCodeError,
};
pub use hand::{
    HandPrivateMessage, HandPublicMessage, SignedHandAction, SignedHandActionError,
    TABLE_TOPIC_PREFIX,
};
pub use hand_roster_protocol::{
    verify_hand_roster_acceptances, HandRosterAcceptance, HandRosterMessage, HandRosterProposal,
    HandRosterProtocolError, RosterEndpoint, SignedHandRosterProposal,
};
pub use room::{FriendRoomInvite, FriendRoomInviteError, RoomId};
pub use table_pool::{
    rank_pool_tickets, select_table_advertisement, PoolTicket, PoolTicketId, TableAdvertisement,
    TablePoolMessage, TablePoolProtocolError,
};
pub use table_session::{
    verify_membership_acceptances, JoinIntent, LeaveIntent, LeaveIntentId, MembershipAcceptance,
    MembershipProposal, MembershipSeatClaim, SignedMembershipProposal, TableSessionMessage,
    TableSessionProtocolError,
};
pub use wire::{
    ArchiveFetchResponse, ArchiveListResponse, ArchiveRequest, ArchiveResponse, ControlRequest,
    ControlResponse, RecoveryFetchResponse, RecoveryStoreRequest, RecoveryStoreResponse,
};

pub const CONTROL_PROTOCOL: &str = "/token-holdem/control/10";
pub const TABLE_POOL_TOPIC_PREFIX: &str = "/token-holdem/table-pool/2/";
pub const TABLE_SESSION_TOPIC_PREFIX: &str = "/token-holdem/table-session/2/";
pub const FRIEND_ROOM_TOPIC_PREFIX: &str = "/token-holdem/friend-room/2/";
pub const PROTOCOL_VERSION: &str = "10";
pub const IDENTIFY_PROTOCOL: &str = "/token-holdem/10.0.0";
pub const RENDEZVOUS_REGISTRATION_TTL_SECONDS: u64 = 120;
pub const RENDEZVOUS_SERVER_MIN_TTL_SECONDS: u64 = 60;
pub const RENDEZVOUS_SERVER_MAX_TTL_SECONDS: u64 = 7_200;
pub use archive_store::{ArchiveReplicaClient, P2pRemoteEventStore};
