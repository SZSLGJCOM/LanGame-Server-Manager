pub(super) async fn assistant_request_operation_inner(
    app_handle: Option<tauri::AppHandle>,
    state: tauri::State<'_, DesktopState>,
    mut input: AssistantRequestInput,
) -> Result<AssistantExecuteOperationOutput, String> {
    assistant_restore_sessions(&state, &input.settings).await?;
    let lease = state.assistant_sessions.begin(
        input.conversation_id.as_deref(),
        assistant_session_binding(&state, &input.settings)?,
        true,
    )?;
    let session = lease.session();
    invalidate_assistant_session_previews(session.id())?;
    input.prior_requests = session.source_user_requests()?;
    input.conversation_messages.clear();
    session.register_user_request(&input.prompt)?;
    session.flush().await?;
    let mut output = tokio::select! {
        result = assistant_request_in_session(app_handle, state, input, session.clone()) => result?,
        () = session.cancelled() => return Err(String::from("Assistant work was stopped. No further operation was prepared; earlier completed changes remain.")),
    };
    assistant_attach_session_output(&mut output, &session);
    if !output.requires_confirmation && output.continuation.is_none() && output.follow_up.is_none()
    {
        session.set_checkpoint(None)?;
    }
    session.flush().await?;
    Ok(output)
}

async fn assistant_request_in_session(
    app_handle: Option<tauri::AppHandle>,
    state: tauri::State<'_, DesktopState>,
    input: AssistantRequestInput,
    session: std::sync::Arc<AssistantSession>,
) -> Result<AssistantExecuteOperationOutput, String> {
    // Conversation and host inspection use the already loaded catalog. Storage
    // initialization and its transition lease belong to actual server work.
    let (instances, modules, storage_identity) = {
        let current = state
            .app_state
            .read()
            .map_err(|_| String::from("Application catalog is unavailable."))?;
        (
            current.instances.clone(),
            current.modules.clone(),
            assistant_catalog_storage_identity(&current),
        )
    };
    let resolution = resolve_assistant_task_intent_in_session(
        &input,
        &instances,
        &modules,
        None,
        Some(&state),
        Some(&session),
    )
    .await;
    let outcome = match &resolution {
        Ok(AssistantIntentResolution::Reply(_)) => "reply",
        Ok(AssistantIntentResolution::Clarification(_)) => "clarification",
        Ok(AssistantIntentResolution::Resolved { .. }) => "task",
        Err(_) => "failed",
    };
    let resolution = resolution?;
    let (request, target, original_request, instance_id, module_id) = match resolution {
        AssistantIntentResolution::Resolved {
            request,
            target,
            original_request,
            instance_id,
            module_id,
        } => (request, target, original_request, instance_id, module_id),
        AssistantIntentResolution::Reply(message)
        | AssistantIntentResolution::Clarification(message) => {
            let plan = assistant_safe_none_plan(message.clone());
            let mut output = assistant_operation_output(&plan, 0);
            output.message = message;
            return Ok(output);
        }
    };
    let _storage_context_operation =
        state.begin_storage_context_operation("assistant server task")?;
    {
        let current = state
            .app_state
            .read()
            .map_err(|_| String::from("Application catalog is unavailable."))?;
        if assistant_catalog_storage_identity(&current) != storage_identity {
            return Err(String::from(
                "Application storage paths changed during the conversation. Repeat the server request in the current context; no operation was prepared or executed.",
            ));
        }
    }
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let actual_identity = [
        storage.settings.servers_root.clone(),
        storage.settings.games_root.clone(),
        storage.settings.modules_root.clone(),
        storage.settings.steamcmd_root.clone(),
        storage.paths.database_path.to_string_lossy().into_owned(),
        storage.paths.migrations_root.to_string_lossy().into_owned(),
    ];
    if actual_identity != storage_identity {
        return Err(String::from(
            "Application storage no longer matches the conversation catalog. Refresh the application and repeat the server request; no operation was prepared or executed.",
        ));
    }
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    append_desktop_app_log(
        &storage,
        "info",
        "assistant.conversation.completed",
        "Assistant conversation turn completed",
        json!({"outcome":outcome}),
    );
    let conversation_reference = assistant_conversation_reference(&input.conversation_messages)?;
    let resolved = AssistantExecuteOperationInput {
        settings: input.settings,
        prompt: original_request,
        context: input.context,
        task: request,
        selected_instance_id: instance_id,
        selected_module_id: module_id,
    };
    let instance = match &resolved.selected_instance_id {
        Some(id) => Some(
            read_instance_details(&storage.paths, id)
                .await
                .map_err(|error| format!("The request's server could not be read: {error}"))?,
        ),
        None => None,
    };
    if instance.as_ref().is_some_and(|instance| {
        Some(&instance.summary.module_id) != resolved.selected_module_id.as_ref()
    }) {
        return Err(String::from(
            "The request's server game changed during interpretation.",
        ));
    }
    let mut task = AssistantTaskContract::capture(&resolved, instance.as_ref())?;
    task.session = Some(session);
    let task = std::sync::Arc::new(task);
    Box::pin(assistant_operation_inner(
        app_handle,
        state,
        resolved,
        AssistantOperationMode::ResolvedPreview {
            task,
            target,
            conversation_reference,
        },
    ))
    .await
}

fn assistant_catalog_storage_identity(state: &app_core::AppState) -> [String; 6] {
    [
        state.settings.servers_root.clone(),
        state.settings.games_root.clone(),
        state.settings.modules_root.clone(),
        state.settings.steamcmd_root.clone(),
        state.storage.database_path.clone(),
        state.storage.migrations_path.clone(),
    ]
}

// Resolve targets before reading private instance evidence. A later operation
// may omit its IDs, but it cannot replace the scope selected during interpretation.
fn validate_assistant_resolved_target(
    task: &AssistantTaskContract,
    target: AssistantIntentTarget,
    plan: &AssistantOperationPlan,
) -> Result<(), String> {
    if plan.action == AssistantOperationAction::None {
        return Ok(());
    }
    if plan
        .instance_id
        .as_ref()
        .is_some_and(|id| Some(id) != task.instance_id.as_ref())
        || plan
            .module_id
            .as_ref()
            .is_some_and(|id| Some(id) != task.module_id.as_ref())
    {
        return Err(String::from(
            "The proposed operation conflicts with the request's resolved target. No operation was prepared.",
        ));
    }
    let needs_instance = !matches!(
        plan.action,
        AssistantOperationAction::CreateServer
            | AssistantOperationAction::InstallServer
            | AssistantOperationAction::ValidateServer
    );
    if task.module_id.is_none() || (needs_instance && task.instance_id.is_none()) {
        return Err(String::from(
            "The request has no resolved target for this operation. Clarify the server or game before changing it.",
        ));
    }
    if plan.action == AssistantOperationAction::CreateServer
        && (target != AssistantIntentTarget::NewInstance || task.instance_id.is_some())
    {
        return Err(String::from(
            "Creating a server requires a new-instance request; the current request targets an existing server.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod request_tests;
