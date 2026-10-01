use std::collections::{HashMap, HashSet};
use std::time::Duration;

use app_core::{
    InstanceDetails, ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIssueCode,
    RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus,
};
use serde::Deserialize;

use super::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};
use super::http_api::{HttpApiError, MAX_HTTP_RESPONSE_BYTES};
use super::service::{failed_snapshot, misconfigured_snapshot};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_PLAYERS: usize = 1024;

#[path = "satisfactory_config.rs"]
mod config;

#[derive(Clone, Copy)]
enum PlayerEndpoint {
    GameApi(u16),
    WebApi(u16),
}

#[derive(Deserialize)]
struct FrmPlayer {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Online")]
    online: bool,
}

pub(crate) async fn collect_satisfactory(
    details: &InstanceDetails,
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let config_error = |summary| {
        Box::new(misconfigured_snapshot(
            instance_id,
            request_id.to_owned(),
            ModulePlayerListSource::HttpApi,
            RuntimeLivePlayerIssueCode::QueryUnavailable,
            summary,
            Vec::new(),
        ))
    };
    if details.summary.module_id != "satisfactory" || details.summary.id != instance_id {
        return Err(config_error(
            "The Satisfactory instance context does not match.",
        ));
    }
    let mut ports = details
        .ports
        .iter()
        .filter(|port| port.name == "game_tcp" && port.protocol.eq_ignore_ascii_case("tcp"));
    let port = ports
        .next()
        .filter(|port| port.port != 0)
        .ok_or_else(|| config_error("The instance has no Satisfactory HTTPS API port."))?;
    if ports.next().is_some() {
        return Err(config_error(
            "The Satisfactory HTTPS API port is ambiguous.",
        ));
    }
    let instance_config = details.config_file_path.clone();
    let web_port = tokio::task::spawn_blocking(move || config::read_web_port(&instance_config))
        .await
        .map_err(|_| config_error("The instance's FRM configuration reader did not complete."))?
        .map_err(config_error)?;
    let endpoint = web_port.map_or(PlayerEndpoint::GameApi(port.port), PlayerEndpoint::WebApi);
    let body = fetch_players(endpoint)
        .await
        .map_err(|error| fetch_failure(instance_id, request_id, error))?;
    parse_players(instance_id, request_id, observed_at, &body)
}

fn fetch_failure(
    instance_id: &str,
    request_id: &str,
    error: HttpApiError,
) -> Box<RuntimeLivePlayerSnapshot> {
    let (code, summary) = match error {
        HttpApiError::HttpStatus(400 | 404 | 422) => (
            RuntimeLivePlayerIssueCode::ExtensionUnavailable,
            "The server has no available FRM player endpoint. Install a compatible Ficsit Remote Monitoring server mod, enable its HTTP autostart option, and start the game world.",
        ),
        HttpApiError::HttpStatus(503) => (
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            "The Satisfactory world is not ready for player queries.",
        ),
        HttpApiError::Authentication => (
            RuntimeLivePlayerIssueCode::AuthenticationFailed,
            "The server rejected the read-only FRM player query.",
        ),
        HttpApiError::Timeout => (
            RuntimeLivePlayerIssueCode::CollectionTimeout,
            "The FRM player endpoint did not finish responding in time.",
        ),
        HttpApiError::TooLarge => (
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The FRM response exceeds the bounded player collection limit.",
        ),
        HttpApiError::Transport => (
            RuntimeLivePlayerIssueCode::QueryUnavailable,
            "The local Satisfactory player endpoint could not be reached.",
        ),
        HttpApiError::HttpStatus(_) => (
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "The FRM player endpoint returned an unsuccessful response.",
        ),
    };
    failure(instance_id, request_id, code, summary)
}

async fn fetch_players(endpoint: PlayerEndpoint) -> Result<Vec<u8>, HttpApiError> {
    let (client, request) = player_request(endpoint)?;
    execute_players(client, request).await
}

fn player_request(
    endpoint: PlayerEndpoint,
) -> Result<(reqwest::Client, reqwest::Request), HttpApiError> {
    // Both FRM transports expose the same read-only roster without admin tokens.
    // Self-signed certificates apply only to the game's numeric loopback API.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(matches!(endpoint, PlayerEndpoint::GameApi(_)))
        .connect_timeout(Duration::from_secs(1))
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| HttpApiError::Transport)?;
    let request = match endpoint {
        PlayerEndpoint::GameApi(port) => client
            .post(format!("https://127.0.0.1:{port}/api/v1"))
            .json(&serde_json::json!({"function": "frm", "endpoint": "getPlayer"})),
        PlayerEndpoint::WebApi(port) => {
            client.get(format!("http://127.0.0.1:{port}/api/getPlayer"))
        }
    }
    .header(reqwest::header::ACCEPT, "application/json")
    .timeout(REQUEST_TIMEOUT)
    .build()
    .map_err(fetch_error)?;
    Ok((client, request))
}

async fn execute_players(
    client: reqwest::Client,
    request: reqwest::Request,
) -> Result<Vec<u8>, HttpApiError> {
    let mut response = client.execute(request).await.map_err(fetch_error)?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err(HttpApiError::Authentication);
    }
    if !response.status().is_success() {
        return Err(HttpApiError::HttpStatus(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_HTTP_RESPONSE_BYTES as u64)
    {
        return Err(HttpApiError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(fetch_error)? {
        if body.len().saturating_add(chunk.len()) > MAX_HTTP_RESPONSE_BYTES {
            return Err(HttpApiError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn fetch_error(error: reqwest::Error) -> HttpApiError {
    if error.is_timeout() {
        HttpApiError::Timeout
    } else {
        HttpApiError::Transport
    }
}

fn parse_players(
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
    body: &[u8],
) -> LivePlayerCollectionResult {
    let invalid = || {
        failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "The FRM endpoint did not return a complete online-player response.",
        )
    };
    if body.len() > MAX_HTTP_RESPONSE_BYTES {
        return Err(failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The FRM response exceeds the bounded player collection limit.",
        ));
    }
    let players: Vec<FrmPlayer> = serde_json::from_slice(body).map_err(|_| invalid())?;
    if players.len() > MAX_PLAYERS {
        return Err(failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The FRM response exceeds the bounded player count.",
        ));
    }
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for player in players.into_iter().filter(|player| player.online) {
        if !valid_field(&player.id, 256)
            || !valid_field(&player.name, 256)
            || !seen.insert(player.id.clone())
        {
            return Err(invalid());
        }
        entries.push(RuntimeLivePlayerEntry {
            player_key: format!("{request_id}:{}", entries.len()),
            display_name: player.name,
            // FRM's ID identifies an Actor, not a Steam/EOS account.
            identifiers: Vec::new(),
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
) -> Box<RuntimeLivePlayerSnapshot> {
    Box::new(failed_snapshot(
        instance_id,
        request_id.to_owned(),
        ModulePlayerListSource::HttpApi,
        code,
        summary,
        code == RuntimeLivePlayerIssueCode::CaptureLimit,
    ))
}

#[cfg(test)]
#[path = "satisfactory_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "satisfactory_transport_tests.rs"]
mod transport_tests;
