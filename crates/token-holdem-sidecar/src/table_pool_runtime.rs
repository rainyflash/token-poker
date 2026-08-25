use crate::network_address::{preferred_dial_address, should_initiate_peer_dial};
use anyhow::{Context, Result};
use libp2p::{
    gossipsub,
    multiaddr::Protocol,
    swarm::dial_opts::{DialOpts, PeerCondition},
    Multiaddr, PeerId,
};
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
use token_holdem_domain::{Chips, PlayerId, StakeLevel, TableId, TableLifecycle};
use token_holdem_identity::{DeviceCertificate, DeviceIdentity};
use token_holdem_network::{
    rank_pool_tickets, select_table_advertisement, NetworkBehaviour, PoolTicket, PoolTicketId,
    TableAdvertisement, TablePoolMessage,
};

pub(crate) const POOL_TICK_INTERVAL: Duration = Duration::from_millis(500);
const TICKET_LIFETIME_MS: u64 = 30 * 60 * 1_000;
const ADVERTISEMENT_LIFETIME_MS: u64 = 20_000;
const TICKET_RENEWAL_MARGIN_MS: u64 = 5 * 60 * 1_000;
const TICKET_REPUBLISH_INTERVAL: Duration = Duration::from_secs(5);
const ADVERTISEMENT_REPUBLISH_INTERVAL: Duration = Duration::from_secs(5);
const FIRST_CREATOR_DELAY: Duration = Duration::from_secs(4);
const CREATOR_FALLBACK_INTERVAL: Duration = Duration::from_secs(2);
const JOIN_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_MESSAGE_BYTES: usize = 128 * 1_024;
const MAX_TICKETS: usize = 256;
const MAX_ADVERTISEMENTS: usize = 128;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum TablePoolEvent {
    #[serde(rename = "pool_joined")]
    Joined {
        topic: String,
        level_id: String,
        buy_in: u64,
    },
    #[serde(rename = "pool_ticket_published")]
    TicketPublished {
        ticket_id: String,
        published_to_mesh: bool,
    },
    #[serde(rename = "pool_directory_updated")]
    DirectoryUpdated {
        discovered_tables: u16,
        waiting_players: u16,
    },
    #[serde(rename = "pool_joining_table")]
    JoiningTable {
        table_id: String,
        members: u8,
        waiting: u8,
    },
    #[serde(rename = "pool_join_attempt_expired")]
    JoinAttemptExpired { table_id: String },
    #[serde(rename = "pool_creating_table")]
    CreatingTable { table_id: String },
    #[serde(rename = "pool_table_joined")]
    TableJoined { table_id: String },
    #[serde(rename = "pool_cancelled")]
    Cancelled,
}

