use std::collections::{HashMap, HashSet};

use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus,
    RuntimePlayerIdentityKind,
};
use serde::Deserialize;

use super::{MAX_RESPONSE_BYTES, failure};
use crate::live_players::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};

const MAX_KNOWN_PLAYERS: usize = 4096;
const MAX_ONLINE_PLAYERS: usize = 256;

#[derive(Deserialize)]
struct PlayerList {
    #[serde(rename = "playerInfo")]
    players: Vec<Player>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Player {
    player_guid: String,
    player_name: String,
    in_game: bool,
}

pub(super) fn parse(
    instance_id: &str,
    body: &[u8],
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let invalid = || {
        failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "The ASTRONEER response is not a complete online-player identity list.",
            false,
        )
    };
    let limit = || {
        failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The ASTRONEER player response exceeds the collection limit.",
            true,
        )
    };
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(limit());
    }
    // Typed deserialization rejects missing fields, duplicate protocol fields,
    // invalid UTF-8 and trailing JSON; historical rows still require inGame.
    let list: PlayerList = serde_json::from_slice(body).map_err(|_| invalid())?;
    if list.players.len() > MAX_KNOWN_PLAYERS {
        return Err(limit());
    }
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for player in list.players {
        if !player.in_game {
            continue;
        }
        // Astroneer's GUID is its own opaque account key, not a Steam64 ID.
        // Preserve the exact wire value; do not parse it through a number type.
        if player.player_guid.is_empty()
            || player.player_guid.len() > 128
            || player
                .player_guid
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace())
            || player.player_name.trim().is_empty()
            || player.player_name.chars().count() > 256
            || player.player_name.chars().any(char::is_control)
            || !seen.insert(player.player_guid.clone())
        {
            return Err(invalid());
        }
        if entries.len() == MAX_ONLINE_PLAYERS {
            return Err(limit());
        }
        entries.push(RuntimeLivePlayerEntry {
            player_key: format!("astroneer:{}", player.player_guid),
            display_name: player.player_name,
            identifiers: vec![RuntimeLivePlayerIdentifier {
                kind: RuntimePlayerIdentityKind::AstroneerGuid,
                value: player.player_guid,
                stable: true,
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
            snapshot_id: request_id.to_owned(),
            instance_id: instance_id.to_owned(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::TcpConsole),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: None,
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(entries.len()),
            max_players: None,
            entries,
            issue: None,
        },
        private_action_bindings: HashMap::new(),
        collected_at: observed_at,
    })
}
