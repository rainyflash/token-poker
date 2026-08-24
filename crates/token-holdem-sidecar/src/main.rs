#![forbid(unsafe_code)]

mod archive_runtime;
mod discovery_runtime;
mod hand_runtime;
mod network_address;
mod node_identity;
mod receipt_runtime;
mod relay_runtime;
mod table_pool_runtime;
mod table_session_runtime;
mod volunteer_runtime;

use anyhow::{Context, Result};
use futures::StreamExt;
use libp2p::{
    gossipsub,
    multiaddr::Protocol,
    request_response,
    swarm::{
        dial_opts::{DialOpts, PeerCondition},
        SwarmEvent,
    },
    Multiaddr, PeerId,
};
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use token_holdem_domain::{
    public_stake_level, Chips, PlayerAction, PlayerId, PlayerStatistics, StakeLevel,
};
use token_holdem_identity::{
    derive_recovery_locator, DeviceCertificate, DeviceIdentity, RecoveryEnvelope, RootIdentity,
};
use token_holdem_network::{
    add_bootstrap_address, build_swarm, decode_code, decode_code_with_payload, decode_payload,
    encode_code, encode_payload, encode_payload_code, listen, ControlRequest, ControlResponse,
    FriendRoomInvite, NetworkConfig, NetworkEvent, RelayServerLimits, PROTOCOL_VERSION,
    TABLE_POOL_TOPIC_PREFIX, TABLE_TOPIC_PREFIX,
};
use token_holdem_sidecar::{
    decode_command_line, SidecarCommand, TokenSnapshotSource, VolunteerInputs, VolunteerPolicy,
    MAX_COMMAND_LINE_BYTES,
};
use tokio_util::codec::{FramedRead, LinesCodec};
use zeroize::Zeroizing;

use archive_runtime::{parse_content_address, ArchiveEvent, ArchiveRuntime};
use discovery_runtime::{DiscoveryEvent, DiscoveryRuntime};
use hand_runtime::{HandEvent, HandExecutionContext, HandRuntime};
use network_address::is_publishable_address;
use relay_runtime::{RelayEvent, RelayRuntime};
use table_pool_runtime::{PoolDecision, TablePoolEvent, TablePoolRuntime, POOL_TICK_INTERVAL};
use table_session_runtime::{
    issue_local_session_ticket, LocalRoomRole, TableSessionEvent, TableSessionRuntime,
};
use volunteer_runtime::VolunteerRuntime;

const DEVICE_CERTIFICATE_LIFETIME_MS: u64 = 365 * 24 * 60 * 60 * 1_000;
const FRIEND_INVITE_LIFETIME_MS: u64 = 30 * 60 * 1_000;
const RECOVERY_CODE_PREFIX: &str = "THR1-";
const FRIEND_INVITE_PREFIX: &str = "TH1-";
const MAX_RECOVERY_CODE_BYTES: usize = 16_384;
const MAX_INVITE_CODE_BYTES: usize = 16_384;
const MIN_RECOVERY_SECRET_CHARS: usize = 12;
const MAX_RECOVERY_SECRET_CHARS: usize = 256;
const DEFAULT_DISCOVERY_NAMESPACE: &str = "token-holdem/v1";

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SidecarEvent {
    Ready {
        peer_id: String,
        protocol_version: &'static str,
    },
    TokenSnapshotAccepted {
        lifetime_tokens: u64,
        username: Option<String>,
        display_name: Option<String>,
        account_fingerprint: String,
        observed_at_unix_ms: u64,
        peer_verifiable: bool,
        source: TokenSnapshotSource,
    },
    IdentityReady {
        request_id: Option<String>,
        player_id: String,
        device_public_key: String,
        device_label: String,
        certificate_expires_at_unix_ms: u64,
        recovery_envelope: String,
        remote_replicas: u16,
    },
    CommandFailed {
        request_id: String,
        command_type: &'static str,
        message: String,
    },
    CommandConfirmed {
        request_id: String,
        command_type: &'static str,
    },
    ListenAddress {
        address: String,
    },
    PeerConnected {
        peer_id: String,
        connection_id: String,
        remote_address: String,
        established_connections: u32,
    },
    PeerDisconnected {
        peer_id: String,
        connection_id: String,
        remote_address: String,
        remaining_connections: u32,
        reason: String,
        retained: bool,
    },
    FriendRoomCreated {
        invite_code: String,
        room_id: String,
        buy_in: u64,
        expires_at_unix_ms: u64,
    },
    FriendRoomJoining {
        room_id: String,
        host_peer_id: String,
    },
    FriendRoomJoined {
        room_id: String,
        host_peer_id: String,
    },
    StatisticsUpdated {
        completed_hands: u64,
        won_hands: u64,
        lost_hands: u64,
        split_hands: u64,
        gross_won: u64,
        gross_lost: u64,
        net_chips: i128,
        largest_win: u64,
        largest_loss: u64,
        recent_hands: Vec<RecentHand>,
    },
    Warning {
        message: String,
    },
    ShutdownComplete,
}

#[derive(Debug, Serialize)]
struct RecentHand {
    address: String,
    receipt_id: String,
    hand_number: u64,
    level_id: String,
    players: u8,
    settled_at_unix_ms: u64,
    delta: i128,
    archived: bool,
}

struct ActiveIdentity {
    player_id: PlayerId,
    device: DeviceIdentity,
    certificate: DeviceCertificate,
    recovery_envelope: String,
    recovery_locator: [u8; 32],
    recovery_payload: Vec<u8>,
    remote_replicas: u16,
}

struct PendingRemoteRestore {
    locator: [u8; 32],
    recovery_secret: Zeroizing<String>,
    device_label: String,
}

struct PendingRoomJoin {
    room_id: String,
}

struct LocalTokenObservation {
    lifetime_tokens: u64,
    account_fingerprint: String,
    identity_binding_available: bool,
    observed_at_unix_ms: u64,
}

struct RuntimeState {
    identity: Option<ActiveIdentity>,
    pending_remote_restore: Option<PendingRemoteRestore>,
    token_observation: Option<LocalTokenObservation>,
    listen_addresses: Vec<Multiaddr>,
    external_addresses: Vec<Multiaddr>,
    pending_room_joins: HashMap<PeerId, PendingRoomJoin>,
    pool: TablePoolRuntime,
    session: TableSessionRuntime,
    hand: HandRuntime,
    archive: ArchiveRuntime,
    discovery: DiscoveryRuntime,
    relay: RelayRuntime,
    volunteer: VolunteerRuntime,
}