#[derive(Debug, Clone)]
pub(crate) enum PoolDecision {
    Join(Box<TableAdvertisement>),
    Create {
        table_id: TableId,
        creator_player_id: PlayerId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalTableAdvertisement {
    pub(crate) table_id: TableId,
    pub(crate) member_count: u8,
    pub(crate) waiting_count: u8,
    pub(crate) lifecycle: TableLifecycle,
    pub(crate) membership_version: u64,
    pub(crate) membership_hash: [u8; 32],
    pub(crate) creator_player_id: PlayerId,
    pub(crate) convergence_eligible: bool,
}

#[derive(Debug, Clone)]
enum PoolPhase {
    Searching,
    Joining {
        table_id: TableId,
        started_at: Instant,
    },
    Creating {
        table_id: TableId,
    },
    InRoom,
}

struct ActivePool {
    topic: String,
    local_ticket: PoolTicket,
    tickets: BTreeMap<PoolTicketId, PoolTicket>,
    advertisements: BTreeMap<TableId, TableAdvertisement>,
    rejected_tables: BTreeSet<TableId>,
    explicit_peers: BTreeSet<PeerId>,
    search_started_at: Instant,
    phase: PoolPhase,
    local_advertisement: Option<LocalTableAdvertisement>,
    last_ticket_published_at: Option<Instant>,
    last_advertisement_published_at: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct TablePoolRuntime {
    active: Option<ActivePool>,
}

impl TablePoolRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn join(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        topic: String,
        level: StakeLevel,
        buy_in: Chips,
        session_addresses: Vec<Multiaddr>,
        device: &DeviceIdentity,
        certificate: DeviceCertificate,
        now_unix_ms: u64,
        now_monotonic: Instant,
    ) -> Result<Vec<TablePoolEvent>> {
        let ticket = issue_ticket(
            swarm,
            level,
            buy_in,
            session_addresses,
            device,
            certificate,
            now_unix_ms,
        )?;
        if let Some(previous) = self.active.take() {
            for peer_id in previous.explicit_peers {
                swarm
                    .behaviour_mut()
                    .gossipsub
                    .remove_explicit_peer(&peer_id);
                swarm.behaviour_mut().release_peer_connection(peer_id);
            }
            swarm
                .behaviour_mut()
                .gossipsub
                .unsubscribe(&gossipsub::IdentTopic::new(previous.topic));
        }
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&gossipsub::IdentTopic::new(topic.clone()))
            .with_context(|| format!("无法订阅公开牌桌池：{topic}"))?;
        let ticket_id = ticket.id();
        let level_id = ticket.level().id().to_owned();
        let buy_in = ticket.requested_buy_in().value();
        self.active = Some(ActivePool {
            topic: topic.clone(),
            local_ticket: ticket.clone(),
            tickets: BTreeMap::from([(ticket_id, ticket)]),
            advertisements: BTreeMap::new(),
            rejected_tables: BTreeSet::new(),
            explicit_peers: BTreeSet::new(),
            search_started_at: now_monotonic,
            phase: PoolPhase::Searching,
            local_advertisement: None,
            last_ticket_published_at: None,
            last_advertisement_published_at: None,
        });
        Ok(vec![TablePoolEvent::Joined {
            topic,
            level_id,
            buy_in,
        }])
    }

    pub(crate) fn handles_topic(&self, topic: &str) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.topic == topic)
    }

    pub(crate) fn local_ticket(&self) -> Option<&PoolTicket> {
        self.active.as_ref().map(|active| &active.local_ticket)
    }

    pub(crate) fn set_local_advertisement(
        &mut self,
        advertisement: Option<LocalTableAdvertisement>,
    ) {
        if let Some(active) = self.active.as_mut() {
            if active.local_advertisement == advertisement {
                return;
            }
            active.local_advertisement = advertisement;
            active.last_advertisement_published_at = None;
        }
    }

    pub(crate) fn mark_joined(
        &mut self,
        table_id: TableId,
    ) -> Option<(TablePoolEvent, Vec<PeerId>)> {
        let active = self.active.as_mut()?;
        if matches!(active.phase, PoolPhase::InRoom) {
            return None;
        }
        let expected_table_id = match active.phase {
            PoolPhase::Joining { table_id, .. } | PoolPhase::Creating { table_id } => table_id,
            PoolPhase::Searching => return None,
            PoolPhase::InRoom => unreachable!("已在牌桌状态已提前返回"),
        };
        if expected_table_id != table_id {
            return None;
        }
        active.phase = PoolPhase::InRoom;
        active.rejected_tables.clear();
        let transferred_peers = std::mem::take(&mut active.explicit_peers)
            .into_iter()
            .collect();
        Some((
            TablePoolEvent::TableJoined {
                table_id: table_id.to_string(),
            },
            transferred_peers,
        ))
    }

    pub(crate) fn adopt_explicit_peers(
        &mut self,
        peers: impl IntoIterator<Item = PeerId>,
    ) -> Result<()> {
        // Singleton convergence transfers connection leases from the discarded
        // room into the joining pool so the admission path cannot go half-open.
        let active = self.active.as_mut().context("公开池迁移时尚未加入匹配池")?;
        anyhow::ensure!(
            matches!(active.phase, PoolPhase::Joining { .. }),
            "只有正在加入牌桌的公开池才能接管连接租约"
        );
        active.explicit_peers.extend(peers);
        Ok(())
    }

    pub(crate) fn transfer_explicit_peer(&mut self, peer_id: PeerId) {
        if let Some(active) = self.active.as_mut() {
            active.explicit_peers.remove(&peer_id);
        }
    }

    pub(crate) fn cancel(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    ) -> Option<TablePoolEvent> {
        let active = self.active.take()?;
        for peer_id in active.explicit_peers {
            swarm
                .behaviour_mut()
                .gossipsub
                .remove_explicit_peer(&peer_id);
            swarm.behaviour_mut().release_peer_connection(peer_id);
        }
        swarm
            .behaviour_mut()
            .gossipsub
            .unsubscribe(&gossipsub::IdentTopic::new(active.topic));
        Some(TablePoolEvent::Cancelled)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tick(
        &mut self,
        swarm: &mut libp2p::Swarm<NetworkBehaviour>,
        session_addresses: &[Multiaddr],
        device: &DeviceIdentity,
        certificate: &DeviceCertificate,
        now_unix_ms: u64,
        now_monotonic: Instant,
    ) -> Result<(Vec<TablePoolEvent>, Option<PoolDecision>)> {
        let Some(active) = self.active.as_mut() else {
            return Ok((Vec::new(), None));
        };
        active
            .tickets
            .retain(|_, ticket| ticket.verify_at(now_unix_ms).is_ok());
        active
            .advertisements
            .retain(|_, advertisement| advertisement.verify_at(now_unix_ms).is_ok());
        synchronize_explicit_pool_peers(active, swarm)?;

        if active
            .local_ticket
            .expires_at_unix_ms()
            .saturating_sub(now_unix_ms)
            <= TICKET_RENEWAL_MARGIN_MS
        {
            let previous_id = active.local_ticket.id();
            let renewed = issue_ticket(
                swarm,
                active.local_ticket.level().clone(),
                active.local_ticket.requested_buy_in(),
                session_addresses.to_vec(),
                device,
                certificate.clone(),
                now_unix_ms,
            )?;
            active.tickets.remove(&previous_id);
            active.tickets.insert(renewed.id(), renewed.clone());
            active.local_ticket = renewed;
            active.search_started_at = now_monotonic;
            active.last_ticket_published_at = None;
        }

        let mut events = Vec::new();
        let ticket_republish_due = active.last_ticket_published_at.is_none_or(|last| {
            now_monotonic.saturating_duration_since(last) >= TICKET_REPUBLISH_INTERVAL
        });
        if ticket_republish_due
            && matches!(
                active.phase,
                PoolPhase::Searching | PoolPhase::Joining { .. }
            )
        {
            let published_to_mesh = publish(
                swarm,
                &active.topic,
                &TablePoolMessage::Ticket(active.local_ticket.clone()),
            )?;
            active.last_ticket_published_at = Some(now_monotonic);
            events.push(TablePoolEvent::TicketPublished {
                ticket_id: active.local_ticket.id().to_string(),
                published_to_mesh,
            });
        }

        let advertisement_republish_due =
            active.last_advertisement_published_at.is_none_or(|last| {
                now_monotonic.saturating_duration_since(last) >= ADVERTISEMENT_REPUBLISH_INTERVAL
            });
        if advertisement_republish_due {
            if let Some(snapshot) = active.local_advertisement.clone() {
                let advertisement = issue_advertisement(
                    &active.local_ticket,
                    snapshot,
                    session_addresses,
                    device,
                    certificate.clone(),
                    now_unix_ms,
                )?;
                active
                    .advertisements
                    .insert(advertisement.table_id(), advertisement.clone());
                publish(
                    swarm,
                    &active.topic,
                    &TablePoolMessage::Advertisement(advertisement),
                )?;
                active.last_advertisement_published_at = Some(now_monotonic);
            }
        }

        let timed_out_table = match active.phase {
            PoolPhase::Joining {
                table_id,
                started_at,
            } if now_monotonic.saturating_duration_since(started_at) >= JOIN_ATTEMPT_TIMEOUT => {
                Some(table_id)
            }
            _ => None,
        };
        if let Some(table_id) = timed_out_table {
            active.rejected_tables.insert(table_id);
            active.phase = PoolPhase::Searching;
            events.push(TablePoolEvent::JoinAttemptExpired {
                table_id: table_id.to_string(),
            });
        }

        let decision = match active.phase {
            PoolPhase::Searching => decide_next_action(active, swarm, now_unix_ms, now_monotonic)?,
            PoolPhase::InRoom => decide_singleton_convergence(active, swarm, now_unix_ms)?,
            PoolPhase::Joining { .. } | PoolPhase::Creating { .. } => None,
        };
        if let Some(decision) = &decision {
            match decision {
                PoolDecision::Join(advertisement) => {
                    active.phase = PoolPhase::Joining {
                        table_id: advertisement.table_id(),
                        started_at: now_monotonic,
                    };
                    events.push(TablePoolEvent::JoiningTable {
                        table_id: advertisement.table_id().to_string(),
                        members: advertisement.member_count(),
                        waiting: advertisement.waiting_count(),
                    });
                }
                PoolDecision::Create { table_id, .. } => {
                    active.phase = PoolPhase::Creating {
                        table_id: *table_id,
                    };
                    events.push(TablePoolEvent::CreatingTable {
                        table_id: table_id.to_string(),
                    });
                }
            }
        }
        events.push(TablePoolEvent::DirectoryUpdated {
            discovered_tables: u16::try_from(active.advertisements.len()).unwrap_or(u16::MAX),
            waiting_players: u16::try_from(active.tickets.len()).unwrap_or(u16::MAX),
        });
        Ok((events, decision))
    }

    pub(crate) fn handle_message(
        &mut self,
        source: Option<PeerId>,
        payload: &[u8],
        now_unix_ms: u64,
    ) -> Result<Vec<TablePoolEvent>> {
        let active = self.active.as_mut().context("尚未加入公开牌桌池")?;
        if payload.is_empty() || payload.len() > MAX_MESSAGE_BYTES {
            anyhow::bail!("公开牌桌池消息必须为 1 字节到 128 KiB")
        }
        let message: TablePoolMessage =
            cbor4ii::serde::from_slice(payload).context("公开牌桌池消息不是合法 CBOR")?;
        match message {
            TablePoolMessage::Ticket(ticket) => {
                ticket.verify_at(now_unix_ms)?;
                verify_source(source, ticket.session_peer_id())?;
                if ticket.level() != active.local_ticket.level() {
                    anyhow::bail!("公开池票据的牌局级别与当前池不一致")
                }
                let ticket_id = ticket.id();
                if active.tickets.len() >= MAX_TICKETS && !active.tickets.contains_key(&ticket_id) {
                    anyhow::bail!("公开池票据缓存达到 {MAX_TICKETS} 张上限")
                }
                let is_new_ticket = !active.tickets.contains_key(&ticket_id);
                active.tickets.insert(ticket_id, ticket);
                if is_new_ticket
                    && active
                        .local_advertisement
                        .as_ref()
                        .is_some_and(|advertisement| {
                            advertisement.lifecycle != TableLifecycle::Closing
                                && advertisement.member_count < token_holdem_domain::TABLE_CAPACITY
                        })
                {
                    // When a candidate appears, an existing table with capacity
                    // responds on the next 500 ms tick instead of waiting for its
                    // lease renewal. This avoids split creation without chatty ads.
                    active.last_advertisement_published_at = None;
                }
            }
            TablePoolMessage::Advertisement(advertisement) => {
                advertisement.verify_at(now_unix_ms)?;
                verify_source(source, advertisement.admission_peer_id())?;
                if advertisement.level() != active.local_ticket.level() {
                    anyhow::bail!("牌桌广告的牌局级别与当前池不一致")
                }
                let table_id = advertisement.table_id();
                if active.advertisements.len() >= MAX_ADVERTISEMENTS
                    && !active.advertisements.contains_key(&table_id)
                {
                    anyhow::bail!("牌桌广告缓存达到 {MAX_ADVERTISEMENTS} 张上限")
                }
                let should_replace = active.advertisements.get(&table_id).is_none_or(|current| {
                    advertisement.membership_version() > current.membership_version()
                        || (advertisement.membership_version() == current.membership_version()
                            && advertisement.expires_at_unix_ms() > current.expires_at_unix_ms())
                });
                if should_replace {
                    active.advertisements.insert(table_id, advertisement);
                }
            }
        }
        Ok(vec![TablePoolEvent::DirectoryUpdated {
            discovered_tables: u16::try_from(active.advertisements.len()).unwrap_or(u16::MAX),
            waiting_players: u16::try_from(active.tickets.len()).unwrap_or(u16::MAX),
        }])
    }
}

