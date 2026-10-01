#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantConversationStateInput {
    conversation_id: String,
    settings: AssistantProviderSettings,
    #[serde(default)]
    after_cursor: Option<u64>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantConversationStatus {
    Idle,
    Running,
    Paused,
    Unavailable,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantConversationState {
    conversation_id: String,
    status: AssistantConversationStatus,
    revision: Option<u64>,
    continuation: Option<AssistantRunPause>,
    progress: Option<crate::assistant::AssistantProgressSnapshot>,
    messages: Vec<crate::assistant_sessions::AssistantPublicMessage>,
    messages_truncated: bool,
}

#[tauri::command]
pub async fn assistant_get_conversation_state(
    state: tauri::State<'_, DesktopState>,
    input: AssistantConversationStateInput,
) -> Result<AssistantConversationState, String> {
    assistant_restore_sessions(&state, &input.settings).await?;
    let binding = assistant_session_binding(&state, &input.settings)?;
    let snapshot = state
        .assistant_sessions
        .inspect(&input.conversation_id, &binding)?;
    let session = state
        .assistant_sessions
        .get_bound(&input.conversation_id, &binding)?;
    if let Some(session) = &session
        && snapshot.is_some_and(|snapshot| !snapshot.busy)
    {
        assistant_restore_task_checkpoint(session, &input.settings)?;
    }
    let mut output = assistant_conversation_state(&input.conversation_id, snapshot)?;
    if let Some(session) = session
        && snapshot.is_some()
    {
        output.progress = Some(session.progress.snapshot(input.after_cursor)?);
        if input.after_cursor.is_none() {
            (output.messages, output.messages_truncated) = session.public_messages()?;
        }
        if session
            .checkpoint()
            .ok()
            .flatten()
            .is_some_and(|checkpoint| checkpoint["requires_restatement"] == true)
            && let Some(pause) = output.continuation.as_mut()
            && pause.reason == AssistantRunPauseReason::StateUnavailable
        {
            pause.summary = "Recovery needs a new request because sensitive values were excluded from the saved task. Review the current configuration and restate the task; no previous confirmation or operation will be replayed.".into();
            pause.can_resume = false;
        }
    }
    Ok(output)
}

// This is an observation, never permission to repeat an operation. In particular,
// idle after a lost response does not establish success or absence of changes.
fn assistant_conversation_state(
    id: &str,
    snapshot: Option<crate::assistant_sessions::AssistantSessionSnapshot>,
) -> Result<AssistantConversationState, String> {
    let mut output = AssistantConversationState {
        conversation_id: id.into(),
        status: AssistantConversationStatus::Unavailable,
        revision: None,
        continuation: None,
        progress: None,
        messages: Vec::new(),
        messages_truncated: false,
    };
    let Some(snapshot) = snapshot else {
        return Ok(output);
    };
    output.revision = Some(snapshot.revision);
    if snapshot.busy {
        output.status = AssistantConversationStatus::Running;
        return Ok(output);
    }
    let mut checkpoints = assistant_continuations()
        .lock()
        .map_err(|_| "Assistant checkpoint store is unavailable.")?;
    checkpoints.retain(|_, saved| saved.is_current());
    output.continuation = checkpoints
        .get(id)
        .filter(|saved| saved.revision == snapshot.revision)
        .and_then(|saved| saved.task.run.pause_receipt());
    output.status = if output.continuation.is_some() {
        AssistantConversationStatus::Paused
    } else {
        AssistantConversationStatus::Idle
    };
    Ok(output)
}

#[cfg(test)]
#[path = "session_state_tests.rs"]
mod session_state_tests;
