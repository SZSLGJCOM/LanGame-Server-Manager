fn assistant_saved_repair_start_plan(
    mode: &AssistantOperationMode,
    current: Option<&InstanceDetails>,
) -> Result<Option<AssistantOperationPlan>, String> {
    let AssistantOperationMode::FollowUp {
        step,
        instance_id,
        module_id,
        verified_precondition,
        verification: packet,
        task,
        ..
    } = mode
    else {
        return Ok(None);
    };
    let goal = task
        .as_ref()
        .map_or(AssistantTaskGoal::RestoreService, |task| task.request.goal);
    if *step == 0 || *step >= assistant_operation_limit(goal) {
        return Err(String::from(
            "The assistant repair has no remaining follow-up confirmation step.",
        ));
    }
    // Launch readiness does not prove the user's initial setup is complete.
    // Let the planner finish that setup before its first confirmed start.
    if task.as_ref().is_some_and(|task| {
        task.request.goal == AssistantTaskGoal::PrepareService
            || (task.request.goal == AssistantTaskGoal::LaunchService
                && task.configuration_stage != AssistantConfigurationStage::Protected)
    }) {
        return Ok(None);
    }
    if packet["instanceId"].as_str() != Some(instance_id.as_str())
        || packet["moduleId"].as_str() != Some(module_id.as_str())
        || packet["step"].as_u64() != u64::try_from(*step).ok()
    {
        return Err(String::from(
            "The repair verification binding changed; request a new preview.",
        ));
    }
    let Some(current) = current else {
        return Ok(None);
    };
    if current.summary.id != *instance_id || current.summary.module_id != *module_id {
        return Err(String::from(
            "The repair target changed; request a new preview.",
        ));
    }
    let verification = &packet["verification"];
    let evidence = &verification["evidence"];
    if !matches!(
        packet["previousAction"].as_str(),
        Some(
            "customize_config"
                | "apply_beginner_config"
                | "repair_ports"
                | "patch_instance_text"
                | "install_server"
                | "validate_server"
        )
    ) || verification["status"].as_str() != Some("inconclusive")
        || verification["canContinue"].as_bool() != Some(true)
        || verification.get("runId") != Some(&Value::Null)
        || evidence.get("operationError") != Some(&Value::Null)
        || evidence.get("observedRunId") != Some(&Value::Null)
        || evidence["launchReady"].as_bool() != Some(true)
        || evidence["instanceStatus"].as_str() != Some("Stopped")
        || ["readError", "readbackError"]
            .iter()
            .any(|key| evidence.get(*key).is_some_and(|value| !value.is_null()))
        || !matches!(current.summary.status, InstanceStatus::Stopped)
        || current.summary.active_process_count != 0
        || current.active_run.is_some()
    {
        return Ok(None);
    }
    let Some(precondition) = verified_precondition else {
        return Ok(None);
    };
    precondition.validate(current)?;
    // Only the private follow-up mode carries this locally produced verification.
    // Old failure logs cannot undo a successful save; a new run still requires
    // the normal single-use confirmation and its current-state precondition.
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::StartServer,
        instance_id: Some(instance_id.clone()),
        module_id: Some(module_id.clone()),
        ..assistant_safe_none_plan(String::from(
            "The saved configuration passed launch preflight. Confirm starting this server to verify runtime recovery.",
        ))
    };
    if task.as_ref().is_some_and(|task| {
        task.requirements.as_ref().is_some_and(|requirements| {
            requirements
                .validate_action(&plan, Some(current), task.requirements_schema.as_deref())
                .is_err()
        })
    }) {
        return Ok(None);
    }
    validate_assistant_repair_follow_up(&plan, instance_id, module_id, *step, goal)?;
    Ok(Some(plan))
}