impl RuntimeState {
    fn new(archive_directory: Option<PathBuf>, volunteer: VolunteerRuntime) -> Result<Self> {
        Ok(Self {
            identity: None,
            pending_remote_restore: None,
            token_observation: None,
            listen_addresses: Vec::new(),
            external_addresses: Vec::new(),
            pending_room_joins: HashMap::new(),
            pool: TablePoolRuntime::default(),
            session: TableSessionRuntime::default(),
            hand: HandRuntime::default(),
            archive: ArchiveRuntime::new(archive_directory)?,
            discovery: DiscoveryRuntime::default(),
            relay: RelayRuntime::default(),
            volunteer,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let startup = startup_options_from_args()?;
    let volunteer_decision = VolunteerPolicy::evaluate(startup.volunteer_inputs);
    let discovery_server_enabled =
        startup.enable_rendezvous_server || volunteer_decision.enable_discovery_server;
    let relay_server_enabled =
        startup.enable_relay_server || volunteer_decision.enable_relay_server;
    let upnp_enabled = startup.enable_upnp || volunteer_decision.enable_upnp;
    let relay_limits = startup.relay_limits;
    let daemon = startup.daemon;
    let network_identity = startup
        .node_identity_path()
        .map(|path| node_identity::load_or_create(&path))
        .transpose()?;
    let mut swarm = build_swarm(NetworkConfig {
        enable_rendezvous_server: discovery_server_enabled,
        enable_relay_server: relay_server_enabled,
        enable_upnp: upnp_enabled,
        relay_limits,
        identity: network_identity,
    })
    .context("无法初始化 P2P 网络")?;
    if startup.listen_addresses.is_empty() {
        listen(&mut swarm).context("无法监听本地 P2P 端口")?;
    } else {
        for address in &startup.listen_addresses {
            swarm
                .listen_on(address.clone())
                .with_context(|| format!("无法监听指定 P2P 地址：{address}"))?;
        }
    }
    for address in &startup.external_addresses {
        swarm.add_external_address(address.clone());
    }
    let volunteer = VolunteerRuntime::new(
        startup.volunteer_inputs,
        volunteer_decision,
        discovery_server_enabled,
        relay_server_enabled,
        upnp_enabled,
        startup.assume_public,
        relay_limits,
    );
    let mut state = RuntimeState::new(startup.archive_directory, volunteer)?;
    state
        .external_addresses
        .extend(startup.external_addresses.iter().cloned());
    emit(&SidecarEvent::Ready {
        peer_id: swarm.local_peer_id().to_string(),
        protocol_version: PROTOCOL_VERSION,
    })?;
    if let Some(event) = state.archive.node_ready_event() {
        emit(&event)?;
    }
    emit(&state.volunteer.status())?;

    let stdin = tokio::io::stdin();
    let mut lines = FramedRead::new(
        stdin,
        LinesCodec::new_with_max_length(MAX_COMMAND_LINE_BYTES),
    );
    let mut protocol_interval = tokio::time::interval(POOL_TICK_INTERVAL);
    protocol_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            line = lines.next(), if !daemon => {
                match line {
                    Some(Ok(line)) if !line.trim().is_empty() => {
                        match decode_command_line(&line) {
                            Ok(SidecarCommand::Shutdown) => {
                                emit(&SidecarEvent::ShutdownComplete)?;
                                return Ok(());
                            }
                            Ok(command) => {
                                let request_id = command.request_id().map(str::to_owned);
                                let command_type = command.command_type();
                                match handle_command(&mut swarm, &mut state, command) {
                                    Ok(()) => {
                                        if let Some(request_id) = request_id {
                                            emit(&SidecarEvent::CommandConfirmed {
                                                request_id,
                                                command_type,
                                            })?;
                                        }
                                    }
                                    Err(error) => {
                                        if let Some(request_id) = request_id {
                                            emit(&SidecarEvent::CommandFailed {
                                                request_id,
                                                command_type,
                                                message: format!("{error:#}"),
                                            })?;
                                        }
                                        emit(&SidecarEvent::Warning {
                                            message: format!("控制命令执行失败：{error:#}"),
                                        })?;
                                    }
                                }
                            }
                            Err(error) => emit(&SidecarEvent::Warning {
                                message: format!("控制命令无效：{error}"),
                            })?,
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => emit(&SidecarEvent::Warning {
                        message: format!("读取控制命令失败：{error}"),
                    })?,
                    None => return Ok(()),
                }
            }
            event = swarm.select_next_some() => handle_network_event(&mut swarm, &mut state, event)?,
            _ = protocol_interval.tick() => protocol_tick(&mut swarm, &mut state)?,
            result = tokio::signal::ctrl_c() => {
                result.context("监听退出信号失败")?;
                emit(&SidecarEvent::ShutdownComplete)?;
                return Ok(());
            }
        }
    }
}

fn handle_command(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    command: SidecarCommand,
) -> Result<()> {
    match command {
        SidecarCommand::TokenSnapshot {
            lifetime_tokens,
            username,
            display_name,
            account_identifier,
            observed_at_unix_ms,
            source,
        } => {
            let source = source.unwrap_or_default();
            let (account_fingerprint, identity_binding_available) = account_fingerprint(
                source,
                account_identifier.as_deref(),
                username.as_deref(),
                display_name.as_deref(),
            )?;
            emit(&SidecarEvent::TokenSnapshotAccepted {
                lifetime_tokens,
                username: username.clone(),
                display_name: display_name.clone(),
                account_fingerprint: account_fingerprint.clone(),
                observed_at_unix_ms,
                peer_verifiable: false,
                source,
            })?;
            state.token_observation = Some(LocalTokenObservation {
                lifetime_tokens,
                account_fingerprint,
                identity_binding_available,
                observed_at_unix_ms,
            });
        }
        SidecarCommand::Dial { address } => dial_address(swarm, &address)?,
        SidecarCommand::UseRelay { address } => {
            let events = state.relay.add_explicit(swarm, &address)?;
            emit_relay_events(events)?;
        }
        SidecarCommand::ConfigureDiscovery {
            addresses,
            namespace,
        } => {
            let events = state.discovery.configure(
                swarm,
                addresses,
                namespace.unwrap_or_else(|| DEFAULT_DISCOVERY_NAMESPACE.to_owned()),
            )?;
            emit_discovery_events(events)?;
        }
        SidecarCommand::AddExternalAddress { address } => {
            let address = address
                .parse::<Multiaddr>()
                .with_context(|| format!("公开 P2P 地址无效：{address}"))?;
            let events = state.discovery.add_external_address(swarm, address)?;
            emit_discovery_events(events)?;
        }
        SidecarCommand::JoinPublicPool { level_id, buy_in } => {
            join_public_pool(swarm, state, &level_id, buy_in)?
        }
        SidecarCommand::CancelPublicPool => {
            anyhow::ensure!(
                !state.session.is_active(),
                "已经进入牌桌房间，请使用安全离桌"
            );
            let event = state.pool.cancel(swarm).context("当前没有可取消的匹配")?;
            emit(&event)?;
        }
        SidecarCommand::EnsureIdentity {
            request_id,
            recovery_secret,
            device_label,
        } => {
            if let Some(identity) = state.identity.as_ref() {
                emit_identity_ready(identity, request_id.as_deref())?;
            } else {
                let account_fingerprint = identity_account_fingerprint(state)?.to_owned();
                let recovery_secret = Zeroizing::new(recovery_secret);
                let identity = create_identity(
                    &recovery_secret,
                    &device_label,
                    &account_fingerprint,
                    unix_time_ms()?,
                )?;
                emit_identity_ready(&identity, request_id.as_deref())?;
                state.identity = Some(identity);
                publish_identity_recovery(swarm, state)?;
                sync_statistics(swarm, state)?;
            }
        }
        SidecarCommand::CreateIdentity {
            recovery_secret,
            device_label,
        } => {
            ensure_identity_slot_is_empty(state)?;
            let account_fingerprint = identity_account_fingerprint(state)?.to_owned();
            let recovery_secret = Zeroizing::new(recovery_secret);
            let identity = create_identity(
                &recovery_secret,
                &device_label,
                &account_fingerprint,
                unix_time_ms()?,
            )?;
            emit_identity_ready(&identity, None)?;
            state.identity = Some(identity);
            publish_identity_recovery(swarm, state)?;
            sync_statistics(swarm, state)?;
        }
        SidecarCommand::RestoreIdentity {
            recovery_envelope,
            recovery_secret,
            device_label,
        } => {
            ensure_identity_slot_is_empty(state)?;
            let account_fingerprint = identity_account_fingerprint(state)?.to_owned();
            let recovery_secret = Zeroizing::new(recovery_secret);
            let identity = restore_identity(
                &recovery_envelope,
                &recovery_secret,
                &device_label,
                &account_fingerprint,
                unix_time_ms()?,
            )?;
            emit_identity_ready(&identity, None)?;
            state.identity = Some(identity);
            publish_identity_recovery(swarm, state)?;
            sync_statistics(swarm, state)?;
        }
        SidecarCommand::RestoreRemoteIdentity {
            recovery_secret,
            device_label,
        } => {
            ensure_identity_slot_is_empty(state)?;
            if state.pending_remote_restore.is_some() {
                anyhow::bail!("已有远端身份恢复请求正在等待归档节点响应")
            }
            validate_recovery_secret(&recovery_secret)?;
            let locator =
                derive_recovery_locator(identity_account_fingerprint(state)?, &recovery_secret)
                    .context("无法派生远端身份恢复定位符")?;
            state.pending_remote_restore = Some(PendingRemoteRestore {
                locator,
                recovery_secret: Zeroizing::new(recovery_secret),
                device_label,
            });
            let events = state.archive.fetch_recovery(swarm, locator)?;
            process_archive_events(swarm, state, events)?;
        }
        SidecarCommand::CreateFriendRoom { level_id, buy_in } => {
            create_friend_room(swarm, state, &level_id, buy_in)?
        }
        SidecarCommand::JoinFriendRoom {
            invite_code,
            buy_in,
        } => join_friend_room(swarm, state, &invite_code, buy_in)?,
        SidecarCommand::ConfigureArchiveNodes {
            addresses,
            minimum_confirmed_replicas,
        } => configure_archive_nodes(swarm, state, addresses, minimum_confirmed_replicas)?,
        SidecarCommand::SyncStatistics => sync_statistics(swarm, state)?,
        SidecarCommand::FetchArchivedReceipt { address } => {
            let address = parse_content_address(&address)?;
            let events = state.archive.fetch_receipt(swarm, address)?;
            process_archive_events(swarm, state, events)?;
        }
        SidecarCommand::SubmitAction { action, amount } => {
            submit_hand_action(swarm, state, &action, amount)?
        }
        SidecarCommand::LeaveTable { .. } => leave_table(swarm, state)?,
        SidecarCommand::Shutdown => unreachable!("退出命令已在事件循环中处理"),
    }
    Ok(())
}

fn dial_address(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    address: &str,
) -> Result<()> {
    let multiaddr = address
        .parse::<Multiaddr>()
        .with_context(|| format!("P2P 地址无效：{address}"))?;
    if let Some(peer_id) = first_peer_id(&multiaddr) {
        add_bootstrap_address(swarm, peer_id, multiaddr.clone());
    }
    swarm
        .dial(multiaddr)
        .with_context(|| format!("无法拨号：{address}"))?;
    Ok(())
}

fn configure_archive_nodes(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    addresses: Vec<String>,
    minimum_confirmed_replicas: u16,
) -> Result<()> {
    if addresses.is_empty() || addresses.len() > 16 {
        anyhow::bail!("志愿归档节点地址数量必须为 1 到 16")
    }
    let mut peers = Vec::new();
    for value in addresses {
        let address = value
            .parse::<Multiaddr>()
            .with_context(|| format!("志愿归档节点地址无效：{value}"))?;
        let peer_id = first_peer_id(&address)
            .with_context(|| format!("志愿归档节点地址缺少 /p2p/<PeerId>：{value}"))?;
        add_bootstrap_address(swarm, peer_id, address.clone());
        if !swarm.is_connected(&peer_id) {
            let dial_address = strip_trailing_peer(address, peer_id)?;
            let _ = swarm.dial(
                DialOpts::peer_id(peer_id)
                    .condition(PeerCondition::DisconnectedAndNotDialing)
                    .addresses(vec![dial_address])
                    .build(),
            );
        }
        peers.push(peer_id);
    }
    let event = state
        .archive
        .configure_peers(peers, minimum_confirmed_replicas)?;
    emit(&event)?;
    if state.identity.is_some() {
        publish_identity_recovery(swarm, state)?;
        sync_statistics(swarm, state)?;
    }
    Ok(())
}

fn sync_statistics(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
) -> Result<()> {
    let player_id = state
        .identity
        .as_ref()
        .context("尚未载入持久玩家身份")?
        .player_id;
    let events = state.archive.sync_player(swarm, player_id)?;
    process_archive_events(swarm, state, events)
}

fn publish_identity_recovery(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
) -> Result<()> {
    let Some(identity) = state.identity.as_ref() else {
        return Ok(());
    };
    let locator = identity.recovery_locator;
    let payload = identity.recovery_payload.clone();
    let events = state.archive.publish_recovery(swarm, locator, payload)?;
    process_archive_events(swarm, state, events)
}

fn validate_local_buy_in(state: &RuntimeState, buy_in: u64) -> Result<()> {
    let observation = state
        .token_observation
        .as_ref()
        .context("尚未读取 Codex 官方累计 Token；请在牌桌中刷新官方账户用量")?;
    let now_unix_ms = unix_time_ms()?;
    if observation.observed_at_unix_ms > now_unix_ms.saturating_add(60_000) {
        anyhow::bail!("官方 Token 快照时间来自未来，拒绝使用")
    }
    if buy_in > observation.lifetime_tokens {
        anyhow::bail!(
            "买入额 {buy_in} 超过本机官方累计 Token {}（账户指纹 {}…）",
            observation.lifetime_tokens,
            &observation.account_fingerprint[..12],
        )
    }
    Ok(())
}

fn join_public_pool(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    level_id: &str,
    buy_in: u64,
) -> Result<()> {
    anyhow::ensure!(
        !state.session.is_active() && !state.hand.is_active(),
        "已经进入牌桌房间，请先安全离桌"
    );
    validate_local_buy_in(state, buy_in)?;
    let identity = state
        .identity
        .as_ref()
        .context("尚未载入持久玩家身份；请先在“身份与设备”创建或恢复身份")?;
    let level = stake_level(level_id)?;
    let session_addresses = collect_session_addresses(swarm, state)?;
    let topic = table_pool_topic(level_id)?;
    let events = state.pool.join(
        swarm,
        topic,
        level,
        Chips::new(buy_in),
        session_addresses,
        &identity.device,
        identity.certificate.clone(),
        unix_time_ms()?,
        Instant::now(),
    )?;
    process_pool_events(swarm, state, events)
}

fn process_pool_decision(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    decision: PoolDecision,
) -> Result<()> {
    let migrated_peers = if matches!(&decision, PoolDecision::Join(_)) && state.session.is_active()
    {
        if let Some((event, peers)) = state.session.migrate_to_pool(swarm) {
            emit(&event)?;
            peers
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    anyhow::ensure!(!state.session.is_active(), "公开池决策到达时已有牌桌会话");
    if !migrated_peers.is_empty() {
        state.pool.adopt_explicit_peers(migrated_peers)?;
    }
    let ticket = state
        .pool
        .local_ticket()
        .cloned()
        .context("公开池决策缺少本地入池票据")?;
    let identity = state.identity.as_ref().context("公开池决策缺少玩家身份")?;
    let now_unix_ms = unix_time_ms()?;
    let now_monotonic = Instant::now();
    let events = match decision {
        PoolDecision::Create {
            table_id,
            creator_player_id,
        } => state.session.create(
            swarm,
            table_id,
            creator_player_id,
            ticket,
            &identity.device,
            identity.certificate.clone(),
            now_unix_ms,
            now_monotonic,
        )?,
        PoolDecision::Join(advertisement) => state.session.join(
            swarm,
            advertisement.table_id(),
            advertisement.creator_player_id(),
            ticket,
            &identity.device,
            identity.certificate.clone(),
            now_unix_ms,
            now_monotonic,
        )?,
    };
    process_table_session_events(swarm, state, events)
}

fn protocol_tick(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
) -> Result<()> {
    state.discovery.tick(swarm);
    emit_relay_events(state.relay.tick(swarm))?;
    if state.identity.is_none() {
        return Ok(());
    }
    let now_unix_ms = unix_time_ms()?;
    let now_monotonic = Instant::now();
    let session_addresses = collect_session_addresses(swarm, state)?;
    state
        .pool
        .set_local_advertisement(state.session.advertisement());
    let (pool_events, decision) = {
        let identity = state.identity.as_ref().context("协议轮询缺少玩家身份")?;
        state.pool.tick(
            swarm,
            &session_addresses,
            &identity.device,
            &identity.certificate,
            now_unix_ms,
            now_monotonic,
        )?
    };
    process_pool_events(swarm, state, pool_events)?;
    if let Some(decision) = decision {
        process_pool_decision(swarm, state, decision)?;
    }
    // The preceding step may create a session during this tick. Resample time;
    // reusing the table-pool poll timestamp makes a newly signed join claim look
    // future-dated and removes it during first validation.
    let session_now_unix_ms = unix_time_ms()?;
    let session_now_monotonic = Instant::now();
    let session_events = {
        let identity = state
            .identity
            .as_ref()
            .context("牌桌会话轮询缺少玩家身份")?;
        state.session.tick(
            swarm,
            &session_addresses,
            &identity.device,
            &identity.certificate,
            session_now_unix_ms,
            session_now_monotonic,
        )?
    };
    process_table_session_events(swarm, state, session_events)?;
    let hand_now_unix_ms = unix_time_ms()?;
    let hand_events = {
        let identity = state.identity.as_ref().context("手牌轮询缺少玩家身份")?;
        state.hand.tick(
            swarm,
            &identity.device,
            &identity.certificate,
            hand_now_unix_ms,
        )?
    };
    process_hand_events(swarm, state, hand_events)?;
    let forced_leave_events = state
        .session
        .force_local_leave_if_due(state.hand.safe_leave_is_stalled(), Instant::now())?;
    process_table_session_events(swarm, state, forced_leave_events)
}

fn create_friend_room(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    level_id: &str,
    buy_in: u64,
) -> Result<()> {
    anyhow::ensure!(
        !state.session.is_active() && !state.hand.is_active(),
        "已经进入牌桌房间，请先安全离桌"
    );
    if let Some(event) = state.pool.cancel(swarm) {
        emit(&event)?;
    }
    validate_local_buy_in(state, buy_in)?;
    let identity = state
        .identity
        .as_ref()
        .context("尚未载入持久玩家身份；请先在“身份与设备”创建或恢复身份")?;
    let level = stake_level(level_id)?;
    let host_addresses = collect_session_addresses(swarm, state)?;
    let now = unix_time_ms()?;
    let expires_at = now
        .checked_add(FRIEND_INVITE_LIFETIME_MS)
        .context("好友房邀请有效期溢出")?;
    let mut room_secret = [0_u8; 32];
    OsRng.fill_bytes(&mut room_secret);
    let invite = FriendRoomInvite::issue(
        room_secret,
        swarm.local_peer_id().to_bytes(),
        host_addresses
            .iter()
            .map(Multiaddr::to_vec)
            .collect::<Vec<_>>(),
        level.clone(),
        now,
        expires_at,
        &identity.device,
        identity.certificate.clone(),
    )
    .context("无法签发好友房邀请")?;
    let table_id = invite.room_id();
    let room_id = table_id.to_string();
    let invite_code = encode_code(FRIEND_INVITE_PREFIX, &invite).context("无法序列化好友房邀请")?;
    emit(&SidecarEvent::FriendRoomCreated {
        invite_code,
        room_id: room_id.clone(),
        buy_in,
        expires_at_unix_ms: expires_at,
    })?;
    let ticket = issue_local_session_ticket(
        swarm,
        level,
        Chips::new(buy_in),
        host_addresses,
        &identity.device,
        identity.certificate.clone(),
        now,
    )?;
    let events = state.session.create(
        swarm,
        table_id,
        identity.player_id,
        ticket,
        &identity.device,
        identity.certificate.clone(),
        now,
        Instant::now(),
    )?;
    process_table_session_events(swarm, state, events)
}

fn join_friend_room(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    invite_code: &str,
    buy_in: u64,
) -> Result<()> {
    anyhow::ensure!(
        !state.session.is_active() && !state.hand.is_active(),
        "已经进入牌桌房间，请先安全离桌"
    );
    if let Some(event) = state.pool.cancel(swarm) {
        emit(&event)?;
    }
    validate_local_buy_in(state, buy_in)?;
    state
        .identity
        .as_ref()
        .context("尚未载入持久玩家身份；请先在“身份与设备”创建或恢复身份")?;
    let invite: FriendRoomInvite =
        decode_code(FRIEND_INVITE_PREFIX, invite_code, MAX_INVITE_CODE_BYTES)
            .context("好友房邀请码无效")?;
    invite
        .verify_at(unix_time_ms()?)
        .context("好友房邀请签名验证失败")?;
    if Chips::new(buy_in) < invite.level().minimum_buy_in()
        || Chips::new(buy_in) > invite.level().maximum_buy_in()
    {
        anyhow::bail!(
            "买入额不在好友房级别范围内：允许 {}–{}，当前选择 {}",
            invite.level().minimum_buy_in(),
            invite.level().maximum_buy_in(),
            buy_in,
        );
    }

    let host_peer_id = PeerId::from_bytes(invite.host_session_peer_id())
        .context("好友房邀请中的房主 PeerId 无效")?;
    let addresses = invite
        .host_session_addresses()
        .iter()
        .map(|raw| address_without_trailing_peer(raw, host_peer_id))
        .collect::<Result<Vec<_>>>()?;
    let table_id = invite.room_id();
    let room_id = table_id.to_string();
    let session_addresses = collect_session_addresses(swarm, state)?;
    let identity = state
        .identity
        .as_ref()
        .context("好友房加入时玩家身份缺失")?;
    let now_unix_ms = unix_time_ms()?;
    let ticket = issue_local_session_ticket(
        swarm,
        invite.level().clone(),
        Chips::new(buy_in),
        session_addresses,
        &identity.device,
        identity.certificate.clone(),
        now_unix_ms,
    )?;
    let events = state.session.join(
        swarm,
        table_id,
        invite.host_player_id(),
        ticket,
        &identity.device,
        identity.certificate.clone(),
        now_unix_ms,
        Instant::now(),
    )?;
    process_table_session_events(swarm, state, events)?;

    if swarm.is_connected(&host_peer_id) {
        emit(&SidecarEvent::FriendRoomJoined {
            room_id,
            host_peer_id: host_peer_id.to_string(),
        })?;
        return Ok(());
    }

    swarm
        .dial(
            DialOpts::peer_id(host_peer_id)
                .condition(PeerCondition::Disconnected)
                .addresses(addresses)
                .build(),
        )
        .context("无法发起好友房 P2P 连接")?;
    state.pending_room_joins.insert(
        host_peer_id,
        PendingRoomJoin {
            room_id: room_id.clone(),
        },
    );
    emit(&SidecarEvent::FriendRoomJoining {
        room_id,
        host_peer_id: host_peer_id.to_string(),
    })?;
    Ok(())
}

fn handle_network_event(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    event: SwarmEvent<NetworkEvent>,
) -> Result<()> {
    match event {
        SwarmEvent::NewListenAddr { address, .. } => {
            remember_address(&mut state.listen_addresses, address.clone());
            if is_publishable_address(&address) {
                swarm.add_external_address(address.clone());
                state.discovery.on_publishable_address_added(swarm);
            }
            emit(&SidecarEvent::ListenAddress {
                address: address.to_string(),
            })?;
        }
        SwarmEvent::ExpiredListenAddr { address, .. } => {
            forget_address(&mut state.listen_addresses, &address);
        }
        SwarmEvent::ExternalAddrConfirmed { address } => {
            remember_address(&mut state.external_addresses, address.clone());
            state.discovery.on_external_address_confirmed(swarm);
            emit(&state.volunteer.on_external_address_confirmed(&address))?;
        }
        SwarmEvent::ExternalAddrExpired { address } => {
            forget_address(&mut state.external_addresses, &address);
            emit(&state.volunteer.on_external_address_expired(&address))?;
        }
        SwarmEvent::ConnectionEstablished {
            peer_id,
            connection_id,
            endpoint,
            num_established,
            ..
        } => {
            emit(&SidecarEvent::PeerConnected {
                peer_id: peer_id.to_string(),
                connection_id: connection_id.to_string(),
                remote_address: endpoint.get_remote_address().to_string(),
                established_connections: num_established.get(),
            })?;
            if let Some(join) = state.pending_room_joins.remove(&peer_id) {
                emit(&SidecarEvent::FriendRoomJoined {
                    room_id: join.room_id,
                    host_peer_id: peer_id.to_string(),
                })?;
            }
            let hand_events = state.hand.peer_connected(peer_id)?;
            if !hand_events.is_empty() {
                process_hand_events(swarm, state, hand_events)?;
            }
            state.session.peer_connected(peer_id);
            if state.archive.is_configured_peer(peer_id) && state.identity.is_some() {
                sync_statistics(swarm, state)?;
            }
            state.discovery.on_connected(swarm, peer_id);
            emit_relay_events(state.relay.on_connected(swarm, peer_id)?)?;
        }
        SwarmEvent::Behaviour(NetworkEvent::RendezvousClient(event)) => {
            let events = state.discovery.handle_event(swarm, event);
            emit_discovery_events(events)?;
        }
        SwarmEvent::Behaviour(NetworkEvent::Autonat(event)) => {
            if let libp2p::autonat::Event::StatusChanged {
                new: libp2p::autonat::NatStatus::Public(address),
                ..
            } = &event
            {
                if is_publishable_address(address) {
                    swarm.add_external_address(address.clone());
                    state.discovery.on_publishable_address_added(swarm);
                }
            }
            if let Some(status) = state.volunteer.on_autonat(&event) {
                emit(&status)?;
            }
        }
        SwarmEvent::Behaviour(NetworkEvent::Upnp(event)) => {
            emit(&state.volunteer.on_upnp(&event))?;
        }
        SwarmEvent::Behaviour(NetworkEvent::RelayServer(event)) => {
            for volunteer_event in state.volunteer.on_relay_server(event) {
                emit(&volunteer_event)?;
            }
        }
        SwarmEvent::Behaviour(NetworkEvent::RelayClient(event)) => {
            emit(&state.relay.handle_client_event(event))?;
        }
        SwarmEvent::Behaviour(NetworkEvent::Identify(libp2p::identify::Event::Received {
            peer_id,
            info,
            ..
        })) => {
            let supports_rendezvous = info
                .protocols
                .iter()
                .any(|protocol| protocol.as_ref() == "/rendezvous/1.0.0");
            let supports_relay = info
                .protocols
                .iter()
                .any(|protocol| protocol == &libp2p::relay::HOP_PROTOCOL_NAME);
            if supports_rendezvous {
                let events =
                    state
                        .discovery
                        .observe_rendezvous_service(swarm, peer_id, &info.listen_addrs);
                emit_discovery_events(events)?;
            }
            if supports_relay {
                let events = state
                    .relay
                    .observe_identified(swarm, peer_id, &info.listen_addrs)?;
                emit_relay_events(events)?;
            }
        }
        SwarmEvent::ConnectionClosed {
            peer_id,
            connection_id,
            endpoint,
            num_established,
            cause,
            ..
        } => {
            if num_established == 0 {
                emit(&SidecarEvent::PeerDisconnected {
                    peer_id: peer_id.to_string(),
                    connection_id: connection_id.to_string(),
                    remote_address: endpoint.get_remote_address().to_string(),
                    remaining_connections: num_established,
                    reason: cause
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "远端正常关闭".to_owned()),
                    retained: swarm.behaviour().is_peer_connection_retained(&peer_id),
                })?;
                let hand_events = state.hand.peer_disconnected(peer_id)?;
                if !hand_events.is_empty() {
                    process_hand_events(swarm, state, hand_events)?;
                }
                state.session.peer_disconnected(peer_id, Instant::now());
                state.discovery.on_disconnected(peer_id);
                state.relay.on_disconnected(peer_id);
            }
        }
        SwarmEvent::Behaviour(NetworkEvent::Gossipsub(gossipsub::Event::Message {
            message,
            ..
        })) => {
            let topic = message.topic.to_string();
            if state.pool.handles_topic(&topic) {
                match state
                    .pool
                    .handle_message(message.source, &message.data, unix_time_ms()?)
                {
                    Ok(events) => process_pool_events(swarm, state, events)?,
                    Err(error) => emit(&SidecarEvent::Warning {
                        message: format!("已丢弃无效公开牌桌池消息：{error:#}"),
                    })?,
                }
            } else if state.session.handles_topic(&topic) {
                if let Some(identity) = state.identity.as_ref() {
                    match state.session.handle_message(
                        message.source,
                        &message.data,
                        &identity.device,
                        &identity.certificate,
                        unix_time_ms()?,
                        Instant::now(),
                    ) {
                        Ok(events) => process_table_session_events(swarm, state, events)?,
                        Err(error) => emit(&SidecarEvent::Warning {
                            message: format!("已丢弃无效牌桌成员消息：{error:#}"),
                        })?,
                    }
                }
            } else if topic.starts_with(TABLE_TOPIC_PREFIX) {
                if let Some(identity) = state.identity.as_ref() {
                    match state.hand.handle_public(
                        swarm,
                        &topic,
                        message.source,
                        &message.data,
                        HandExecutionContext::new(
                            unix_time_ms()?,
                            &identity.device,
                            &identity.certificate,
                        ),
                    ) {
                        Ok(events) => process_hand_events(swarm, state, events)?,
                        Err(error) => emit(&HandEvent::Warning {
                            message: format!("已丢弃无效牌桌消息：{error:#}"),
                        })?,
                    }
                }
            }
        }
        SwarmEvent::Behaviour(NetworkEvent::Control(request_response::Event::Message {
            peer,
            message,
            ..
        })) => match message {
            request_response::Message::Request {
                request, channel, ..
            } => {
                let response = match request {
                    ControlRequest::TableSession(payload) => {
                        let identity = state
                            .identity
                            .as_ref()
                            .context("牌桌成员消息到达时玩家身份缺失")?;
                        match state.session.handle_message(
                            Some(peer),
                            &payload,
                            &identity.device,
                            &identity.certificate,
                            unix_time_ms()?,
                            Instant::now(),
                        ) {
                            Ok(events) => {
                                process_table_session_events(swarm, state, events)?;
                                ControlResponse::Accepted
                            }
                            Err(error) => {
                                emit(&SidecarEvent::Warning {
                                    message: format!("已拒绝无效点对点牌桌成员消息：{error:#}"),
                                })?;
                                ControlResponse::Rejected {
                                    reason: error.to_string(),
                                }
                            }
                        }
                    }
                    ControlRequest::HandPublic(message) => {
                        match state.hand.handle_direct_public(
                            swarm,
                            peer,
                            message,
                            HandExecutionContext::new(
                                unix_time_ms()?,
                                &state
                                    .identity
                                    .as_ref()
                                    .context("牌桌会话缺少玩家身份")?
                                    .device,
                                &state
                                    .identity
                                    .as_ref()
                                    .context("牌桌会话缺少玩家身份")?
                                    .certificate,
                            ),
                        ) {
                            Ok(events) => {
                                process_hand_events(swarm, state, events)?;
                                ControlResponse::Accepted
                            }
                            Err(error) => {
                                emit(&HandEvent::Warning {
                                    message: format!("已拒绝无效点对点牌桌消息：{error:#}"),
                                })?;
                                ControlResponse::Rejected {
                                    reason: error.to_string(),
                                }
                            }
                        }
                    }
                    ControlRequest::HandPrivate(message) => {
                        match state.hand.handle_private(
                            swarm,
                            peer,
                            message,
                            unix_time_ms()?,
                            &state
                                .identity
                                .as_ref()
                                .context("牌桌会话缺少玩家身份")?
                                .device,
                            &state
                                .identity
                                .as_ref()
                                .context("牌桌会话缺少玩家身份")?
                                .certificate,
                        ) {
                            Ok(events) => {
                                process_hand_events(swarm, state, events)?;
                                ControlResponse::Accepted
                            }
                            Err(error) => {
                                emit(&HandEvent::Warning {
                                    message: format!("已拒绝无效私密牌桌消息：{error:#}"),
                                })?;
                                ControlResponse::Rejected {
                                    reason: error.to_string(),
                                }
                            }
                        }
                    }
                    request @ (ControlRequest::Archive(_)
                    | ControlRequest::FetchArchive { .. }
                    | ControlRequest::ListPlayerArchives { .. }
                    | ControlRequest::StoreRecovery(_)
                    | ControlRequest::FetchRecovery { .. }) => state
                        .archive
                        .serve_request(request, unix_time_ms()?)
                        .unwrap_or_else(|error| ControlResponse::Rejected {
                            reason: error.to_string(),
                        }),
                    _ => ControlResponse::Rejected {
                        reason: "当前 sidecar 不接受该控制请求".to_owned(),
                    },
                };
                swarm
                    .behaviour_mut()
                    .control
                    .send_response(channel, response)
                    .map_err(|_| anyhow::anyhow!("无法回复点对点控制请求"))?;
            }
            request_response::Message::Response {
                request_id,
                response,
            } => {
                let handled_by_session = state.session.handle_direct_response(
                    request_id,
                    matches!(&response, ControlResponse::Accepted),
                );
                let handled_by_hand = state.hand.handle_direct_response(
                    request_id,
                    matches!(&response, ControlResponse::Accepted),
                );
                if let Some(events) = state.archive.handle_response(
                    swarm,
                    request_id,
                    peer,
                    response.clone(),
                    unix_time_ms()?,
                )? {
                    process_archive_events(swarm, state, events)?;
                } else if handled_by_session {
                    if let ControlResponse::Rejected { reason } = response {
                        emit(&SidecarEvent::Warning {
                            message: format!("对手拒绝了牌桌共识消息：{reason}"),
                        })?;
                    }
                } else if handled_by_hand {
                    if let ControlResponse::Rejected { reason } = response {
                        emit(&HandEvent::Warning {
                            message: format!("对手拒绝了点对点牌桌消息：{reason}"),
                        })?;
                    }
                } else if let ControlResponse::Rejected { reason } = response {
                    emit(&HandEvent::Warning {
                        message: format!("对手拒绝了点对点牌桌消息：{reason}"),
                    })?;
                }
            }
        },
        SwarmEvent::Behaviour(NetworkEvent::Control(
            request_response::Event::OutboundFailure {
                peer,
                request_id,
                error,
                ..
            },
        )) => {
            if let Some(events) = state.archive.handle_failure(request_id, &error)? {
                process_archive_events(swarm, state, events)?;
            } else if state.session.handle_direct_failure(request_id) {
                // Signed Gossipsub backs table consensus. Preserve a healthy
                // connection instead of tearing down a session that still carries
                // other protocol messages after one response is lost.
            } else if state.hand.handle_direct_failure(request_id) {
                // A bounded retry queue recovers private messages. A lost response
                // does not mean the connection failed; disconnecting here would
                // manufacture a false offline event during an active hand.
            } else {
                emit(&HandEvent::Warning {
                    message: format!("向牌桌对手 {peer} 发送可靠消息失败：{error}"),
                })?;
            }
        }
        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
            let pending_room = peer_id.and_then(|peer| state.pending_room_joins.remove(&peer));
            if let Some(peer) = peer_id {
                if state.discovery.forget_unreachable_peer(swarm, peer) {
                    return Ok(());
                }
                state.relay.on_dial_failure(peer);
            }
            let summary = summarize_dial_error(&error.to_string());
            let message = if let Some(room) = pending_room {
                format!("好友房 {} 暂时无法连接：{summary}", room.room_id)
            } else if peer_id.is_some_and(|peer| {
                state.discovery.is_configured_node(peer) || state.relay.is_candidate(peer)
            }) {
                "社区引导节点暂时不可达；客户端会自动重试".to_owned()
            } else {
                return Ok(());
            };
            emit(&SidecarEvent::Warning { message })?;
        }
        _ => {}
    }
    Ok(())
}

fn create_identity(
    recovery_secret: &str,
    device_label: &str,
    account_fingerprint: &str,
    now_unix_ms: u64,
) -> Result<ActiveIdentity> {
    validate_recovery_secret(recovery_secret)?;
    let recovery_locator = derive_recovery_locator(account_fingerprint, recovery_secret)
        .context("无法派生身份恢复定位符")?;
    let root = RootIdentity::generate(&mut OsRng);
    let envelope = RecoveryEnvelope::seal_for_account(&root, recovery_secret, account_fingerprint)
        .context("无法生成 Codex 账户绑定的根身份恢复包")?;
    let recovery_payload = encode_payload(&envelope).context("无法序列化身份恢复包")?;
    activate_identity(
        root,
        envelope,
        recovery_locator,
        recovery_payload,
        device_label,
        now_unix_ms,
    )
}

fn restore_identity(
    recovery_code: &str,
    recovery_secret: &str,
    device_label: &str,
    account_fingerprint: &str,
    now_unix_ms: u64,
) -> Result<ActiveIdentity> {
    validate_recovery_secret(recovery_secret)?;
    let (envelope, recovery_payload): (RecoveryEnvelope, Vec<u8>) =
        decode_code_with_payload(RECOVERY_CODE_PREFIX, recovery_code, MAX_RECOVERY_CODE_BYTES)
            .context("身份恢复码无效")?;
    let root = envelope
        .open_for_account(recovery_secret, account_fingerprint)
        .context("无法用当前 Codex 账户解密根身份恢复包")?;
    let recovery_locator = derive_recovery_locator(account_fingerprint, recovery_secret)
        .context("无法派生身份恢复定位符")?;
    activate_identity(
        root,
        envelope,
        recovery_locator,
        recovery_payload,
        device_label,
        now_unix_ms,
    )
}

fn validate_recovery_secret(recovery_secret: &str) -> Result<()> {
    let length = recovery_secret.chars().count();
    anyhow::ensure!(
        (MIN_RECOVERY_SECRET_CHARS..=MAX_RECOVERY_SECRET_CHARS).contains(&length),
        "恢复口令长度必须为 {MIN_RECOVERY_SECRET_CHARS}–{MAX_RECOVERY_SECRET_CHARS} 个字符"
    );
    Ok(())
}

fn activate_identity(
    root: RootIdentity,
    envelope: RecoveryEnvelope,
    recovery_locator: [u8; 32],
    recovery_payload: Vec<u8>,
    device_label: &str,
    now_unix_ms: u64,
) -> Result<ActiveIdentity> {
    let player_id = root.player_id();
    let device = DeviceIdentity::generate(&mut OsRng);
    let expires_at = now_unix_ms
        .checked_add(DEVICE_CERTIFICATE_LIFETIME_MS)
        .context("设备证书有效期溢出")?;
    let certificate = root
        .issue_device_certificate(
            device.public_key(),
            device_label.trim(),
            now_unix_ms,
            expires_at,
        )
        .context("无法签发当前设备证书")?;
    let decoded_envelope: RecoveryEnvelope =
        decode_payload(&recovery_payload).context("身份恢复包载荷无效")?;
    anyhow::ensure!(decoded_envelope == envelope, "身份恢复包载荷与身份不一致");
    let recovery_envelope = encode_payload_code(RECOVERY_CODE_PREFIX, &recovery_payload);
    Ok(ActiveIdentity {
        player_id,
        device,
        certificate,
        recovery_envelope,
        recovery_locator,
        recovery_payload,
        remote_replicas: 0,
    })
}

fn emit_identity_ready(identity: &ActiveIdentity, request_id: Option<&str>) -> Result<()> {
    emit(&SidecarEvent::IdentityReady {
        request_id: request_id.map(str::to_owned),
        player_id: identity.player_id.to_string(),
        device_public_key: identity.device.public_key().to_string(),
        device_label: identity.certificate.label().to_owned(),
        certificate_expires_at_unix_ms: identity.certificate.expires_at_unix_ms(),
        recovery_envelope: identity.recovery_envelope.clone(),
        remote_replicas: identity.remote_replicas,
    })
}

fn identity_account_fingerprint(state: &RuntimeState) -> Result<&str> {
    let observation = state
        .token_observation
        .as_ref()
        .context("尚未读取 Codex 官方账户；创建或远端恢复身份前，请先刷新官方账户用量")?;
    let now_unix_ms = unix_time_ms()?;
    if observation.observed_at_unix_ms > now_unix_ms.saturating_add(60_000) {
        anyhow::bail!("Codex 官方资料快照时间来自未来，拒绝用于身份恢复")
    }
    if !observation.identity_binding_available {
        anyhow::bail!("Codex 官方资料响应没有用户名或显示名称，无法安全绑定远端身份")
    }
    Ok(&observation.account_fingerprint)
}

fn ensure_identity_slot_is_empty(state: &RuntimeState) -> Result<()> {
    if state.identity.is_some() {
        anyhow::bail!("当前进程已经载入玩家身份；为避免误覆盖，必须重启 sidecar 后再切换身份");
    }
    Ok(())
}

fn collect_session_addresses(
    swarm: &libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &RuntimeState,
) -> Result<Vec<Multiaddr>> {
    let mut result = Vec::new();
    for address in state
        .external_addresses
        .iter()
        .chain(&state.listen_addresses)
        .chain(swarm.listeners())
    {
        if !is_dialable_address(address) {
            continue;
        }
        let mut dialable = address.clone();
        let local_peer_id = *swarm.local_peer_id();
        match dialable.iter().last() {
            Some(Protocol::P2p(peer_id)) if peer_id == local_peer_id => {}
            Some(Protocol::P2p(_)) => continue,
            _ => dialable.push(Protocol::P2p(local_peer_id)),
        }
        if !result.contains(&dialable) {
            result.push(dialable);
        }
        if result.len() == 8 {
            break;
        }
    }
    if result.is_empty() {
        anyhow::bail!("当前没有可写入邀请的拨号地址；请等待监听地址或 Circuit Relay 保留就绪");
    }
    Ok(result)
}

fn address_without_trailing_peer(raw: &[u8], expected_peer_id: PeerId) -> Result<Multiaddr> {
    let mut address = Multiaddr::try_from(raw.to_vec()).context("好友房拨号地址无效")?;
    match address.pop() {
        Some(Protocol::P2p(peer_id)) if peer_id == expected_peer_id => Ok(address),
        _ => anyhow::bail!("好友房拨号地址没有以房主 PeerId 结尾"),
    }
}

fn strip_trailing_peer(mut address: Multiaddr, expected_peer_id: PeerId) -> Result<Multiaddr> {
    match address.pop() {
        Some(Protocol::P2p(peer_id)) if peer_id == expected_peer_id => Ok(address),
        _ => anyhow::bail!("节点地址没有以预期 PeerId 结尾"),
    }
}

fn is_dialable_address(address: &Multiaddr) -> bool {
    address.iter().all(|protocol| match protocol {
        Protocol::Ip4(ip) => !ip.is_unspecified(),
        Protocol::Ip6(ip) => !ip.is_unspecified(),
        _ => true,
    })
}

fn summarize_dial_error(error: &str) -> &'static str {
    if error.contains("timed out") || error.contains("超时") {
        "连接超时"
    } else if error.contains("10061") || error.contains("refused") || error.contains("拒绝") {
        "对方拒绝连接"
    } else if error.contains("10048") || error.contains("in use") || error.contains("使用一次")
    {
        "本机端口正忙"
    } else {
        "网络协商失败"
    }
}

fn remember_address(target: &mut Vec<Multiaddr>, address: Multiaddr) {
    if is_dialable_address(&address) && !target.contains(&address) {
        target.push(address);
    }
}

fn forget_address(target: &mut Vec<Multiaddr>, address: &Multiaddr) {
    target.retain(|candidate| candidate != address);
}

fn first_peer_id(address: &Multiaddr) -> Option<PeerId> {
    address.iter().find_map(|protocol| match protocol {
        Protocol::P2p(peer_id) => Some(peer_id),
        _ => None,
    })
}

fn table_pool_topic(level_id: &str) -> Result<String> {
    Ok(format!(
        "{TABLE_POOL_TOPIC_PREFIX}{}",
        normalize_level_id(level_id)?
    ))
}

fn stake_level(level_id: &str) -> Result<StakeLevel> {
    let normalized = normalize_level_id(level_id)?;
    public_stake_level(&normalized)
        .context("内置牌桌级别配置无效")?
        .with_context(|| format!("未知牌桌级别：{normalized}"))
}

fn normalize_level_id(level_id: &str) -> Result<String> {
    let normalized = level_id.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.len() > 64
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        anyhow::bail!("牌桌级别编号只能包含 1 到 64 个 ASCII 字母、数字、短横线或下划线");
    }
    Ok(normalized)
}

fn unix_time_ms() -> Result<u64> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("系统时间早于 Unix epoch")?
        .as_millis();
    u64::try_from(milliseconds).context("系统时间超出协议范围")
}

