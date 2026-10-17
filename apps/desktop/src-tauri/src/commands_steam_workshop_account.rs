//! Temporary SteamCMD sessions in LanGameCMD, authorized only locally.
use super::*;
use crate::steam_workshop_account_sessions::{
    SteamAccountLoginTarget, SteamWorkshopAccountLoginSnapshot, SteamWorkshopAccountSessions,
};

const LOGIN_BUDGET: Duration = Duration::from_secs(10 * 60);

async fn login_target_module(
    storage: &StorageBootstrap,
    target: &SteamAccountLoginTarget,
) -> Result<String, String> {
    match target {
        SteamAccountLoginTarget::Instance(id) => read_instance_details(&storage.paths, id)
            .await
            .map(|instance| instance.summary.module_id)
            .map_err(|error| error.to_string()),
        SteamAccountLoginTarget::Module(id) => Ok(id.clone()),
    }
}

fn uses_steam_install(descriptor: &ModuleDescriptor) -> bool {
    descriptor.summary.steam_app_id.is_some_and(|id| id > 0)
        && descriptor.install.as_ref().is_some_and(|install| {
            install.download_url_windows.is_none() && install.source.is_none()
        })
}

fn target_supports_login(target: &SteamAccountLoginTarget, descriptor: &ModuleDescriptor) -> bool {
    uses_steam_install(descriptor)
        || (matches!(target, SteamAccountLoginTarget::Instance(_))
            && descriptor
                .workshop
                .as_ref()
                .is_some_and(|workshop| workshop.provider.eq_ignore_ascii_case("steam")))
}

pub(super) async fn authorize_server_install_account(
    state: &DesktopState,
    target: &SteamAccountLoginTarget,
    account_name: &str,
    expected_module: Option<&str>,
) -> Result<
    (
        String,
        crate::steam_workshop_account_sessions::ServerAccountAuthorization,
    ),
    String,
> {
    let account = app_steamcmd::normalize_workshop_account_name(account_name)
        .map_err(|error| steamcmd_error_message(&error))?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let module_id = login_target_module(&storage, target).await?;
    if expected_module.is_some_and(|expected| expected != module_id) {
        return Err(String::from(
            "The SteamCMD login target does not match this game.",
        ));
    }
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    if !uses_steam_install(find_descriptor(&descriptors, &module_id)?) {
        return Err(String::from(
            "This game does not use SteamCMD for server installation.",
        ));
    }
    let authorization = state.steam_workshop_account_sessions.authorize_account(
        target,
        &account,
        &storage.paths.steamcmd_root,
        &storage.paths.app_data_root,
    )?;
    Ok((account, authorization))
}

fn validate_session_id(id: &str) -> Result<(), String> {
    if !(24..=128).contains(&id.len())
        || !id.starts_with("steam-workshop-login-")
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(String::from("Invalid SteamCMD login session ID."));
    }
    Ok(())
}

struct LoginCompletion {
    sessions: Arc<SteamWorkshopAccountSessions>,
    id: String,
    finished: bool,
}

impl LoginCompletion {
    fn finish(
        &mut self,
        result: Result<app_steamcmd::SteamWorkshopAccountAuthenticationResult, String>,
    ) {
        self.sessions.finish(&self.id, result);
        self.finished = true;
    }
}
impl Drop for LoginCompletion {
    fn drop(&mut self) {
        if !self.finished {
            self.sessions.finish(
                &self.id,
                Err(String::from(
                    "SteamCMD login was interrupted before completion.",
                )),
            );
        }
    }
}

#[tauri::command]
pub async fn start_steam_workshop_account_login(
    state: tauri::State<'_, DesktopState>,
    target: SteamAccountLoginTarget,
    session_id: String,
) -> Result<SteamWorkshopAccountLoginSnapshot, String> {
    validate_session_id(&session_id)?;
    let operation = state.begin_storage_context_operation("SteamCMD login in LanGameCMD")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let module_id = login_target_module(&storage, &target).await?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &module_id)?;
    if !target_supports_login(&target, descriptor) {
        return Err(String::from("This target does not use SteamCMD."));
    }
    let login_operation = state.install_operations.begin(session_id.clone())?;
    let (sender, mut input) =
        tokio::sync::mpsc::channel::<app_steamcmd::SteamWorkshopAccountInput>(1);
    let sessions = state.steam_workshop_account_sessions.clone();
    let snapshot = sessions.begin(&target, &session_id, sender, &storage.paths.app_data_root)?;
    let install_locks = state.install_locks.clone();
    let task_id = session_id.clone();
    let worker_sessions = sessions.clone();
    let task = spawn_storage_context_task(&operation, async move {
        let mut completion = LoginCompletion {
            sessions: worker_sessions.clone(),
            id: session_id.clone(),
            finished: false,
        };
        let started = tokio::time::Instant::now();
        let cancellation = login_operation.cancellation();
        let result = async {
            // The first response is already normalized from `login ACCOUNT` by
            // the session registry. Passwords use only later challenge slots.
            let account = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(String::from("installation_cancelled")),
                result = tokio::time::timeout(LOGIN_BUDGET, input.recv()) => {
                    match result {
                        Ok(Some(account)) if account.challenge_revision.is_none() => account.value,
                        Ok(Some(_)) => return Err(String::from("SteamCMD account entry challenge is invalid.")),
                        Ok(None) => return Err(String::from("installation_cancelled")),
                        Err(_) => return Err(steamcmd_error_message(&app_steamcmd::SteamCmdError::OperationTimedOut {
                            operation: "Steam account authentication", timeout_seconds: LOGIN_BUDGET.as_secs(),
                        })),
                    }
                }
            };
            let result = app_steamcmd::authenticate_workshop_account_with_progress(
                &install_locks, &storage.settings, &account, cancellation, &mut input,
                LOGIN_BUDGET.saturating_sub(started.elapsed()),
                |progress| worker_sessions.progress(&session_id, progress),
            ).await.map_err(|error| steamcmd_error_message(&error))?;
            if cancellation.is_cancelled() { return Err(String::from("installation_cancelled")); }
            Ok(result)
        }.await;
        completion.finish(result);
        // login_operation remains owned until native cleanup has completed.
        drop(login_operation);
    });
    sessions.attach_task(&task_id, task)?;
    Ok(snapshot)
}

