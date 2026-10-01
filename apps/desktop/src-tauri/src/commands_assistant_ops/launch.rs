fn assistant_install_result_check(
    module_id: &str,
    result: &app_steamcmd::ModuleInstallResult,
) -> AssistantTaskCheck {
    let ready = result.module_id == module_id
        && result.install_state == InstallState::Installed
        && result.executable_exists;
    assistant_task_check(
        "server_files_ready",
        if ready {
            AssistantTaskCheckStatus::Satisfied
        } else {
            AssistantTaskCheckStatus::Failed
        },
        "The selected game's installation workflow must report installed files and an available executable.",
        json!({"moduleId": result.module_id, "installState": result.install_state, "executableExists": result.executable_exists, "operation": result.operation}),
    )
}

static ASSISTANT_PREPARATION_PROBE_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(2);

async fn assistant_prepared_files_check(
    storage: &StorageBootstrap,
    instance: Option<&InstanceDetails>,
) -> AssistantTaskCheck {
    let checked = async {
        let permit = ASSISTANT_PREPARATION_PROBE_SLOTS.try_acquire().map_err(|_| {
            String::from("Server installation checks are still running. No preparation completion was established.")
        })?;
        let instance = instance
            .ok_or("The prepared server has no readable bound instance.")?
            .clone();
        let modules_root = storage.paths.modules_root.clone();
        let settings = storage.settings.clone();
        tokio::task::spawn_blocking(move || {
            // A timeout drops the waiter, not a blocked filesystem call. Keep
            // its slot until that worker actually exits so retries stay bounded.
            let _permit = permit;
            let module_id = &instance.summary.module_id;
            let root = super::commands_runtime_lifecycle::private_runtime_install_root(&instance)?;
            let descriptors = discover_modules(&modules_root).map_err(|error| error.to_string())?;
            let descriptor = find_descriptor(&descriptors, module_id)?;
            let probe = probe_module_install_state_with_override(
                &settings,
                module_id,
                descriptor.summary.steam_app_id,
                descriptor.install.as_ref(),
                descriptor.process.as_ref(),
                Some(&root),
            );
            Ok::<_, String>(
                probe.install_state == InstallState::Installed && probe.executable_exists,
            )
        })
        .await
        .map_err(|error| format!("Installation evidence worker failed: {error}"))?
    };
    let result = tokio::time::timeout(Duration::from_secs(10), checked)
        .await
        .unwrap_or_else(|_| Err(String::from("Installation evidence check timed out.")));
    assistant_task_check(
        "prepared_files_ready",
        match &result {
            Ok(true) => AssistantTaskCheckStatus::Satisfied,
            Ok(false) => AssistantTaskCheckStatus::Failed,
            Err(_) => AssistantTaskCheckStatus::Unknown,
        },
        "The prepared game files and declared executable must remain available; runtime behavior is not tested.",
        json!({"moduleId":instance.map(|value| &value.summary.module_id),"instanceId":instance.map(|value| &value.summary.id),"error":result.err().map(|error| redact_assistant_provider_text(&error))}),
    )
}

fn assistant_unbound_install_verification(
    output: &AssistantExecuteOperationOutput,
    operation_error: Option<&str>,
) -> AssistantOperationVerification {
    let ready = operation_error.is_none()
        && output.task.as_ref().is_some_and(|task| {
            task.checks.iter().any(|check| {
                check.name == "server_files_ready"
                    && check.status == AssistantTaskCheckStatus::Satisfied
            })
        });
    AssistantOperationVerification {
        status: if ready { AssistantVerificationStatus::Inconclusive } else { AssistantVerificationStatus::Failed },
        summary: if ready { "Server files are ready. Creating and configuring an instance still require confirmation." } else { "Server file preparation failed; no instance was created or started." }.into(),
        run_id: None,
        evidence: json!({"operationError": operation_error.map(redact_assistant_provider_text), "serverFilesReady": ready}),
        can_continue: ready,
    }
}

