use super::*;
use app_storage::InstanceIsolationReport;

#[derive(Debug, Deserialize)]
pub struct ReadInstanceIsolationInput {
    #[serde(alias = "instanceId")]
    pub instance_id: String,
}

#[tauri::command]
pub async fn read_instance_isolation(
    state: tauri::State<'_, DesktopState>,
    input: ReadInstanceIsolationInput,
) -> Result<InstanceIsolationReport, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("instance isolation inspection")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    app_storage::read_instance_isolation(&storage.paths, &input.instance_id)
        .await
        .map_err(|error| error.to_string())
}
