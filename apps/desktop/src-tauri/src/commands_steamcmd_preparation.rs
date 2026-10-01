use super::*;
use crate::steamcmd_preparation::SteamCmdPrepareSnapshot;

#[tauri::command]
pub async fn ensure_steamcmd_ready(
    state: tauri::State<'_, DesktopState>,
    operation_id: String,
) -> Result<SteamCmdStatus, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("SteamCMD installation")?;
    let mut preparation = state.steamcmd_preparation.begin(operation_id)?;
    let cancellation = preparation.cancellation()?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let result = app_steamcmd::ensure_steamcmd_installed_with_progress_and_cancellation(
        &storage.settings,
        &cancellation,
        |progress| {
            preparation.update(progress);
        },
    )
    .await
    .map_err(|error| {
        if matches!(error, app_steamcmd::SteamCmdError::InstallCancelled { .. }) {
            return String::from("installation_cancelled");
        }
        append_desktop_app_log(
            &storage,
            "error",
            "steamcmd.ensure.failed",
            &error.to_string(),
            json!({ "output_excerpt": steamcmd_error_excerpt(&error) }),
        );
        steamcmd_error_message(&error)
    });
    if result
        .as_ref()
        .is_err_and(|error| error == "installation_cancelled")
    {
        preparation.finish_cancelled();
    } else {
        preparation.finish(result.as_ref().err().cloned());
    }
    result
}

#[tauri::command]
pub fn cancel_steamcmd_preparation(
    state: tauri::State<'_, DesktopState>,
    operation_id: String,
) -> Result<SteamCmdPrepareSnapshot, String> {
    state.steamcmd_preparation.request_cancel(&operation_id)
}

#[tauri::command]
pub fn read_steamcmd_prepare_progress(
    state: tauri::State<'_, DesktopState>,
    operation_id: Option<String>,
) -> Result<Option<SteamCmdPrepareSnapshot>, String> {
    state.steamcmd_preparation.snapshot(operation_id.as_deref())
}
