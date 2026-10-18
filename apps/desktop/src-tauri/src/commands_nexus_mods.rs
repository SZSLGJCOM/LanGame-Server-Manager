//! Local-host Nexus authentication and installation boundaries.
use super::commands_mods::{acquire_stopped_instance_mod_mutation, resolve_manual_mod_target};
use super::nexus_mods::{self, NexusAccountStatus, NexusFileSelection, NexusModReview};
use super::*;

#[path = "nexus_mod_credentials.rs"]
mod credentials;

static CREDENTIAL_WRITES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn require_nexus_api() -> Result<(), String> {
    Err(serde_json::json!({
        "code": "nexus_api_unavailable",
        "message": "Nexus API downloads are not available in this release. Download files from the Nexus Mods website and import them locally.",
    })
    .to_string())
}

fn unconfigured_account() -> NexusAccountStatus {
    NexusAccountStatus {
        configured: false,
        user_name: None,
        is_premium: false,
    }
}

fn auth_required() -> String {
    serde_json::json!({
        "code": "nexus_auth_required",
        "message": "Configure a Nexus Mods API key on this computer before continuing.",
    })
    .to_string()
}

fn normalize_api_key(api_key: &str) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() || key.len() > 4096 || !key.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err(serde_json::json!({
            "code": "nexus_auth_invalid",
            "message": "Enter a Nexus API key with no internal whitespace or control characters (maximum 4096 ASCII characters).",
        }).to_string());
    }
    Ok(key.to_owned())
}

async fn configured_key() -> Result<credentials::ApiKey, String> {
    credentials::read().await?.ok_or_else(auth_required)
}

#[tauri::command]
pub(crate) async fn nexus_account_status() -> Result<NexusAccountStatus, String> {
    require_nexus_api()?;
    match credentials::read().await? {
        Some(key) => nexus_mods::validate_api_key(key.expose()).await,
        None => Ok(unconfigured_account()),
    }
}

#[tauri::command]
pub(crate) async fn nexus_store_api_key(api_key: String) -> Result<NexusAccountStatus, String> {
    require_nexus_api()?;
    let normalized = normalize_api_key(&api_key)?;
    let _write = CREDENTIAL_WRITES.lock().await;
    let account = nexus_mods::validate_api_key(&normalized).await?;
    credentials::write(normalized).await?;
    Ok(account)
}

#[tauri::command]
pub(crate) async fn nexus_clear_api_key() -> Result<NexusAccountStatus, String> {
    let _write = CREDENTIAL_WRITES.lock().await;
    credentials::clear().await?;
    Ok(unconfigured_account())
}

async fn instance_source(
    storage: &StorageBootstrap,
    instance_id: &str,
) -> Result<(app_core::ModuleModSourceSpec, Vec<String>), String> {
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    super::commands_mod_staging::validate_game_mod_mutation(&instance.summary.module_id)?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    let mods = module_mods_spec_from_manifest(&descriptor.manifest_toml)
        .ok_or("This module does not declare a Mod installation workflow.")?;
    let source = mods
        .source
        .filter(|source| source.provider.trim().eq_ignore_ascii_case("nexus"))
        .ok_or("This instance does not declare Nexus Mods as its Mod source.")?;
    let staging = mods
        .manual_staging
        .ok_or("This module does not declare a local Mod installation target.")?;
    if mods.enablement.is_some() {
        return Err(
            "This module uses game-managed Mod IDs rather than Nexus file installation.".into(),
        );
    }
    Ok((source, staging.accepts))
}

#[tauri::command]
pub(crate) async fn review_nexus_mod_references(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    references: Vec<String>,
) -> Result<Vec<NexusModReview>, String> {
    require_nexus_api()?;
    let operation = state.begin_storage_context_operation("Nexus Mod reference review")?;
    let storage = state.bootstrap_storage()?;
    let (source, accepts) = instance_source(&storage, &instance_id).await?;
    let key = configured_key().await?;
    tokio::select! {
        biased;
        _ = operation.cancelled() => Err("Nexus Mod reference review was cancelled.".into()),
        result = nexus_mods::review_references(&source, &accepts, &references, key.expose()) => result,
    }
}

#[tauri::command]
pub(crate) async fn install_nexus_mod_files(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    selections: Vec<NexusFileSelection>,
) -> Result<ManualModStageResult, String> {
    require_nexus_api()?;
    let operation = state.begin_storage_context_operation("Nexus Mod installation")?;
    let storage = state.bootstrap_storage()?;
    let (source, accepts) = instance_source(&storage, &instance_id).await?;
    let key = configured_key().await?;
    let instance_lock =
        acquire_stopped_instance_mod_mutation(&state, &storage, &instance_id).await?;
    let instance_lock = super::commands_program_storage::prepare_instance_mod_program(
        &state.install_locks,
        &storage,
        &operation,
        &instance_id,
        instance_lock,
    )
    .await?;
    let target = resolve_manual_mod_target(&storage, instance_id, true).await?;
    let downloads = tokio::select! {
        biased;
        _ = operation.cancelled() => return Err("Nexus Mod installation was cancelled before publication.".into()),
        result = nexus_mods::download_selected_files(&source, &accepts, &selections, key.expose()) => result?,
    };
    let cancellation = operation.cancellation_token();
    spawn_blocking_storage_context_task(&operation, move || {
        let _instance_lock = instance_lock;
        if cancellation.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Nexus Mod installation was cancelled before publication.".into());
        }
        // The downloaded paths stay alive through publication or rollback, even
        // when the IPC observer disappears. This is the existing local-import
        // transaction, including each game's payload and ownership checks.
        let result = super::commands_mod_staging::stage_manual_mod_sources(
            target,
            downloads.paths().to_vec(),
        );
        drop(downloads);
        result.map_err(|error| {
            serde_json::json!({"code": "nexus_installation_failed", "message": error}).to_string()
        })
    })
    .await
    .map_err(|_| "The Nexus Mod installation worker did not complete.".to_string())?
}

#[cfg(test)]
#[path = "commands_nexus_mods_tests.rs"]
mod tests;