fn validate_assistant_repair_follow_up(
    plan: &AssistantOperationPlan,
    instance_id: &str,
    module_id: &str,
    step: usize,
    goal: AssistantTaskGoal,
) -> Result<(), String> {
    if goal == AssistantTaskGoal::PrepareService
        && plan.action == AssistantOperationAction::StartServer
    {
        return Err(String::from(
            "A preparation-only task cannot propose starting a server.",
        ));
    }
    if step >= assistant_operation_limit(goal) {
        return Err(String::from(
            "The assistant task reached its operation limit.",
        ));
    }
    if plan
        .instance_id
        .as_deref()
        .is_some_and(|id| id != instance_id)
        || plan.module_id.as_deref().is_some_and(|id| id != module_id)
    {
        return Err(String::from(
            "A repair follow-up cannot change the selected server or game.",
        ));
    }
    if !matches!(
        plan.action,
        AssistantOperationAction::None
            | AssistantOperationAction::StartServer
            | AssistantOperationAction::ApplyBeginnerConfig
            | AssistantOperationAction::CustomizeConfig
            | AssistantOperationAction::PatchInstanceText
            | AssistantOperationAction::PatchInstanceFiles
            | AssistantOperationAction::RepairPorts
            | AssistantOperationAction::ValidateServer
    ) && !(matches!(
        goal,
        AssistantTaskGoal::PrepareService | AssistantTaskGoal::LaunchService
    ) && plan.action == AssistantOperationAction::InstallServer)
    {
        return Err(String::from(
            "A repair follow-up can only inspect, change settings, private Mod text or ports, validate files, or propose starting this server.",
        ));
    }
    Ok(())
}

fn assistant_operation_can_verify(action: AssistantOperationAction) -> bool {
    matches!(
        action,
        AssistantOperationAction::StartServer
            | AssistantOperationAction::StopServer
            | AssistantOperationAction::RestartServer
            | AssistantOperationAction::CreateBackup
            | AssistantOperationAction::RestoreBackup
            | AssistantOperationAction::ApplyBeginnerConfig
            | AssistantOperationAction::CustomizeConfig
            | AssistantOperationAction::PatchInstanceText
            | AssistantOperationAction::PatchInstanceFiles
            | AssistantOperationAction::RepairPorts
            | AssistantOperationAction::CreateServer
            | AssistantOperationAction::InstallServer
            | AssistantOperationAction::ValidateServer
    )
}

fn repair_evidence_text_value(value: &Value, json_bytes: usize, keep_tail: bool) -> Value {
    let Some(text) = value.as_str() else {
        return Value::Null;
    };
    let text = redact_assistant_provider_text(text);
    let original = json!(text);
    if original.to_string().len() <= json_bytes {
        return original;
    }
    let mut bytes = text.len().min(json_bytes);
    loop {
        let shortened = if keep_tail {
            truncate_assistant_log_tail(&text, bytes)
        } else {
            truncate_assistant_prompt_text(&text, bytes)
        };
        let value = json!(shortened);
        if value.to_string().len() <= json_bytes {
            return value;
        }
        bytes /= 2;
    }
}

fn assistant_repair_evidence_text(packet: &Value) -> Result<String, String> {
    let text = redact_assistant_provider_text(&packet.to_string());
    if text.len() <= ASSISTANT_TOOL_RESULT_BYTES {
        return Ok(text);
    }
    let verification = &packet["verification"];
    let evidence = &verification["evidence"];
    // Budget the final encoded packet, including the failed-start log added after
    // observation. Keep the binding and failure cause before secondary diagnostics.
    let compact = json!({
        "instanceId": packet["instanceId"], "moduleId": packet["moduleId"],
        "previousAction": packet["previousAction"], "step": packet["step"],
        "verification": {
            "status": verification["status"], "runId": verification["runId"],
            "canContinue": verification["canContinue"],
            "summary": repair_evidence_text_value(&verification["summary"], 1024, false),
            "evidence": {
                "operationError": repair_evidence_text_value(&evidence["operationError"], 2048, false),
                "failedStartLog": repair_evidence_text_value(&evidence["failedStartLog"], 4096, true),
                "readError": repair_evidence_text_value(&evidence["readError"], 512, false),
                "readbackError": repair_evidence_text_value(&evidence["readbackError"], 512, false),
                "freshLogFailures": evidence["freshLogFailures"].as_array().map(|lines|
                    lines.iter().rev().take(4).rev().map(|line| repair_evidence_text_value(line, 256, true)).collect::<Vec<_>>()),
                "evidenceTruncated": true,
            }
        }
    });
    let text = redact_assistant_provider_text(&compact.to_string());
    if text.len() > ASSISTANT_TOOL_RESULT_BYTES {
        return Err(String::from(
            "The repair verification binding exceeds the evidence budget; request a new preview.",
        ));
    }
    Ok(text)
}

