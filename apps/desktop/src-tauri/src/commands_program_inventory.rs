use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleProgramsInput {
    pub module_id: String,
    pub program_mode: Option<app_core::InstanceProgramMode>,
    #[serde(default)]
    pub program_source: app_core::InstanceProgramSource,
    pub include_archived_sources: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceRemovalInput {
    pub instance_id: String,
}

#[tauri::command]
pub async fn inspect_module_programs(
    state: tauri::State<'_, DesktopState>,
    input: ModuleProgramsInput,
) -> Result<app_storage::ModuleProgramInventory, String> {
    let operation = state.begin_storage_context_operation("program inventory")?;
    let storage = state.bootstrap_storage()?;
    let worker_operation = operation.clone();
    spawn_storage_context_task(&operation, async move {
        initialize_database(&storage.paths)
            .await
            .map_err(|error| error.to_string())?;
        let descriptors =
            discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
        let descriptor = find_descriptor(&descriptors, &input.module_id)?;
        let mut inventory = app_storage::inspect_module_programs(
            &storage.paths,
            descriptor,
            input.program_mode,
            input.program_source,
            worker_operation.cancellation_token(),
            input.include_archived_sources,
        )
        .await
        .map_err(|error| error.to_string())?;
        if input.program_source == app_core::InstanceProgramSource::Verified
            && input.program_mode != Some(app_core::InstanceProgramMode::Shared)
            && inventory.creation.action == "independent_install"
            && let Some(app_id) = descriptor.summary.steam_app_id.filter(|id| *id != 0)
            && let Some(installation) = inventory.installations.iter().find(|installation| {
                installation.scope == "library"
                    && installation.used_by.is_empty()
                    && !installation.pending_removal
                    && installation.install_state == InstallState::Installed
                    && installation.install_root == inventory.creation.program_path
            })
            && let Some(version) = installation.current_version.clone()
        {
            let root = PathBuf::from(&installation.install_root);
            let steamcmd = storage.paths.steamcmd_root.clone();
            let descriptor = descriptor.clone();
            let cancellation = worker_operation.cancellation_token();
            let reusable = spawn_blocking_storage_context_task(&worker_operation, move || {
                let defaults = super::commands_program_storage::verify_steam_original_defaults(
                    &root,
                    &steamcmd,
                    &descriptor,
                    app_id,
                    &version,
                    &cancellation,
                    &mut |_| {},
                )
                .map_err(|error| error.to_string())?;
                app_storage::library_reuse_preview_with_original_defaults(
                    &root,
                    &descriptor.summary.id,
                    &defaults,
                    Some(&cancellation),
                )
                .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| format!("Original program preview failed: {error}"))??;
            if reusable {
                inventory.creation.action = "existing_install".into();
                inventory.creation.additional_bytes = Some(0);
                inventory.creation.reason = None;
            }
        }
        Ok(inventory)
    })
    .await
    .map_err(|error| format!("Program inventory task failed: {error}"))?
}

#[tauri::command]
pub async fn inspect_instance_removal(
    state: tauri::State<'_, DesktopState>,
    input: InstanceRemovalInput,
) -> Result<app_storage::InstanceRemovalPlan, String> {
    let operation = state.begin_storage_context_operation("instance removal preview")?;
    let storage = state.bootstrap_storage()?;
    spawn_storage_context_task(&operation, async move {
        initialize_database(&storage.paths)
            .await
            .map_err(|error| error.to_string())?;
        app_storage::inspect_instance_removal(&storage.paths, &input.instance_id)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Instance removal preview failed: {error}"))?
}
