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
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let worker_operation = operation.clone();
    spawn_storage_context_task(&operation, async move {
        initialize_database(&storage.paths)
            .await
            .map_err(|error| error.to_string())?;
        let descriptors =
            discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
        let descriptor = find_descriptor(&descriptors, &input.module_id)?;
        app_storage::inspect_module_programs(
            &storage.paths,
            descriptor,
            input.program_mode,
            input.program_source,
            worker_operation.cancellation_token(),
            input.include_archived_sources,
        )
        .await
        .map_err(|error| error.to_string())
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
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
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
