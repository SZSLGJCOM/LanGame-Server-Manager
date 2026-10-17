//! Read the game's own bans and authorize offline unban from a fresh native
//! record. Saved access-control settings never own or render these files.
use super::commands_assistant_ops::{
    DeclaredRuntimeActionRequest, dispatch_declared_runtime_action,
};
use super::*;
use app_core::{ExecuteInstancePlayerActionResult, NativeBanRoster, RuntimeLivePlayerActionStatus};

#[tauri::command]
pub async fn read_instance_native_bans(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<NativeBanRoster, String> {
    let operation = state.begin_storage_context_operation("native ban roster read")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let permit = state.acquire_instance_mutation(&instance_id).await;
    let details = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    spawn_blocking_storage_context_task(&operation, move || {
        // A disconnected waiter cannot release either lease during file I/O.
        let _permit = permit;
        read_saved_bans(&details).map(|(_, _, roster)| roster)
    })
    .await
    .map_err(|_| "The native ban roster worker failed.")?
}

#[tauri::command]
pub async fn unban_instance_native_ban<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    account_id: String,
) -> Result<ExecuteInstancePlayerActionResult, String> {
    if account_id.is_empty()
        || account_id.trim() != account_id
        || account_id.len() > 256
        || account_id.chars().any(char::is_control)
    {
        return Err("An exact native banned account is required.".into());
    }
    let operation = state.begin_storage_context_operation("native account unban")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let permit = state.acquire_instance_mutation(&instance_id).await;
    let details = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let run_id = details.active_run.as_ref().map(|run| run.run_id);
    if details.summary.module_id != "squad" && run_id.is_none() {
        return Err("The server is not running.".into());
    }
    let worker_operation = operation.clone();
    spawn_storage_context_task(&operation, async move {
        // Keep lifecycle ownership across read, native dispatch, and reload even
        // when the UI/HTTP caller goes away after the operation was accepted.
        let _permit = permit;
        let state = app.state::<DesktopState>();
        let instance_id = details.summary.id.clone();
        let module_id = details.summary.module_id.clone();
        let read_details = details.clone();
        let (runtime_root, instance_root, roster) = spawn_blocking_storage_context_task(&worker_operation,
            move || read_saved_bans(&read_details)).await.map_err(|_| "The native ban authorization worker failed.")??;
        authorize_account(&roster, &account_id)?;
        state.live_player_registry.invalidate_instance(&instance_id);
        let result = if module_id == "squad" {
            unban_squad(&worker_operation, runtime_root, instance_root, account_id, run_id.is_some()).await
        } else {
            let action_id = native_unban_action(&module_id)?;
            dispatch_declared_runtime_action(&state, DeclaredRuntimeActionRequest {
                instance_id: &instance_id, expected_run_id: run_id.ok_or("The server is not running.")?, action_id,
                target: Some(&account_id), role: None, request_id: None, require_target_binding: true,
            }).await.map(|_| "The native unban command was sent; refresh the game's ban roster to inspect persistence.".to_owned())
        };
        append_desktop_app_log(&storage, if result.is_ok() { "info" } else { "warning" },
            "instance.native_bans.unban", "Native ban roster unban completed",
            json!({"instance_id": instance_id, "module_id": module_id,
                "outcome": if result.is_ok() { "sent" } else { "failed_or_unknown" }}));
        Ok(ExecuteInstancePlayerActionResult {
            action_id: native_unban_action(&module_id)?.into(), status: RuntimeLivePlayerActionStatus::Sent,
            executed_at_unix_ms: state.live_player_registry.now_unix_ms(), summary: result?,
        })
    }).await.map_err(|_| "The native unban worker failed; its outcome is unknown. Refresh before retrying.")?
}

fn read_saved_bans(
    details: &InstanceDetails,
) -> Result<(PathBuf, PathBuf, NativeBanRoster), String> {
    native_unban_action(&details.summary.module_id)?;
    let instance_root = super::commands_program_storage::instance_root(details)?.to_owned();
    let runtime_root = app_storage::resolve_instance_runtime_root(&instance_root)
        .map_err(|error| error.to_string())?;
    let roster = app_storage::read_native_ban_roster(
        &runtime_root,
        &instance_root,
        &details.summary.module_id,
        &details.summary.id,
    )
    .map_err(|error| error.to_string())?;
    Ok((runtime_root, instance_root, roster))
}

fn native_unban_action(module_id: &str) -> Result<&'static str, String> {
    match module_id {
        "rimworld" => Ok("unban_user"),
        "conanexiles"
        | "palworld"
        | "soulmask"
        | "arksurvivalascended"
        | "arksurvivalevolved"
        | "squad"
        | "necesse"
        | "unturned" => Ok("unban_player"),
        _ => Err("This game does not expose a native saved ban roster.".into()),
    }
}

fn authorize_account(roster: &NativeBanRoster, account_id: &str) -> Result<(), String> {
    if roster
        .entries
        .iter()
        .filter(|entry| entry.account_id == account_id)
        .count()
        != 1
    {
        return Err("The exact account is absent or ambiguous in the current native ban roster. Refresh before unbanning.".into());
    }
    if roster.entries.iter().any(|entry| {
        entry.account_id == account_id
            && entry.identity_kind == app_core::RuntimePlayerIdentityKind::PalworldPlayerUid
    }) {
        return Err("This older Palworld ban has only a player GUID. The native REST unban requires the original platform user ID.".into());
    }
    // ASA falls back to display-name removal if the ID disappears before the
    // game processes its command. Refuse a known collision in the current file.
    if roster.module_id == "arksurvivalascended"
        && roster.entries.iter().any(|entry| {
            entry.account_id != account_id && entry.display_name.as_deref() == Some(account_id)
        })
    {
        return Err("Another ARK ban uses this account ID as its display name. Native unban is ambiguous; inspect the original ban records before retrying.".into());
    }
    Ok(())
}

async fn unban_squad(
    operation: &StorageContextOperationGuard,
    runtime_root: PathBuf,
    instance_root: PathBuf,
    account_id: String,
    running: bool,
) -> Result<String, String> {
    // Current Squad reloads admins/remote hosts but keeps local bans in the
    // GameSession cache. A new server session is required; never send a reload
    // command or report an immediate native unban for a file-only edit.
    spawn_blocking_storage_context_task(operation, move || {
        app_storage::remove_squad_native_ban(&runtime_root, &instance_root, &account_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "The Squad ban-file worker failed; inspect the native file before retrying.")??;
    Ok(if running {
        "The saved Squad ban was removed and its original retained. Restart the server for the local ban cache to update; the running session has not been unbanned."
    } else {
        "The saved Squad ban was removed and its original retained. This takes effect when the server next starts."
    }.into())
}

#[cfg(test)]
#[path = "commands_native_bans_tests.rs"]
mod tests;
