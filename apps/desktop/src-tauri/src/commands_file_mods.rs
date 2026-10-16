//! File Mod lifecycle for instance-owned plugin directories. Native Workshop and
//! game-managed enablement remain with their existing settings workflows.
use super::*;

/// Caller owns the instance and actual program-root leases before startup.
pub(super) async fn recover_before_start<T: Send + 'static>(
    storage: &StorageBootstrap,
    operation: &impl StorageContextTaskLease,
    instance_id: &str,
    manifest_toml: &str,
    leases: T,
) -> Result<T, String> {
    let Some(mods) = commands_mods::module_mods_spec_from_manifest(manifest_toml) else {
        return Ok(leases);
    };
    if mods.manual_staging.is_none() || mods.enablement.is_some() {
        return Ok(leases);
    }
    let target =
        commands_mods::resolve_manual_mod_target(storage, instance_id.into(), false).await?;
    spawn_blocking_storage_context_task(operation, move || {
        let lock = commands_mod_staging::manual_mod_target_lock(&target.target_path)?;
        let _guard = lock
            .lock()
            .map_err(|_| "Mod management target lock poisoned")?;
        commands_mod_staging::recover_management(&target)?;
        Ok(leases)
    })
    .await
    .map_err(|error| format!("Mod recovery worker failed: {error}"))?
}

#[tauri::command]
pub async fn manage_manual_mod(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    package_id: String,
    action: String,
) -> Result<ManagedModActionResult, String> {
    if !matches!(action.as_str(), "enable" | "disable" | "delete" | "adopt") {
        return Err("Unknown Mod management action".into());
    }
    if package_id.is_empty() || package_id.len() > 512 {
        return Err("Invalid Mod package identity".into());
    }
    let operation = state.begin_storage_context_operation("file Mod management")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    commands_mod_staging::validate_game_mod_mutation(&instance.summary.module_id)?;
    let mods = commands_mods::module_mods_spec_from_manifest(&descriptor.manifest_toml)
        .ok_or("This game does not declare a Mod workflow")?;
    if mods.manual_staging.is_none() || mods.enablement.is_some() {
        return Err("Manage this game's Mods through its native enablement workflow".into());
    }
    let instance_lock =
        commands_mods::acquire_stopped_instance_mod_mutation(&state, &storage, &instance_id)
            .await?;
    let program = commands_program_storage::prepare_instance_mod_program(
        &state.install_locks,
        &storage,
        &operation,
        &instance_id,
        instance_lock,
    )
    .await?;
    let target = commands_mods::resolve_manual_mod_target(&storage, instance_id, true).await?;
    let cancellation = operation.cancellation_token();
    let result = spawn_blocking_storage_context_task(&operation, move || {
        // Both the instance permit and the real program-root lease belong to
        // the worker, including compensation after a disconnected IPC waiter.
        let _program = program;
        if cancellation.load(Ordering::Acquire) {
            return Err("Mod management was cancelled before publication".into());
        }
        commands_mod_staging::manage_package(target, &package_id, &action)
    })
    .await
    .map_err(|error| format!("Mod management worker failed: {error}"))??;
    append_desktop_app_log(
        &storage,
        "info",
        "instance.mod.managed",
        "Instance Mod operation completed",
        json!({"instance_id": result.inventory.instance_id, "removed_file_count": result.removed_file_count}),
    );
    Ok(result)
}
