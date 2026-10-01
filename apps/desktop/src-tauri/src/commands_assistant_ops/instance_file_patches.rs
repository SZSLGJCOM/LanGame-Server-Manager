fn validate_assistant_file_patches_plan(plan: &AssistantOperationPlan) -> Result<(), String> {
    if plan.action != AssistantOperationAction::PatchInstanceFiles {
        return if plan.file_patches.is_empty() {
            Ok(())
        } else {
            Err(String::from(
                "filePatches is only valid for patch_instance_files.",
            ))
        };
    }
    app_storage::validate_instance_file_edits(&plan.file_patches)
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    if plan.text_patch.is_some()
        || plan.settings_patch.is_some()
        || plan.port_patch.is_some()
        || !plan.runtime_commands.is_empty()
        || !plan.workshop_item_ids.is_empty()
        || !plan.mod_references.is_empty()
        || !plan.source_paths.is_empty()
        || plan.broadcast_intent.is_some()
    {
        return Err(String::from(
            "A file edit set cannot include another operation.",
        ));
    }
    Ok(())
}

fn assistant_file_patches_preview(
    prepared: &app_storage::PreparedInstanceFilePatches,
) -> Result<Vec<app_storage::InstanceFileEditsPreview>, String> {
    let mut previews = prepared.previews().to_vec();
    for preview in &mut previews {
        for edit in &mut preview.edits {
            edit.before = redact_assistant_file_content(Path::new(&preview.file), &edit.before)?;
            edit.after = redact_assistant_file_content(Path::new(&preview.file), &edit.after)?;
        }
    }
    Ok(previews)
}

async fn prepare_assistant_file_patches(
    storage: &StorageBootstrap,
    plan: &AssistantOperationPlan,
    instance: Option<&InstanceDetails>,
) -> Result<Option<app_storage::PreparedInstanceFilePatches>, String> {
    validate_assistant_text_patch_plan(plan)?;
    if plan.action != AssistantOperationAction::PatchInstanceFiles {
        return Ok(None);
    }
    let instance = instance.ok_or("Select an existing instance before editing its files.")?;
    ensure_assistant_patch_target_stopped(instance)?;
    app_storage::prepare_instance_file_patches(
        &storage.paths,
        &instance.summary.id,
        plan.file_patches.clone(),
    )
    .await
    .map(Some)
    .map_err(|error| redact_assistant_provider_text(&error.to_string()))
}

async fn execute_assistant_file_patches(
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
        .prepared_file_patches
        .clone()
        .ok_or("The confirmed file edit set is missing.")?;
    let precondition = pending
        .precondition
        .clone()
        .ok_or("The confirmed target state is missing.")?;
    let lease = state.begin_storage_context_operation("assistant instance file edit set")?;
    reconcile_runtime_state(state).await?;
    let paths = storage.paths.clone();
    let target = instance_id.clone();
    let mut result = super::commands_storage::run_instance_mutation_to_completion(
        state.inner(),
        &lease,
        &instance_id,
        move || async move {
            let current = read_instance_details(&paths, &target)
                .await
                .map_err(|error| error.to_string())?;
            precondition.validate(&current)?;
            ensure_assistant_patch_target_stopped(&current)?;
            app_storage::apply_instance_file_patches(&paths, &target, prepared)
                .await
                .map_err(|error| redact_assistant_provider_text(&error.to_string()))
        },
    )
    .await?;
    result.error = result
        .error
        .map(|error| redact_assistant_provider_text(&error));
    for file in &mut result.files {
        file.error = file
            .error
            .take()
            .map(|error| redact_assistant_provider_text(&error));
    }
    output.instance_id = Some(instance_id.clone());
    output.module_id = pending.plan.module_id.clone();
    output.message = match result.status {
        app_storage::InstanceFilePatchesStatus::Applied => format!(
            "Patched and read back {} files. Original backups are retained. Game behavior and runtime recovery still require verification.",
            result.files.len()
        ),
        app_storage::InstanceFilePatchesStatus::NotApplied => String::from(
            "The file edit set was not applied. Inspect its error and any retained backup receipts before retrying.",
        ),
        app_storage::InstanceFilePatchesStatus::RolledBack => String::from(
            "The file edit set failed. Changes made by this attempt were rolled back and read back; original backups are retained.",
        ),
        app_storage::InstanceFilePatchesStatus::Partial => String::from(
            "The file edit set failed and rollback could not be fully verified. Inspect each file and its retained backup; do not start the server or assume recovery.",
        ),
    };
    append_desktop_app_log(
        storage,
        if result.status == app_storage::InstanceFilePatchesStatus::Applied {
            "info"
        } else {
            "warn"
        },
        "assistant.instance_files.finished",
        "Confirmed instance file edit set finished",
        json!({"instance_id": instance_id, "result": result}),
    );
    // Failure receipts must survive the command boundary. The central operation
    // acceptance gate converts non-applied status into failed task verification.
    output.file_changes_result = Some(result);
    Ok(())
}

#[cfg(test)]
#[path = "instance_file_patches_tests.rs"]
mod instance_file_patches_tests;
