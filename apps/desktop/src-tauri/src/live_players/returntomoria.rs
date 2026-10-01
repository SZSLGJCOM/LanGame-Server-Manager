use app_core::{
    InstanceDetails, ModulePlayerListSource, RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot,
};
use app_runtime::native_player_console::{NativePlayerConsoleError, collect_native_player_console};

use super::cache::LivePlayerCollectionResult;
use super::service::{failed_snapshot, misconfigured_snapshot};

#[path = "returntomoria_response.rs"]
mod response;

pub(crate) async fn collect_returntomoria(
    details: &InstanceDetails,
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let invalid = || {
        failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            "The Return to Moria process identity is unavailable.",
        )
    };
    if details.summary.module_id != "returntomoria" || details.summary.id != instance_id {
        return Err(invalid());
    }
    let settings: serde_json::Value =
        serde_json::from_str(&details.settings_json).map_err(|_| invalid())?;
    if settings
        .get("console_enabled")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err(Box::new(misconfigured_snapshot(
            instance_id,
            request_id.to_owned(),
            ModulePlayerListSource::NativeConsole,
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            "Enable the Return to Moria console and restart the server to read online players.",
            vec!["console_enabled".to_owned()],
        )));
    }
    let mut processes = details
        .active_run
        .as_ref()
        .ok_or_else(invalid)?
        .processes
        .iter()
        .filter(|process| process.is_primary);
    let process = processes.next().ok_or_else(invalid)?;
    if processes.next().is_some() {
        return Err(invalid());
    }
    let pid = process.pid.ok_or_else(invalid)?;
    let identity = process.process_identity.clone().ok_or_else(invalid)?;
    let nonce = request_id.to_owned();
    // The blocking operation owns its six-second helper deadline. Cancelling
    // this future cannot leave an attached helper or an unbounded screen reader.
    let text = tokio::task::spawn_blocking(move || collect_native_player_console(pid, &identity, &nonce))
        .await.map_err(|_| invalid())?
        .map_err(|error| {
            let (code, summary) = match error {
                NativePlayerConsoleError::ProcessUnavailable => (RuntimeLivePlayerIssueCode::ProcessUnavailable,
                    "The Return to Moria process changed before its player query completed."),
                NativePlayerConsoleError::Timeout => (RuntimeLivePlayerIssueCode::CollectionTimeout,
                    "The Return to Moria console did not complete its player response in time."),
                NativePlayerConsoleError::CaptureLimit => (RuntimeLivePlayerIssueCode::CaptureLimit,
                    "The Return to Moria console exceeded the player capture limit."),
                NativePlayerConsoleError::Incomplete => (RuntimeLivePlayerIssueCode::ProtocolIncomplete,
                    "The Return to Moria console response was incomplete, wrapped or changed during capture."),
                NativePlayerConsoleError::Unsupported => (RuntimeLivePlayerIssueCode::AdapterUnavailable,
                    "Return to Moria native console collection requires Windows."),
                NativePlayerConsoleError::Io => (RuntimeLivePlayerIssueCode::IoFailed,
                    "The Return to Moria native console could not be read."),
            };
            failure(instance_id, request_id, code, summary)
        })?;
    response::parse(instance_id, request_id, observed_at, &text)
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
        ModulePlayerListSource::NativeConsole,
        code,
        summary,
        code == RuntimeLivePlayerIssueCode::CaptureLimit,
    ))
}
