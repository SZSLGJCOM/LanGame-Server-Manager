async fn assess_assistant_task(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    task: &AssistantTaskContract,
    after: Option<&InstanceDetails>,
    output: &AssistantExecuteOperationOutput,
    operation_error: Option<&str>,
    verification: Option<&AssistantOperationVerification>,
) -> AssistantTaskReceipt {
    use AssistantTaskCheckStatus::{Failed, Satisfied, Unknown};
    let mut checks = vec![assistant_task_check(
        "operation_result",
        if operation_error.is_some() {
            Failed
        } else {
            Satisfied
        },
        if operation_error.is_some() {
            "The confirmed operation failed."
        } else {
            "The confirmed operation completed its command and readback checks."
        },
        json!({"action": output.action, "error": operation_error.map(redact_assistant_provider_text)}),
    )];
    if let Some(requirements) = &task.requirements {
        checks.extend(requirements.checks(after, task.requirements_schema.as_deref()));
    } else if task.requires_requirements() {
        checks.push(assistant_task_check(
            "request_requirements_bound",
            Unknown,
            "The request requirements have not been reviewed and bound to this task.",
            Value::Null,
        ));
    }
    if assistant_is_lifecycle_operation(output.action) {
        checks.push(assistant_task_check("lifecycle_result_verified",
            match verification.map(|value| value.status) {
                Some(AssistantVerificationStatus::Verified) => Satisfied,
                Some(AssistantVerificationStatus::Failed) => Failed,
                _ => Unknown,
            },
            "The lifecycle or backup receipt must match the confirmed instance and current readback.",
            verification.map(|value| value.evidence.clone()).unwrap_or(Value::Null)));
    }
    if task.request.goal == AssistantTaskGoal::PrepareService {
        let prepared = after.filter(|instance| {
            task.instance_id.as_deref() == Some(instance.summary.id.as_str())
                && task.module_id.as_deref() == Some(instance.summary.module_id.as_str())
        });
        checks.push(assistant_prepared_files_check(storage, prepared).await);
        let bound = prepared.is_some();
        checks.push(assistant_task_check(
            "prepared_instance_bound",
            if bound { Satisfied } else { Unknown },
            "Preparation requires a readable instance bound to this task.",
            json!({"instanceId": task.instance_id}),
        ));
        checks.push(assistant_task_check(
            "initial_configuration_saved",
            if bound && task.configuration_stage != AssistantConfigurationStage::Unconfigured {
                Satisfied
            } else {
                Unknown
            },
            "The initial configuration must be confirmed and saved before preparation is complete.",
            Value::Null,
        ));
    }
    if let Some(receipt) = &output.task {
        checks.extend(
            receipt
                .checks
                .iter()
                .filter(|check| {
                    matches!(
                        check.name.as_str(),
                        "runtime_command_delivery" | "server_files_ready" | "instance_created"
                    )
                })
                .cloned(),
        );
    }
    if task.request.preserve_existing_mods
        && task.configuration_stage == AssistantConfigurationStage::Protected
        && task.instance_id.is_some()
    {
        let preservation = after
            .ok_or_else(|| String::from("The saved configuration could not be read."))
            .and_then(|details| {
                if task.instance_id.as_deref() != Some(details.summary.id.as_str())
                    || task.module_id.as_deref() != Some(details.summary.module_id.as_str())
                {
                    return Err(String::from("The task target changed."));
                }
                let settings = serde_json::from_str(&details.settings_json)
                    .map_err(|error| error.to_string())?;
                task.validate_settings(&settings)
            });
        checks.push(assistant_task_check("mod_configuration_preserved",
            if preservation.is_ok() { Satisfied } else if after.is_none() { Unknown } else { Failed },
            "Existing mod configuration is checked against the original task baseline.",
            json!({"error": preservation.err().map(|error| redact_assistant_provider_text(&error))})));
    }
    if task.requires_running_service()
        || matches!(
            output.action,
            AssistantOperationAction::StartServer | AssistantOperationAction::RestartServer
        )
    {
        let ready =
            verification.is_some_and(|value| value.status == AssistantVerificationStatus::Verified);
        let failed =
            verification.is_some_and(|value| value.status == AssistantVerificationStatus::Failed);
        checks.push(assistant_task_check(
            "new_run_ready",
            if ready {
                Satisfied
            } else if failed {
                Failed
            } else {
                Unknown
            },
            "A new run must pass the managed process-identity and startup-readiness checks.",
            json!({"runId": verification.and_then(|value| value.run_id)}),
        ));
        if let Some(after) = after {
            // Include the saved intended mod set as well as the protected initial
            // set. A newly enabled dependency also needs runtime evidence.
            let current_settings = serde_json::from_str::<Value>(&after.settings_json);
            if let Ok(settings) = current_settings {
                let mut current = AssistantTaskContract {
                    session: task.session.clone(),
                    run: task.run.clone(),
                    investigation_draft: task.investigation_draft.clone(),
                    id: task.id.clone(),
                    request: task.request.clone(),
                    original_request: task.original_request.clone(),
                    requirements: task.requirements.clone(),
                    requirements_schema: task.requirements_schema.clone(),
                    instance_id: task.instance_id.clone(),
                    module_id: task.module_id.clone(),
                    initial_settings: settings,
                    configuration_stage: AssistantConfigurationStage::Protected,
                    required_mods: Vec::new(),
                    mod_evidence_known: true,
                    file_changes: task.file_changes.clone(),
                };
                let captured = current.capture_mod_requirements();
                if captured.is_err()
                    || !current.mod_evidence_known
                    || (task.request.preserve_existing_mods && !task.mod_evidence_known)
                {
                    checks.push(assistant_task_check("mod_requirements_known", Unknown,
                        "The complete intended mod set cannot be determined safely for this game/configuration.", Value::Null));
                }
                if task.request.preserve_existing_mods {
                    for required in &task.required_mods {
                        if !current.required_mods.iter().any(|item| {
                            item.shard == required.shard && item.folder_name == required.folder_name
                        }) {
                            current.required_mods.push(required.clone());
                        }
                    }
                }
                if !current.required_mods.is_empty() {
                    checks.push(if ready {
                        verify_assistant_required_mods(
                            state,
                            storage,
                            after,
                            &current.required_mods,
                        )
                        .await
                    } else {
                        assistant_task_check(
                            "required_mods_running",
                            Unknown,
                            "Required mods need evidence from a verified new run.",
                            Value::Null,
                        )
                    });
                }
            } else {
                checks.push(assistant_task_check(
                    "mod_requirements_known",
                    Unknown,
                    "The current mod requirements could not be read.",
                    Value::Null,
                ));
            }
        }
    }
    // Native probes may wait for game replies. Check the source after those
    // awaits so updates during a probe cannot leave a stale successful check.
    if !task.file_changes.is_empty() {
        let verified = verify_assistant_task_file_changes(storage, task).await;
        checks.push(assistant_task_check(
            "file_changes_preserved",
            if verified.is_ok() { Satisfied } else { Unknown },
            "Confirmed file content must remain unchanged through task verification.",
            json!({"files": task.file_changes, "error": verified.err()}),
        ));
    }
    if task.instance_id.is_some() || after.is_some() {
        let stable = match after {
            Some(observed)
                if task.instance_id.as_deref() == Some(observed.summary.id.as_str())
                    && task.module_id.as_deref() == Some(observed.summary.module_id.as_str()) =>
            {
                match read_instance_details(&storage.paths, &observed.summary.id).await {
                    Ok(current) => {
                        AssistantOperationPrecondition::from_details(observed).validate(&current)
                    }
                    Err(_) => Err(String::from(
                        "The task target could not be read after verification.",
                    )),
                }
            }
            Some(_) => Err(String::from("The task target changed during verification.")),
            None => Err(String::from("The task target result could not be read.")),
        };
        checks.push(assistant_task_check("task_snapshot_unchanged",
            if stable.is_ok() { Satisfied } else { Unknown },
            "Configuration and run identity must remain unchanged while collecting completion evidence.",
            json!({"error": stable.err().map(|error| redact_assistant_provider_text(&error))})));
    }
    let mut status = assistant_task_status(&checks);
    // A successful intermediate operation can leave final requirements unmet.
    // Keep those individual mismatches visible without claiming the whole task
    // failed while its remaining configuration steps are still possible.
    if status == AssistantTaskStatus::Failed
        && task.requires_requirements()
        && output.action != AssistantOperationAction::StartServer
        && operation_error.is_none()
        && checks
            .iter()
            .filter(|check| check.status == Failed)
            .all(|check| check.name.starts_with("requirement_"))
    {
        status = AssistantTaskStatus::Inconclusive;
    }
    task.receipt(status, checks)
}

fn assistant_task_check(
    name: &str,
    status: AssistantTaskCheckStatus,
    summary: &str,
    evidence: Value,
) -> AssistantTaskCheck {
    AssistantTaskCheck {
        name: name.into(),
        status,
        summary: summary.into(),
        evidence,
    }
}

fn assistant_task_status(checks: &[AssistantTaskCheck]) -> AssistantTaskStatus {
    if checks
        .iter()
        .any(|check| check.status == AssistantTaskCheckStatus::Failed)
    {
        AssistantTaskStatus::Failed
    } else if checks.is_empty()
        || checks
            .iter()
            .any(|check| check.status == AssistantTaskCheckStatus::Unknown)
    {
        AssistantTaskStatus::Inconclusive
    } else {
        AssistantTaskStatus::Completed
    }
}

#[cfg(all(test, windows))]
#[path = "task_file_probe_tests.rs"]
mod task_file_probe_tests;