fn account_fingerprint(
    source: TokenSnapshotSource,
    account_identifier: Option<&str>,
    username: Option<&str>,
    display_name: Option<&str>,
) -> Result<(String, bool)> {
    let account_identifier = account_identifier
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if account_identifier
        .is_some_and(|value| value.len() > 512 || value.chars().any(char::is_control))
    {
        anyhow::bail!("Codex 账户标识长度或字符无效")
    }
    let username = username.map(str::trim).filter(|value| !value.is_empty());
    let display_name = display_name
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let binding = account_identifier
        .map(|value| (b"codex-account-id".as_slice(), value.to_lowercase()))
        .or_else(|| match source {
            TokenSnapshotSource::CodexAppServerAccountUsage => None,
            TokenSnapshotSource::LegacyAgentProfileObservation => username
                .map(|value| (b"username".as_slice(), value.to_ascii_lowercase()))
                .or_else(|| {
                    display_name.map(|value| (b"display-name".as_slice(), value.to_owned()))
                }),
        });
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"token-holdem/local-codex-profile/v1\0");
    if let Some((kind, value)) = &binding {
        hasher.update(kind);
        hasher.update(&(value.len() as u32).to_be_bytes());
        hasher.update(value.as_bytes());
    } else {
        hasher.update(b"unavailable");
    }
    Ok((hasher.finalize().to_hex().to_string(), binding.is_some()))
}

