use crate::{decode_command_line, SidecarCommand, MAX_COMMAND_LINE_BYTES};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use thiserror::Error;

pub const RUNTIME_PROTOCOL_VERSION: u16 = 7;
const EVENT_LIMIT: usize = 4_096;
const EVENT_BYTES_LIMIT: usize = 8 * 1_024 * 1_024;
const WORKER_ARGUMENT_LIMIT: usize = 64;
const BOOTSTRAP_COMMAND_LIMIT: usize = 64;

#[derive(Debug)]
pub enum RuntimeClientRequest {
    Attach,
    ForwardWorkerCommand {
        encoded: String,
    },
    Restart {
        worker_args: Vec<String>,
        bootstrap_commands: Vec<String>,
    },
    ShutdownRuntime,
}

#[derive(Debug, Error)]
pub enum RuntimeProtocolError {
    #[error("运行时消息超过 {maximum} 字节上限，实际 {actual} 字节")]
    TooLong { maximum: usize, actual: usize },
    #[error("运行时消息不是合法 JSON：{0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("运行时消息缺少字符串 type")]
    MissingType,
    #[error("运行时协议版本不兼容：收到 {actual}，需要 {expected}")]
    IncompatibleVersion { expected: u16, actual: u16 },
    #[error("客户端不得关闭共享牌局内核")]
    WorkerShutdownForbidden,
    #[error("运行时重启参数无效：{0}")]
    InvalidRestart(String),
    #[error("牌局命令无效：{0}")]
    InvalidWorkerCommand(String),
}

#[derive(Debug, Deserialize)]
struct AttachFrame {
    protocol_version: u16,
}

