use app_steamcmd::SteamCmdError;
use serde_json::json;

/// Keep application-owned failure reasons structured at the desktop/LAN boundary.
pub(super) fn steamcmd_error_message(error: &SteamCmdError) -> String {
    if matches!(error, SteamCmdError::InstallCancelled { .. }) {
        return String::from("installation_cancelled");
    }
    let mut payload = match error {
        SteamCmdError::SteamCmdNotReady { path } => json!({
            "code": "steamcmd_not_ready", "path": path,
        }),
        SteamCmdError::MissingSteamCmdExecutable { path } => json!({
            "code": "steamcmd_executable_missing", "path": path,
        }),
        SteamCmdError::MissingInstalledExecutable {
            module_id,
            operation,
            path,
        } => json!({
            "code": "installed_executable_missing", "module_id": module_id,
            "operation": operation, "path": path,
        }),
        SteamCmdError::InstallationVerificationFailed {
            module_id,
            operation,
            detail,
        } => json!({
            "code": "installation_verification_failed", "module_id": module_id,
            "operation": operation, "detail": detail,
        }),
        SteamCmdError::OperationTimedOut {
            operation,
            timeout_seconds,
        } => json!({
            "code": "install_operation_timed_out", "operation": operation,
            "timeout_seconds": timeout_seconds,
        }),
        SteamCmdError::SteamCmdPreparationStalled {
            timeout_seconds, ..
        } => json!({
            "code": "steamcmd_preparation_stalled", "timeout_seconds": timeout_seconds,
        }),
        SteamCmdError::SteamCmdPreparationTimedOut {
            timeout_seconds, ..
        } => json!({
            "code": "install_operation_timed_out", "operation": "SteamCMD preparation",
            "timeout_seconds": timeout_seconds,
        }),
        SteamCmdError::UnmanagedSteamCmdRoot { path } => json!({
            "code": "steamcmd_root_unmanaged", "path": path.to_string_lossy(),
        }),
        SteamCmdError::InvalidSteamCmdOwnership { path } => json!({
            "code": "steamcmd_ownership_invalid", "path": path.to_string_lossy(),
        }),
        SteamCmdError::MissingInstallSource { module_id } => json!({
            "code": "module_install_source_missing", "module_id": module_id,
        }),
        SteamCmdError::MissingInstallSpec { module_id } => json!({
            "code": "module_install_spec_missing", "module_id": module_id,
        }),
        SteamCmdError::MissingProcessSpec { module_id } => json!({
            "code": "module_process_spec_missing", "module_id": module_id,
        }),
        SteamCmdError::SteamCmdCommandFailed { .. } => json!({"code": "steamcmd_command_failed"}),
        SteamCmdError::PrepareSteamCmd { .. } => json!({"code": "steamcmd_prepare_failed"}),
        SteamCmdError::SteamCmdPreparationCleanupFailed { .. }
        | SteamCmdError::SteamCmdPreparationLogRead { .. } => {
            json!({"code": "steamcmd_prepare_failed"})
        }
        SteamCmdError::DirectDownloadFailed { .. } => json!({"code": "module_download_failed"}),
        _ => return error.to_string(),
    };
    payload["message"] = json!(error.to_string());
    payload["output_excerpt"] = json!(steamcmd_error_excerpt(error));
    payload.to_string()
}

pub(super) fn steamcmd_error_excerpt(error: &SteamCmdError) -> Option<String> {
    match error {
        SteamCmdError::PrepareSteamCmd { output_excerpt }
        | SteamCmdError::SteamCmdPreparationStalled { output_excerpt, .. }
        | SteamCmdError::SteamCmdPreparationTimedOut { output_excerpt, .. }
        | SteamCmdError::SteamCmdPreparationCleanupFailed { output_excerpt }
        | SteamCmdError::SteamCmdCommandFailed { output_excerpt }
        | SteamCmdError::DirectDownloadFailed { output_excerpt } => Some(output_excerpt.clone()),
        SteamCmdError::SteamCmdPreparationLogRead {
            source,
            output_excerpt,
        } => Some(format!(
            "Could not read SteamCMD bootstrap log: {source}\n{output_excerpt}"
        )),
        SteamCmdError::MissingInstalledExecutable { path, .. } => Some(path.clone()),
        SteamCmdError::InstallationVerificationFailed { detail, .. } => Some(detail.clone()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "commands_steamcmd_error_tests.rs"]
mod tests;