fn emit_discovery_events(events: impl IntoIterator<Item = DiscoveryEvent>) -> Result<()> {
    for event in events {
        emit(&event)?;
    }
    Ok(())
}

fn emit_relay_events(events: impl IntoIterator<Item = RelayEvent>) -> Result<()> {
    for event in events {
        emit(&event)?;
    }
    Ok(())
}

fn process_pool_events(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    events: impl IntoIterator<Item = TablePoolEvent>,
) -> Result<()> {
    for event in events {
        if let TablePoolEvent::JoinAttemptExpired { table_id } = &event {
            let stale_join = state.session.local_role() == Some(LocalRoomRole::Joining)
                && state
                    .session
                    .table_id()
                    .is_some_and(|active| active.to_string() == *table_id);
            if stale_join {
                if let Some(closed) = state.session.close(swarm) {
                    emit(&closed)?;
                }
            }
        }
        emit(&event)?;
    }
    Ok(())
}

fn process_table_session_events(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    events: impl IntoIterator<Item = TableSessionEvent>,
) -> Result<()> {
    for event in events {
        if let TableSessionEvent::HandAbortedForLeave {
            table_id,
            hand_number,
            ..
        } = &event
        {
            state
                .hand
                .abort_for_signed_leave(swarm, table_id, *hand_number)?;
        }
        emit(&event)?;
    }
    if state.session.local_admission_acknowledged()
        || matches!(
            state.session.local_role(),
            Some(LocalRoomRole::Waiting | LocalRoomRole::Seated | LocalRoomRole::Playing)
        )
    {
        if let Some(table_id) = state.session.table_id() {
            if let Some((event, transferred_peers)) = state.pool.mark_joined(table_id) {
                state.session.adopt_explicit_peers(transferred_peers);
                emit(&event)?;
            }
        }
    }
    if state.session.take_leave_completed() {
        complete_local_leave(swarm, state)?;
        return Ok(());
    }
    if let Some(table) = state.session.take_ready_table() {
        let identity = state
            .identity
            .as_ref()
            .context("逐手名单完成后玩家身份缺失")?;
        let events = state.hand.start(
            swarm,
            table,
            &identity.device,
            &identity.certificate,
            unix_time_ms()?,
        )?;
        process_hand_events(swarm, state, events)?;
    }
    Ok(())
}

