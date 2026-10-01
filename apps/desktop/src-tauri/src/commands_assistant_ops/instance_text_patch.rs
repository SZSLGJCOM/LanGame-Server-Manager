fn validate_assistant_text_patch_plan(plan: &AssistantOperationPlan) -> Result<(), String> {
    validate_assistant_file_patches_plan(plan)?;
    if plan.action != AssistantOperationAction::PatchInstanceText {
        return if plan.text_patch.is_some() {
            Err(String::from(
                "textPatch is only valid for patch_instance_text.",
            ))
        } else {
            Ok(())
        };
    }
    let patch = plan
        .text_patch
        .as_ref()
        .ok_or("Supply textPatch from a read_instance_file result.")?;
    if patch.before.is_empty()
        || patch.before == patch.after
        || patch.before.len() + patch.after.len() > 8 * 1024
        || patch.source_sha256.len() != 64
        || !patch
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(String::from(
            "Supply a nonempty unique before segment, changed after text, and the exact source SHA256; replacement text is limited to 8 KiB.",
        ));
    }
    if plan.settings_patch.is_some()
        || plan.port_patch.is_some()
        || !plan.runtime_commands.is_empty()
        || !plan.workshop_item_ids.is_empty()
        || !plan.mod_references.is_empty()
        || !plan.source_paths.is_empty()
        || plan.broadcast_intent.is_some()
    {
        return Err(String::from(
            "A text patch cannot include another operation.",
        ));
    }
    Ok(())
}

fn ensure_assistant_patch_target_stopped(instance: &InstanceDetails) -> Result<(), String> {
    if !matches!(instance.summary.status, InstanceStatus::Stopped)
        || instance.summary.active_process_count != 0
        || instance.active_run.is_some()
    {
        return Err(String::from(
            "Stop the selected server before previewing or applying a Mod file patch.",
        ));
    }
    Ok(())
}

fn assistant_text_patch_preview(
    prepared: &app_storage::PreparedInstanceTextPatch,
) -> Result<app_storage::InstanceTextPatchPreview, String> {
    let mut preview = prepared.preview().clone();
    preview.before = redact_assistant_file_content(Path::new(&preview.file), &preview.before)?;
    preview.after = redact_assistant_file_content(Path::new(&preview.file), &preview.after)?;
    Ok(preview)
}

async fn prepare_assistant_text_patch(
    storage: &StorageBootstrap,
    plan: &AssistantOperationPlan,
    instance: Option<&InstanceDetails>,
) -> Result<Option<app_storage::PreparedInstanceTextPatch>, String> {
    validate_assistant_text_patch_plan(plan)?;
    if plan.action != AssistantOperationAction::PatchInstanceText {
        return Ok(None);
    }
    let instance = instance.ok_or("Select an existing instance before editing its files.")?;
    ensure_assistant_patch_target_stopped(instance)?;
    let patch = plan
        .text_patch
        .clone()
        .ok_or("The text patch is missing.")?;
    app_storage::prepare_instance_file_patch(&storage.paths, &instance.summary.id, patch)
        .await
        .map(Some)
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))
}

async fn execute_assistant_text_patch(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    mode: &AssistantOperationMode,
    output: &mut AssistantExecuteOperationOutput,
) -> Result<(), String> {
    let AssistantOperationMode::Confirmed(pending) = mode else {
        return Err(String::from(
            "File changes require a concrete, single-use preview confirmation.",
        ));
    };
    let instance_id = pending
        .plan
        .instance_id
        .clone()
        .ok_or("The file target is missing.")?;
    let prepared = pending
        .prepared_text_patch
        .clone()
        .ok_or("The confirmed file change is missing.")?;
    let precondition = pending
        .precondition
        .clone()
        .ok_or("The confirmed file target state is missing.")?;
    let lease = state.begin_storage_context_operation("assistant instance file patch")?;
    reconcile_runtime_state(state).await?;
    let paths = storage.paths.clone();
    let target = instance_id.clone();
    let result = super::commands_storage::run_instance_mutation_to_completion(
        state.inner(),
        &lease,
        &instance_id,
        move || async move {
            let current = read_instance_details(&paths, &target)
                .await
                .map_err(|error| error.to_string())?;
            precondition.validate(&current)?;
            ensure_assistant_patch_target_stopped(&current)?;
            app_storage::apply_instance_file_patch(&paths, &target, prepared)
                .await
                .map_err(|error| redact_assistant_provider_text(&error.to_string()))
        },
    )
    .await?;
    if !result.read_back_verified {
        return Err(format!(
            "File verification failed; preserved backup: {}.",
            result.backup_id
        ));
    }
    output.instance_id = Some(instance_id.clone());
    output.module_id = pending.plan.module_id.clone();
    output.message = format!(
        "Patched and read back `{}`. Backup: {}. Game behavior and runtime recovery still require verification.",
        result.file, result.backup_id
    );
    append_desktop_app_log(
        storage,
        "info",
        "assistant.instance_file.patched",
        "Confirmed instance file patch saved",
        json!({
            "instance_id": instance_id, "file": result.file, "backup_id": result.backup_id,
            "source_sha256": result.source_sha256, "result_sha256": result.result_sha256,
        }),
    );
    output.file_change_result = Some(result);
    Ok(())
}

async fn verify_assistant_task_file_changes(
    storage: &StorageBootstrap,
    task: &AssistantTaskContract,
) -> Result<(), String> {
    if task.file_changes.is_empty() {
        return Ok(());
    }
    let instance_id = task
        .instance_id
        .as_deref()
        .ok_or("The file verification target is missing.")?;
    for expected in &task.file_changes {
        let current =
            app_storage::read_instance_patch_file(&storage.paths, instance_id, &expected.file)
                .await
                .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
        if current.source_sha256 != expected.result_sha256 {
            return Err(format!(
                "Confirmed file `{}` changed after its patch. Recovery is unverified; backup {} remains available.",
                expected.file, expected.backup_id
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "instance_text_patch_tests.rs"]
mod instance_text_patch_tests;

include!("instance_file_patches.rs");