fn decide_singleton_convergence(
    active: &mut ActivePool,
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    now_unix_ms: u64,
) -> Result<Option<PoolDecision>> {
    let Some(local) = active.local_advertisement.as_ref() else {
        return Ok(None);
    };
    if !local.convergence_eligible {
        return Ok(None);
    }

    // Discovery may migrate only a one-player table that nobody has joined.
    // Existing members, waiters, pending joins, and active hands are owned by the
    // table membership state machine and must never move at the matchmaking layer.
    let candidates = active
        .advertisements
        .values()
        .filter(|advertisement| advertisement.table_id() != local.table_id)
        .filter(|advertisement| !active.rejected_tables.contains(&advertisement.table_id()))
        .filter(|advertisement| {
            advertisement.member_count() >= 2 || advertisement.table_id() < local.table_id
        })
        .cloned()
        .collect::<Vec<_>>();
    let Some(advertisement) =
        select_table_advertisement(&candidates, active.local_ticket.level(), now_unix_ms)
    else {
        return Ok(None);
    };
    dial_advertisement(swarm, advertisement)?;
    Ok(Some(PoolDecision::Join(Box::new(advertisement.clone()))))
}

fn decide_next_action(
    active: &mut ActivePool,
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    now_unix_ms: u64,
    now_monotonic: Instant,
) -> Result<Option<PoolDecision>> {
    let candidates = active
        .advertisements
        .values()
        .filter(|advertisement| !active.rejected_tables.contains(&advertisement.table_id()))
        .cloned()
        .collect::<Vec<_>>();
    if let Some(advertisement) =
        select_table_advertisement(&candidates, active.local_ticket.level(), now_unix_ms)
    {
        dial_advertisement(swarm, advertisement)?;
        return Ok(Some(PoolDecision::Join(Box::new(advertisement.clone()))));
    }

    let ranked = rank_pool_tickets(
        active.tickets.values(),
        active.local_ticket.level(),
        now_unix_ms,
    );
    let Some(rank) = ranked
        .iter()
        .position(|ticket| ticket.id() == active.local_ticket.id())
    else {
        anyhow::bail!("本地公开池票据不在确定性候选排序中")
    };
    let rank_u32 = u32::try_from(rank).context("公开池候选排名超出范围")?;
    let due = FIRST_CREATOR_DELAY
        .checked_add(CREATOR_FALLBACK_INTERVAL.saturating_mul(rank_u32))
        .context("自动建桌等待时间溢出")?;
    if now_monotonic.saturating_duration_since(active.search_started_at) < due {
        return Ok(None);
    }
    let mut nonce = [0_u8; 32];
    OsRng.fill_bytes(&mut nonce);
    let table_id = TableId::derive(
        active.local_ticket.player_id(),
        active.local_ticket.device_public_key(),
        nonce,
    );
    Ok(Some(PoolDecision::Create {
        table_id,
        creator_player_id: active.local_ticket.player_id(),
    }))
}

