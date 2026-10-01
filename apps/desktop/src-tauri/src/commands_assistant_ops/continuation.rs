struct AssistantTaskContinuation {
    input: AssistantExecuteOperationInput,
    mode: AssistantOperationMode,
    task: std::sync::Arc<AssistantTaskContract>,
    revision: u64,
    expires_at: Instant,
}

impl AssistantTaskContinuation {
    fn is_current(&self) -> bool {
        self.expires_at > Instant::now()
            && self.task.session.as_ref().is_some_and(|session| {
                self.revision == session.revision() && session.check_active().is_ok()
            })
    }
}

static ASSISTANT_CONTINUATIONS: OnceLock<StdMutex<HashMap<String, AssistantTaskContinuation>>> =
    OnceLock::new();

fn assistant_continuations() -> &'static StdMutex<HashMap<String, AssistantTaskContinuation>> {
    ASSISTANT_CONTINUATIONS.get_or_init(|| StdMutex::new(HashMap::new()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantResumeConversationInput {
    conversation_id: String,
    settings: AssistantProviderSettings,
}

fn assistant_pause_task(
    input: &AssistantExecuteOperationInput,
    mode: &AssistantOperationMode,
    task: &std::sync::Arc<AssistantTaskContract>,
) -> Result<AssistantExecuteOperationOutput, String> {
    let pause = task
        .run
        .pause_receipt()
        .ok_or("The task has no resumable budget checkpoint.")?;
    let session = task.session.as_ref().ok_or_else(|| pause.summary.clone())?;
    session.check_active()?;
    if let Some(mut checkpoint) = session.checkpoint()? {
        checkpoint["budget"] =
            serde_json::to_value(task.run.checkpoint()?).map_err(|error| error.to_string())?;
        session.set_checkpoint(Some(checkpoint))?;
    }
    let resume_mode = assistant_continuation_mode(mode, task)?;
    let mut input = input.clone();
    input.settings.api_key.clear();
    let mut checkpoints = assistant_continuations()
        .lock()
        .map_err(|_| "Assistant checkpoint store is unavailable.")?;
    checkpoints.retain(|_, saved| saved.is_current());
    if !checkpoints.contains_key(session.id()) && checkpoints.len() >= 16 {
        return Err("Assistant checkpoint capacity is full. Close an unused conversation before continuing.".into());
    }
    session.check_active()?;
    checkpoints.insert(
        session.id().to_string(),
        AssistantTaskContinuation {
            input,
            mode: resume_mode,
            task: task.clone(),
            revision: session.revision(),
            expires_at: Instant::now() + Duration::from_secs(30 * 24 * 60 * 60),
        },
    );
    let mut output =
        assistant_operation_output(&assistant_safe_none_plan(pause.summary.clone()), 0);
    output.message = pause.summary.clone();
    output.task = Some(task.receipt(AssistantTaskStatus::Inconclusive, Vec::new()));
    output.continuation = Some(pause);
    assistant_attach_session_output(&mut output, session);
    Ok(output)
}

fn assistant_continuation_mode(
    mode: &AssistantOperationMode,
    task: &std::sync::Arc<AssistantTaskContract>,
) -> Result<AssistantOperationMode, String> {
    Ok(match mode {
        #[cfg(test)]
        AssistantOperationMode::Preview | AssistantOperationMode::ExecuteImmediately => {
            return Err("A task continuation requires a resolved conversation scope.".into());
        }
        AssistantOperationMode::ResolvedPreview {
            target,
            conversation_reference,
            ..
        } => AssistantOperationMode::ResolvedPreview {
            task: task.clone(),
            target: *target,
            conversation_reference: conversation_reference.clone(),
        },
        AssistantOperationMode::FollowUp {
            step,
            instance_id,
            module_id,
            original_prompt,
            verified_precondition,
            verification,
            ..
        } => AssistantOperationMode::FollowUp {
            step: *step,
            instance_id: instance_id.clone(),
            module_id: module_id.clone(),
            original_prompt: original_prompt.clone(),
            verified_precondition: verified_precondition.clone(),
            verification: verification.clone(),
            task: Some(task.clone()),
        },
        AssistantOperationMode::Confirmed(pending) => AssistantOperationMode::ResolvedPreview {
            task: task.clone(),
            target: if pending.plan.action == AssistantOperationAction::CreateServer {
                AssistantIntentTarget::NewInstance
            } else if task.instance_id.is_some() {
                AssistantIntentTarget::ExistingInstance
            } else if task.prepares_service() {
                AssistantIntentTarget::NewInstance
            } else {
                AssistantIntentTarget::Module
            },
            conversation_reference: None,
        },
    })
}

#[cfg(test)]
#[path = "continuation_tests.rs"]
mod continuation_tests;

#[cfg(test)]
#[path = "continuation_storage_tests.rs"]
mod continuation_storage_tests;

#[tauri::command]
pub async fn assistant_resume_conversation(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: AssistantResumeConversationInput,
) -> Result<AssistantExecuteOperationOutput, String> {
    assistant_resume_conversation_inner(Some(app_handle), state, input).await
}

async fn assistant_resume_conversation_inner(
    app_handle: Option<tauri::AppHandle>,
    state: tauri::State<'_, DesktopState>,
    input: AssistantResumeConversationInput,
) -> Result<AssistantExecuteOperationOutput, String> {
    // Bind the session and resolve its saved IDs under the same storage lease.
    // A path transition must not fit between identity validation and dispatch.
    let _storage_context_operation =
        state.begin_storage_context_operation("assistant continuation")?;
    assistant_restore_sessions(&state, &input.settings).await?;
    let lease = state.assistant_sessions.begin(
        Some(&input.conversation_id),
        assistant_session_binding(&state, &input.settings)?,
        false,
    )?;
    let session = lease.session();
    assistant_restore_task_checkpoint(&session, &input.settings)?;
    let mut saved = {
        let mut checkpoints = assistant_continuations()
            .lock()
            .map_err(|_| "Assistant checkpoint store is unavailable.")?;
        let saved = checkpoints
            .get(&input.conversation_id)
            .ok_or("This conversation has no saved continuation.")?;
        if saved.revision != session.revision() || !saved.is_current() {
            checkpoints.remove(&input.conversation_id);
            return Err("The saved task was superseded or expired. Send a new request; no operation was executed.".into());
        }
        saved
            .task
            .run
            .grant_continuation()
            .map_err(|pause| pause.summary)?;
        checkpoints
            .remove(&input.conversation_id)
            .ok_or("The saved continuation became unavailable.")?
    };
    saved.input.settings = input.settings;
    let retry_mode = assistant_continuation_mode(&saved.mode, &saved.task)?;
    let result = tokio::select! {
        result = Box::pin(assistant_operation_inner(app_handle, state, saved.input.clone(), saved.mode)) => result,
        () = session.cancelled() => return Err("Assistant continuation was stopped; completed changes remain.".into()),
    };
    let mut output = match result {
        Ok(output) => output,
        Err(error) => {
            assistant_retain_failed_continuation(&saved.input, &retry_mode, &saved.task, &error)?
        }
    };
    assistant_attach_session_output(&mut output, &session);
    session.flush().await?;
    Ok(output)
}

fn assistant_retain_failed_continuation(
    input: &AssistantExecuteOperationInput,
    mode: &AssistantOperationMode,
    task: &std::sync::Arc<AssistantTaskContract>,
    error: &str,
) -> Result<AssistantExecuteOperationOutput, String> {
    task.session
        .as_ref()
        .ok_or("No recorded conversation is available.")?
        .check_active()?;
    task.run.pause_after_investigation_failure()?;
    let mut output = assistant_pause_task(input, mode, task)?;
    output.message = format!(
        "{}\n{}",
        output.message,
        truncate_assistant_prompt_text(&redact_assistant_provider_text(error), 1024)
    );
    Ok(output)
}
