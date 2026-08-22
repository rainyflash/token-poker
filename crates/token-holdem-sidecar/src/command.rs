use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_COMMAND_LINE_BYTES: usize = 64 * 1_024;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SidecarCommand {
    TokenSnapshot {
        lifetime_tokens: u64,
        username: Option<String>,
        display_name: Option<String>,
        account_identifier: Option<String>,
        observed_at_unix_ms: u64,
        source: Option<TokenSnapshotSource>,
    },
    Dial {
        address: String,
    },
    UseRelay {
        address: String,
    },
    ConfigureDiscovery {
        addresses: Vec<String>,
        namespace: Option<String>,
    },
    AddExternalAddress {
        address: String,
    },
    JoinPublicPool {
        level_id: String,
        buy_in: u64,
    },
    CancelPublicPool,
    EnsureIdentity {
        recovery_secret: String,
        device_label: String,
    },
    CreateIdentity {
        recovery_secret: String,
        device_label: String,
    },
    RestoreIdentity {
        recovery_envelope: String,
        recovery_secret: String,
        device_label: String,
    },
    RestoreRemoteIdentity {
        recovery_secret: String,
        device_label: String,
    },
    CreateFriendRoom {
        level_id: String,
        buy_in: u64,
    },
    JoinFriendRoom {
        invite_code: String,
        buy_in: u64,
    },
    ConfigureArchiveNodes {
        addresses: Vec<String>,
        minimum_confirmed_replicas: u16,
    },
    SyncStatistics,
    FetchArchivedReceipt {
        address: String,
    },
    SubmitAction {
        action: String,
        amount: Option<u64>,
    },
    LeaveTable,
    Shutdown,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenSnapshotSource {
    CodexAppServerAccountUsage,
    #[default]
    LegacyAgentProfileObservation,
}

#[derive(Debug, Error)]
pub enum SidecarCommandError {
    #[error("控制命令超过 {maximum} 字节上限，实际 {actual} 字节")]
    TooLong { maximum: usize, actual: usize },
    #[error("控制命令不是合法 JSON：{0}")]
    InvalidJson(#[from] serde_json::Error),
}

pub fn decode_command_line(line: &str) -> Result<SidecarCommand, SidecarCommandError> {
    if line.len() > MAX_COMMAND_LINE_BYTES {
        return Err(SidecarCommandError::TooLong {
            maximum: MAX_COMMAND_LINE_BYTES,
            actual: line.len(),
        });
    }
    Ok(serde_json::from_str(line)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 命令解码接受合法消息并拒绝超长行() {
        let command =
            decode_command_line(r#"{"type":"sync_statistics"}"#).expect("合法命令必须能被解码");
        assert!(matches!(command, SidecarCommand::SyncStatistics));

        let ensure_identity = decode_command_line(
            r#"{"type":"ensure_identity","recovery_secret":"abcdefghijkl","device_label":"Windows 工作站"}"#,
        )
        .expect("自动确保身份命令必须能被解码");
        assert!(matches!(
            ensure_identity,
            SidecarCommand::EnsureIdentity { .. }
        ));

        let oversized = "x".repeat(MAX_COMMAND_LINE_BYTES + 1);
        assert!(matches!(
            decode_command_line(&oversized),
            Err(SidecarCommandError::TooLong { .. })
        ));
    }
}