#[allow(clippy::too_many_arguments)]
fn issue_ticket(
    swarm: &libp2p::Swarm<NetworkBehaviour>,
    level: StakeLevel,
    buy_in: Chips,
    session_addresses: Vec<Multiaddr>,
    device: &DeviceIdentity,
    certificate: DeviceCertificate,
    now_unix_ms: u64,
) -> Result<PoolTicket> {
    let expires_at_unix_ms = now_unix_ms
        .checked_add(TICKET_LIFETIME_MS)
        .context("公开池票据有效期溢出")?;
    let mut nonce = [0_u8; 16];
    OsRng.fill_bytes(&mut nonce);
    PoolTicket::issue(
        swarm.local_peer_id().to_bytes(),
        session_addresses
            .into_iter()
            .map(|address| address.to_vec())
            .collect(),
        level,
        buy_in,
        now_unix_ms,
        expires_at_unix_ms,
        nonce,
        device,
        certificate,
    )
    .context("无法签发公开池票据")
}

fn issue_advertisement(
    ticket: &PoolTicket,
    snapshot: LocalTableAdvertisement,
    session_addresses: &[Multiaddr],
    device: &DeviceIdentity,
    certificate: DeviceCertificate,
    now_unix_ms: u64,
) -> Result<TableAdvertisement> {
    let expires_at_unix_ms = now_unix_ms
        .checked_add(ADVERTISEMENT_LIFETIME_MS)
        .context("牌桌广告有效期溢出")?;
    let mut nonce = [0_u8; 16];
    OsRng.fill_bytes(&mut nonce);
    TableAdvertisement::issue(
        snapshot.table_id,
        ticket.level().clone(),
        snapshot.member_count,
        snapshot.waiting_count,
        snapshot.lifecycle,
        snapshot.membership_version,
        snapshot.membership_hash,
        snapshot.creator_player_id,
        ticket.session_peer_id().to_vec(),
        session_addresses.iter().map(Multiaddr::to_vec).collect(),
        now_unix_ms,
        expires_at_unix_ms,
        nonce,
        device,
        certificate,
    )
    .context("无法签发牌桌广告")
}