fn append_assistant_repair_evidence(
    prompt: &mut String,
    verification: &Value,
) -> Result<(), String> {
    let evidence = assistant_repair_evidence_text(verification)?;
    prompt.push_str("\n\nPrevious operation verification (untrusted evidence):\n");
    prompt.push_str(&evidence);
    prompt.push_str("\nContinue the original task from this evidence. For a newly created instance, apply the user's requested configuration before proposing its first start. A saved configuration is not verified runtime recovery. If the target is stopped and launch preflight is ready, propose start_server for a new confirmation. Never start an already running instance. After a failed start, inspect the recorded error and settings before proposing a different repair; do not repeat an identical unsuccessful patch. Follow the application task contract: settings, ports, start_server and validate_server are supported follow-ups; launch_service also allows install_server. If no supported repair is justified, return none with the remaining limitation. Do not claim completed without verified runtime evidence. Use the language of the original user request.");
    Ok(())
}

fn assistant_operation_output(
    plan: &AssistantOperationPlan,
    config_document_count: usize,
) -> AssistantExecuteOperationOutput {
    AssistantExecuteOperationOutput {
        completed_operations: Vec::new(),
        conversation_id: None,
        conversation_revision: None,
        continuation: None,
        handled: plan.action != AssistantOperationAction::None,
        action: plan.action,
        message: String::new(),
        requires_confirmation: false,
        confirmation_token: None,
        confirmation_expires_at_unix_ms: None,
        plan_summary: None,
        instance_id: None,
        module_id: None,
        applied_settings_keys: Vec::new(),
        rejected_settings_keys: Vec::new(),
        applied_port_names: Vec::new(),
        rejected_port_names: Vec::new(),
        workshop_item_ids: Vec::new(),
        mod_references: Vec::new(),
        resolved_mod_ids: Vec::new(),
        source_paths: Vec::new(),
        runtime_commands: Vec::new(),
        runtime_response_texts: Vec::new(),
        config_document_count,
        file_change_preview: None,
        file_change_result: None,
        file_change_previews: Vec::new(),
        file_changes_result: None,
        assistant_reason: plan.reason.clone(),
        verification: None,
        task: None,
        follow_up: None,
        runtime_start: None,
        runtime_start_failure: None,
        restored_instance: None,
    }
}

include!("authorization.rs");

