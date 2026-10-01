use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::Duration;

use app_core::{
    InstanceDetails, ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus,
    RuntimePlayerIdentityKind,
};
use serde::Deserialize;

use super::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};
use super::http_api::{HttpApiError, MAX_HTTP_RESPONSE_BYTES, fetch_json};
use super::service::{failed_snapshot, misconfigured_snapshot};

const MAX_PLAYERS: usize = 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Deserialize)]
struct StatusResponse {
    status: String,
    player_count: usize,
    player_names: Vec<String>,
}

pub(crate) async fn collect_nightingale(
    details: &InstanceDetails,
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
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
    if details.summary.module_id != "nightingale" || details.summary.id != instance_id {
        return Err(config_error(
            "The Nightingale instance context does not match.",
            Vec::new(),
        ));
    }
    let settings: serde_json::Value = serde_json::from_str(&details.settings_json)
        .map_err(|_| config_error("The Nightingale settings could not be read.", Vec::new()))?;
    if settings
        .get("status_endpoint_enabled")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err(config_error(
            "Enable the Nightingale status endpoint to read online players.",
            vec![String::from("status_endpoint_enabled")],
        ));
    }
    let mut ports = details
        .ports
        .iter()
        .filter(|port| port.name == "status" && port.protocol.eq_ignore_ascii_case("tcp"));
    let port = ports
        .next()
        .ok_or_else(|| config_error("The instance has no status endpoint TCP port.", Vec::new()))?;
    if port.port == 0 || ports.next().is_some() {
        return Err(config_error(
            "The instance status port is invalid or ambiguous.",
            Vec::new(),
        ));
    }
    let address = local_address(&details.summary.bind_ip, port.port).ok_or_else(|| {
        config_error(
            "The status endpoint address is not bound to this computer.",
            Vec::new(),
        )
    })?;
    let body = fetch_json(address, "/status", None, REQUEST_TIMEOUT)
        .await
        .map_err(|error| match error {
            HttpApiError::HttpStatus(503) => failure(
                instance_id,
                request_id,
                RuntimeLivePlayerIssueCode::ProcessUnavailable,
                "The Nightingale server is still preparing its player-query interface.",
                false,
            ),
            HttpApiError::Timeout => failure(
                instance_id,
                request_id,
                RuntimeLivePlayerIssueCode::CollectionTimeout,
                "The Nightingale status endpoint did not finish responding in time.",
                false,
            ),
            HttpApiError::TooLarge => failure(
                instance_id,
                request_id,
                RuntimeLivePlayerIssueCode::CaptureLimit,
                "The Nightingale player response exceeds the collection limit.",
                true,
            ),
            HttpApiError::Authentication
            | HttpApiError::HttpStatus(_)
            | HttpApiError::Transport => failure(
                instance_id,
                request_id,
                RuntimeLivePlayerIssueCode::IoFailed,
                "The Nightingale status endpoint could not provide a player response.",
                false,
            ),
        })?;
    parse_nightingale(instance_id, &body, request_id, observed_at)
}

fn local_address(bind_ip: &str, port: u16) -> Option<SocketAddr> {
    let address = match bind_ip.parse::<IpAddr>().ok()? {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        address => address,
    };
    // Binding an ephemeral socket verifies the persisted literal still belongs
    // to this host without DNS or an external probe; it is immediately released.
    let socket = UdpSocket::bind(SocketAddr::new(address, 0)).ok()?;
    drop(socket);
    Some(SocketAddr::new(address, port))
}

fn parse_nightingale(
    instance_id: &str,
    body: &[u8],
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    if body.len() > MAX_HTTP_RESPONSE_BYTES {
        return Err(failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The Nightingale player response exceeds the collection limit.",
            true,
        ));
    }
    let invalid = || {
        failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "The Nightingale response is not a complete current player-name list.",
            false,
        )
    };
    let response: StatusResponse = serde_json::from_slice(body).map_err(|_| invalid())?;
    if response.player_count > MAX_PLAYERS || response.player_names.len() > MAX_PLAYERS {
        return Err(failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The Nightingale player count exceeds the collection limit.",
            true,
        ));
    }
    if response.status != "ready" || response.player_count != response.player_names.len() {
        return Err(invalid());
    }
    let mut entries = Vec::with_capacity(response.player_names.len());
    for (index, name) in response.player_names.into_iter().enumerate() {
        if name.trim().is_empty()
            || name.chars().count() > 256
            || name.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        entries.push(RuntimeLivePlayerEntry {
            // Names can collide and are not durable account identities. The
            // snapshot-local key keeps equal names as separate read-only rows.
            player_key: format!("{request_id}:{index}"),
            display_name: name.clone(),
            identifiers: vec![RuntimeLivePlayerIdentifier {
                kind: RuntimePlayerIdentityKind::PlayerName,
                value: name,
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
            snapshot_id: request_id.to_owned(),
            instance_id: instance_id.to_owned(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::HttpApi),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: None,
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(response.player_count),
            max_players: None,
            entries,
            issue: None,
        },
        private_action_bindings: HashMap::new(),
        collected_at: observed_at,
    })
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
#[path = "nightingale_tests.rs"]
mod tests;