// A module-only stage cannot use the instance-bound investigation continuation.
// This transition has one deterministic next operation and consumes a new token.
fn assistant_create_after_install_preview(
    settings: &AssistantProviderSettings,
    original_prompt: &str,
    task: std::sync::Arc<AssistantTaskContract>,
    step: usize,
) -> Result<AssistantExecuteOperationOutput, String> {
    if !task.prepares_service() || task.instance_id.is_some() || step >= task.operation_limit() {
        return Err("The task cannot create another server at this stage.".into());
    }
    let module_id = task
        .module_id
        .as_ref()
        .ok_or("The launch task has no selected game.")?;
    let input = AssistantExecuteOperationInput {
        settings: settings.clone(),
        prompt: original_prompt.into(),
        context: None,
        selected_instance_id: None,
        selected_module_id: Some(module_id.clone()),
        task: task.request.clone(),
    };
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CreateServer, module_id: Some(module_id.clone()),
        ..assistant_safe_none_plan("The server files are ready. Create an isolated instance before applying the requested configuration.".into())
    };
    task.validate_plan(&plan, None)?;
    let summary = format!(
        "{}\n{}",
        task.summary(),
        summarize_assistant_operation_preview(&input, &plan, None)
    );
    let (token, expires) = store_assistant_pending_operation(
        &input,
        plan.clone(),
        None,
        summary.clone(),
        0,
        task.clone(),
    )?;
    let mut pending = assistant_pending_operations()
        .lock()
        .map_err(|_| "The confirmation store is unavailable.")?;
    let operation = pending
        .get_mut(&token)
        .ok_or("The create preview expired.")?;
    operation.repair_step = step;
    operation.original_prompt = original_prompt.into();
    let mut output = assistant_operation_output(&plan, 0);
    output.requires_confirmation = true;
    output.confirmation_token = Some(token);
    output.confirmation_expires_at_unix_ms = Some(expires);
    output.plan_summary = Some(summary.clone());
    output.message = format!("Pending confirmation: {summary}");
    output.module_id = Some(module_id.clone());
    output.task = Some(task.receipt(AssistantTaskStatus::Proposed, Vec::new()));
    Ok(output)
}

fn assistant_unbound_operation_result(
    task: &AssistantTaskContract,
    output: &mut AssistantExecuteOperationOutput,
    operation_error: Option<&str>,
) {
    let failed = operation_error.is_some();
    output.verification = Some(AssistantOperationVerification {
        status: if failed { AssistantVerificationStatus::Failed } else { AssistantVerificationStatus::Inconclusive },
        summary: "The operation has no readable bound instance. Inspect its result before continuing; no runtime success has been verified.".into(),
        run_id: None,
        evidence: json!({"reason": "no_bound_instance", "operationError": operation_error.map(redact_assistant_provider_text)}),
        can_continue: false,
    });
    let mut checks = output
        .task
        .as_ref()
        .map(|receipt| receipt.checks.clone())
        .unwrap_or_default();
    checks.push(assistant_task_check(
        "bound_target", if failed { AssistantTaskCheckStatus::Failed } else { AssistantTaskCheckStatus::Unknown },
        "The operation result could not be bound to a readable instance; completed changes have not been undone.",
        json!({"instanceId": output.instance_id, "error": operation_error.map(redact_assistant_provider_text)}),
    ));
    output.task = Some(task.receipt(assistant_task_status(&checks), checks));
}

async fn assistant_created_instance_baseline(
    storage: &StorageBootstrap,
    task: &AssistantTaskContract,
    output: &AssistantExecuteOperationOutput,
) -> Result<(std::sync::Arc<AssistantTaskContract>, InstanceDetails), String> {
    let id = output
        .instance_id
        .as_deref()
        .ok_or("The create operation returned no instance.")?;
    let created = read_instance_details(&storage.paths, id)
        .await
        .map_err(|error| {
            format!("The instance was created but its configuration could not be read: {error}")
        })?;
    Ok((
        std::sync::Arc::new(task.bind_created_instance(&created)?),
        created,
    ))
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod launch_tests;