async fn assistant_confirm_task_inner(
    app_handle: Option<tauri::AppHandle>,
    state: tauri::State<'_, DesktopState>,
    input: AssistantConfirmOperationInput,
    mut pending: AssistantPendingOperation,
) -> Result<AssistantExecuteOperationOutput, String> {
    let _storage_operation =
        state.begin_storage_context_operation("assistant repair confirmation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let mut before = match pending.expected_instance.as_ref() {
        Some(expected) => {
            let current = read_instance_details(&storage.paths, &expected.summary.id)
                .await
                .map_err(|error| error.to_string())?;
            if let Some(precondition) = &pending.precondition {
                precondition.validate(&current)?;
            }
            Some(current)
        }
        None => None,
    };
    if pending.plan.action == AssistantOperationAction::StartServer
        && pending.task.configuration_stage == AssistantConfigurationStage::Configuring
    {
        let details = before
            .as_ref()
            .ok_or("The created server baseline is unavailable.")?;
        pending.task = std::sync::Arc::new(pending.task.protect_created_configuration(details)?);
    }
    let step = pending.repair_step;
    let lifecycle_locale = pending.lifecycle_locale;
    let mut task = pending.task.clone();
    let original_prompt = pending.original_prompt.clone();
    verify_assistant_task_file_changes(&storage, &task).await?;
    let plan = pending.plan.clone();
    let document_count = pending.config_document_count;
    let operation_input = AssistantExecuteOperationInput {
        task: task.request.clone(),
        settings: input.settings.clone(),
        prompt: pending.prompt.clone(),
        context: None,
        selected_instance_id: pending.selected_instance_id.clone(),
        selected_module_id: pending.selected_module_id.clone(),
    };
    // Keep the large operation state machine out of this orchestration future.
    // Inline nesting can exhaust the Windows stack when confirmation is polled.
    let result = Box::pin(assistant_operation_inner(
        app_handle.clone(),
        state.clone(),
        operation_input,
        AssistantOperationMode::Confirmed(Box::new(pending)),
    ))
    .await;
    let (mut output, operation_error) = match result {
        Ok(output) => {
            if output.continuation.is_some() {
                return Ok(output);
            }
            let error = output
                .runtime_start_failure
                .as_ref()
                .map(|failure| failure.message.clone())
                .or_else(|| output.file_changes_result.as_ref()
                    .filter(|result| result.status != app_storage::InstanceFilePatchesStatus::Applied)
                    .map(|result| result.error.clone().unwrap_or_else(|| "The file change set did not complete; inspect its recovery receipts.".into())));
            (output, error)
        }
        Err(error) if assistant_operation_can_verify(plan.action) => {
            let mut output = assistant_operation_output(&plan, document_count);
            output.instance_id = plan.instance_id.clone();
            output.module_id = plan.module_id.clone();
            output.message = error.clone();
            (output, Some(error))
        }
        Err(error) => return Err(error),
    };
    if assistant_is_lifecycle_operation(plan.action) {
        // Readback and runtime observation stay outside the common confirmation
        // frame, including when this branch is not selected by a repair task.
        Box::pin(complete_assistant_lifecycle_operation(
            &state,
            &storage,
            (&task, lifecycle_locale),
            before.as_ref(),
            &mut output,
            operation_error.as_deref(),
        ))
        .await;
        return Ok(output);
    }
    if plan.action == AssistantOperationAction::CreateServer && operation_error.is_none() {
        match assistant_created_instance_baseline(&storage, &task, &output).await {
            Ok((bound, created)) => {
                task = bound;
                before = Some(created);
            }
            Err(error) => {
                assistant_unbound_operation_result(&task, &mut output, Some(&error));
                return Ok(output);
            }
        }
    }
    if let Some(change) = &output.file_change_result {
        let tracked = &mut std::sync::Arc::make_mut(&mut task).file_changes;
        tracked.retain(|existing| existing.file != change.file);
        tracked.push(change.clone());
    }
    if let Some(changes) = &output.file_changes_result {
        let tracked = &mut std::sync::Arc::make_mut(&mut task).file_changes;
        for change in &changes.files {
            if change.state == app_storage::InstanceFilePatchState::Applied
                && change.read_back_verified
                && let Some(backup_id) = &change.backup_id
            {
                tracked.retain(|existing| existing.file != change.file);
                tracked.push(app_storage::InstanceFilePatchResult {
                    file: change.file.clone(),
                    source_sha256: change.source_sha256.clone(),
                    result_sha256: change.result_sha256.clone(),
                    backup_id: backup_id.clone(),
                    read_back_verified: true,
                });
            }
        }
    }
    if let Err(error) = assistant_checkpoint_task(&task).await {
        assistant_unrecorded_operation(&mut output, &error);
        return Ok(output);
    }
    if matches!(
        plan.action,
        AssistantOperationAction::PatchInstanceText | AssistantOperationAction::PatchInstanceFiles
    ) && task.request.goal == AssistantTaskGoal::ApplyChange
    {
        let after = match task.instance_id.as_deref() {
            Some(id) => read_instance_details(&storage.paths, id).await.ok(),
            None => None,
        };
        let receipt = assess_assistant_task(
            &state,
            &storage,
            &task,
            after.as_ref(),
            &output,
            operation_error.as_deref(),
            None,
        )
        .await;
        output.verification = Some(AssistantOperationVerification {
            status: match receipt.status {
                AssistantTaskStatus::Completed => AssistantVerificationStatus::Verified,
                AssistantTaskStatus::Failed => AssistantVerificationStatus::Failed,
                _ => AssistantVerificationStatus::Inconclusive,
            },
            summary: String::from(
                "This result verifies only the requested file change and its readback. Server startup and Mod behavior were not tested.",
            ),
            run_id: None,
            evidence: json!({"fileChange":output.file_change_result, "fileChanges":output.file_changes_result, "operationError":operation_error}),
            can_continue: false,
        });
        output.task = Some(receipt);
        return Ok(output);
    }
    if before.is_none()
        && matches!(
            plan.action,
            AssistantOperationAction::InstallServer | AssistantOperationAction::ValidateServer
        )
    {
        let mut verification =
            assistant_unbound_install_verification(&output, operation_error.as_deref());
        let preparation = task.prepares_service();
        let mut continue_preparation = preparation && verification.can_continue;
        output.task = Some(
            assess_assistant_task(
                &state,
                &storage,
                &task,
                None,
                &output,
                operation_error.as_deref(),
                task.requires_running_service().then_some(&verification),
            )
            .await,
        );
        if let Some(session) = &task.session {
            let recorded = assistant_record_session_event(
                session,
                "operation_verification",
                json!({
                    "action":plan.action,"moduleId":task.module_id,"verification":verification,
                    "task":output.task,"operationError":operation_error,
                }),
            );
            if session.check_active().is_err() || recorded.is_err() {
                continue_preparation = false;
                verification.can_continue = false;
                verification.summary.push_str("\nFurther assistant work was stopped. The completed file preparation is retained.");
            }
        }
        if continue_preparation && step + 1 < task.operation_limit() {
            match assistant_create_after_install_preview(
                &input.settings,
                &original_prompt,
                task,
                step + 1,
            ) {
                Ok(next) => output.follow_up = Some(Box::new(next)),
                Err(error) => {
                    verification.can_continue = false;
                    verification.summary.push_str(&format!("\nNext preview could not be prepared: {}. Completed file preparation has not been undone.", redact_assistant_provider_text(&error)));
                }
            }
        } else {
            verification.can_continue = false;
        }
        output.verification = preparation.then_some(verification);
        return Ok(output);
    }
    if !assistant_operation_can_verify(plan.action) {
        let after = match task.instance_id.as_deref() {
            Some(id) => read_instance_details(&storage.paths, id).await.ok(),
            None => None,
        };
        output.task = Some(
            assess_assistant_task(
                &state,
                &storage,
                &task,
                after.as_ref(),
                &output,
                operation_error.as_deref(),
                None,
            )
            .await,
        );
        return Ok(output);
    }
    let Some(before) = before else {
        assistant_unbound_operation_result(&task, &mut output, operation_error.as_deref());
        return Ok(output);
    };
    let after = read_instance_details(&storage.paths, &before.summary.id).await;
    if operation_error.is_none()
        && task.configuration_stage != AssistantConfigurationStage::Protected
        && matches!(
            plan.action,
            AssistantOperationAction::CustomizeConfig
                | AssistantOperationAction::ApplyBeginnerConfig
        )
        && let Ok(after) = &after
    {
        task = std::sync::Arc::new(task.record_initial_configuration(after)?);
        if let Err(error) = assistant_checkpoint_task(&task).await {
            assistant_unrecorded_operation(&mut output, &error);
            return Ok(output);
        }
    }
    let preparing = task.request.goal == AssistantTaskGoal::PrepareService;
    let mut verification = if preparing {
        AssistantOperationVerification {
            status: AssistantVerificationStatus::Inconclusive,
            summary: String::from(
                "The saved preparation requirements are being checked; no start operation was performed.",
            ),
            run_id: None,
            evidence: json!({"operationError":operation_error.as_deref(),"scope":"saved_configuration"}),
            can_continue: false,
        }
    } else {
        observe_assistant_operation(
            &state,
            &storage,
            plan.action,
            &before,
            after.as_ref().ok(),
            output.runtime_start.as_ref(),
            operation_error.as_deref(),
        )
        .await
    };
    if let Err(error) = &after {
        verification.evidence["readbackError"] =
            json!(redact_assistant_provider_text(&error.to_string()));
    }
    if let Some(log) = output
        .runtime_start_failure
        .as_ref()
        .and_then(|failure| failure.log.as_ref())
    {
        verification.evidence["failedStartLog"] = json!(truncate_assistant_log_tail(
            &redact_assistant_provider_text(&format_assistant_runtime_log_snapshot(log)),
            4096
        ));
    }

    let assessment = assess_assistant_task(
        &state,
        &storage,
        &task,
        after.as_ref().ok(),
        &output,
        operation_error.as_deref(),
        Some(&verification),
    )
    .await;
    if preparing {
        verification.status = match assessment.status {
            AssistantTaskStatus::Completed => AssistantVerificationStatus::Verified,
            AssistantTaskStatus::Failed => AssistantVerificationStatus::Failed,
            _ => AssistantVerificationStatus::Inconclusive,
        };
        verification.summary = match assessment.status {
            AssistantTaskStatus::Completed => "The requested server preparation and saved configuration were verified. No start operation was performed; runtime behavior was not tested.",
            AssistantTaskStatus::Failed => "Server preparation failed its saved-state checks. No start operation was performed.",
            _ => "Server preparation is incomplete. Remaining configuration requires confirmation; no start operation was performed.",
        }.into();
        verification.can_continue = operation_error.is_none()
            && assessment.status == AssistantTaskStatus::Inconclusive
            && assessment.checks.iter().any(|check| {
                check.name == "task_snapshot_unchanged"
                    && check.status == AssistantTaskCheckStatus::Satisfied
            })
            && assessment.checks.iter().any(|check| {
                check.name == "prepared_files_ready"
                    && check.status == AssistantTaskCheckStatus::Satisfied
            });
    }
    if verification.status == AssistantVerificationStatus::Verified
        && assessment.status != AssistantTaskStatus::Completed
    {
        verification.status = if assessment.status == AssistantTaskStatus::Failed {
            AssistantVerificationStatus::Failed
        } else {
            AssistantVerificationStatus::Inconclusive
        };
        verification
            .summary
            .push_str(" Startup readiness alone did not satisfy the task's remaining checks.");
        // A live run needs an explicit restart/stop workflow; never silently
        // mutate it repeatedly to satisfy an unproven completion condition.
        verification.can_continue = false;
    }
    output.task = Some(assessment);
    let needs_more_work = task.requires_running_service() || preparing;
    if let Some(session) = &task.session {
        let recorded = assistant_record_session_event(
            session,
            "operation_verification",
            json!({
                "action":plan.action,"instanceId":before.summary.id,"moduleId":before.summary.module_id,
                "verification":verification,"task":output.task,"operationError":operation_error,
            }),
        );
        if session.check_active().is_err() || recorded.is_err() {
            verification.can_continue = false;
            verification.summary.push_str("\nFurther assistant work was stopped. The completed operation and its verification are reported above.");
        }
    }
    if !assistant_file_set_allows_follow_up(output.file_changes_result.as_ref()) {
        verification.can_continue = false;
        verification.summary.push_str("\nThe file change set needs review of its rollback/recovery receipts before any further operation.");
    }
    if needs_more_work
        && verification.can_continue
        && verification.status != AssistantVerificationStatus::Verified
    {
        if step + 1 < task.operation_limit() {
            let follow_up_input = AssistantExecuteOperationInput {
                task: task.request.clone(),
                settings: input.settings,
                prompt: original_prompt.clone(),
                context: None,
                selected_instance_id: Some(before.summary.id.clone()),
                selected_module_id: Some(before.summary.module_id.clone()),
            };
            let follow_up_mode = AssistantOperationMode::FollowUp {
                step: step + 1,
                instance_id: before.summary.id.clone(),
                module_id: before.summary.module_id.clone(),
                original_prompt,
                verified_precondition: after
                    .as_ref()
                    .ok()
                    .map(AssistantOperationPrecondition::from_details)
                    .map(Box::new),
                task: Some(task.clone()),
                verification: json!({"instanceId": before.summary.id, "moduleId": before.summary.module_id,
                        "previousAction": plan.action, "step": step + 1, "verification": verification}),
            };
            let retry_mode = assistant_continuation_mode(&follow_up_mode, &task)?;
            let follow_up = Box::pin(assistant_operation_inner(
                app_handle,
                state.clone(),
                follow_up_input.clone(),
                follow_up_mode,
            ))
            .await;
            match follow_up {
                Ok(next) if next.requires_confirmation => output.follow_up = Some(Box::new(next)),
                Ok(next) if next.continuation.is_some() => {
                    output.continuation = next.continuation;
                    verification
                        .summary
                        .push_str(&format!("\n{}", next.message));
                    verification.can_continue = false;
                }
                Ok(diagnosis) => {
                    verification.summary.push_str("\n\n");
                    verification.summary.push_str(&diagnosis.message);
                    verification.can_continue = false;
                }
                Err(error) => {
                    let error = redact_assistant_provider_text(&error);
                    verification.summary.push_str(&format!("\n\nFurther investigation could not finish: {error}. The completed operation has not been undone."));
                    verification.evidence["investigationError"] = json!(error);
                    verification.can_continue = false;
                    if task.session.is_some() {
                        match assistant_retain_failed_continuation(
                            &follow_up_input,
                            &retry_mode,
                            &task,
                            &error,
                        ) {
                            Ok(paused) => {
                                // Preserve the completed write's result and
                                // verification; only the next investigation pauses.
                                output.continuation = paused.continuation;
                                if let Some(pause) = &output.continuation {
                                    verification
                                        .summary
                                        .push_str(&format!("\n{}", pause.summary));
                                }
                            }
                            Err(checkpoint_error) => {
                                let checkpoint_error =
                                    redact_assistant_provider_text(&checkpoint_error);
                                verification.evidence["continuationError"] =
                                    json!(checkpoint_error);
                                verification.summary.push_str(&format!("\nThe investigation checkpoint could not be retained: {checkpoint_error}"));
                            }
                        }
                    }
                }
            }
        } else {
            verification.summary.push_str(
                "\nThe task operation limit was reached. No further operation was executed.",
            );
            verification.can_continue = false;
        }
    } else {
        verification.can_continue = false;
    }
    output.verification = Some(verification);
    Ok(output)
}

#[cfg(test)]
#[path = "repair_tests.rs"]
mod repair_tests;

#[cfg(test)]
#[path = "repair_continuation_tests.rs"]
mod repair_continuation_tests;
