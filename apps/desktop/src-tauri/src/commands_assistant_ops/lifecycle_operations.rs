fn assistant_is_lifecycle_operation(action: AssistantOperationAction) -> bool {
    matches!(
        action,
        AssistantOperationAction::StopServer
            | AssistantOperationAction::RestartServer
            | AssistantOperationAction::CreateBackup
            | AssistantOperationAction::RestoreBackup
    )
}

fn validate_assistant_lifecycle_plan(
    plan: &AssistantOperationPlan,
    goal: AssistantTaskGoal,
    instance: Option<&InstanceDetails>,
) -> Result<(), String> {
    if plan.action != AssistantOperationAction::RestoreBackup && plan.backup_id.is_some() {
        return Err("backupId is only valid for restore_backup.".into());
    }
    if !assistant_is_lifecycle_operation(plan.action) {
        return Ok(());
    }
    if goal != AssistantTaskGoal::ApplyChange {
        return Err("Stopping, restarting and backups require an explicit change request and their own confirmation; they are not automatic repair steps.".into());
    }
    if plan.settings_patch.is_some()
        || plan.port_patch.is_some()
        || plan.text_patch.is_some()
        || !plan.file_patches.is_empty()
        || !plan.runtime_commands.is_empty()
        || !plan.workshop_item_ids.is_empty()
        || !plan.mod_references.is_empty()
        || !plan.source_paths.is_empty()
        || plan.broadcast_intent.is_some()
        || plan.process_key.is_some()
        || plan.transport.is_some()
        || plan.port_name.is_some()
        || plan.password_setting_key.is_some()
        || plan.enabled_setting_key.is_some()
    {
        return Err(
            "A lifecycle or backup operation cannot include unrelated changes or commands.".into(),
        );
    }
    let instance =
        instance.ok_or("Select an existing instance for lifecycle and backup operations.")?;
    match plan.action {
        AssistantOperationAction::StopServer | AssistantOperationAction::RestartServer => {
            if instance.active_run.is_none()
                || instance.summary.active_process_count == 0
                || !matches!(instance.summary.status, InstanceStatus::Running)
            {
                return Err("Stop and restart require the selected instance's current managed running session.".into());
            }
        }
        AssistantOperationAction::CreateBackup | AssistantOperationAction::RestoreBackup => {
            ensure_assistant_lifecycle_stopped(instance)?;
            if plan.action == AssistantOperationAction::RestoreBackup {
                let id = plan.backup_id.as_deref().ok_or(
                    "Choose an exact backupId from list_backups before preparing a restore.",
                )?;
                if id.is_empty()
                    || id.len() > 256
                    || !id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    return Err("backupId must be a plain ID from the selected instance's backup list, never a path.".into());
                }
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn ensure_assistant_lifecycle_stopped(instance: &InstanceDetails) -> Result<(), String> {
    if instance.active_run.is_some()
        || instance.summary.active_process_count != 0
        || !matches!(instance.summary.status, InstanceStatus::Stopped)
    {
        return Err("This backup operation requires a stopped instance. Request and confirm stopping it separately.".into());
    }
    Ok(())
}

fn validate_assistant_lifecycle_saved_state(
    before: &InstanceDetails,
    after: &InstanceDetails,
) -> Result<(), String> {
    let mut expected = before.clone();
    expected.summary.status = after.summary.status.clone();
    expected.summary.active_process_count = after.summary.active_process_count;
    expected.active_run = after.active_run.clone();
    if serde_json::to_value(&expected).map_err(|error| error.to_string())?
        != serde_json::to_value(after).map_err(|error| error.to_string())?
    {
        return Err("The server configuration or backup policy changed during this operation; request a new preview.".into());
    }
    Ok(())
}

fn validate_assistant_lifecycle_readback(
    before: &InstanceDetails,
    after: &InstanceDetails,
    output: &AssistantExecuteOperationOutput,
) -> Result<(), String> {
    let expected = if output.action == AssistantOperationAction::RestoreBackup {
        output.restored_instance.as_deref().ok_or(
            "The verified restored server state is missing; inspect the restore before continuing.",
        )?
    } else {
        before
    };
    validate_assistant_lifecycle_saved_state(expected, after)
}

async fn execute_assistant_lifecycle_operation(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    mode: &AssistantOperationMode,
    output: &mut AssistantExecuteOperationOutput,
) -> Result<(), String> {
    let AssistantOperationMode::Confirmed(pending) = mode else {
        return Err(
            "Lifecycle and backup operations require their own single-use preview confirmation."
                .into(),
        );
    };
    let expected = pending
        .expected_instance
        .clone()
        .ok_or("The confirmed instance baseline is missing.")?;
    let target = expected.summary.id.clone();
    validate_assistant_lifecycle_plan(&pending.plan, pending.task.request.goal, Some(&expected))?;
    output.instance_id = Some(target.clone());
    output.module_id = Some(expected.summary.module_id.clone());
    if matches!(
        pending.plan.action,
        AssistantOperationAction::StopServer | AssistantOperationAction::RestartServer
    ) {
        // Match the native start boundary below: both native lifecycle futures
        // include reconciliation and must not enlarge the assistant frame.
        let stopped = Box::pin(
            super::commands_runtime_lifecycle::stop_instance_process_with_precondition(
                state.clone(),
                target.clone(),
                Some(expected.clone()),
            ),
        )
        .await?;
        let current = read_instance_details(&storage.paths, &target)
            .await
            .map_err(|error| error.to_string())?;
        ensure_assistant_lifecycle_stopped(&current)?;
        validate_assistant_lifecycle_saved_state(&expected, &current)?;
        if expected.active_run.as_ref().map(|run| run.run_id) != Some(stopped.run_id)
            || stopped.summary.id != target
            || stopped.summary.module_id != expected.summary.module_id
        {
            return Err(
                "The stop receipt does not match the confirmed run; no restart was attempted."
                    .into(),
            );
        }
        let stopped_evidence = json!({"instanceId":target,"stoppedRunId":stopped.run_id,
            "stoppedSessionId":stopped.session_id,"stoppedProcessCount":stopped.process_count,
            "observedActiveProcesses":current.summary.active_process_count});
        output.message = pending.lifecycle_locale.stopped(&expected.summary.name);
        output.verification = Some(AssistantOperationVerification {
            status: AssistantVerificationStatus::Verified,
            summary: output.message.clone(),
            run_id: Some(stopped.run_id),
            evidence: stopped_evidence,
            can_continue: false,
        });
        if pending.plan.action == AssistantOperationAction::StopServer {
            return Ok(());
        }
        if let Some(session) = &pending.task.session
            && session.check_active().is_err()
        {
            output.message = pending.lifecycle_locale.restart_cancelled();
            if let Some(verification) = &mut output.verification {
                verification.status = AssistantVerificationStatus::Failed;
                verification.summary = output.message.clone();
            }
            return Ok(());
        }
        // Stop and start are separately locked. The exact post-stop snapshot is
        // checked by native start under its lock, so intervening changes reject it.
        let started = Box::pin(
            super::commands_runtime_lifecycle::start_instance_process_with_evidence(
                app_handle,
                state,
                storage,
                target,
                "manual",
                super::commands_runtime_lifecycle::RuntimeStartPreconditions {
                    world_start: None,
                    instance: Some(current),
                    file_changes: pending.task.file_changes.clone(),
                },
            ),
        )
        .await;
        match started {
            Ok(started) => {
                output.message = pending.lifecycle_locale.restarting(&expected.summary.name);
                output.runtime_start = Some(started);
            }
            Err(failure) => {
                output.message = pending.lifecycle_locale.restart_failed(&failure.message);
                if let Some(verification) = &mut output.verification {
                    verification.status = AssistantVerificationStatus::Failed;
                    verification.summary = output.message.clone();
                    verification.evidence["restartError"] =
                        json!(redact_assistant_provider_text(&failure.message));
                }
                output.runtime_start_failure = Some(failure);
            }
        }
        return Ok(());
    }
    execute_assistant_backup_operation(state, storage, pending, output).await
}

async fn complete_assistant_lifecycle_operation(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    task_context: (&AssistantTaskContract, AssistantLifecycleLocale),
    before: Option<&InstanceDetails>,
    output: &mut AssistantExecuteOperationOutput,
    operation_error: Option<&str>,
) {
    let (task, locale) = task_context;
    let after_read = match task.instance_id.as_deref() {
        Some(id) => read_instance_details(&storage.paths, id)
            .await
            .map_err(|error| error.to_string()),
        None => Err("The lifecycle instance target is missing.".into()),
    };
    let after = after_read.as_ref().ok();
    let mut verification = if output.action == AssistantOperationAction::RestartServer
        && output.runtime_start.is_some()
    {
        match before {
            Some(before) => {
                observe_assistant_operation(
                    state,
                    storage,
                    AssistantOperationAction::StartServer,
                    before,
                    after,
                    output.runtime_start.as_ref(),
                    operation_error,
                )
                .await
            }
            None => assistant_verification_read_error(
                "The original run is unavailable.",
                operation_error,
                false,
            ),
        }
    } else {
        output.verification.clone().unwrap_or_else(|| {
            assistant_verification_read_error(
                "No verified lifecycle receipt is available.",
                operation_error,
                false,
            )
        })
    };
    if let Some(error) = operation_error {
        verification.status = AssistantVerificationStatus::Failed;
        verification.summary = redact_assistant_provider_text(error);
        verification.evidence["operationError"] = json!(redact_assistant_provider_text(error));
    }
    if operation_error.is_none() {
        let readback =
            before
                .zip(after)
                .ok_or_else(|| {
                    after_read.as_ref().err().cloned().unwrap_or_else(|| {
                        String::from("The original lifecycle target is missing.")
                    })
                })
                .and_then(|(before, after)| {
                    validate_assistant_lifecycle_readback(before, after, output)?;
                    if output.action != AssistantOperationAction::RestartServer {
                        ensure_assistant_lifecycle_stopped(after)?;
                    }
                    Ok(())
                });
        if let Err(error) = readback {
            verification.status = AssistantVerificationStatus::Inconclusive;
            verification.evidence["readbackError"] = json!(&error);
            verification.summary = error;
        }
    }
    if output.action == AssistantOperationAction::RestartServer {
        verification.evidence["stopReceipt"] = output
            .verification
            .as_ref()
            .map(|value| value.evidence.clone())
            .unwrap_or(Value::Null);
    }
    verification.can_continue = false;
    let receipt = assess_assistant_task(
        state,
        storage,
        task,
        after,
        output,
        operation_error,
        Some(&verification),
    )
    .await;
    if receipt.status != AssistantTaskStatus::Completed
        && verification.status == AssistantVerificationStatus::Verified
    {
        verification.status = AssistantVerificationStatus::Inconclusive;
        verification
            .summary
            .push_str(" Current state could not satisfy every task check.");
    }
    let name = before
        .map(|instance| instance.summary.name.as_str())
        .or_else(|| after.map(|instance| instance.summary.name.as_str()))
        .unwrap_or_else(|| {
            if locale == AssistantLifecycleLocale::ZhCn {
                "所选服务器"
            } else {
                "the selected server"
            }
        });
    output.message = if verification.status == AssistantVerificationStatus::Verified {
        locale.verified(output.action, name, &verification.evidence)
    } else {
        // Preserve the diagnostic cause, including partial effects, while keeping
        // the result's leading copy in the language chosen at preview time.
        let detail = if output.action == AssistantOperationAction::RestartServer
            && output.runtime_start_failure.is_some()
        {
            locale.restart_failed(operation_error.unwrap_or(&verification.summary))
        } else {
            verification.summary.clone()
        };
        locale.unverified(
            name,
            verification.status == AssistantVerificationStatus::Failed,
            &detail,
        )
    };
    verification.summary = output.message.clone();
    output.task = Some(receipt);
    output.verification = Some(verification);
    output.follow_up = None;
    output.continuation = None;
}

#[cfg(test)]
#[path = "lifecycle_operations_tests.rs"]
mod lifecycle_operations_tests;
#[cfg(test)]
#[path = "lifecycle_requirements_tests.rs"]
mod lifecycle_requirements_tests;
