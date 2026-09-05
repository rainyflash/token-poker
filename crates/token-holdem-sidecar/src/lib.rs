#![forbid(unsafe_code)]

mod command;
mod runtime_protocol;
pub mod runtime_supervisor;
mod volunteer;

pub use command::{
    decode_command_line, HandActionPrecondition, SidecarCommand, SidecarCommandError,
    TokenSnapshotSource, MAX_COMMAND_LINE_BYTES,
};
pub use runtime_protocol::{
    parse_runtime_client_line, EventJournal, JournalSnapshot, RuntimeClientRequest, RuntimeEvent,
    RuntimeServerFrame, RUNTIME_PROTOCOL_VERSION,
};
pub use volunteer::{
    HostNetworkCost, PowerSource, VolunteerBlockReason, VolunteerConsent, VolunteerDecision,
    VolunteerInputs, VolunteerPolicy,
};
