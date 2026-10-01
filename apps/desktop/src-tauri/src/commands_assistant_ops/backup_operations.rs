async fn read_assistant_backup_catalog(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    offset: usize,
) -> Result<Value, String> {
    let paths = storage.paths.clone();
    let instance_id = instance.summary.id.clone();
    let runtime = tokio::runtime::Handle::current();
    // The desktop catalog reconciles runtime state and can launch due restarts.
    // Use only the storage reader, within the bounded evidence worker whose
    // lease survives cancellation until database/filesystem work actually ends.
    let backups = run_assistant_evidence_read(state, move || {
        runtime
            .block_on(app_storage::list_instance_backups(&paths, &instance_id))
            .map_err(|error| error.to_string())
    })
    .await?;
    if offset > backups.len() {
        return Err("Backup offset is outside the current list; start at zero.".into());
    }
    let entries: Vec<_> = backups.iter().skip(offset).take(20).map(|backup| json!({
        "backupId": backup.backup_id, "displayName": backup.display_name.as_deref().map(redact_assistant_provider_text),
        "createdAtUnixMs": backup.created_at_unix_ms, "kind": backup.backup_kind,
        "fileCount": backup.file_count, "totalBytes": backup.total_bytes,
    })).collect();
    let end = offset + entries.len();
    Ok(json!({"instanceId":instance.summary.id,"backups":entries,
        "nextOffset":(end < backups.len()).then_some(end),
        "scope":"Save backups only. Exact source bytes and the current saves are bound when a restore preview is prepared."}))
}

#[cfg(test)]
#[path = "backup_catalog_tests.rs"]
mod backup_catalog_tests;

async fn prepare_assistant_backup_restore(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    plan: &AssistantOperationPlan,
    instance: Option<&InstanceDetails>,
) -> Result<Option<app_storage::PreparedInstanceBackupRestore>, String> {
    if plan.action != AssistantOperationAction::RestoreBackup {
        return Ok(None);
    }
    let instance = instance.ok_or("A restore requires an existing instance.")?;
    ensure_assistant_lifecycle_stopped(instance)?;
    let target = instance.summary.id.clone();
    let id = target.clone();
    let backup_id = plan
        .backup_id
        .clone()
        .ok_or("The restore backup ID is missing.")?;
    let expected = AssistantOperationPrecondition::from_details(instance);
    let paths = storage.paths.clone();
    let lease = state.begin_storage_context_operation("assistant backup preview")?;
    super::commands_storage::run_instance_mutation_to_completion(
        state.inner(),
        &lease,
        &id,
        move || async move {
            let current = read_instance_details(&paths, &target)
                .await
                .map_err(|error| error.to_string())?;
            expected.validate(&current)?;
            ensure_assistant_lifecycle_stopped(&current)?;
            app_storage::prepare_instance_backup_restore(&paths, &target, &backup_id)
                .await
                .map(Some)
                .map_err(|error| redact_assistant_provider_text(&error.to_string()))
        },
    )
    .await
}

