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
        request_id: Option<String>,
        expected_account_fingerprint: String,
        recovery_secret: String,
        device_label: String,
    },
    CreateIdentity {
        request_id: Option<String>,
        expected_account_fingerprint: String,
        recovery_secret: String,
        device_label: String,
    },
    RestoreIdentity {
        request_id: Option<String>,
        expected_account_fingerprint: String,
        recovery_envelope: String,
        recovery_secret: String,
        device_label: String,
    },
    RestoreRemoteIdentity {
        request_id: Option<String>,
        expected_account_fingerprint: String,
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
        request_id: Option<String>,
        expected: HandActionPrecondition,
        action: String,
        amount: Option<u64>,
    },
    LeaveTable {
        request_id: Option<String>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandActionPrecondition {
    pub table_id: String,
    pub hand_number: u64,
    pub sequence: u64,
    pub public_state_hash: String,
}

impl HandActionPrecondition {
    pub fn matches(
        &self,
        table_id: &str,
        hand_number: u64,
        sequence: u64,
        public_state_hash: &str,
    ) -> bool {
        self.table_id == table_id
            && self.hand_number == hand_number
            && self.sequence == sequence
            && self.public_state_hash == public_state_hash
    }
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
    #[error("控制命令请求 ID 必须是规范 UUID")]
    InvalidRequestId,
}

pub fn decode_command_line(line: &str) -> Result<SidecarCommand, SidecarCommandError> {
    if line.len() > MAX_COMMAND_LINE_BYTES {
        return Err(SidecarCommandError::TooLong {
            maximum: MAX_COMMAND_LINE_BYTES,
            actual: line.len(),
        });
    }
    let command: SidecarCommand = serde_json::from_str(line)?;
    if command
        .request_id()
        .is_some_and(|request_id| !is_canonical_uuid(request_id))
    {
        return Err(SidecarCommandError::InvalidRequestId);
    }
    Ok(command)
}

impl SidecarCommand {
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::EnsureIdentity { request_id, .. }
            | Self::CreateIdentity { request_id, .. }
            | Self::RestoreIdentity { request_id, .. }
            | Self::RestoreRemoteIdentity { request_id, .. }
            | Self::SubmitAction { request_id, .. }
            | Self::LeaveTable { request_id } => request_id.as_deref(),
            _ => None,
        }
    }

    #[must_use]
    pub const fn command_type(&self) -> &'static str {
        match self {
            Self::TokenSnapshot { .. } => "token_snapshot",
            Self::Dial { .. } => "dial",
            Self::UseRelay { .. } => "use_relay",
            Self::ConfigureDiscovery { .. } => "configure_discovery",
            Self::AddExternalAddress { .. } => "add_external_address",
            Self::JoinPublicPool { .. } => "join_public_pool",
            Self::CancelPublicPool => "cancel_public_pool",
            Self::EnsureIdentity { .. } => "ensure_identity",
            Self::CreateIdentity { .. } => "create_identity",
            Self::RestoreIdentity { .. } => "restore_identity",
            Self::RestoreRemoteIdentity { .. } => "restore_remote_identity",
            Self::CreateFriendRoom { .. } => "create_friend_room",
            Self::JoinFriendRoom { .. } => "join_friend_room",
            Self::ConfigureArchiveNodes { .. } => "configure_archive_nodes",
            Self::SyncStatistics => "sync_statistics",
            Self::FetchArchivedReceipt { .. } => "fetch_archived_receipt",
            Self::SubmitAction { .. } => "submit_action",
            Self::LeaveTable { .. } => "leave_table",
            Self::Shutdown => "shutdown",
        }
    }
}

fn is_canonical_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 下注必须携带完整状态条件且所有条件参与匹配() {
        assert!(decode_command_line(r#"{"type":"submit_action","action":"call"}"#).is_err());
        let expected = HandActionPrecondition {
            table_id: "table-a".to_owned(),
            hand_number: 3,
            sequence: 7,
            public_state_hash: "hash-a".to_owned(),
        };
        assert!(expected.matches("table-a", 3, 7, "hash-a"));
        assert!(!expected.matches("table-b", 3, 7, "hash-a"));
        assert!(!expected.matches("table-a", 4, 7, "hash-a"));
        assert!(!expected.matches("table-a", 3, 8, "hash-a"));
        assert!(!expected.matches("table-a", 3, 7, "hash-b"));
    }

    #[test]
    fn 命令解码接受合法消息并拒绝超长行() {
        let command =
            decode_command_line(r#"{"type":"sync_statistics"}"#).expect("合法命令必须能被解码");
        assert!(matches!(command, SidecarCommand::SyncStatistics));

        let ensure_identity = decode_command_line(
            r#"{"type":"ensure_identity","expected_account_fingerprint":"account-a","recovery_secret":"abcdefghijkl","device_label":"Windows 工作站"}"#,
        )
        .expect("自动确保身份命令必须能被解码");
        assert!(matches!(
            ensure_identity,
            SidecarCommand::EnsureIdentity { .. }
        ));

        let correlated_identity = decode_command_line(
            r#"{"type":"ensure_identity","expected_account_fingerprint":"account-a","request_id":"7c98e82f-55fd-45e4-9a62-bd26dcdebb18","recovery_secret":"abcdefghijkl","device_label":"Windows 工作站"}"#,
        )
        .expect("带请求 ID 的身份命令必须能被解码");
        assert_eq!(
            correlated_identity.request_id(),
            Some("7c98e82f-55fd-45e4-9a62-bd26dcdebb18")
        );

        let correlated_restore = decode_command_line(
            r#"{"type":"restore_identity","expected_account_fingerprint":"account-a","request_id":"332865b3-c77c-474f-a60e-263fe687540e","recovery_envelope":"THR1-envelope","recovery_secret":"abcdefghijkl","device_label":"Windows 工作站"}"#,
        )
        .expect("带请求 ID 的身份恢复命令必须能被解码");
        assert_eq!(
            correlated_restore.request_id(),
            Some("332865b3-c77c-474f-a60e-263fe687540e")
        );

        let correlated_leave = decode_command_line(
            r#"{"type":"leave_table","request_id":"4e4b4d25-e75f-4b1c-a7e8-89e953fd14ab"}"#,
        )
        .expect("带请求 ID 的离桌命令必须能被解码");
        assert_eq!(
            correlated_leave.request_id(),
            Some("4e4b4d25-e75f-4b1c-a7e8-89e953fd14ab")
        );

        assert!(matches!(
            decode_command_line(
                r#"{"type":"ensure_identity","expected_account_fingerprint":"account-a","request_id":"not-a-uuid","recovery_secret":"abcdefghijkl","device_label":"Windows 工作站"}"#,
            ),
            Err(SidecarCommandError::InvalidRequestId)
        ));

        let oversized = "x".repeat(MAX_COMMAND_LINE_BYTES + 1);
        assert!(matches!(
            decode_command_line(&oversized),
            Err(SidecarCommandError::TooLong { .. })
        ));
    }
}