fn submit_hand_action(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    action: &str,
    amount: Option<u64>,
) -> Result<()> {
    let action = match action {
        "fold" => PlayerAction::Fold,
        "check" => PlayerAction::Check,
        "call" => PlayerAction::Call,
        "raise" => PlayerAction::RaiseTo(Chips::new(amount.context("加注动作缺少目标筹码额")?)),
        _ => anyhow::bail!("未知牌桌动作：{action}"),
    };
    let identity = state.identity.as_ref().context("尚未载入持久玩家身份")?;
    let events = state.hand.submit_action(
        swarm,
        action,
        unix_time_ms()?,
        &identity.device,
        identity.certificate.clone(),
    )?;
    process_hand_events(swarm, state, events)
}

fn leave_table(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
) -> Result<()> {
    if state.session.is_active() {
        let identity = state.identity.as_ref().context("安全离桌时玩家身份缺失")?;
        let events = state.session.request_leave(
            swarm,
            &identity.device,
            identity.certificate.clone(),
            unix_time_ms()?,
            Instant::now(),
        )?;
        if state.hand.is_active() {
            state.hand.request_safe_leave();
        }
        return process_table_session_events(swarm, state, events);
    }
    if let Some(event) = state.pool.cancel(swarm) {
        emit(&event)?;
        return Ok(());
    }
    anyhow::bail!("当前既没有公开匹配，也没有可离开的牌桌")
}