#[derive(Debug, Deserialize)]
struct RestartFrame {
    worker_args: Vec<String>,
    bootstrap_commands: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEvent {
    pub generation: u64,
    pub sequence: u64,
    pub event: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeServerFrame {
    RuntimeAttached {
        protocol_version: u16,
        runtime_id: String,
        worker_pid: Option<u32>,
        generation: u64,
        latest_sequence: u64,
        earliest_sequence: u64,
        history_truncated: bool,
    },
    RuntimeEvent {
        generation: u64,
        sequence: u64,
        event: Value,
    },
    RuntimeReplayComplete {
        generation: u64,
        latest_sequence: u64,
    },
    RuntimeError {
        code: &'static str,
        message: String,
    },
}

impl From<RuntimeEvent> for RuntimeServerFrame {
    fn from(value: RuntimeEvent) -> Self {
        Self::RuntimeEvent {
            generation: value.generation,
            sequence: value.sequence,
            event: value.event,
        }
    }
}

#[derive(Debug, Clone)]
struct StoredRuntimeEvent {
    value: RuntimeEvent,
    encoded_bytes: usize,
}

#[derive(Debug, Default)]
struct SessionEventProjection {
    slots: BTreeMap<&'static str, RuntimeEvent>,
}

impl SessionEventProjection {
    fn observe(&mut self, event: &RuntimeEvent) {
        let Some(event_type) = event.event.get("type").and_then(Value::as_str) else {
            return;
        };
        match event_type {
            "identity_ready" => {
                if self
                    .slots
                    .get("identity")
                    .and_then(|entry| entry.event.get("player_id"))
                    != event.event.get("player_id")
                {
                    self.slots.remove("statistics");
                }
                self.insert("identity", event);
            }
            "identity_cleared" => {
                self.slots.remove("identity");
                self.slots.remove("statistics");
            }
            "statistics_updated" => self.insert("statistics", event),
            "pool_joined" => {
                self.clear_pool();
                self.insert("pool", event);
            }
            "pool_directory_updated" => self.insert("pool_directory", event),
            "pool_joining_table"
            | "pool_join_attempt_expired"
            | "pool_creating_table"
            | "pool_table_joined" => self.insert("pool_phase", event),
            "pool_cancelled" => self.clear_pool(),
            "friend_room_created" | "friend_room_joining" | "friend_room_joined" => {
                self.insert("friend_room", event);
            }
            "room_entered" => {
                self.clear_room();
                self.clear_hand();
                self.insert("room_entered", event);
            }
            "room_snapshot" => {
                self.insert("room_snapshot", event);
                if event.event.get("local_role").and_then(Value::as_str) != Some("leaving") {
                    self.slots.remove("safe_leave");
                }
            }
            "membership_confirmation" => self.insert("membership", event),
            "hand_roster_confirmation" => self.insert("roster", event),
            "next_hand_ready" => self.insert("next_hand", event),
            "safe_leave_requested" | "safe_leave_forced" => self.insert("safe_leave", event),
            "safe_leave_completed" => {
                self.clear_pool();
                self.clear_room();
                self.clear_hand();
            }
            "room_closed" => {
                self.clear_room();
                self.clear_hand();
            }
            "hand_protocol_started" => {
                if !self.should_replace_hand(event) {
                    return;
                }
                self.clear_hand();
                self.insert("hand_started", event);
            }
            "hand_protocol_progress" => {
                if self.should_store_hand_progress(event) {
                    self.insert("hand_progress", event);
                }
            }
            "hand_ready" => {
                if self.belongs_to_projected_hand(event) {
                    self.slots.remove("hand_progress");
                    self.insert("hand_ready", event);
                }
            }
            "hand_state" => {
                if self.belongs_to_projected_hand(event) {
                    self.slots.remove("hand_progress");
                    self.insert("hand_state", event);
                }
            }
            "hand_action_conflict" | "hand_settled" => {
                if self.belongs_to_projected_hand(event) {
                    self.slots.remove("hand_progress");
                    self.insert("hand_terminal", event);
                }
            }
            "receipt_consensus_progress" | "receipt_finalized" => {
                if self.belongs_to_projected_hand(event) {
                    self.insert("receipt", event);
                }
            }
            "hand_session_interrupted" | "hand_session_resumed" => {
                if self.belongs_to_projected_hand(event) {
                    self.insert("interruption", event);
                }
            }
            "hand_left" | "hand_aborted_for_leave" => self.clear_hand(),
            _ => {}
        }
    }

    fn insert(&mut self, slot: &'static str, event: &RuntimeEvent) {
        self.slots.insert(slot, event.clone());
    }

    fn should_replace_hand(&self, event: &RuntimeEvent) -> bool {
        let Some(current) = self.slots.get("hand_started") else {
            return true;
        };
        let (Some((current_table, current_hand)), Some((incoming_table, incoming_hand))) =
            (hand_scope(current), hand_scope(event))
        else {
            return true;
        };
        current_table == incoming_table && incoming_hand > current_hand
    }

    fn belongs_to_projected_hand(&self, event: &RuntimeEvent) -> bool {
        let Some(started) = self.slots.get("hand_started") else {
            return true;
        };
        match (hand_scope(started), hand_scope(event)) {
            (Some(left), Some(right)) => left == right,
            _ => true,
        }
    }

    fn should_store_hand_progress(&self, event: &RuntimeEvent) -> bool {
        if !self.belongs_to_projected_hand(event) {
            return false;
        }
        if let Some(scope) = hand_scope(event) {
            for slot in ["hand_ready", "hand_state", "hand_terminal"] {
                if self
                    .slots
                    .get(slot)
                    .and_then(hand_scope)
                    .is_some_and(|projected| projected == scope)
                {
                    return false;
                }
            }
        }
        let Some(current) = self.slots.get("hand_progress") else {
            return true;
        };
        if hand_scope(current) != hand_scope(event) {
            return true;
        }
        let Some(current_order) = hand_progress_order(current) else {
            return true;
        };
        let Some(incoming_order) = hand_progress_order(event) else {
            return true;
        };
        if incoming_order < current_order {
            return false;
        }
        incoming_order != current_order
            || event
                .event
                .get("completed")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                >= current
                    .event
                    .get("completed")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
    }

    fn clear(&mut self) {
        self.slots.clear();
    }

    fn clear_pool(&mut self) {
        self.slots.remove("pool");
        self.slots.remove("pool_directory");
        self.slots.remove("pool_phase");
    }

    fn clear_room(&mut self) {
        self.slots.remove("friend_room");
        self.slots.remove("room_entered");
        self.slots.remove("room_snapshot");
        self.slots.remove("membership");
        self.slots.remove("roster");
        self.slots.remove("next_hand");
        self.slots.remove("safe_leave");
    }

    fn clear_hand(&mut self) {
        self.slots.remove("hand_started");
        self.slots.remove("hand_progress");
        self.slots.remove("hand_ready");
        self.slots.remove("hand_state");
        self.slots.remove("hand_terminal");
        self.slots.remove("receipt");
        self.slots.remove("interruption");
    }
}

fn hand_scope(event: &RuntimeEvent) -> Option<(&str, u64)> {
    Some((
        event.event.get("table_id")?.as_str()?,
        event.event.get("hand_number")?.as_u64()?,
    ))
}

fn hand_progress_order(event: &RuntimeEvent) -> Option<u8> {
    match event.event.get("phase")?.as_str()? {
        "key_exchange" => Some(0),
        "shuffling" => Some(1),
        "dealing" => Some(2),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct JournalSnapshot {
    pub runtime_id: String,
    pub generation: u64,
    pub latest_sequence: u64,
    pub earliest_sequence: u64,
    pub history_truncated: bool,
    pub events: Vec<RuntimeEvent>,
}

#[derive(Debug)]
pub struct EventJournal {
    runtime_id: String,
    generation: u64,
    generation_first_sequence: u64,
    latest_sequence: u64,
    encoded_bytes: usize,
    events: VecDeque<StoredRuntimeEvent>,
    session_projection: SessionEventProjection,
    pool_active: bool,
    room_active: bool,
    hand_active: bool,
}

impl EventJournal {
    #[must_use]
    pub fn new(runtime_id: String) -> Self {
        Self {
            runtime_id,
            generation: 1,
            generation_first_sequence: 1,
            latest_sequence: 0,
            encoded_bytes: 0,
            events: VecDeque::new(),
            session_projection: SessionEventProjection::default(),
            pool_active: false,
            room_active: false,
            hand_active: false,
        }
    }

    pub fn append(&mut self, event: Value) -> Result<RuntimeEvent, RuntimeProtocolError> {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or(RuntimeProtocolError::MissingType)?;
        let encoded_bytes = serde_json::to_vec(&event)?.len();
        if encoded_bytes > MAX_COMMAND_LINE_BYTES {
            return Err(RuntimeProtocolError::TooLong {
                maximum: MAX_COMMAND_LINE_BYTES,
                actual: encoded_bytes,
            });
        }
        self.update_busy_state(event_type);
        self.latest_sequence = self.latest_sequence.saturating_add(1);
        let value = RuntimeEvent {
            generation: self.generation,
            sequence: self.latest_sequence,
            event,
        };
        self.session_projection.observe(&value);
        self.events.push_back(StoredRuntimeEvent {
            value: value.clone(),
            encoded_bytes,
        });
        self.encoded_bytes = self.encoded_bytes.saturating_add(encoded_bytes);
        while self.events.len() > EVENT_LIMIT || self.encoded_bytes > EVENT_BYTES_LIMIT {
            if let Some(removed) = self.events.pop_front() {
                self.encoded_bytes = self.encoded_bytes.saturating_sub(removed.encoded_bytes);
            } else {
                break;
            }
        }
        Ok(value)
    }

    pub fn reset_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.generation_first_sequence = self.latest_sequence.saturating_add(1);
        self.encoded_bytes = 0;
        self.events.clear();
        self.session_projection.clear();
        self.pool_active = false;
        self.room_active = false;
        self.hand_active = false;
    }

    #[must_use]
    pub fn snapshot(&self) -> JournalSnapshot {
        let earliest_sequence = self
            .events
            .front()
            .map_or(self.latest_sequence.saturating_add(1), |entry| {
                entry.value.sequence
            });
        let mut replay = self
            .session_projection
            .slots
            .values()
            .cloned()
            .map(|event| (event.sequence, event))
            .collect::<BTreeMap<_, _>>();
        for entry in &self.events {
            replay.insert(entry.value.sequence, entry.value.clone());
        }
        JournalSnapshot {
            runtime_id: self.runtime_id.clone(),
            generation: self.generation,
            latest_sequence: self.latest_sequence,
            earliest_sequence,
            history_truncated: earliest_sequence > self.generation_first_sequence,
            events: replay.into_values().collect(),
        }
    }

    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.pool_active || self.room_active || self.hand_active
    }

    fn update_busy_state(&mut self, event_type: &str) {
        match event_type {
            "pool_joined" => self.pool_active = true,
            "friend_room_created"
            | "friend_room_joining"
            | "friend_room_joined"
            | "room_entered"
            | "room_snapshot" => self.room_active = true,
            "pool_cancelled" => self.pool_active = false,
            "safe_leave_completed" => {
                self.pool_active = false;
                self.room_active = false;
                self.hand_active = false;
            }
            "room_closed" => {
                self.room_active = false;
                self.hand_active = false;
            }
            "hand_protocol_started" | "hand_ready" | "hand_state" => {
                self.hand_active = true;
                self.room_active = true;
            }
            "hand_left" | "hand_aborted_for_leave" => {
                self.hand_active = false;
            }
            _ => {}
        }
    }
}

pub fn parse_runtime_client_line(line: &str) -> Result<RuntimeClientRequest, RuntimeProtocolError> {
    if line.len() > MAX_COMMAND_LINE_BYTES {
        return Err(RuntimeProtocolError::TooLong {
            maximum: MAX_COMMAND_LINE_BYTES,
            actual: line.len(),
        });
    }
    let value: Value = serde_json::from_str(line)?;
    let message_type = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or(RuntimeProtocolError::MissingType)?;
    match message_type {
        "runtime_attach" => {
            let frame: AttachFrame = serde_json::from_value(value)?;
            if frame.protocol_version != RUNTIME_PROTOCOL_VERSION {
                return Err(RuntimeProtocolError::IncompatibleVersion {
                    expected: RUNTIME_PROTOCOL_VERSION,
                    actual: frame.protocol_version,
                });
            }
            Ok(RuntimeClientRequest::Attach)
        }
        "runtime_restart" => {
            let frame: RestartFrame = serde_json::from_value(value)?;
            validate_worker_args(&frame.worker_args)?;
            let bootstrap_commands = validate_bootstrap_commands(frame.bootstrap_commands)?;
            Ok(RuntimeClientRequest::Restart {
                worker_args: frame.worker_args,
                bootstrap_commands,
            })
        }
        "runtime_shutdown" => Ok(RuntimeClientRequest::ShutdownRuntime),
        _ => {
            let command = decode_command_line(line)
                .map_err(|error| RuntimeProtocolError::InvalidWorkerCommand(error.to_string()))?;
            if matches!(command, SidecarCommand::Shutdown) {
                return Err(RuntimeProtocolError::WorkerShutdownForbidden);
            }
            Ok(RuntimeClientRequest::ForwardWorkerCommand {
                encoded: line.to_owned(),
            })
        }
    }
}

pub fn validate_worker_args(worker_args: &[String]) -> Result<(), RuntimeProtocolError> {
    if worker_args.len() > WORKER_ARGUMENT_LIMIT {
        return Err(RuntimeProtocolError::InvalidRestart(format!(
            "牌局内核参数不得超过 {WORKER_ARGUMENT_LIMIT} 个"
        )));
    }
    for argument in worker_args {
        if argument.is_empty() || argument.len() > 4_096 {
            return Err(RuntimeProtocolError::InvalidRestart(
                "牌局内核参数为空或过长".to_owned(),
            ));
        }
        if argument == "--daemon" {
            return Err(RuntimeProtocolError::InvalidRestart(
                "共享运行时牌局内核不得使用 --daemon".to_owned(),
            ));
        }
    }
    Ok(())
}

pub fn validate_bootstrap_commands(
    commands: Vec<Value>,
) -> Result<Vec<String>, RuntimeProtocolError> {
    if commands.len() > BOOTSTRAP_COMMAND_LIMIT {
        return Err(RuntimeProtocolError::InvalidRestart(format!(
            "引导命令不得超过 {BOOTSTRAP_COMMAND_LIMIT} 条"
        )));
    }
    commands
        .into_iter()
        .map(|command| {
            let encoded = serde_json::to_string(&command)?;
            let parsed = decode_command_line(&encoded).map_err(|error| {
                RuntimeProtocolError::InvalidRestart(format!("引导命令无效：{error}"))
            })?;
            if matches!(parsed, SidecarCommand::Shutdown) {
                return Err(RuntimeProtocolError::InvalidRestart(
                    "引导命令不得关闭牌局内核".to_owned(),
                ));
            }
            Ok(encoded)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 运行时协议要求先握手并拒绝共享内核退出命令() {
        assert!(matches!(
            parse_runtime_client_line(r#"{"type":"runtime_attach","protocol_version":7}"#),
            Ok(RuntimeClientRequest::Attach)
        ));
        assert!(matches!(
            parse_runtime_client_line(r#"{"type":"shutdown"}"#),
            Err(RuntimeProtocolError::WorkerShutdownForbidden)
        ));
        assert!(matches!(
            parse_runtime_client_line(r#"{"type":"runtime_attach","protocol_version":6}"#),
            Err(RuntimeProtocolError::IncompatibleVersion { .. })
        ));
    }

    #[test]
    fn 事件日志跨代次单调编号并投影忙碌状态() {
        let mut journal = EventJournal::new("runtime-test".to_owned());
        let first = journal
            .append(serde_json::json!({"type": "pool_joined", "topic": "x"}))
            .expect("事件应写入");
        assert_eq!(first.sequence, 1);
        assert!(journal.is_busy());
        journal
            .append(serde_json::json!({"type": "hand_protocol_started"}))
            .expect("事件应写入");
        journal
            .append(serde_json::json!({"type": "hand_left"}))
            .expect("事件应写入");
        journal
            .append(serde_json::json!({"type": "pool_cancelled"}))
            .expect("事件应写入");
        assert!(journal.is_busy());
        journal
            .append(serde_json::json!({"type": "room_closed"}))
            .expect("事件应写入");
        assert!(!journal.is_busy());

        journal.reset_generation();
        let next = journal
            .append(serde_json::json!({"type": "ready"}))
            .expect("新代次事件应写入");
        assert_eq!(next.generation, 2);
        assert_eq!(next.sequence, 6);
        assert_eq!(journal.snapshot().events.len(), 1);
    }

    #[test]
    fn 事件日志截断后仍回放当前房间与手牌投影() {
        let mut journal = EventJournal::new("runtime-projection".to_owned());
        journal
            .append(serde_json::json!({
                "type": "identity_ready",
                "player_id": "player-a",
                "device_public_key": "device-a"
            }))
            .expect("身份事件应写入");
        journal
            .append(serde_json::json!({
                "type": "pool_joined",
                "level_id": "1m-2m",
                "buy_in": 80_000_000
            }))
            .expect("匹配事件应写入");
        journal
            .append(serde_json::json!({
                "type": "room_entered",
                "table_id": "table-a",
                "level_id": "1m-2m"
            }))
            .expect("入桌事件应写入");
        journal
            .append(serde_json::json!({
                "type": "room_snapshot",
                "table_id": "table-a",
                "local_role": "playing"
            }))
            .expect("房间快照应写入");
        journal
            .append(serde_json::json!({
                "type": "hand_protocol_started",
                "table_id": "table-a",
                "hand_number": 9
            }))
            .expect("手牌启动事件应写入");
        for index in 0..=EVENT_LIMIT {
            journal
                .append(serde_json::json!({"type": "warning", "message": index.to_string()}))
                .expect("填充事件应写入");
        }

        let snapshot = journal.snapshot();
        assert!(snapshot.history_truncated);
        let replayed_types = snapshot
            .events
            .iter()
            .filter_map(|event| event.event.get("type").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(replayed_types.contains(&"identity_ready"));
        assert!(replayed_types.contains(&"pool_joined"));
        assert!(replayed_types.contains(&"room_entered"));
        assert!(replayed_types.contains(&"room_snapshot"));
        assert!(replayed_types.contains(&"hand_protocol_started"));
    }

    #[test]
    fn 关闭临时房间保留匹配而安全离桌清空投影() {
        let mut journal = EventJournal::new("runtime-room-close".to_owned());
        journal
            .append(serde_json::json!({"type": "pool_joined"}))
            .expect("匹配事件应写入");
        journal
            .append(serde_json::json!({"type": "room_entered"}))
            .expect("入桌事件应写入");
        journal
            .append(serde_json::json!({"type": "room_closed"}))
            .expect("关房事件应写入");
        let projected = journal
            .session_projection
            .slots
            .values()
            .filter_map(|event| event.event.get("type").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(projected, vec!["pool_joined"]);

        journal
            .append(serde_json::json!({"type": "safe_leave_completed"}))
            .expect("安全离桌事件应写入");
        assert!(journal.session_projection.slots.is_empty());
        assert!(!journal.is_busy());
    }

    #[test]
    fn 签名离桌作废手牌后保留房间并解除手牌忙碌状态() {
        let mut journal = EventJournal::new("runtime-abandoned-hand".to_owned());
        journal
            .append(serde_json::json!({"type": "room_entered", "table_id": "table-a"}))
            .expect("入桌事件应写入");
        journal
            .append(serde_json::json!({"type": "room_snapshot", "table_id": "table-a"}))
            .expect("房间快照应写入");
        journal
            .append(serde_json::json!({"type": "hand_protocol_started", "hand_number": 4}))
            .expect("手牌启动事件应写入");
        journal
            .append(serde_json::json!({"type": "hand_aborted_for_leave", "hand_number": 4}))
            .expect("手牌作废事件应写入");

        assert!(journal.is_busy());
        let projected = journal
            .session_projection
            .slots
            .values()
            .filter_map(|event| event.event.get("type").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(projected.contains(&"room_entered"));
        assert!(projected.contains(&"room_snapshot"));
        assert!(!projected.contains(&"hand_protocol_started"));
    }

    #[test]
    fn 下注态建立后恢复投影不会回退到迟到的协议阶段() {
        let mut journal = EventJournal::new("runtime-monotonic-hand".to_owned());
        journal
            .append(serde_json::json!({
                "type": "hand_protocol_started",
                "table_id": "table-a",
                "hand_number": 3
            }))
            .expect("手牌启动事件应写入");
        journal
            .append(serde_json::json!({
                "type": "hand_protocol_progress",
                "table_id": "table-a",
                "hand_number": 3,
                "phase": "dealing",
                "completed": 1
            }))
            .expect("发牌进度应写入");
        journal
            .append(serde_json::json!({
                "type": "hand_ready",
                "table_id": "table-a",
                "hand_number": 3
            }))
            .expect("私牌就绪事件应写入");
        journal
            .append(serde_json::json!({
                "type": "hand_state",
                "table_id": "table-a",
                "hand_number": 3,
                "sequence": 0
            }))
            .expect("下注状态应写入");
        journal
            .append(serde_json::json!({
                "type": "hand_protocol_progress",
                "table_id": "table-a",
                "hand_number": 3,
                "phase": "key_exchange",
                "completed": 2
            }))
            .expect("迟到事件仍应保留为诊断历史");

        let projected = journal
            .session_projection
            .slots
            .values()
            .filter_map(|event| event.event.get("type").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            projected,
            vec!["hand_ready", "hand_protocol_started", "hand_state"]
        );
    }

    #[test]
    fn 重启请求校验牌局参数和引导命令() {
        let parsed = parse_runtime_client_line(
            r#"{"type":"runtime_restart","worker_args":["--volunteer-consent=granted"],"bootstrap_commands":[{"type":"sync_statistics"}]}"#,
        )
        .expect("合法重启请求应通过");
        assert!(matches!(parsed, RuntimeClientRequest::Restart { .. }));

        assert!(matches!(
            parse_runtime_client_line(
                r#"{"type":"runtime_restart","worker_args":["--daemon"],"bootstrap_commands":[]}"#
            ),
            Err(RuntimeProtocolError::InvalidRestart(_))
        ));
    }

    #[test]
    fn 身份切换会清除保留投影中的旧身份和战绩() {
        let mut journal = EventJournal::new("account-switch".to_owned());
        for event in [
            serde_json::json!({ "type": "identity_ready", "player_id": "a" }),
            serde_json::json!({ "type": "statistics_updated", "completed_hands": 99 }),
            serde_json::json!({ "type": "identity_cleared" }),
        ] {
            journal.append(event).unwrap();
        }
        assert!(journal.session_projection.slots.is_empty());
        for event in [
            serde_json::json!({ "type": "identity_ready", "player_id": "b" }),
            serde_json::json!({ "type": "statistics_updated", "completed_hands": 3 }),
            serde_json::json!({ "type": "identity_ready", "player_id": "c" }),
        ] {
            journal.append(event).unwrap();
        }
        assert_eq!(journal.session_projection.slots.len(), 1);
        assert_eq!(
            journal.session_projection.slots["identity"].event["player_id"],
            "c"
        );
    }
}
