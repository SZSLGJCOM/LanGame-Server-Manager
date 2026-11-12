use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveInput {
    pub instance_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalResult {
    #[serde(flatten)]
    pub status: ToolsStatus,
    pub framework_retained: bool,
}

#[tauri::command]
pub async fn remove_ark_tools(
    state: tauri::State<'_, DesktopState>,
    input: RemoveInput,
) -> Result<RemovalResult, String> {
    let operation = state.begin_storage_context_operation("ARK creature extension removal")?;
    let storage = std::sync::Arc::new(state.bootstrap_storage()?);
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let guard = state.acquire_instance_mutation(&input.instance_id).await;
    let details = instance(&storage, &input.instance_id).await?;
    if details.active_run.is_some()
        || state
            .runtime_supervisor
            .lock()
            .map_err(|_| "Runtime supervisor lock poisoned.")?
            .is_tracked(&input.instance_id)
        || state
            .pending_runtime_start_instance_ids()?
            .contains(&input.instance_id)
    {
        return Err("请先停止实例并取消待启动操作，再卸载方舟生物工具扩展。".into());
    }
    if state
        .ark_tools_progress
        .snapshot(&input.instance_id, None)?
        .is_some_and(|snapshot| snapshot.active)
    {
        return Err("方舟生物工具扩展正在安装，请等待完成后再卸载。".into());
    }
    let root = owned_root(&storage, &details).await?;
    let install_guard = app_steamcmd::acquire_game_install_lifecycle(
        &state.install_locks,
        &details.summary.module_id,
        std::slice::from_ref(&root),
    )
    .await
    .map_err(|error| steamcmd_error_message(&error))?;
    app_storage::ensure_program_archive_dependencies(&storage.paths, &root)
        .await
        .map_err(|error| error.to_string())?;
    let payload = embedded(&details.summary.module_id)?;
    let module_id = details.summary.module_id;
    let instance_id = input.instance_id;
    let operation_id = uuid::Uuid::new_v4().simple().to_string();
    let task_storage = std::sync::Arc::clone(&storage);
    let task_progress = std::sync::Arc::clone(&state.ark_tools_progress);
    let task_id = instance_id.clone();
    let task_operation_id = operation_id.clone();
    append_desktop_app_log(
        &storage,
        "info",
        "instance.ark_tools.remove.started",
        "ARK server extension removal started",
        json!({"instance_id": instance_id, "operation_id": operation_id}),
    );
    // Keep both lifecycle locks until the filesystem transaction finishes,
    // even when the originating view or IPC waiter has gone away.
    spawn_storage_context_task(&operation, async move {
        let (_instance, _install) = (guard, install_guard);
        let result =
            tokio::task::spawn_blocking(move || install::uninstall(&root, &module_id, payload))
                .await
                .map_err(|error| format!("ARK extension removal worker failed: {error}"))
                .and_then(|result| result);
        if result.is_ok() {
            task_progress.forget_completed(&task_id);
        }
        append_desktop_app_log(
            &task_storage,
            if result.is_ok() { "info" } else { "error" },
            if result.is_ok() {
                "instance.ark_tools.remove.completed"
            } else {
                "instance.ark_tools.remove.failed"
            },
            result
                .as_ref()
                .err()
                .map(String::as_str)
                .unwrap_or("ARK server extension removed and verified"),
            json!({"instance_id": task_id, "operation_id": task_operation_id,
                "framework_retained": result.as_ref().ok()}),
        );
        result.map(|framework_retained| RemovalResult {
            status: ToolsStatus {
                installed: false,
                update_available: false,
                connected: false,
                can_uninstall: false,
                issue: None,
            },
            framework_retained,
        })
    })
    .await
    .map_err(|error| {
        let error = format!("ARK extension removal task failed: {error}");
        append_desktop_app_log(
            &storage,
            "error",
            "instance.ark_tools.remove.failed",
            &error,
            json!({"instance_id": instance_id, "operation_id": operation_id}),
        );
        error
    })?
}
