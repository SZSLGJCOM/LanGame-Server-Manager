use std::collections::{HashMap, HashSet};

use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus,
    RuntimePlayerIdentityKind,
};

use super::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};
use super::service::failed_snapshot;

const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_PLAYERS: usize = 1024;

/// The Mojang 26.2 server's ListPlayersCommand uses commands.list.players and
/// commands.list.nameAndId from en_us.json. Fixtures are synthesized from that
/// package contract, not presented as captures from connected players.
pub(crate) fn parse_minecraft(
    instance_id: &str,
    text: &str,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let failure = |code, summary, truncated| {
        Box::new(failed_snapshot(
            instance_id,
            request_id.to_owned(),
            ModulePlayerListSource::RuntimeAction,
            code,
            summary,
            truncated,
        ))
    };
    if text.len() > MAX_RESPONSE_BYTES {
        return Err(failure(
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The Minecraft player response exceeds the collection limit.",
            true,
        ));
    }
    let invalid = || {
        failure(
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "The Minecraft response is not a complete list of player UUIDs.",
            false,
        )
    };
    let text = text.trim();
    let (counts, players) = text
        .strip_prefix("There are ")
        .and_then(|text| text.split_once(" players online:"))
        .ok_or_else(invalid)?;
    let (count, maximum) = counts.split_once(" of a max of ").ok_or_else(invalid)?;
    let count = parse_count(count).ok_or_else(invalid)?;
    let maximum = parse_count(maximum).ok_or_else(invalid)?;
    if count > MAX_PLAYERS {
        return Err(failure(
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The Minecraft player count exceeds the collection limit.",
            true,
        ));
    }
    let mut entries = Vec::with_capacity(count);
    let mut identifiers = HashSet::with_capacity(count);
    let players = players.trim();
    if !players.is_empty() {
        for row in players.split(", ") {
            let (name, identifier) = row
                .strip_suffix(')')
                .and_then(|row| row.rsplit_once(" ("))
                .ok_or_else(invalid)?;
            if name.is_empty()
                || name.chars().count() > 256
                || name.chars().any(char::is_control)
                || identifier.len() != 36
            {
                return Err(invalid());
            }
            let identifier = uuid::Uuid::parse_str(identifier).map_err(|_| invalid())?;
            if identifier.is_nil() || !identifiers.insert(identifier) || entries.len() >= count {
                return Err(invalid());
            }
            let identifier = identifier.hyphenated().to_string();
            entries.push(RuntimeLivePlayerEntry {
                player_key: identifier.clone(),
                display_name: name.to_owned(),
                identifiers: vec![RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::MinecraftUuid,
                    value: identifier,
                    stable: true,
                }],
                available_action_ids: Vec::new(),
                ping_ms: None,
                session_started_at_unix_ms: None,
                role: None,
                attributes: Vec::new(),
            });
        }
    }
    if entries.len() != count {
        return Err(invalid());
    }
    Ok(CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: request_id.to_owned(),
            instance_id: instance_id.to_owned(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::RuntimeAction),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: None,
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(count),
            max_players: Some(maximum),
            entries,
            issue: None,
        },
        // Moderation commands require names, but this roster establishes UUID
        // identity only; no verified action binding is available.
        private_action_bindings: HashMap::new(),
        collected_at: observed_at,
    })
}

fn parse_count(value: &str) -> Option<usize> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().ok())
        .flatten()
}

#[cfg(test)]
#[path = "minecraft_tests.rs"]
mod tests;
