use std::collections::{HashMap, HashSet};

use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus, RuntimePlayerIdentityKind,
};
use serde::Deserialize;

use super::super::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};
use super::{BridgeError, Game, MAX_BYTES, bridge_failure, valid_nonce};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    protocol: u32,
    request_id: String,
    boot_id: String,
    timestamp: u64,
    complete: bool,
    source: String,
    current_players: usize,
    max_players: Option<usize>,
    players: Vec<Player>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Player {
    name: String,
    session_id: String,
}

pub(super) fn parse(
    game: Game,
    instance_id: &str,
    nonce: &str,
    observed_at: u64,
    body: &[u8],
) -> LivePlayerCollectionResult {
    let invalid = || bridge_failure(game, instance_id, nonce, BridgeError::Incomplete);
    if body.len() > MAX_BYTES {
        return Err(bridge_failure(game, instance_id, nonce, BridgeError::Limit));
    }
    let response: Response = serde_json::from_slice(body).map_err(|_| invalid())?;
    if !valid_nonce(nonce)
        || response.protocol != 1
        || response.request_id != nonce
        || !valid_nonce(&response.boot_id)
        || !response.complete
        || response.error.is_some()
        || response.source != "net_driver_client_connections"
        || response.current_players > 256
        || response.current_players != response.players.len()
        || response
            .max_players
            .is_some_and(|maximum| maximum > 256 || maximum < response.current_players)
        || response.timestamp < observed_at / 1000
        || response.timestamp > observed_at / 1000 + 6
    {
        return Err(invalid());
    }
    let mut seen = HashSet::new();
    let mut entries = Vec::with_capacity(response.players.len());
    for player in response.players {
        if player.name.trim().is_empty()
            || player.name.len() > 1024
            || player.name.chars().any(char::is_control)
            || player.session_id.is_empty()
            || player.session_id.len() > 128
            || !player.session_id.bytes().all(|byte| byte.is_ascii_digit())
            || !seen.insert(player.session_id.clone())
        {
            return Err(invalid());
        }
        entries.push(RuntimeLivePlayerEntry {
            player_key: format!(
                "{}:{}:{}",
                game.module_id(),
                response.boot_id,
                player.session_id
            ),
            display_name: player.name,
            identifiers: vec![RuntimeLivePlayerIdentifier {
                kind: RuntimePlayerIdentityKind::SessionId,
                value: player.session_id,
                stable: false,
            }],
            available_action_ids: Vec::new(),
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: None,
            attributes: Vec::new(),
        });
    }
    Ok(CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: nonce.to_owned(),
            instance_id: instance_id.to_owned(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::FileIpc),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: None,
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(response.current_players),
            max_players: response.max_players,
            entries,
            issue: None,
        },
        private_action_bindings: HashMap::new(),
        collected_at: observed_at,
    })
}
