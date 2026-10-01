use std::time::Duration;

use crate::astroneer_console::{self, FetchError, Query};
use app_core::{
    InstanceDetails, ModulePlayerListSource, RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot,
};
#[cfg(test)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::cache::LivePlayerCollectionResult;
use super::service::{failed_snapshot, misconfigured_snapshot};

#[path = "astroneer_response.rs"]
mod response;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

pub(crate) async fn collect_astroneer(
    details: &InstanceDetails,
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let config_error = |summary, keys| {
        Box::new(misconfigured_snapshot(
            instance_id,
            request_id.to_owned(),
            ModulePlayerListSource::TcpConsole,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            summary,
            keys,
        ))
    };
    if details.summary.module_id != "astroneer" || details.summary.id != instance_id {
        return Err(config_error(
            "The ASTRONEER instance context does not match.",
            Vec::new(),
        ));
    }
    let settings: serde_json::Value = serde_json::from_str(&details.settings_json)
        .map_err(|_| config_error("The ASTRONEER settings could not be read.", Vec::new()))?;
    let password = settings.get("console_password").and_then(serde_json::Value::as_str)
        .filter(|password| valid_password(password))
        .ok_or_else(|| config_error(
            "Set a non-empty ASTRONEER console password without control characters to read online players.",
            vec![String::from("console_password")],
        ))?;
    let mut ports = details.ports.iter().filter(|port| port.name == "console");
    let port = ports.next().ok_or_else(|| {
        config_error(
            "The instance has no ASTRONEER console TCP port.",
            Vec::new(),
        )
    })?;
    if port.port == 0 || !port.protocol.eq_ignore_ascii_case("tcp") || ports.next().is_some() {
        return Err(config_error(
            "The instance console port is invalid or ambiguous.",
            Vec::new(),
        ));
    }
    // ASTRONEER has no separate console bind setting. Only the local process's
    // assigned port is eligible; the advertised PublicIP must never receive credentials.
    let body = fetch_players(port.port, password, REQUEST_TIMEOUT).await.map_err(|error| {
        if error == FetchError::ClosedBeforeResponse {
            let mut error = failure(instance_id, request_id, RuntimeLivePlayerIssueCode::IoFailed,
                "The ASTRONEER console closed the connection before returning players. Check the console password and server state.", false);
            if let Some(issue) = &mut error.issue {
                issue.setting_keys = vec![String::from("console_password")];
            }
            return error;
        }
        let (code, summary, truncated) = match error {
            FetchError::Timeout => (RuntimeLivePlayerIssueCode::CollectionTimeout,
                "The ASTRONEER console did not return a complete player response in time.", false),
            FetchError::TooLarge => (RuntimeLivePlayerIssueCode::CaptureLimit,
                "The ASTRONEER player response exceeds the collection limit.", true),
            FetchError::Transport => (RuntimeLivePlayerIssueCode::IoFailed,
                "The local ASTRONEER console could not provide a player response.", false),
            FetchError::Incomplete => (RuntimeLivePlayerIssueCode::ProtocolIncomplete,
                "The ASTRONEER console returned an incomplete or invalid player response.", false),
            FetchError::ClosedBeforeResponse => (RuntimeLivePlayerIssueCode::IoFailed,
                "The ASTRONEER console closed the connection before returning players.", false),
        };
        failure(instance_id, request_id, code, summary, truncated)
    })?;
    response::parse(instance_id, &body, request_id, observed_at)
}

fn valid_password(password: &str) -> bool {
    !password.trim().is_empty()
        && password.chars().count() <= 128
        && !password.chars().any(char::is_control)
}

async fn fetch_players(
    port: u16,
    password: &str,
    timeout: Duration,
) -> Result<Vec<u8>, FetchError> {
    astroneer_console::fetch(port, password, Query::Players, timeout, MAX_RESPONSE_BYTES).await
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
        ModulePlayerListSource::TcpConsole,
        code,
        summary,
        truncated,
    ))
}

#[cfg(test)]
#[path = "astroneer_tests.rs"]
mod tests;
