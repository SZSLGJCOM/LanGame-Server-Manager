use super::*;

#[path = "commands_program_update_policy.rs"]
mod program_update_policy;

#[tauri::command]
pub async fn ensure_storage_ready(
    state: tauri::State<'_, DesktopState>,
) -> Result<StorageStatus, String> {
    let storage_context_operation =
        state.begin_storage_context_operation("storage initialization")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    append_desktop_app_log(
        &storage,
        "info",
        "storage.ensure.request",
        "Ensuring storage is ready",
        json!({}),
    );

    let status = initialize_database(&storage.paths).await.map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "storage.ensure.failed",
            &message,
            json!({}),
        );
        logged_error_message(&storage, message)
    })?;
    commands_storage_management::recover_archives_with(
        &state,
        &storage,
        &storage_context_operation,
    )
    .await?;
    let mut state_guard = state.app_state.write().map_err(|_| {
        let message = String::from("desktop state lock poisoned");
        append_desktop_app_log(
            &storage,
            "error",
            "storage.ensure.state_write_failed",
            &message,
            json!({}),
        );
        logged_error_message(&storage, message)
    })?;
    state_guard.storage = status.clone();

    append_desktop_app_log(
        &storage,
        "info",
        "storage.ensure.success",
        "Storage is ready",
        json!({
            "database_path": status.database_path,
            "schema_version": status.schema_version,
            "migrations_applied": status.migrations_applied,
        }),
    );

    Ok(status)
}
#[tauri::command]
pub async fn sync_modules_to_storage(
    state: tauri::State<'_, DesktopState>,
) -> Result<Vec<ModuleSummary>, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("module synchronization")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let modules = sync_modules_to_storage_with(&state, &storage).await?;
    state.autostart.mark_modules_ready()?;
    Ok(modules)
}

async fn sync_modules_to_storage_with(
    state: &DesktopState,
    storage: &StorageBootstrap,
) -> Result<Vec<ModuleSummary>, String> {
    let _module_sync_guard = state.acquire_module_sync().await;
    append_desktop_app_log(
        storage,
        "info",
        "modules.sync.request",
        "Syncing module descriptors to storage",
        json!({}),
    );

    initialize_database(&storage.paths).await.map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            storage,
            "error",
            "modules.sync.init_failed",
            &message,
            json!({}),
        );
        logged_error_message(storage, message)
    })?;
    let descriptors = discover_modules(&storage.paths.modules_root).map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            storage,
            "error",
            "modules.sync.discover_failed",
            &message,
            json!({ "modules_root": storage.paths.modules_root.to_string_lossy() }),
        );
        logged_error_message(storage, message)
    })?;
    sync_modules(&storage.paths, &descriptors)
        .await
        .map_err(|error| {
            let message = error.to_string();
            append_desktop_app_log(
                storage,
                "error",
                "modules.sync.persist_failed",
                &message,
                json!({ "module_count": descriptors.len() }),
            );
            logged_error_message(storage, message)
        })?;
    persist_descriptor_install_states(storage, &descriptors)
        .await
        .map_err(|message| {
            append_desktop_app_log(
                storage,
                "error",
                "modules.sync.install_state_failed",
                &message,
                json!({ "module_count": descriptors.len() }),
            );
            logged_error_message(storage, message)
        })?;
    let modules = load_module_summaries_with_install_state(storage, &descriptors).await?;

    let mut state_guard = state.app_state.write().map_err(|_| {
        let message = String::from("desktop state lock poisoned");
        append_desktop_app_log(
            storage,
            "error",
            "modules.sync.state_write_failed",
            &message,
            json!({}),
        );
        logged_error_message(storage, message)
    })?;
    state_guard.modules = modules.clone();

    append_desktop_app_log(
        storage,
        "info",
        "modules.sync.success",
        "Module descriptors synced to storage",
        json!({
            "module_count": modules.len(),
            "installed_count": modules
                .iter()
                .filter(|module| matches!(module.install_state, InstallState::Installed))
                .count(),
        }),
    );

    Ok(modules)
}
#[tauri::command]
pub async fn list_instances_from_storage(
    state: tauri::State<'_, DesktopState>,
) -> Result<Vec<InstanceSummary>, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("instance state reconciliation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    append_desktop_app_log(
        &storage,
        "info",
        "instances.list.request",
        "Loading instance list from storage",
        json!({}),
    );

    initialize_database(&storage.paths).await.map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "instances.list.init_failed",
            &message,
            json!({}),
        );
        logged_error_message(&storage, message)
    })?;
    reconcile_runtime_state(&state).await.map_err(|message| {
        append_desktop_app_log(
            &storage,
            "error",
            "instances.list.reconcile_failed",
            &message,
            json!({}),
        );
        logged_error_message(&storage, message)
    })?;
    super::commands_instance_reconciliation::reconcile_missing_instances(&state, &storage).await?;
    let instances = list_instances(&storage.paths).await.map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "instances.list.load_failed",
            &message,
            json!({}),
        );
        logged_error_message(&storage, message)
    })?;

    update_state_instances(&state, instances.clone()).map_err(|message| {
        append_desktop_app_log(
            &storage,
            "error",
            "instances.list.state_write_failed",
            &message,
            json!({ "instance_count": instances.len() }),
        );
        logged_error_message(&storage, message)
    })?;

    append_desktop_app_log(
        &storage,
        "info",
        "instances.list.success",
        "Instance list loaded from storage",
        json!({
            "instance_count": instances.len(),
            "running_count": instances
                .iter()
                .filter(|instance| matches!(instance.status, InstanceStatus::Running))
                .count(),
        }),
    );

    if state.is_storage_ready() {
        state.autostart.capture(&storage, &instances)?;
    }
    Ok(instances)
}
#[tauri::command]
pub async fn read_instance_details_from_storage(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<InstanceDetails, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("instance detail reconciliation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    read_instance_details_with_runtime_recovery(
        state.inner(),
        &_storage_context_operation,
        &storage.paths,
        &instance_id,
    )
    .await
}

