//! Strict projection of the native Dragonwilds control bridge's current roster.
//! The owning transport authenticates the server process before calling this parser.

use std::collections::{HashMap, HashSet};

use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus, RuntimePlayerIdentityKind,
};
use serde::Deserialize;

use super::super::cache::CachedLivePlayerSnapshot;

const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_PLAYERS: usize = 256;
const SOURCE: &str = "dragonwilds_native_control";

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
    players: Vec<Player>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Player {
    name: String,
    session_id: String,
    account_id: String,
    session_token: String,
    admin: bool,
    actions: Vec<String>,
}

fn hex_nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn session_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 10
        && (value.len() == 1 || !value.starts_with('0'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<i32>().is_ok_and(|id| id >= 0)
}

/// Capability lists come from the current validated native response. Recognizing
/// a player identity never adds a capability the extension did not advertise.
pub(super) fn parse(
    instance_id: &str,
    nonce: &str,
    observed_at: u64,
    body: &[u8],
) -> Result<CachedLivePlayerSnapshot, String> {
    let invalid = || String::from("The Dragonwilds control response is invalid or incomplete.");
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(String::from(
            "The Dragonwilds control response exceeds the capture limit.",
        ));
    }
    if !hex_nonce(nonce) {
        return Err(invalid());
    }
    let response: Response = serde_json::from_slice(body).map_err(|_| invalid())?;
    let observed_seconds = observed_at / 1000;
    if response.protocol != 1
        || response.request_id != nonce
        || !hex_nonce(&response.boot_id)
        || response.timestamp < observed_seconds
        || response.timestamp > observed_seconds + 6
        || !response.complete
        || response.source != SOURCE
        || response.current_players > MAX_PLAYERS
        || response.current_players != response.players.len()
        || response.error.is_some()
    {
        return Err(invalid());
    }

    let mut sessions = HashSet::new();
    let mut accounts = HashSet::new();
    let mut session_tokens = HashSet::new();
    let mut bindings = HashMap::new();
    let mut entries = Vec::with_capacity(response.players.len());
    for player in response.players {
        if player.name.trim().is_empty()
            || player.name.len() > 512
            || player.name.chars().any(char::is_control)
            || !session_id(&player.session_id)
            || !hex_nonce(&player.account_id)
            || !hex_nonce(&player.session_token)
            || !sessions.insert(player.session_id.clone())
            || !accounts.insert(player.account_id.clone())
            || !session_tokens.insert(player.session_token.clone())
        {
            return Err(invalid());
        }
        let mut advertised = HashSet::new();
        if player.actions.iter().any(|action| {
            !matches!(action.as_str(), "kick_player" | "ban_player")
                || !advertised.insert(action.as_str())
        }) {
            return Err(invalid());
        }

        let player_key = format!("dragonwilds:{}:{}", response.boot_id, player.session_token);
        let target = format!(
            "{}:{}:{}:{}",
            response.boot_id, player.session_id, player.session_token, player.account_id
        );
        for action in &player.actions {
            bindings.insert((player_key.clone(), action.clone()), target.clone());
        }
        entries.push(RuntimeLivePlayerEntry {
            player_key,
            display_name: player.name,
            identifiers: vec![
                RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::EosId,
                    value: player.account_id,
                    stable: true,
                },
                RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::SessionId,
                    value: player.session_id,
                    stable: false,
                },
            ],
            available_action_ids: player.actions,
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: player.admin.then(|| String::from("admin")),
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

#[cfg(test)]
#[path = "dragonwilds_control_response_tests.rs"]
mod tests;