fn publish(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    topic: &str,
    message: &TablePoolMessage,
) -> Result<bool> {
    let payload =
        cbor4ii::serde::to_vec(Vec::new(), message).context("无法序列化公开牌桌池消息")?;
    Ok(swarm
        .behaviour_mut()
        .gossipsub
        .publish(gossipsub::IdentTopic::new(topic), payload)
        .is_ok())
}

fn verify_source(source: Option<PeerId>, expected: &[u8]) -> Result<()> {
    let source = source.context("严格签名的公开池消息缺少源 PeerId")?;
    let expected = PeerId::from_bytes(expected).context("公开池消息声明的 PeerId 无效")?;
    anyhow::ensure!(source == expected, "公开池消息源与声明的会话 PeerId 不一致");
    Ok(())
}

fn dial_advertisement(
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
    advertisement: &TableAdvertisement,
) -> Result<()> {
    let peer_id = PeerId::from_bytes(advertisement.admission_peer_id())
        .context("牌桌广告的接入 PeerId 无效")?;
    if swarm.is_connected(&peer_id)
        || peer_id == *swarm.local_peer_id()
        || !should_initiate_peer_dial(*swarm.local_peer_id(), peer_id)
    {
        return Ok(());
    }
    let address = preferred_dial_address(
        advertisement
            .admission_addresses()
            .iter()
            .map(|raw| {
                let mut address = Multiaddr::try_from(raw.clone()).context("牌桌广告地址无效")?;
                match address.pop() {
                    Some(Protocol::P2p(actual)) if actual == peer_id => Ok(address),
                    _ => anyhow::bail!("牌桌广告地址没有以接入 PeerId 结尾"),
                }
            })
            .collect::<Result<Vec<_>>>()?,
    )
    .context("牌桌广告没有可用的接入地址")?;
    let _ = swarm.dial(
        DialOpts::peer_id(peer_id)
            .condition(PeerCondition::DisconnectedAndNotDialing)
            .addresses(vec![address])
            .build(),
    );
    Ok(())
}