pub(super) async fn read_instance_details_with_runtime_recovery(
    state: &DesktopState,
    storage_context_operation: &StorageContextOperationGuard,
    paths: &app_storage::StoragePaths,
    instance_id: &str,
) -> Result<InstanceDetails, String> {
    match read_instance_details(paths, instance_id).await {
        Ok(instance) => return Ok(instance),
        Err(app_storage::StorageError::PrivateRuntimeRefresh { .. }) => {}
        Err(error) => return Err(error.to_string()),
    }
    // Recovery changes files, so retain the same instance and storage-context
    // ownership as mutations, including when the requesting panel is closed.
    let paths = paths.clone();
    let recovery_instance_id = instance_id.to_owned();
    run_instance_mutation_to_completion(
        state,
        storage_context_operation,
        instance_id,
        move || async move {
            app_storage::recover_interrupted_instance_runtime(&paths, &recovery_instance_id)
                .await
                .map_err(|error| error.to_string())?;
            read_instance_details(&paths, &recovery_instance_id)
                .await
                .map_err(|error| error.to_string())
        },
    )
    .await
}

pub(super) async fn run_instance_mutation_to_completion<T, F, Fut>(
    state: &DesktopState,
    storage_context_operation: &StorageContextOperationGuard,
    instance_id: &str,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, String>> + Send + 'static,
{
    let instance_lock = state.acquire_instance_mutation(instance_id).await;
    spawn_storage_context_task(storage_context_operation, async move {
        let _instance_lock = instance_lock;
        operation().await
    })
    .await
    .map_err(|error| format!("instance mutation task failed: {error}"))?
}

#[tauri::command]
pub async fn create_instance_backup(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<InstanceBackupResult, String> {
    let storage_context_operation =
        state.begin_storage_context_operation("instance backup creation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let task_instance_id = instance_id.clone();
    let result = run_instance_mutation_to_completion(
        state.inner(),
        &storage_context_operation,
        &instance_id,
        move || {
            let storage = storage;
            async move {
                let result = create_instance_backup_snapshot(&storage.paths, &task_instance_id)
                    .await
                    .map_err(|error| error.to_string())?;
                append_desktop_app_log(
                    &storage,
                    "info",
                    "instance.backup.created",
                    "Instance backup created",
                    json!({
                        "instance_id": result.instance_id.as_str(),
                        "backup_id": result.backup_id,
                        "backup_path": result.backup_path,
                        "file_count": result.file_count,
                        "total_bytes": result.total_bytes,
                    }),
                );
                Ok(result)
            }
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn list_instance_backups(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<Vec<InstanceBackupResult>, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("instance backup reconciliation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    list_instance_backups_snapshot(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn rename_instance_backup(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    backup_id: String,
    display_name: Option<String>,
) -> Result<InstanceBackupResult, String> {
    let storage_context_operation =
        state.begin_storage_context_operation("instance backup rename")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let task_instance_id = instance_id.clone();
    let result = run_instance_mutation_to_completion(
        state.inner(),
        &storage_context_operation,
        &instance_id,
        move || {
            let storage = storage;
            async move {
                let result = rename_instance_backup_snapshot(
                    &storage.paths,
                    &task_instance_id,
                    &backup_id,
                    display_name,
                )
                .await
                .map_err(|error| error.to_string())?;
                append_desktop_app_log(
                    &storage,
                    "info",
                    "instance.backup.renamed",
                    "Instance backup renamed",
                    json!({
                        "instance_id": result.instance_id.as_str(),
                        "backup_id": result.backup_id,
                        "display_name": result.display_name,
                        "backup_kind": result.backup_kind,
                    }),
                );
                Ok(result)
            }
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn delete_instance_backup(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    backup_id: String,
) -> Result<InstanceBackupResult, String> {
    let storage_context_operation =
        state.begin_storage_context_operation("instance backup deletion")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let task_instance_id = instance_id.clone();
    let result = run_instance_mutation_to_completion(
        state.inner(),
        &storage_context_operation,
        &instance_id,
        move || {
            let storage = storage;
            async move {
                let result =
                    delete_instance_backup_snapshot(&storage.paths, &task_instance_id, &backup_id)
                        .await
                        .map_err(|error| error.to_string())?;
                append_desktop_app_log(
                    &storage,
                    "info",
                    "instance.backup.deleted",
                    "Instance backup deleted",
                    json!({
                        "instance_id": result.instance_id.as_str(),
                        "backup_id": result.backup_id,
                        "display_name": result.display_name,
                        "backup_kind": result.backup_kind,
                        "backup_path": result.backup_path,
                    }),
                );
                Ok(result)
            }
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn restore_instance_backup(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    backup_id: String,
    locale: Option<String>,
) -> Result<InstanceBackupRestoreResult, String> {
    let preference = app_network::SourcePreference::from_locale(locale.as_deref());
    let storage_context_operation =
        state.begin_storage_context_operation("instance backup restore")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let task_instance_id = instance_id.clone();
    let restore_operation = storage_context_operation.clone();
    let result = run_instance_mutation_to_completion(
        state.inner(),
        &storage_context_operation,
        &instance_id,
        move || {
            let storage = storage;
            async move {
                let active_run = read_active_instance_run(&storage.paths, &task_instance_id)
                    .await
                    .map_err(|error| error.to_string())?;
                if active_run.is_some() {
                    return Err(String::from("Stop the server before restoring a backup."));
                }

                let details = read_instance_details(&storage.paths, &task_instance_id)
                    .await
                    .map_err(|error| error.to_string())?;
                let result = if details.summary.module_id == "dontstarve" {
                    let prepared = app_storage::prepare_instance_backup_restore(
                        &storage.paths,
                        &task_instance_id,
                        &backup_id,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                    super::commands_dst_import_operation::restore_dst_backup_locked(
                        &storage,
                        &restore_operation,
                        &task_instance_id,
                        prepared,
                        preference,
                    )
                    .await?
                } else {
                    restore_instance_backup_snapshot(&storage.paths, &task_instance_id, &backup_id)
                        .await
                        .map_err(|error| error.to_string())?
                };
                append_desktop_app_log(
                    &storage,
                    "info",
                    "instance.backup.restored",
                    "Instance backup restored",
                    json!({
                        "instance_id": result.instance_id.as_str(),
                        "backup_id": result.backup_id,
                        "safeguard_backup_id": result.safeguard_backup_id,
                        "safeguard_backup_path": result.safeguard_backup_path,
                        "restored_file_count": result.restored_file_count,
                        "restored_total_bytes": result.restored_total_bytes,
                    }),
                );
                Ok(result)
            }
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn read_instance_runtime_overview_from_storage(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<InstanceRuntimeOverview, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("runtime overview reconciliation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let runtime_by_module = load_module_runtime_capability_map(&storage.paths.modules_root)
        .map_err(|message| logged_error_message(&storage, message))?;
    let mut overview = read_instance_runtime_overview(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    crate::runtime_astroneer_health::apply(&state, &storage.paths, &instance, &mut overview.health)
        .await;
    if let Some(pending_log) = pending_start_console_log_snapshot(&state, &instance_id, 32) {
        overview.log_tail = pending_log;
    }
    let module_runtime = runtime_by_module.get(&instance.summary.module_id);
    if module_runtime.is_some_and(|runtime| {
        runtime.player_count_source == app_core::ModulePlayerCountSource::PlayerList
    }) {
        overview.players =
            super::commands_player_counts::collect_player_list_count(&state, &instance_id).await?;
    }
    if let Some(configured_capacity) = extract_instance_player_capacity(&instance.settings_json) {
        overview.players.max_players = Some(configured_capacity);
    }
    overview.performance = build_runtime_performance_snapshot(
        &state,
        &instance,
        module_runtime.map(|runtime| &runtime.performance),
    );
    if let Some(diagnostic) = runtime_performance_state_diagnostic_signal(&overview.performance) {
        overview.diagnostics.push(diagnostic);
    }
    let startup_queue_process_count = instance
        .active_run
        .as_ref()
        .map(|run| run.process_count)
        .filter(|count| *count > 0)
        .unwrap_or(1);
    overview.startup_queue = build_runtime_startup_queue_snapshot(
        &state,
        &storage,
        &overview.performance.policy,
        startup_queue_process_count,
    )
    .await;
    let restart_policy = runtime_restart_policy_from_settings(&instance.settings_json);
    overview.stability.restart_policy_enabled = restart_policy.enabled;
    overview.stability.restart_limit = restart_policy.max_restarts;
    overview.stability.restart_backoff_ms = restart_policy.backoff_ms;
    overview.stability.pending_restart = state
        .runtime_restart_scheduler
        .lock()
        .ok()
        .and_then(|scheduler| scheduler.pending_restart_for(&instance_id));
    if restart_policy.enabled {
        overview.stability.summary = format!(
            "{} Auto-restart is armed with a {} crash restart limit and {}ms backoff.",
            overview.stability.summary, restart_policy.max_restarts, restart_policy.backoff_ms
        );
    }
    if let Some(pending_restart) = overview.stability.pending_restart.as_ref() {
        overview.stability.summary = format!(
            "{} A guarded restart is pending in {}ms after {} recent crash(es).",
            overview.stability.summary,
            pending_restart.delay_ms,
            pending_restart.recent_crash_count
        );
    }
    if let Some(diagnostic) = build_runtime_performance_diagnostic(
        &instance,
        module_runtime.map(|runtime| &runtime.performance),
    ) {
        overview.diagnostics.push(diagnostic);
    }
    Ok(overview)
}

#[tauri::command]
pub async fn read_instance_runtime_window_snapshot(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<RuntimeWindowSnapshot, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("runtime window reconciliation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;

    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;

    Ok(build_runtime_window_snapshot(&storage, &instance))
}

#[tauri::command]
pub async fn suppress_instance_runtime_windows(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<RuntimeWindowSuppressionResult, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("runtime window suppression")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;

    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    let install_root_override =
        load_module_install_root_override(&storage.paths, &instance.summary.module_id).await;
    let module = map_module_details_with_install_state(
        &storage.settings,
        descriptor,
        install_root_override.as_deref(),
    );

    if !matches!(
        module_window_policy(&module),
        ProcessWindowPolicy::Background
    ) {
        return Err(format!(
            "instance `{}` uses an external window policy, so LanGame will not suppress native windows for this module",
            instance.summary.id
        ));
    }

    let targets = build_window_inspection_targets_from_instance(&instance);
    match suppress_runtime_windows_for_targets(&instance.summary.id, &targets, "manual") {
        Ok(result) => {
            append_desktop_app_log(
                &storage,
                "info",
                RUNTIME_WINDOW_MANUAL_SUPPRESSION_ACTION,
                &result.summary,
                json!({
                    "instance_id": instance.summary.id,
                    "instance_name": instance.summary.name,
                    "visible_window_count_before": result.visible_window_count_before,
                    "suppressed_window_count": result.suppressed_window_count,
                    "remaining_visible_window_count": result.remaining_visible_window_count,
                    "inspected_process_count": result.inspected_process_count,
                }),
            );

            Ok(result)
        }
        Err(error) => {
            append_desktop_app_log(
                &storage,
                "error",
                RUNTIME_WINDOW_MANUAL_SUPPRESSION_FAILED_ACTION,
                &error,
                json!({
                    "instance_id": instance.summary.id,
                    "instance_name": instance.summary.name,
                    "process_count": targets.len(),
                }),
            );
            Err(logged_error_message(&storage, error))
        }
    }
}

#[tauri::command]
pub async fn read_palworld_operator_snapshot(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<PalworldOperatorSnapshot, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("Palworld operator reconciliation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;

    if !instance.summary.module_id.eq_ignore_ascii_case("palworld") {
        return Err(format!(
            "instance `{}` does not use the `palworld` module",
            instance.summary.id
        ));
    }

    Ok(build_palworld_operator_snapshot(&instance).await)
}

#[tauri::command]
pub async fn read_sevendaystodie_operator_snapshot(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<SevenDaysOperatorSnapshot, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("7 Days to Die operator reconciliation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;

    if !instance
        .summary
        .module_id
        .eq_ignore_ascii_case("sevendaystodie")
    {
        return Err(format!(
            "instance `{}` does not use the `sevendaystodie` module",
            instance.summary.id
        ));
    }

    Ok(build_sevendaystodie_operator_snapshot(&instance).await)
}

#[derive(serde::Serialize)]
pub struct RuntimeLogDocument {
    #[serde(flatten)]
    snapshot: LogTailSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_revision: Option<u64>,
}

#[tauri::command]
pub async fn read_instance_log_document_from_storage(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    max_lines: Option<usize>,
    run_id: Option<i64>,
    source: Option<String>,
) -> Result<RuntimeLogDocument, String> {
    if source
        .as_deref()
        .is_some_and(|value| value != "game" && value != "console")
    {
        return Err("Unknown runtime log source".into());
    }
    if source.is_some() && run_id.is_none() {
        return Err("An explicit runtime log source requires a run ID".into());
    }
    let _storage_context_operation =
        state.begin_storage_context_operation("instance log reconciliation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    if source.as_deref() == Some("game") {
        // Reconciliation precedes the lock; start/stop hold this same instance
        // lock through process registration and file generation changes.
        let _instance_mutation = state.acquire_instance_mutation(&instance_id).await;
        let document = app_storage::read_instance_game_log_document(
            &storage.paths,
            &instance_id,
            max_lines.unwrap_or(200),
            run_id.ok_or("Game log requires a run ID")?,
        )
        .await
        .map_err(|error| error.to_string())?;
        super::commands_runtime_lifecycle::start_runtime_game_log_stream(
            &app_handle,
            &state,
            &instance_id,
            &document,
        )?;
        return Ok(RuntimeLogDocument {
            snapshot: document.snapshot,
            snapshot_revision: Some(document.snapshot_revision),
        });
    }
    if run_id.is_none()
        && let Some(snapshot) =
            pending_start_console_log_snapshot(&state, &instance_id, max_lines.unwrap_or(200))
    {
        return Ok(RuntimeLogDocument {
            snapshot,
            snapshot_revision: None,
        });
    }
    read_instance_log_document(
        &storage.paths,
        &instance_id,
        max_lines.unwrap_or(200),
        run_id,
    )
    .await
    .map(|snapshot| RuntimeLogDocument {
        snapshot,
        snapshot_revision: None,
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_instance_record(
    state: tauri::State<'_, DesktopState>,
    input: CreateInstanceInput,
    program_mode: Option<app_core::InstanceProgramMode>,
) -> Result<InstanceProvisioning, String> {
    create_instance_record_with_program_mode(state, input, program_mode).await
}

pub(super) async fn create_instance_record_inner(
    state: tauri::State<'_, DesktopState>,
    input: CreateInstanceInput,
) -> Result<InstanceProvisioning, String> {
    create_instance_record_with_program_mode(state, input, None).await
}

async fn create_instance_record_with_program_mode(
    state: tauri::State<'_, DesktopState>,
    input: CreateInstanceInput,
    program_mode: Option<app_core::InstanceProgramMode>,
) -> Result<InstanceProvisioning, String> {
    let program_source = app_core::InstanceProgramSource::Verified;
    let storage_context_operation = state.begin_storage_context_operation("instance creation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let request_context = json!({
        "name": input.name.clone(),
        "module_id": input.module_id.clone(),
    });
    append_desktop_app_log(
        &storage,
        "info",
        "instance.create.request",
        "Create server requested",
        request_context.clone(),
    );

    initialize_database(&storage.paths).await.map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "instance.create.init_failed",
            &message,
            request_context.clone(),
        );
        logged_error_message(&storage, message)
    })?;

    let descriptors = discover_modules(&storage.paths.modules_root).map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "instance.create.discover_failed",
            &message,
            request_context.clone(),
        );
        logged_error_message(&storage, message)
    })?;
    sync_modules(&storage.paths, &descriptors)
        .await
        .map_err(|error| {
            let message = error.to_string();
            append_desktop_app_log(
                &storage,
                "error",
                "instance.create.sync_modules_failed",
                &message,
                json!({
                    "request": request_context.clone(),
                    "module_count": descriptors.len(),
                }),
            );
            logged_error_message(&storage, message)
        })?;
    let module_id = input.module_id.clone();
    let descriptor = find_descriptor(&descriptors, &module_id).map_err(|message| {
        append_desktop_app_log(
            &storage,
            "error",
            "instance.create.module_missing",
            &message,
            request_context.clone(),
        );
        logged_error_message(&storage, message)
    })?;
    let library_root = app_storage::read_library_program_install(&storage.paths, &module_id)
        .await
        .map_err(|error| error.to_string())?
        .map(|record| record.install_root)
        .or_else(|| {
            descriptor
                .install
                .as_ref()
                .map(|install| storage.paths.games_root.join(&install.shared_game_dir))
        })
        .ok_or_else(|| String::from("module has no program installation contract"))?;
    // A repaired source outlives every instance created from it.
    let repair_root =
        storage
            .paths
            .games_root
            .join(format!("{}-original-{}", module_id, uuid::Uuid::new_v4()));
    let mut program_root = library_root;
    // Creation copies and materializes game files. Serialize its initial probe
    // and complete worker with uninstall/update; it owns no existing instance lock.
    #[cfg(test)]
    let creation_hooks = super::tests::creation_lifecycle_tests::current_hooks();
    #[cfg(test)]
    let forbid_program_install =
        super::tests::creation_lifecycle_tests::program_install_forbidden();
    #[cfg(test)]
    if let Some(hooks) = &creation_hooks {
        hooks.before_lock.notify_one();
    }
    let mut attempts = 0;
    let lifecycle_guard = loop {
        attempts += 1;
        let program_roots = [program_root.clone(), repair_root.clone()];
        let guard = tokio::select! {
        biased;
        _ = storage_context_operation.cancelled() => {
            let message = String::from("Instance creation cancelled for application shutdown.");
            append_desktop_app_log(
                &storage,
                "info",
                "instance.create.cancelled",
                &message,
                request_context.clone(),
            );
            return Err(message);
        }
        result = app_steamcmd::acquire_game_install_lifecycle(&module_id, &program_roots) => {
            result.map_err(|error| steamcmd_error_message(&error))?
        }
        };
        let selected = app_storage::read_library_program_install(&storage.paths, &module_id)
            .await
            .map_err(|error| error.to_string())?
            .map(|record| record.install_root)
            .unwrap_or_else(|| program_root.clone());
        if selected == program_root {
            break guard;
        }
        drop(guard);
        if attempts >= 3 {
            return Err(
                "The program installation changed while creation was queued; retry creation."
                    .into(),
            );
        }
        program_root = selected;
    };

    if program_mode == Some(app_core::InstanceProgramMode::Shared)
        && descriptor.storage.program_sharing != app_modules::ModuleProgramSharing::Shared
    {
        return Err(String::from(
            "此游戏尚不支持共享服务器程序，请使用独立安装。",
        ));
    }
    let creation_storage = storage.clone();
    let creation_operation = storage_context_operation.clone();
    let creation_descriptor = descriptor.clone();
    use super::commands_install_progress::{
        InstallationJobLease, complete_install_progress, fail_install_progress,
        queued_install_progress,
    };
    let job_id = new_background_job_id("instance-create", &module_id);
    let creation_job = InstallationJobLease::begin(&state, job_id.clone())?;
    insert_background_job(
        &state,
        &storage,
        BackgroundJob {
            id: job_id,
            kind: JobKind::ValidateGame,
            label: format!("Create {}", input.name),
            status: JobStatus::Pending,
            progress_percent: 1.0,
            install_progress: Some(queued_install_progress()),
            cancellable: true,
            cancel_requested: false,
            target_id: Some(module_id.clone()),
            detail: Some("正在核验服务器程序…".into()),
            output_excerpt: None,
        },
    )?;
    let provisioning = spawn_storage_context_task(&storage_context_operation, async move {
        // A dropped IPC waiter must not unlock while this owned task still writes.
        let lifecycle_guard = lifecycle_guard;
        #[cfg(test)]
        if let Some(hooks) = creation_hooks {
            hooks.pause_worker().await;
        }
        let work = super::commands_program_storage::create_with_program_repair(
            super::commands_program_storage::CreationProgramRequest {
                storage: &creation_storage,
                descriptor: &creation_descriptor,
                operation: &creation_operation,
                guard: &lifecycle_guard,
                input,
                mode: program_mode,
                source: program_source,
                program_root: &program_root,
                repair_root: &repair_root,
                job: &creation_job,
            },
            |module, root, cancellation| {
                let settings = &creation_storage.settings;
                let guard = &lifecycle_guard;
                let job = &creation_job;
                async move {
                    #[cfg(test)]
                    if forbid_program_install {
                        return Err(
                            "headless creation unexpectedly requested an external installer".into(),
                        );
                    }
                    app_steamcmd::install_or_update_module_at_with_progress_and_cancellation(
                        settings,
                        &module,
                        &root,
                        guard,
                        true,
                        &cancellation,
                        |update| super::commands_program_storage::report_progress(job, &update),
                    )
                    .await
                    .map_err(|error| steamcmd_error_message(&error))
                }
            },
        );
        tokio::pin!(work);
        // Cancellation signals providers, then joins their cleanup. Never drop
        // the worker or release its game lease while it is still writing files.
        let result = tokio::select! {
            biased;
            _ = creation_job.cancellation().cancelled() => {
                creation_operation.cancellation_token().store(true, Ordering::SeqCst);
                work.await
            }
            _ = creation_operation.cancelled() => {
                creation_job.cancellation().cancel();
                work.await
            }
            result = &mut work => result,
        };
        creation_job.update(|job| match &result {
            Ok(_) => {
                complete_install_progress(job);
                job.detail = Some("服务器创建完成".into());
            }
            Err(error) => {
                fail_install_progress(job, 1.0);
                if error == "installation_cancelled" {
                    job.status = JobStatus::Cancelled;
                }
                job.detail = Some(error.clone());
            }
        })?;
        result
    })
    .await
    .map_err(|error| format!("instance creation task failed: {error}"))?
    .map_err(|error| {
        let message = error.to_string();
        let cancelled = message == "installation_cancelled";
        append_desktop_app_log(
            &storage,
            if cancelled { "info" } else { "error" },
            if cancelled {
                "instance.create.cancelled"
            } else {
                "instance.create.persist_failed"
            },
            &message,
            request_context.clone(),
        );
        if cancelled {
            message
        } else {
            logged_error_message(&storage, message)
        }
    })?;
    let instances = list_instances(&storage.paths).await.map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "instance.create.reload_failed",
            &message,
            json!({
                "request": request_context.clone(),
                "instance_id": provisioning.summary.id.clone(),
            }),
        );
        logged_error_message(&storage, message)
    })?;

    update_state_instances(&state, instances).map_err(|message| {
        append_desktop_app_log(
            &storage,
            "error",
            "instance.create.state_write_failed",
            &message,
            json!({
                "request": request_context.clone(),
                "instance_id": provisioning.summary.id.clone(),
            }),
        );
        logged_error_message(&storage, message)
    })?;

    append_desktop_app_log(
        &storage,
        "info",
        "instance.create.success",
        "Create server succeeded",
        json!({
            "request": request_context,
            "instance_id": provisioning.summary.id,
            "port_count": provisioning.ports.len(),
            "config_file_path": provisioning.config_file_path,
        }),
    );

    Ok(provisioning)
}
#[cfg(test)]
pub async fn update_instance_record(
    state: tauri::State<'_, DesktopState>,
    input: UpdateInstanceInput,
) -> Result<InstanceDetails, String> {
    update_instance_record_with_precondition(state, input, None, None).await
}

#[tauri::command]
pub async fn update_instance_record_if_current(
    state: tauri::State<'_, DesktopState>,
    input: UpdateInstanceInput,
    expected_settings_json: String,
) -> Result<InstanceDetails, String> {
    update_instance_record_with_precondition(
        state,
        input,
        Some(expected_settings_json.as_str()),
        None,
    )
    .await
}

pub(super) async fn update_instance_record_if_current_state(
    state: tauri::State<'_, DesktopState>,
    input: UpdateInstanceInput,
    expected: InstanceDetails,
) -> Result<InstanceDetails, String> {
    let settings_json = expected.settings_json.clone();
    update_instance_record_with_precondition(state, input, Some(&settings_json), Some(expected))
        .await
}

async fn update_instance_record_with_precondition(
    state: tauri::State<'_, DesktopState>,
    input: UpdateInstanceInput,
    expected_settings_json: Option<&str>,
    expected_instance: Option<InstanceDetails>,
) -> Result<InstanceDetails, String> {
    let storage_context_operation = state.begin_storage_context_operation("instance update")?;
    let expected_settings_json = expected_settings_json.map(str::to_owned);
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let request_context = json!({
        "instance_id": input.id.as_str(),
        "bind_ip": input.bind_ip.as_str(),
        "auto_backup_on_stop": input.auto_backup_on_stop,
        "backup_retention_count": input.backup_retention_count,
        "port_count": input.ports.len(),
        "has_settings_precondition": expected_settings_json.is_some(),
    });
    append_desktop_app_log(
        &storage,
        "info",
        "instance.update.request",
        "Update server requested",
        request_context.clone(),
    );

    initialize_database(&storage.paths).await.map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "instance.update.init_failed",
            &message,
            request_context.clone(),
        );
        logged_error_message(&storage, message)
    })?;

    let instance_lock = state.acquire_instance_mutation(&input.id).await;
    let current = read_instance_details(&storage.paths, &input.id)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(expected) = expected_instance
        && serde_json::to_value(&current).map_err(|error| error.to_string())?
            != serde_json::to_value(&expected).map_err(|error| error.to_string())?
    {
        return Err(String::from(
            "The server changed after the operation was prepared. Request a new preview before saving.",
        ));
    }
    let program_policy_guard = program_update_policy::acquire_policy_change(
        &state,
        &storage,
        &current,
        &input.settings_json,
    )
    .await?;
    let update_paths = storage.paths.clone();
    let update_result = spawn_storage_context_task(&storage_context_operation, async move {
        let _instance_lock = instance_lock;
        let _program_policy_guard = program_policy_guard;
        match expected_settings_json {
            Some(expected) => update_instance_if_current(&update_paths, input, &expected).await,
            None => update_instance(&update_paths, input).await,
        }
    })
    .await
    .map_err(|error| format!("instance update task failed: {error}"))?;
    let details = update_result.map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "instance.update.failed",
            &message,
            request_context.clone(),
        );
        logged_error_message(&storage, message)
    })?;
    state
        .live_player_registry
        .invalidate_instance(&details.summary.id);
    if details.summary.module_id == "unturned" {
        super::commands_managed_save::invalidate_instance_policy(&state);
    }
    let instances = list_instances(&storage.paths).await.map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "instance.update.list_failed",
            &message,
            request_context.clone(),
        );
        logged_error_message(&storage, message)
    })?;

    update_state_instances(&state, instances).map_err(|message| {
        append_desktop_app_log(
            &storage,
            "error",
            "instance.update.state_write_failed",
            &message,
            request_context.clone(),
        );
        logged_error_message(&storage, message)
    })?;

    append_desktop_app_log(
        &storage,
        "info",
        "instance.update.success",
        "Update server succeeded",
        json!({
            "request": request_context,
            "instance_id": details.summary.id,
            "module_id": details.summary.module_id,
            "config_file_path": details.config_file_path,
            "saves_path": details.saves_path,
            "port_count": details.ports.len(),
        }),
    );

    Ok(details)
}

#[tauri::command]
pub async fn import_dontstarve_world_data(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    source_path: String,
    locale: Option<String>,
) -> Result<DstWorldImportResult, String> {
    let preference = app_network::SourcePreference::from_locale(locale.as_deref());
    let storage_context_operation = state.begin_storage_context_operation("DST world import")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let trimmed_source_path = source_path.trim().to_owned();
    append_desktop_app_log(
        &storage,
        "info",
        "instance.dst_world_import.request",
        "Importing DST world data",
        json!({
            "instance_id": instance_id.as_str(),
            "source_path": trimmed_source_path.as_str(),
        }),
    );

    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let task_instance_id = instance_id.clone();
    let import_operation = storage_context_operation.clone();
    run_instance_mutation_to_completion(
        state.inner(),
        &storage_context_operation,
        &instance_id,
        move || async move {
            if trimmed_source_path.is_empty() { return Err("Select a DST cluster folder before importing world data.".into()); }
            let replacement = super::commands_dst_import_operation::replace_dst_world_locked(
                &storage, &import_operation, &task_instance_id, PathBuf::from(trimmed_source_path), None, preference,
            ).await?;
            let result = replacement.result;
            append_desktop_app_log(&storage, "info", "instance.dst_world_import.success", "DST world data imported",
                json!({ "instance_id": result.instance_id, "imported_shards": result.imported_shards,
                    "imported_workshop_mod_ids": result.imported_workshop_mod_ids, "safeguard_path": result.safeguard_path,
                    "copied_file_count": result.copied_file_count, "copied_total_bytes": result.copied_total_bytes }));
            Ok(result)
        },
    )
    .await
}

#[cfg(test)]
mod module_sync_tests {
    use super::*;

    #[test]
    fn dst_import_command_owns_the_complete_mutation_pipeline() {
        let source = include_str!("commands_storage.rs");
        let command = source
            .split("pub async fn import_dontstarve_world_data(")
            .nth(1)
            .expect("DST import command")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let owner = command
            .find("run_instance_mutation_to_completion(")
            .expect("DST import must retain its mutation owner when its caller is cancelled");
        for operation in [
            "replace_dst_world_locked(",
            "instance.dst_world_import.success",
        ] {
            assert!(
                command.find(operation).unwrap() > owner,
                "{operation} must run inside the owned pipeline"
            );
        }
        let operation = include_str!("commands_dst_import_operation.rs");
        for required in [
            "read_active_instance_run(",
            "prepare_dontstarve_target_cluster(",
            "create_instance_backup_snapshot(",
            "import_dontstarve_world_transaction(",
            "update_instance_if_current(",
        ] {
            assert!(
                operation.contains(required),
                "Missing owned operation {required}"
            );
        }
        assert!(
            !operation.contains("run_instance_mutation_to_completion("),
            "Shared operation must not recursively acquire its caller's instance owner"
        );
    }

    #[tokio::test]
    async fn dst_import_cancelled_caller_retains_locks_through_validation_and_publication() {
        use std::future::Future;
        use std::task::Poll;

        let state = Arc::new(DesktopState::default());
        let worker_state = Arc::clone(&state);
        let (validation_started_tx, validation_started_rx) = tokio::sync::oneshot::channel();
        let (release_validation_tx, release_validation_rx) = std::sync::mpsc::channel();
        let (publication_started_tx, publication_started_rx) = tokio::sync::oneshot::channel();
        let (release_publication_tx, release_publication_rx) = std::sync::mpsc::channel();
        let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
        let caller = tokio::spawn(async move {
            let operation = worker_state
                .begin_storage_context_operation("DST import cancellation test")
                .unwrap();
            run_instance_mutation_to_completion(
                &worker_state,
                &operation,
                "dst-import-cancel",
                move || async move {
                    tokio::task::spawn_blocking(move || {
                        validation_started_tx.send(()).unwrap();
                        release_validation_rx.recv().unwrap();
                    })
                    .await
                    .unwrap();
                    tokio::task::spawn_blocking(move || {
                        publication_started_tx.send(()).unwrap();
                        release_publication_rx.recv().unwrap();
                    })
                    .await
                    .unwrap();
                    completed_tx.send(()).unwrap();
                    Ok::<_, String>(())
                },
            )
            .await
        });

        validation_started_rx.await.unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert!(state.begin_storage_context_transition().is_err());
        let mut waiting_lock = Box::pin(state.acquire_instance_mutation("dst-import-cancel"));
        assert!(
            std::future::poll_fn(|context| Poll::Ready(
                waiting_lock.as_mut().poll(context).is_pending()
            ))
            .await
        );

        release_validation_tx.send(()).unwrap();
        publication_started_rx.await.unwrap();
        assert!(state.begin_storage_context_transition().is_err());
        assert!(
            std::future::poll_fn(|context| Poll::Ready(
                waiting_lock.as_mut().poll(context).is_pending()
            ))
            .await
        );
        release_publication_tx.send(()).unwrap();
        completed_rx.await.unwrap();
        let _released_instance_lock = tokio::time::timeout(Duration::from_secs(5), waiting_lock)
            .await
            .expect("completed import releases the instance lock");
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(transition) = state.begin_storage_context_transition() {
                    break transition;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("completed import releases its storage lease");
    }

    #[tokio::test]
    async fn aborted_instance_mutation_caller_keeps_storage_lease_until_worker_finishes() {
        let state = Arc::new(DesktopState::default());
        let worker_state = Arc::clone(&state);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
        let caller = tokio::spawn(async move {
            let operation = worker_state
                .begin_storage_context_operation("instance mutation abort test")
                .expect("storage operation");
            run_instance_mutation_to_completion(
                &worker_state,
                &operation,
                "abortable-instance",
                move || async move {
                    started_tx.send(()).expect("started signal");
                    release_rx.await.expect("release signal");
                    completed_tx.send(()).expect("completed signal");
                    Ok::<_, String>(())
                },
            )
            .await
        });

        started_rx.await.expect("started signal");
        caller.abort();
        let _ = caller.await;
        assert!(state.begin_storage_context_transition().is_err());

        release_tx.send(()).expect("release worker");
        completed_rx.await.expect("completed signal");
        tokio::task::yield_now().await;
        state
            .begin_storage_context_transition()
            .expect("worker completion should release the cloned storage lease");
    }

    struct ModuleSyncTestRoot(PathBuf);

    impl ModuleSyncTestRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "langame-module-sync-concurrency-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&root).expect("create module sync test root");
            Self(root)
        }

        fn storage(&self) -> StorageBootstrap {
            let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("..");
            let app_data_root = self.0.join("data");
            let runtime_root = self.0.join("runtime");
            let paths = app_storage::StoragePaths {
                app_data_root: app_data_root.clone(),
                settings_path: app_data_root.join("settings.json"),
                database_path: app_data_root.join("db").join("lgs.db"),
                logs_root: app_data_root.join("logs"),
                modules_root: repository_root.join("modules"),
                migrations_root: repository_root.join("migrations"),
                steamcmd_root: runtime_root.join("cmd").join("steamcmd"),
                games_root: runtime_root.join("server-files"),
                instances_root: runtime_root.join("instances"),
                archives_root: runtime_root.join("instances").join(".trash"),
            };
            for path in [
                paths.database_path.parent().expect("database parent"),
                paths.logs_root.as_path(),
                paths.steamcmd_root.as_path(),
                paths.games_root.as_path(),
                paths.instances_root.as_path(),
            ] {
                fs::create_dir_all(path).expect("prepare module sync test path");
            }
            assert!(paths.modules_root.is_dir(), "repository modules must exist");

            StorageBootstrap {
                settings: paths.settings(),
                storage_status: paths.probe_status(),
                paths,
            }
        }
    }

    impl Drop for ModuleSyncTestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_module_sync_calls_share_one_app_writer() {
        let root = ModuleSyncTestRoot::new();
        let storage = root.storage();
        initialize_database(&storage.paths)
            .await
            .expect("pre-initialize test database");
        let state = DesktopState::default();
        let start = Arc::new(tokio::sync::Barrier::new(3));

        let first = async {
            start.wait().await;
            sync_modules_to_storage_with(&state, &storage).await
        };
        let second = async {
            start.wait().await;
            sync_modules_to_storage_with(&state, &storage).await
        };
        let release = async {
            start.wait().await;
        };
        let (first, second, ()) = tokio::join!(first, second, release);
        let first = first.expect("first module sync");
        let second = second.expect("second module sync");

        assert!(!first.is_empty());
        assert_eq!(first.len(), second.len());
        let actions = fs::read_to_string(storage.paths.app_log_path())
            .expect("read module sync log")
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|entry| {
                entry
                    .get("action")
                    .and_then(Value::as_str)
                    .filter(|action| action.starts_with("modules.sync."))
                    .map(String::from)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            vec![
                String::from("modules.sync.request"),
                String::from("modules.sync.success"),
                String::from("modules.sync.request"),
                String::from("modules.sync.success"),
            ],
            "a second sync must not enter before the first has published its result",
        );
    }
}
