use super::*;

#[tauri::command]
pub async fn remove_instance_workshop_collection(
    state: tauri::State<'_, DesktopState>,
    input: UpdateInstanceInput,
    expected_settings_json: String,
    collection_id: String,
    member_ids: Vec<String>,
    retain_collection: Option<bool>,
) -> Result<InstanceDetails, String> {
    let operation = state.begin_storage_context_operation("Workshop collection removal")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let lock = state.acquire_instance_mutation(&input.id).await;
    if state
        .runtime_supervisor
        .lock()
        .map_err(|_| "runtime supervisor lock poisoned")?
        .is_tracked(&input.id)
    {
        return Err("Stop the instance before removing its Workshop collection.".into());
    }
    let paths = storage.paths.clone();
    let details = spawn_storage_context_task(&operation, async move {
        let _lock = lock;
        app_storage::remove_instance_workshop_collection(
            &paths,
            input,
            expected_settings_json,
            collection_id,
            member_ids,
            retain_collection.unwrap_or(false),
        )
        .await
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Workshop collection removal task failed: {error}"))??;
    state
        .live_player_registry
        .invalidate_instance(&details.summary.id);
    update_state_instances(
        &state,
        list_instances(&storage.paths)
            .await
            .map_err(|error| error.to_string())?,
    )?;
    append_desktop_app_log(
        &storage,
        "info",
        "instance.workshop_collection.removed",
        "Workshop instance removal completed; downloaded payloads retained",
        json!({ "instance_id": details.summary.id }),
    );
    Ok(details)
}
