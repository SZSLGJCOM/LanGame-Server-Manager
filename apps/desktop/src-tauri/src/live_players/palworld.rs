use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use app_core::{
    InstanceDetails, ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus,
    RuntimePlayerIdentityKind,
};
use serde::Deserialize;

use super::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};
use super::http_api::{HttpApiError as FetchError, MAX_HTTP_RESPONSE_BYTES, fetch_json};
use super::service::{failed_snapshot, misconfigured_snapshot};

const MAX_RESPONSE_BYTES: usize = MAX_HTTP_RESPONSE_BYTES;
const MAX_PLAYERS: usize = 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Deserialize)]
struct PlayerListResponse {
    players: Vec<PalworldPlayer>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PalworldPlayer {
    name: String,
    user_id: String,
    ping: Option<f64>,
}

pub(crate) async fn collect_palworld(
    details: &InstanceDetails,
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
    action_ids: &[String],
) -> LivePlayerCollectionResult {
    let config_error = |summary, setting_keys| {
        Box::new(misconfigured_snapshot(
            instance_id,
            request_id.to_owned(),
            ModulePlayerListSource::HttpApi,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            summary,
            setting_keys,
        ))
    };
    if details.summary.module_id != "palworld" || details.summary.id != instance_id {
        return Err(config_error(
            "The Palworld instance context does not match.",
            Vec::new(),
        ));
    }
    let endpoint = super::palworld_rest::resolve_endpoint(details)
        .map_err(|error| config_error(error.summary, error.setting_keys))?;
    let body = fetch_players(endpoint.address.port(), &endpoint.password, REQUEST_TIMEOUT)
        .await
        .map_err(|error| match error {
            FetchError::Authentication => config_error(
                "The Palworld REST API rejected the administrator credentials.",
                vec![String::from("admin_password")],
            ),
            FetchError::Timeout => failure(
                instance_id,
                request_id,
                RuntimeLivePlayerIssueCode::CollectionTimeout,
                "The Palworld REST API did not finish responding in time.",
                false,
            ),
            FetchError::TooLarge => failure(
                instance_id,
                request_id,
                RuntimeLivePlayerIssueCode::CaptureLimit,
                "The Palworld player response exceeds the collection limit.",
                true,
            ),
            FetchError::Transport => failure(
                instance_id,
                request_id,
                RuntimeLivePlayerIssueCode::IoFailed,
                "The Palworld REST API could not provide a player response.",
                false,
            ),
            FetchError::HttpStatus(_) => failure(
                instance_id,
                request_id,
                RuntimeLivePlayerIssueCode::ProtocolIncomplete,
                "The Palworld REST API returned an unsuccessful player response.",
                false,
            ),
        })?;
    let mut snapshot = parse_palworld(instance_id, &body, request_id, observed_at)?;
    bind_player_actions(&mut snapshot, action_ids);
    Ok(snapshot)
}

fn bind_player_actions(snapshot: &mut CachedLivePlayerSnapshot, action_ids: &[String]) {
    for entry in &mut snapshot.public_snapshot.entries {
        for action_id in super::palworld_rest::PLAYER_ACTION_IDS {
            if action_ids.iter().any(|id| id == action_id) {
                entry.available_action_ids.push(action_id.to_owned());
                snapshot.private_action_bindings.insert(
                    (entry.player_key.clone(), action_id.to_owned()),
                    entry.player_key.clone(),
                );
            }
        }
    }
}

async fn fetch_players(
    port: u16,
    password: &str,
    timeout: Duration,
) -> Result<Vec<u8>, FetchError> {
    fetch_json(
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        "/v1/api/players",
        Some(password),
        timeout,
    )
    .await
}

/// Parse the public 1.0.4 /players JSON contract. Optional upstream fields are
/// deliberately omitted from the projection, including private IP addresses.
pub(super) fn parse_palworld(
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
            "The Palworld response is not a complete player identity list.",
            false,
        )
    };
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The Palworld player response exceeds the collection limit.",
            true,
        ));
    }
    let response: PlayerListResponse = serde_json::from_slice(body).map_err(|_| invalid())?;
    if response.players.len() > MAX_PLAYERS {
        return Err(failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The Palworld player count exceeds the collection limit.",
            true,
        ));
    }
    let mut identifiers = HashSet::with_capacity(response.players.len());
    let mut entries = Vec::with_capacity(response.players.len());
    for player in response.players {
        if !valid_field(&player.name, 256)
            || !valid_field(&player.user_id, 128)
            || !identifiers.insert(player.user_id.clone())
        {
            return Err(invalid());
        }
        let ping_ms = match player.ping {
            Some(ping) if ping.is_finite() && ping >= 0.0 && ping <= u32::MAX as f64 => {
                Some(ping.round() as u32)
            }
            Some(_) => return Err(invalid()),
            None => None,
        };
        entries.push(RuntimeLivePlayerEntry {
            player_key: player.user_id.clone(),
            display_name: player.name,
            identifiers: vec![RuntimeLivePlayerIdentifier {
                kind: RuntimePlayerIdentityKind::PalworldUserId,
                value: player.user_id,
                stable: true,
            }],
            available_action_ids: Vec::new(),
            ping_ms,
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
            source: Some(ModulePlayerListSource::HttpApi),
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

fn valid_field(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= limit
        && !value.chars().any(char::is_control)
}

fn failure(
    instance_id: &str,
    request_id: &str,
    code: RuntimeLivePlayerIssueCode,
    summary: &str,
    truncated: bool,
) -> Box<RuntimeLivePlayerSnapshot> {
    Box::new(failed_snapshot(
        instance_id,
        request_id.to_owned(),
        ModulePlayerListSource::HttpApi,
        code,
        summary,
        truncated,
    ))
}

#[cfg(test)]
#[path = "palworld_tests.rs"]
pub(super) mod tests;
