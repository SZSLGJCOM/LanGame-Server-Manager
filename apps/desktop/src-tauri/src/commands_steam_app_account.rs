//! Cache-backed server acquisition stays on the local authenticated transport.
//! These commands share anonymous installation ownership and publication logic.
use super::*;
use crate::steam_workshop_account_sessions::SteamAccountLoginTarget;

#[tauri::command]
pub async fn install_authenticated_module_game<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    module_id: String,
    validate: bool,
    account_name: String,
    target: SteamAccountLoginTarget,
) -> Result<ModuleInstallResult, String> {
    let operation = app_handle
        .state::<DesktopState>()
        .begin_storage_context_operation("Authenticated server installation")?;
    spawn_storage_context_task(&operation, async move {
        let state = app_handle.state::<DesktopState>();
        let (account, authorization) =
            super::commands_steam_workshop_account::authorize_server_install_account(
                &state,
                &target,
                &account_name,
                Some(&module_id),
            )
            .await?;
        let result = if validate {
            super::commands_mods::validate_module_game_with_account_inner(
                state.clone(),
                module_id,
                Some(account.clone()),
            )
            .await
        } else {
            super::commands_mods::install_module_game_with_account_inner(
                state.clone(),
                module_id,
                Some(account.clone()),
            )
            .await
        }?;
        record_successful_server_download(&state, &target, &account, &authorization, &result);
        Ok(result)
    })
    .await
    .map_err(|error| format!("Authenticated server installation task failed: {error}"))?
}

#[tauri::command]
pub async fn update_authenticated_instance_program<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    instance_id: String,
    validate: bool,
    account_name: String,
) -> Result<ModuleInstallResult, String> {
    let operation = app_handle
        .state::<DesktopState>()
        .begin_storage_context_operation("Authenticated instance program update")?;
    let worker_operation = operation.clone();
    spawn_storage_context_task(&operation, async move {
        let state = app_handle.state::<DesktopState>();
        let (account, authorization) =
            super::commands_steam_workshop_account::authorize_server_install_account(
                &state,
                &SteamAccountLoginTarget::Instance(instance_id.clone()),
                &account_name,
                None,
            )
            .await?;
        let target = SteamAccountLoginTarget::Instance(instance_id.clone());
        let result = super::commands_program_storage::update_instance_program_inner(
            state.clone(),
            instance_id,
            validate,
            worker_operation,
            Some(account.clone()),
        )
        .await?;
        record_successful_server_download(&state, &target, &account, &authorization, &result);
        Ok(result)
    })
    .await
    .map_err(|error| format!("Authenticated instance program update task failed: {error}"))?
}

fn record_successful_server_download(
    state: &DesktopState,
    target: &SteamAccountLoginTarget,
    account: &str,
    authorization: &crate::steam_workshop_account_sessions::ServerAccountAuthorization,
    result: &ModuleInstallResult,
) {
    // The receipt is an optional in-memory continuation, never installation
    // state. Failure to retain it must not undo a completed publication.
    if let Ok(root) = fs::canonicalize(&result.install_root) {
        state
            .steam_workshop_account_sessions
            .record_server_download(target, account, authorization, &result.module_id, root);
    }
}

#[tauri::command]
pub fn clear_steam_server_account_authorization(
    state: tauri::State<'_, DesktopState>,
    target: SteamAccountLoginTarget,
) -> Result<(), String> {
    state
        .steam_workshop_account_sessions
        .clear_server_download(&target)
}

#[cfg(test)]
mod tests {
    use crate::command_registry::{Completion, Execution, LanAccess, find};

    #[test]
    fn cached_server_credentials_cannot_be_requested_over_the_lan_transport() {
        for (name, completion) in [
            ("install_authenticated_module_game", Completion::Owned),
            ("update_authenticated_instance_program", Completion::Owned),
            (
                "clear_steam_server_account_authorization",
                Completion::Bounded,
            ),
        ] {
            let command = find(name).unwrap();
            assert_eq!(command.execution, Execution::Runtime);
            assert_eq!(command.lan, LanAccess::Denied);
            assert_eq!(command.completion, completion);
        }
        for name in [
            "install_module_game",
            "validate_module_game",
            "update_instance_program",
        ] {
            assert_eq!(find(name).unwrap().lan, LanAccess::Allowed);
        }
    }
}