fn synchronize_explicit_pool_peers(
    active: &mut ActivePool,
    swarm: &mut libp2p::Swarm<NetworkBehaviour>,
) -> Result<()> {
    if matches!(active.phase, PoolPhase::InRoom) {
        return Ok(());
    }
    let local_peer_id = *swarm.local_peer_id();
    let accepts_players = active
        .local_advertisement
        .as_ref()
        .is_some_and(|advertisement| {
            advertisement.lifecycle != TableLifecycle::Closing
                && advertisement.member_count < token_holdem_domain::TABLE_CAPACITY
        });
    if !accepts_players {
        return Ok(());
    }
    for ticket in active.tickets.values() {
        let Ok(peer_id) = PeerId::from_bytes(ticket.session_peer_id()) else {
            continue;
        };
        if peer_id == local_peer_id
            || active.explicit_peers.contains(&peer_id)
            || !should_initiate_peer_dial(local_peer_id, peer_id)
        {
            continue;
        }
        if swarm.is_connected(&peer_id) {
            swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
            swarm.behaviour_mut().retain_peer_connection(peer_id);
            active.explicit_peers.insert(peer_id);
            continue;
        }
        let address = preferred_dial_address(
            ticket
                .session_addresses()
                .iter()
                .map(|raw| {
                    let mut address =
                        Multiaddr::try_from(raw.clone()).context("公开池票据地址无效")?;
                    match address.pop() {
                        Some(Protocol::P2p(actual)) if actual == peer_id => Ok(address),
                        _ => anyhow::bail!("公开池票据地址没有以会话 PeerId 结尾"),
                    }
                })
                .collect::<Result<Vec<_>>>()?,
        )
        .context("公开池票据没有可用的会话地址")?;
        let _ = swarm.dial(
            DialOpts::peer_id(peer_id)
                .condition(PeerCondition::DisconnectedAndNotDialing)
                .addresses(vec![address])
                .build(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 创建顺位延迟不会重新拆分人数池() {
        assert_eq!(FIRST_CREATOR_DELAY, Duration::from_secs(4));
        assert_eq!(CREATOR_FALLBACK_INTERVAL, Duration::from_secs(2));
        const { assert!(MAX_TICKETS > MAX_ADVERTISEMENTS) };
    }
}
