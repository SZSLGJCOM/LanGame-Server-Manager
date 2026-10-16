use std::collections::{HashMap, HashSet};

use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus, RuntimePlayerIdentityKind,
};
use serde::Deserialize;

use super::{Context, Target};
use crate::live_players::cache::CachedLivePlayerSnapshot;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Packet {
    pub protocol: u32,
    pub pid: u32,
    pub creation_time: String,
    pub run_id: String,
    pub sequence: String,
    pub tick_ms: String,
    pub complete: bool,
    pub issue: String,
    pub players: Vec<Player>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Player {
    pub session_handle: u32,
    pub generation: String,
    pub account_id: String,
    pub platform: u8,
    pub platform_id: String,
    pub name: String,
    pub moderation_immune: bool,
}

pub(super) fn decimal(text: &str) -> Result<u64, String> {
    if text.is_empty()
        || text.len() > 20
        || !text.bytes().all(|byte| byte.is_ascii_digit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return Err("The native extension returned an invalid integer.".into());
    }
    text.parse()
        .map_err(|_| "The native extension integer exceeds uint64.".into())
}

pub(super) fn nonce(text: &str) -> bool {
    text.len() == 32
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn parse(context: &Context, bytes: &[u8], now_tick: u64) -> Result<Packet, String> {
    let packet: Packet = serde_json::from_slice(bytes)
        .map_err(|_| "The native extension returned an invalid player response.")?;
    let tick = decimal(&packet.tick_ms)?;
    if packet.protocol != 1
        || packet.pid != context.pid
        || decimal(&packet.creation_time)? != context.identity.creation_time
        || !nonce(&packet.run_id)
        || decimal(&packet.sequence)? == 0
        || tick > now_tick
        || now_tick - tick > 2_000
        || packet.players.len() > 16
        || packet.issue.len() > 64
        || !packet
            .issue
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        || (packet.complete && packet.issue != "none")
    {
        return Err(
            "The native player response is stale, incomplete, or belongs to another process."
                .into(),
        );
    }
    let mut handles = HashSet::new();
    let mut slots = HashSet::new();
    let mut accounts = HashSet::new();
    for player in &packet.players {
        let account = decimal(&player.account_id)?;
        let platform_id = decimal(&player.platform_id)?;
        if player.session_handle == 0
            || player.session_handle & 0x3f >= 16
            || decimal(&player.generation)? == 0
            || account == 0
            || !handles.insert(player.session_handle)
            || !slots.insert(player.session_handle & 0x3f)
            || !accounts.insert(account)
            || player.name.is_empty()
            || player.name.len() > 256
            || player.name.chars().any(char::is_control)
            || player.platform != 3
            || platform_id == 0
            || app_core::enshrouded_steam_account_hash(platform_id) != account
        {
            return Err(
                "The native player response contains an invalid or conflicting identity.".into(),
            );
        }
    }
    Ok(packet)
}

pub(super) fn project(
    packet: Packet,
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
) -> Result<CachedLivePlayerSnapshot, String> {
    if !packet.complete {
        return Err(
            "The Enshrouded extension has not established a complete native player snapshot."
                .into(),
        );
    }
    let mut bindings = HashMap::new();
    let mut entries = Vec::with_capacity(packet.players.len());
    for player in &packet.players {
        let key = format!("enshrouded:{}:{}", player.session_handle, player.generation);
        let target = Target {
            run_id: packet.run_id.clone(),
            sequence: decimal(&packet.sequence)?,
            tick: decimal(&packet.tick_ms)?,
            handle: player.session_handle,
            generation: decimal(&player.generation)?,
            account: decimal(&player.account_id)?,
        }
        .encode();
        let actions = if player.moderation_immune {
            Vec::new()
        } else {
            vec!["kick_player".to_owned(), "ban_player".to_owned()]
        };
        for action in &actions {
            bindings.insert((key.clone(), action.clone()), target.clone());
        }
        entries.push(RuntimeLivePlayerEntry {
            player_key: key,
            display_name: player.name.clone(),
            identifiers: vec![
                RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::EnshroudedAccountId,
                    value: player.account_id.clone(),
                    stable: true,
                },
                RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::SteamId,
                    value: player.platform_id.clone(),
                    stable: true,
                },
            ],
            available_action_ids: actions,
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: None,
            attributes: vec![app_core::RuntimeLivePlayerAttribute {
                key: "moderation_immune".to_owned(),
                value: player.moderation_immune.to_string(),
            }],
        });
    }
    Ok(CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: request_id.to_owned(),
            instance_id: instance_id.to_owned(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::FileIpc),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: Some(observed_at.saturating_add(15_000)),
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(entries.len()),
            max_players: None,
            entries,
            issue: None,
        },
        private_action_bindings: bindings,
        collected_at: observed_at,
    })
}