#[tauri::command]
pub fn read_steam_workshop_account_login(
    state: tauri::State<'_, DesktopState>,
    target: SteamAccountLoginTarget,
    session_id: String,
) -> Result<SteamWorkshopAccountLoginSnapshot, String> {
    validate_session_id(&session_id)?;
    state
        .steam_workshop_account_sessions
        .snapshot(&target, &session_id)
}

#[tauri::command]
pub fn send_steam_workshop_account_input(
    state: tauri::State<'_, DesktopState>,
    target: SteamAccountLoginTarget,
    session_id: String,
    expected_revision: u64,
    input: String,
) -> Result<SteamWorkshopAccountLoginSnapshot, String> {
    validate_session_id(&session_id)?;
    state
        .steam_workshop_account_sessions
        .submit(&target, &session_id, expected_revision, input)
}

#[tauri::command]
pub fn cancel_steam_workshop_account_login(
    state: tauri::State<'_, DesktopState>,
    target: SteamAccountLoginTarget,
    session_id: String,
) -> Result<SteamWorkshopAccountLoginSnapshot, String> {
    validate_session_id(&session_id)?;
    let snapshot = state
        .steam_workshop_account_sessions
        .snapshot(&target, &session_id)?;
    state.install_operations.request_cancel(&session_id)?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn download_authenticated_steam_workshop_items<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    instance_id: String,
    ids: Vec<String>,
    account_name: String,
    missing_only: Option<bool>,
    locale: Option<String>,
) -> Result<SteamWorkshopDownloadResult, String> {
    let account_name = app_steamcmd::normalize_workshop_account_name(&account_name)
        .map_err(|error| steamcmd_error_message(&error))?;
    let operation = app_handle
        .state::<DesktopState>()
        .begin_storage_context_operation("Authenticated Steam Workshop download")?;
    spawn_storage_context_task(&operation, async move {
        download_steam_workshop_items_inner(
            app_handle.state::<DesktopState>(),
            instance_id,
            ids,
            missing_only.unwrap_or(false),
            app_network::SourcePreference::from_locale(locale.as_deref()),
            Some(account_name),
        )
        .await
    })
    .await
    .map_err(|error| format!("Authenticated Workshop download task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_eligibility_follows_the_actual_install_provider_without_requiring_workshop() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let modules = discover_modules(root.join("modules")).unwrap();
        let valheim = find_descriptor(&modules, "valheim").unwrap();
        assert!(valheim.workshop.is_none());
        assert!(target_supports_login(
            &SteamAccountLoginTarget::Module("valheim".into()),
            valheim
        ));
        assert!(target_supports_login(
            &SteamAccountLoginTarget::Instance("valheim-a".into()),
            valheim
        ));
        let terraria = find_descriptor(&modules, "terraria").unwrap();
        assert!(!target_supports_login(
            &SteamAccountLoginTarget::Module("terraria".into()),
            terraria
        ));
        assert!(target_supports_login(
            &SteamAccountLoginTarget::Instance("terraria-a".into()),
            terraria
        ));
        let minecraft = find_descriptor(&modules, "minecraft").unwrap();
        assert!(!target_supports_login(
            &SteamAccountLoginTarget::Module("minecraft".into()),
            minecraft
        ));
        assert!(!target_supports_login(
            &SteamAccountLoginTarget::Instance("minecraft-a".into()),
            minecraft
        ));
    }
    #[test]
    fn session_ids_cannot_address_unrelated_install_operations() {
        assert!(validate_session_id("steam-workshop-login-01234567-89ab-cdef").is_ok());
        for id in [
            "game-install-0123456789",
            "steam-workshop-login-",
            "steam-workshop-login-hello\nworld",
        ] {
            assert!(validate_session_id(id).is_err());
        }
        assert!(validate_session_id(&format!("steam-workshop-login-{}", "a".repeat(129))).is_err());
    }
}