fn complete_local_leave(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
) -> Result<()> {
    if let Some(event) = state.hand.leave(swarm) {
        emit(&event)?;
    }
    if let Some(event) = state.session.close(swarm) {
        emit(&event)?;
    }
    if let Some(event) = state.pool.cancel(swarm) {
        emit(&event)?;
    }
    Ok(())
}

fn process_hand_events(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    events: impl IntoIterator<Item = HandEvent>,
) -> Result<()> {
    for event in events {
        emit(&event)?;
    }
    let receipts = state.hand.take_newly_finalized_receipts();
    for receipt in receipts {
        let archive_events = state
            .archive
            .publish_receipt(swarm, receipt, unix_time_ms()?)?;
        process_archive_events(swarm, state, archive_events)?;
    }
    if let Some(boundary) = state.hand.take_hand_boundary()? {
        let events = state.session.on_hand_boundary(
            boundary.receipt_hash,
            boundary.dealer_seat,
            Instant::now(),
        )?;
        process_table_session_events(swarm, state, events)?;
    }
    Ok(())
}

fn process_archive_events(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
    events: impl IntoIterator<Item = ArchiveEvent>,
) -> Result<()> {
    let mut identity_status_changed = false;
    let mut remote_restore_ready = false;
    for event in events {
        match &event {
            ArchiveEvent::RecoveryBackupStored {
                locator,
                confirmed_replicas,
            } => {
                if let Some(identity) = state.identity.as_mut() {
                    if hex::encode(identity.recovery_locator) == *locator
                        && identity.remote_replicas != *confirmed_replicas
                    {
                        identity.remote_replicas = *confirmed_replicas;
                        identity_status_changed = true;
                    }
                }
            }
            ArchiveEvent::RecoveryBackupFetched { locator } => {
                remote_restore_ready |= state
                    .pending_remote_restore
                    .as_ref()
                    .is_some_and(|pending| hex::encode(pending.locator) == *locator);
            }
            ArchiveEvent::RecoveryBackupFailed { locator, .. }
                if state
                    .pending_remote_restore
                    .as_ref()
                    .is_some_and(|pending| hex::encode(pending.locator) == *locator) =>
            {
                state.pending_remote_restore = None;
            }
            _ => {}
        }
        emit(&event)?;
    }
    if identity_status_changed {
        if let Some(identity) = state.identity.as_ref() {
            emit_identity_ready(identity, None)?;
        }
    }
    if remote_restore_ready {
        if let Err(error) = complete_remote_restore(swarm, state) {
            emit(&SidecarEvent::Warning {
                message: format!("远端身份恢复失败：{error:#}"),
            })?;
        }
    }
    emit_statistics(state)
}