async fn execute_assistant_backup_operation(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    pending: &AssistantPendingOperation,
    output: &mut AssistantExecuteOperationOutput,
) -> Result<(), String> {
    let target = pending
        .plan
        .instance_id
        .clone()
        .ok_or("The backup instance is missing.")?;
    let instance_id = target.clone();
    let expected = pending
        .precondition
        .clone()
        .ok_or("The confirmed backup target state is missing.")?;
    let action = pending.plan.action;
    let prepared = pending.prepared_backup_restore.clone();
    let backup_id = pending.plan.backup_id.clone();
    let paths = storage.paths.clone();
    let lease = state.begin_storage_context_operation("assistant backup operation")?;
    let restore_operation = lease.clone();
    let restore_storage = storage.clone();
    let preference = pending.lifecycle_locale.source_preference();
    let task = pending.task.clone();
    // Reconciliation also owns the automatic restart future. Keep that runtime
    // workflow out of the nested assistant confirmation/backup stack frames.
    Box::pin(reconcile_runtime_state(state)).await?;
    let (evidence, restored_instance) = super::commands_storage::run_instance_mutation_to_completion(state.inner(), &lease, &instance_id, move || async move {
        let current = read_instance_details(&paths, &target).await.map_err(|error| error.to_string())?;
        expected.validate(&current)?;
        ensure_assistant_lifecycle_stopped(&current)?;
        match action {
            AssistantOperationAction::CreateBackup => {
                let created = app_storage::create_instance_backup(&paths, &target).await.map_err(|error| error.to_string())?;
                let verified = app_storage::prepare_instance_backup_restore(&paths, &target, &created.backup_id).await.map_err(|error| error.to_string())?;
                if !verified.matches_current_saves() {
                    return Err("The backup was created, but content readback does not match the current saves; inspect it before restoring.".into());
                }
                Ok((json!({"instanceId":target,"backupId":created.backup_id,"createdAtUnixMs":created.created_at_unix_ms,
                    "fileCount":created.file_count,"totalBytes":created.total_bytes,"sourceSha256":verified.source_sha256(),"readBackVerified":true}), None))
            }
            AssistantOperationAction::RestoreBackup => {
                let prepared = prepared.ok_or("The restore preview snapshot is missing; prepare a new confirmation.")?;
                if backup_id.as_deref() != Some(prepared.backup.backup_id.as_str()) || prepared.backup.instance_id != target {
                    return Err("The backup source does not match the reviewed restore.".into());
                }
                let hash = prepared.source_sha256().to_owned();
                let expected_restored = if current.summary.module_id == "dontstarve" {
                    super::commands_dst_import_operation::prepare_dst_backup_restore_state(&restore_storage, &current, &prepared).await?
                } else {
                    current.clone()
                };
                // A restore may recover world settings, but the task's explicit
                // Mod preservation policy must hold before any saves are replaced.
                let restored_settings = serde_json::from_str(&expected_restored.settings_json).map_err(|error| error.to_string())?;
                task.validate_settings(&restored_settings)?;
                let restored = if current.summary.module_id == "dontstarve" {
                    super::commands_dst_import_operation::restore_dst_backup_locked(&restore_storage, &restore_operation, &target, prepared, preference).await?
                } else {
                    app_storage::restore_prepared_instance_backup(&paths, &target, prepared).await.map_err(|error| error.to_string())?
                };
                let backups = app_storage::list_instance_backups(&paths, &target).await.map_err(|error| error.to_string())?;
                if !backups.iter().any(|backup| backup.backup_id == restored.safeguard_backup_id && backup.instance_id == target) {
                    return Err("Saves were restored, but the safeguard backup could not be read back. Inspect storage before further changes.".into());
                }
                let saved = read_instance_details(&paths, &target).await.map_err(|error| error.to_string())?;
                validate_assistant_lifecycle_saved_state(&expected_restored, &saved)?;
                ensure_assistant_lifecycle_stopped(&saved)?;
                Ok((json!({"instanceId":target,"backupId":restored.backup_id,"safeguardBackupId":restored.safeguard_backup_id,
                    "restoredFileCount":restored.restored_file_count,"restoredTotalBytes":restored.restored_total_bytes,
                    "sourceSha256":hash,"readBackVerified":true,"serverStarted":false}), Some(Box::new(expected_restored))))
            }
            _ => Err("This action is not a backup operation.".into()),
        }
    }).await.map_err(|error| redact_assistant_provider_text(&error))?;
    output.restored_instance = restored_instance;
    let name = pending
        .expected_instance
        .as_ref()
        .ok_or("The confirmed backup target is missing.")?
        .summary
        .name
        .as_str();
    output.message = pending.lifecycle_locale.verified(action, name, &evidence);
    append_desktop_app_log(
        storage,
        "info",
        "assistant.backup.verified",
        &output.message,
        evidence.clone(),
    );
    output.verification = Some(AssistantOperationVerification {
        status: AssistantVerificationStatus::Verified,
        summary: output.message.clone(),
        run_id: None,
        evidence,
        can_continue: false,
    });
    Ok(())
}