fn complete_remote_restore(
    swarm: &mut libp2p::Swarm<token_holdem_network::NetworkBehaviour>,
    state: &mut RuntimeState,
) -> Result<()> {
    ensure_identity_slot_is_empty(state)?;
    let locator = state
        .pending_remote_restore
        .as_ref()
        .context("远端身份恢复状态缺失")?
        .locator;
    let recovery_payload = state
        .archive
        .take_recovered_envelope(locator)
        .context("归档节点没有返回加密身份恢复包")?;
    let identity_result = (|| {
        let pending = state
            .pending_remote_restore
            .as_ref()
            .context("远端身份恢复状态缺失")?;
        let envelope: RecoveryEnvelope =
            decode_payload(&recovery_payload).context("远端身份恢复包载荷无效")?;
        let account_fingerprint = identity_account_fingerprint(state)?;
        let root = envelope
            .open_for_account(pending.recovery_secret.as_str(), account_fingerprint)
            .context("无法用当前 Codex 资料与恢复密语解密远端身份")?;
        activate_identity(
            root,
            envelope,
            locator,
            recovery_payload,
            &pending.device_label,
            unix_time_ms()?,
        )
    })();
    let identity = match identity_result {
        Ok(identity) => identity,
        Err(error) => {
            if state.archive.recovery_fetch_is_exhausted(locator) {
                state.archive.cancel_recovery_fetch(locator);
                state.pending_remote_restore = None;
                return Err(error.context("所有志愿归档节点的恢复候选均不可用"));
            }
            return Err(error.context("已拒绝一个无效恢复候选，继续等待其他归档节点"));
        }
    };
    state.archive.cancel_recovery_fetch(locator);
    state.pending_remote_restore = None;
    emit_identity_ready(&identity, None)?;
    state.identity = Some(identity);
    publish_identity_recovery(swarm, state)?;
    sync_statistics(swarm, state)
}

fn emit_statistics(state: &RuntimeState) -> Result<()> {
    let Some(identity) = state.identity.as_ref() else {
        return Ok(());
    };
    let player_id = identity.player_id;
    let receipts = state.archive.verified_receipts();
    let statistics =
        PlayerStatistics::derive(player_id, receipts.values().map(|receipt| &receipt.receipt))
            .context("无法从已验证凭证聚合战绩")?;
    let mut won_hands = 0_u64;
    let mut lost_hands = 0_u64;
    let mut split_hands = 0_u64;
    let mut recent_hands = receipts
        .iter()
        .filter_map(|(address, receipt)| {
            let outcome = receipt.receipt.outcome_for(player_id)?;
            let delta = outcome.delta().value();
            match delta.cmp(&0) {
                std::cmp::Ordering::Greater => won_hands = won_hands.saturating_add(1),
                std::cmp::Ordering::Less => lost_hands = lost_hands.saturating_add(1),
                std::cmp::Ordering::Equal => split_hands = split_hands.saturating_add(1),
            }
            Some(RecentHand {
                address: address.to_string(),
                receipt_id: hex::encode(receipt.receipt.id().as_bytes()),
                hand_number: receipt.receipt.hand_number(),
                level_id: receipt.receipt.stake_level_id().to_owned(),
                players: u8::try_from(receipt.receipt.outcomes().len()).unwrap_or(u8::MAX),
                settled_at_unix_ms: receipt.receipt.settled_at_unix_ms(),
                delta,
                archived: state.archive.is_archived(*address),
            })
        })
        .collect::<Vec<_>>();
    recent_hands.sort_by_key(|hand| std::cmp::Reverse(hand.settled_at_unix_ms));
    recent_hands.truncate(20);
    emit(&SidecarEvent::StatisticsUpdated {
        completed_hands: statistics.completed_hands,
        won_hands,
        lost_hands,
        split_hands,
        gross_won: statistics.gross_won.value(),
        gross_lost: statistics.gross_lost.value(),
        net_chips: statistics.net_chips.value(),
        largest_win: statistics.largest_win.value(),
        largest_loss: statistics.largest_loss.value(),
        recent_hands,
    })
}

struct StartupOptions {
    archive_directory: Option<PathBuf>,
    listen_addresses: Vec<Multiaddr>,
    external_addresses: Vec<Multiaddr>,
    enable_rendezvous_server: bool,
    enable_relay_server: bool,
    enable_upnp: bool,
    assume_public: bool,
    daemon: bool,
    volunteer_inputs: VolunteerInputs,
    relay_limits: RelayServerLimits,
    node_key_file: Option<PathBuf>,
}

impl StartupOptions {
    fn node_identity_path(&self) -> Option<PathBuf> {
        self.node_key_file.clone().or_else(|| {
            self.archive_directory
                .as_ref()
                .map(|directory| directory.join("libp2p-identity-key"))
        })
    }
}

fn startup_options_from_args() -> Result<StartupOptions> {
    parse_startup_options(std::env::args_os().skip(1))
}

fn parse_startup_options(
    mut arguments: impl Iterator<Item = std::ffi::OsString>,
) -> Result<StartupOptions> {
    let mut archive_directory = None;
    let mut listen_addresses = Vec::new();
    let mut external_addresses = Vec::new();
    let mut enable_rendezvous_server = false;
    let mut enable_relay_server = false;
    let mut enable_upnp = false;
    let mut assume_public = false;
    let mut daemon = false;
    let mut volunteer_inputs = VolunteerInputs::default();
    let mut relay_limits = RelayServerLimits::default();
    let mut node_key_file = None;
    while let Some(argument) = arguments.next() {
        if argument == "--rendezvous-server" {
            if enable_rendezvous_server {
                anyhow::bail!("--rendezvous-server 只能指定一次")
            }
            enable_rendezvous_server = true;
            continue;
        }
        if argument == "--relay-server" {
            if enable_relay_server {
                anyhow::bail!("--relay-server 只能指定一次")
            }
            enable_relay_server = true;
            continue;
        }
        if argument == "--upnp" {
            if enable_upnp {
                anyhow::bail!("--upnp 只能指定一次")
            }
            enable_upnp = true;
            continue;
        }
        if argument == "--public-node" {
            if assume_public {
                anyhow::bail!("--public-node 只能指定一次")
            }
            assume_public = true;
            continue;
        }
        if argument == "--daemon" {
            if daemon {
                anyhow::bail!("--daemon 只能指定一次")
            }
            daemon = true;
            continue;
        }
        if argument == "--archive-dir" {
            let value = arguments.next().context("--archive-dir 缺少目录参数")?;
            if archive_directory.replace(PathBuf::from(value)).is_some() {
                anyhow::bail!("--archive-dir 只能指定一次")
            }
            continue;
        }
        if argument == "--listen" {
            let value = arguments.next().context("--listen 缺少 Multiaddr 参数")?;
            push_listen_address(&mut listen_addresses, &value.to_string_lossy())?;
            continue;
        }
        if argument == "--external-address" {
            let value = arguments
                .next()
                .context("--external-address 缺少 Multiaddr 参数")?;
            push_external_address(&mut external_addresses, &value.to_string_lossy())?;
            continue;
        }
        if argument == "--node-key-file" {
            let value = arguments.next().context("--node-key-file 缺少路径参数")?;
            if node_key_file.replace(PathBuf::from(value)).is_some() {
                anyhow::bail!("--node-key-file 只能指定一次")
            }
            continue;
        }
        let value = argument.to_string_lossy();
        if let Some(consent) = value.strip_prefix("--volunteer-consent=") {
            volunteer_inputs.consent = consent.parse()?;
            continue;
        }
        if let Some(network_cost) = value.strip_prefix("--network-cost=") {
            volunteer_inputs.network_cost = network_cost.parse()?;
            continue;
        }
        if let Some(power_source) = value.strip_prefix("--power-source=") {
            volunteer_inputs.power_source = power_source.parse()?;
            continue;
        }
        if let Some(raw) = value.strip_prefix("--relay-max-reservations=") {
            relay_limits.max_reservations =
                parse_bounded_usize("--relay-max-reservations", raw, 1, 512)?;
            continue;
        }
        if let Some(raw) = value.strip_prefix("--relay-max-reservations-per-peer=") {
            relay_limits.max_reservations_per_peer =
                parse_bounded_usize("--relay-max-reservations-per-peer", raw, 1, 16)?;
            continue;
        }
        if let Some(raw) = value.strip_prefix("--relay-max-circuits=") {
            relay_limits.max_circuits = parse_bounded_usize("--relay-max-circuits", raw, 1, 128)?;
            continue;
        }
        if let Some(raw) = value.strip_prefix("--relay-max-circuits-per-peer=") {
            relay_limits.max_circuits_per_peer =
                parse_bounded_usize("--relay-max-circuits-per-peer", raw, 1, 16)?;
            continue;
        }
        if let Some(raw) = value.strip_prefix("--relay-reservation-seconds=") {
            relay_limits.reservation_duration = std::time::Duration::from_secs(parse_bounded_u64(
                "--relay-reservation-seconds",
                raw,
                60,
                259_200,
            )?);
            continue;
        }
        if let Some(raw) = value.strip_prefix("--relay-circuit-seconds=") {
            relay_limits.max_circuit_duration = std::time::Duration::from_secs(parse_bounded_u64(
                "--relay-circuit-seconds",
                raw,
                60,
                86_400,
            )?);
            continue;
        }
        if let Some(raw) = value.strip_prefix("--relay-circuit-bytes=") {
            relay_limits.max_circuit_bytes = parse_bounded_u64(
                "--relay-circuit-bytes",
                raw,
                64 * 1_024,
                1_024 * 1_024 * 1_024,
            )?;
            continue;
        }
        if let Some(directory) = value.strip_prefix("--archive-dir=") {
            if directory.is_empty()
                || archive_directory
                    .replace(PathBuf::from(directory))
                    .is_some()
            {
                anyhow::bail!("--archive-dir 参数无效或重复")
            }
            continue;
        }
        if let Some(address) = value.strip_prefix("--listen=") {
            push_listen_address(&mut listen_addresses, address)?;
            continue;
        }
        if let Some(address) = value.strip_prefix("--external-address=") {
            push_external_address(&mut external_addresses, address)?;
            continue;
        }
        if let Some(path) = value.strip_prefix("--node-key-file=") {
            if path.is_empty() || node_key_file.replace(PathBuf::from(path)).is_some() {
                anyhow::bail!("--node-key-file 参数无效或重复")
            }
            continue;
        }
        anyhow::bail!("未知启动参数：{value}")
    }
    if relay_limits.max_reservations_per_peer > relay_limits.max_reservations {
        anyhow::bail!("单 Peer Relay 预约上限不能超过总预约上限")
    }
    if relay_limits.max_circuits_per_peer > relay_limits.max_circuits {
        anyhow::bail!("单 Peer Circuit 上限不能超过总 Circuit 上限")
    }
    Ok(StartupOptions {
        archive_directory,
        listen_addresses,
        external_addresses,
        enable_rendezvous_server,
        enable_relay_server,
        enable_upnp,
        assume_public,
        daemon,
        volunteer_inputs,
        relay_limits,
        node_key_file,
    })
}

fn parse_bounded_usize(label: &str, raw: &str, minimum: usize, maximum: usize) -> Result<usize> {
    let value = raw
        .parse::<usize>()
        .with_context(|| format!("{label} 必须是整数"))?;
    if !(minimum..=maximum).contains(&value) {
        anyhow::bail!("{label} 必须在 {minimum} 到 {maximum} 之间")
    }
    Ok(value)
}

fn parse_bounded_u64(label: &str, raw: &str, minimum: u64, maximum: u64) -> Result<u64> {
    let value = raw
        .parse::<u64>()
        .with_context(|| format!("{label} 必须是整数"))?;
    if !(minimum..=maximum).contains(&value) {
        anyhow::bail!("{label} 必须在 {minimum} 到 {maximum} 之间")
    }
    Ok(value)
}

fn push_listen_address(addresses: &mut Vec<Multiaddr>, raw: &str) -> Result<()> {
    if addresses.len() >= 8 {
        anyhow::bail!("--listen 最多指定 8 次")
    }
    let address = raw
        .parse::<Multiaddr>()
        .with_context(|| format!("--listen 不是合法 Multiaddr：{raw}"))?;
    if address
        .iter()
        .any(|protocol| matches!(protocol, Protocol::P2p(_)))
    {
        anyhow::bail!("监听地址不能包含 /p2p/<PeerId>")
    }
    if addresses.contains(&address) {
        anyhow::bail!("监听地址重复：{address}")
    }
    addresses.push(address);
    Ok(())
}

fn push_external_address(addresses: &mut Vec<Multiaddr>, raw: &str) -> Result<()> {
    if addresses.len() >= 8 {
        anyhow::bail!("--external-address 最多指定 8 次")
    }
    let address = raw
        .parse::<Multiaddr>()
        .with_context(|| format!("--external-address 不是合法 Multiaddr：{raw}"))?;
    if address.is_empty()
        || address
            .iter()
            .any(|protocol| matches!(protocol, Protocol::P2p(_)))
    {
        anyhow::bail!("外部地址不能为空，也不能包含 /p2p/<PeerId>")
    }
    if !is_publishable_address(&address) {
        anyhow::bail!("外部地址必须是公网 DNS/IP 或 Circuit Relay 地址")
    }
    if addresses.contains(&address) {
        anyhow::bail!("外部地址重复：{address}")
    }
    addresses.push(address);
    Ok(())
}

fn emit(event: &impl Serialize) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, event).context("序列化 sidecar 事件失败")?;
    stdout.write_all(b"\n").context("写入 sidecar 事件失败")?;
    stdout.flush().context("刷新 sidecar 事件失败")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_holdem_sidecar::{HostNetworkCost, PowerSource, VolunteerConsent};

    #[test]
    fn 匹配频道编号必须可安全拼接() {
        assert_eq!(normalize_level_id(" 10K-20K ").unwrap(), "10k-20k");
        assert!(normalize_level_id("../escape").is_err());
        assert!(normalize_level_id("中文").is_err());
    }

    #[test]
    fn sidecar接受极限推理级别() {
        let level = stake_level(" 100M-200M ").expect("极限推理级别必须进入匹配目录");

        assert_eq!(level.small_blind(), Chips::new(100_000_000));
        assert_eq!(level.big_blind(), Chips::new(200_000_000));
        assert_eq!(level.minimum_buy_in(), Chips::new(8_000_000_000));
        assert_eq!(level.maximum_buy_in(), Chips::new(20_000_000_000));
    }

    #[test]
    fn 恢复包代码可以还原同一玩家根身份() {
        let account = "ab".repeat(32);
        let created = create_identity(
            "correct horse battery staple",
            "Windows 工作站",
            &account,
            1_000,
        )
        .expect("身份应创建成功");
        let player_id = created.player_id;
        let restored = restore_identity(
            &created.recovery_envelope,
            "correct horse battery staple",
            "第二台设备",
            &account,
            2_000,
        )
        .expect("身份应恢复成功");

        assert_eq!(restored.player_id, player_id);
        assert_ne!(restored.device.public_key(), created.device.public_key());
        assert_eq!(
            created.certificate.expires_at_unix_ms() - created.certificate.issued_at_unix_ms(),
            DEVICE_CERTIFICATE_LIFETIME_MS
        );
    }

    #[test]
    fn 恢复口令在加密前执行长度上限校验() {
        assert!(validate_recovery_secret("不足十二位").is_err());
        assert!(validate_recovery_secret(&"密".repeat(257)).is_err());
        assert!(validate_recovery_secret("这是一个超过十二字符的恢复口令").is_ok());
    }

    #[test]
    fn 官方账户标识优先生成稳定且不含明文的指纹() {
        let (first, first_bound) = account_fingerprint(
            TokenSnapshotSource::CodexAppServerAccountUsage,
            Some("chatgpt-email:Player@Example.com"),
            Some("可变用户名"),
            Some("可变名称"),
        )
        .expect("账户指纹应生成");
        let (second, second_bound) = account_fingerprint(
            TokenSnapshotSource::CodexAppServerAccountUsage,
            Some("chatgpt-email:player@example.com"),
            Some("另一个用户名"),
            None,
        )
        .expect("规范化账户指纹应生成");

        assert!(first_bound && second_bound);
        assert_eq!(first, second);
        assert!(!first.contains("player@example.com"));
        assert!(account_fingerprint(
            TokenSnapshotSource::CodexAppServerAccountUsage,
            Some(&"x".repeat(513)),
            None,
            None,
        )
        .is_err());
    }

    #[test]
    fn 官方来源缺少账户标识时不使用展示名伪造绑定() {
        let (_, bound) = account_fingerprint(
            TokenSnapshotSource::CodexAppServerAccountUsage,
            None,
            Some("可能重复的用户名"),
            Some("Codex 账户"),
        )
        .expect("缺少绑定时仍应生成不可用占位指纹");

        assert!(!bound);
    }

    #[test]
    fn 启动参数支持固定监听地址且拒绝携带远端_peer_id() {
        let options = parse_startup_options(
            [
                "--archive-dir",
                "archive-data",
                "--rendezvous-server",
                "--relay-server",
                "--upnp",
                "--public-node",
                "--daemon",
                "--volunteer-consent=granted",
                "--network-cost=unmetered",
                "--power-source=ac",
                "--relay-max-reservations=32",
                "--relay-max-circuits=8",
                "--listen=/ip4/0.0.0.0/tcp/4001",
                "--listen",
                "/ip4/0.0.0.0/udp/4001/quic-v1",
                "--external-address=/dns4/poker.example.com/tcp/4001",
                "--external-address",
                "/dns4/poker.example.com/udp/4001/quic-v1",
            ]
            .into_iter()
            .map(std::ffi::OsString::from),
        )
        .unwrap();
        assert_eq!(
            options.archive_directory,
            Some(PathBuf::from("archive-data"))
        );
        assert_eq!(options.listen_addresses.len(), 2);
        assert_eq!(options.external_addresses.len(), 2);
        assert!(options.enable_rendezvous_server);
        assert!(options.enable_relay_server);
        assert!(options.enable_upnp);
        assert!(options.assume_public);
        assert!(options.daemon);
        assert_eq!(options.volunteer_inputs.consent, VolunteerConsent::Granted);
        assert_eq!(
            options.volunteer_inputs.network_cost,
            HostNetworkCost::Unmetered
        );
        assert_eq!(options.volunteer_inputs.power_source, PowerSource::Ac);
        assert_eq!(options.relay_limits.max_reservations, 32);
        assert_eq!(options.relay_limits.max_circuits, 8);
        assert_eq!(
            options.node_identity_path(),
            Some(PathBuf::from("archive-data").join("libp2p-identity-key"))
        );

        assert!(parse_startup_options(
            ["--listen", "/ip4/0.0.0.0/tcp/4001/p2p/12D3KooWBad"]
                .into_iter()
                .map(std::ffi::OsString::from),
        )
        .is_err());
        assert!(parse_startup_options(
            ["--external-address", "/ip4/127.0.0.1/tcp/4001"]
                .into_iter()
                .map(std::ffi::OsString::from),
        )
        .is_err());
    }
}
