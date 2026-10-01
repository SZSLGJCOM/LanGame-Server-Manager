#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantCompletedOperation {
    action: AssistantOperationAction,
    instance_id: Option<String>,
    message: String,
    verification: Option<AssistantOperationVerification>,
    task: Option<AssistantTaskReceipt>,
    file_change_result: Option<app_storage::InstanceFilePatchResult>,
    file_changes_result: Option<app_storage::InstanceFilePatchesResult>,
}

fn assistant_unrecorded_operation(output: &mut AssistantExecuteOperationOutput, error: &str) {
    output.follow_up = None;
    output.continuation = None;
    output.message.push_str(&format!("\nOperation receipts could not be saved: {}. Earlier file and runtime effects remain. Inspect current state before any further change.", redact_assistant_provider_text(error)));
    if let Some(verification) = &mut output.verification {
        verification.can_continue = false;
    }
    if let Some(task) = &mut output.task {
        task.status = AssistantTaskStatus::Inconclusive;
        task.checks.push(assistant_task_check(
            "receipt_persisted",
            AssistantTaskCheckStatus::Unknown,
            "The operation result is available in this response but could not be persisted.",
            Value::Null,
        ));
    }
}

// A grant exists only in one user-confirmed execution chain. It is never a
// model argument, conversation message, persisted checkpoint or reusable token.
struct AssistantTaskAuthorization {
    task_id: String,
    session_id: String,
    revision: u64,
    instance_id: String,
    module_id: String,
    goal: AssistantTaskGoal,
    expires_at: Instant,
}

impl AssistantTaskAuthorization {
    fn capture(pending: &AssistantPendingOperation) -> Result<Self, String> {
        let task = &pending.task;
        let session = task
            .session
            .as_ref()
            .ok_or("Continuous work requires an active conversation.")?;
        session.check_active()?;
        Ok(Self {
            task_id: task.id.clone(),
            session_id: session.id().into(),
            revision: session.revision(),
            instance_id: task
                .instance_id
                .clone()
                .ok_or("Continuous work requires an existing selected instance.")?,
            module_id: task
                .module_id
                .clone()
                .ok_or("Continuous work requires a bound module.")?,
            goal: task.request.goal,
            expires_at: Instant::now() + Duration::from_secs(20 * 60),
        })
    }

    fn allows(&self, pending: &AssistantPendingOperation) -> bool {
        let task = &pending.task;
        self.expires_at > Instant::now()
            && task.id == self.task_id
            && task.request.goal == self.goal
            && task.instance_id.as_deref() == Some(self.instance_id.as_str())
            && task.module_id.as_deref() == Some(self.module_id.as_str())
            && pending.plan.instance_id.as_deref() == Some(self.instance_id.as_str())
            && pending.plan.module_id.as_deref() == Some(self.module_id.as_str())
            && task.session.as_ref().is_some_and(|session| {
                session.id() == self.session_id
                    && session.revision() == self.revision
                    && pending.conversation_revision == Some(self.revision)
                    && session.check_active().is_ok()
            })
            && assistant_action_allows_continuation(pending.plan.action, self.goal)
    }
}

fn assistant_action_allows_continuation(
    action: AssistantOperationAction,
    goal: AssistantTaskGoal,
) -> bool {
    if goal == AssistantTaskGoal::Inspect {
        return false;
    }
    match action {
        AssistantOperationAction::ApplyBeginnerConfig
        | AssistantOperationAction::CustomizeConfig
        | AssistantOperationAction::PatchInstanceText
        | AssistantOperationAction::PatchInstanceFiles
        | AssistantOperationAction::RepairPorts => true,
        AssistantOperationAction::StartServer => matches!(
            goal,
            AssistantTaskGoal::RestoreService | AssistantTaskGoal::LaunchService
        ),
        _ => false,
    }
}

fn assistant_file_set_allows_follow_up(
    result: Option<&app_storage::InstanceFilePatchesResult>,
) -> bool {
    result.is_none_or(|result| {
        result.status == app_storage::InstanceFilePatchesStatus::Applied
            && result.files.iter().all(|file| {
                file.state == app_storage::InstanceFilePatchState::Applied
                    && file.read_back_verified
            })
    })
}

pub(super) async fn assistant_confirm_operation_with_verification(
    app_handle: Option<tauri::AppHandle>,
    state: tauri::State<'_, DesktopState>,
    input: AssistantConfirmOperationInput,
) -> Result<AssistantExecuteOperationOutput, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("assistant confirmation")?;
    let mut pending = take_assistant_pending_operation(
        &input.confirmation_token,
        &input.plan_summary,
        &input.settings,
    )?;
    let session = pending.task.session.clone();
    let _session_lease = match session.as_ref() {
        Some(session) => {
            if input.conversation_id.as_deref() != Some(session.id())
                || pending.conversation_revision != Some(session.revision())
            {
                return Err(
                    "The preview belongs to a different conversation or an earlier user request."
                        .into(),
                );
            }
            Some(state.assistant_sessions.begin(
                Some(session.id()),
                assistant_session_binding(&state, &input.settings)?,
                false,
            )?)
        }
        None => None,
    };
    let authorization = input
        .continue_task
        .then(|| AssistantTaskAuthorization::capture(&pending))
        .transpose()?;
    if let Some(session) = &session {
        assistant_record_session_event(
            session,
            "user_confirmation",
            json!({
                "taskId":pending.task.id,"action":pending.plan.action,"instanceId":pending.task.instance_id,
                "continueTask":authorization.is_some(),"scope":"current task configuration, private editable files, ports, and task-required start; excludes install, download, broadcast and admin commands"
            }),
        )?;
        session.flush().await?;
    }
    let mut completed = Vec::new();
    loop {
        let revision = session.as_ref().map_or(0, |session| session.revision());
        let action_name =
            serde_json::to_value(pending.plan.action).map_err(|error| error.to_string())?;
        let action_name = action_name.as_str().ok_or("Operation has no name.")?;
        let attempted = json!({"action":pending.plan.action,"instanceId":pending.plan.instance_id,
            "moduleId":pending.plan.module_id,"taskId":pending.task.id});
        let attempted_plan = pending.plan.clone();
        let attempted_task = pending.task.clone();
        // Keep each repair future on the heap: its verification and native
        // provider state must not accumulate in the caller's Windows stack.
        let attempt = async {
            assistant_tool_progress(session.as_ref(), revision, action_name, "tool_started")?;
            Box::pin(assistant_confirm_task_inner(
                app_handle.clone(),
                state.clone(),
                input.clone(),
                pending,
            ))
            .await
        }
        .await;
        let mut output = match attempt {
            Ok(output) => output,
            Err(error) => {
                let mut failure = error;
                if let Some(session) = &session {
                    let _ = assistant_tool_progress(
                        Some(session),
                        revision,
                        action_name,
                        "tool_failed",
                    );
                    let recorded = assistant_record_session_event(
                        session,
                        "operation_error",
                        json!({
                            "attempted":attempted,"error":redact_assistant_provider_text(&failure),"completedOperationCount":completed.len(),
                            "executionStatus":"unknown","guidance":"Partial effects may exist. Read current state before any retry. Earlier receipts remain valid; do not claim nothing executed."
                        }),
                    );
                    let saved = match recorded {
                        Ok(()) => session.flush().await,
                        Err(error) => Err(error),
                    };
                    if let Err(record_error) = saved {
                        failure.push_str(&format!("\nOperation history could not be retained: {record_error}. Inspect current state before retrying."));
                    }
                }
                if completed.is_empty() {
                    return Err(failure);
                }
                let mut output = assistant_operation_output(&attempted_plan, 0);
                output.instance_id = attempted_task.instance_id.clone();
                output.module_id = attempted_task.module_id.clone();
                output.message = format!(
                    "Continuous work stopped: {}. The latest attempt has no verified result and may have partial effects. Earlier completed operations are listed separately.",
                    redact_assistant_provider_text(&failure)
                );
                output.task =
                    Some(attempted_task.receipt(AssistantTaskStatus::Inconclusive, Vec::new()));
                output.completed_operations = completed;
                if let Some(session) = &session {
                    assistant_attach_session_output(&mut output, session);
                }
                return Ok(output);
            }
        };
        if let Some(session) = &session {
            let failed = output
                .task
                .as_ref()
                .is_some_and(|task| task.status == AssistantTaskStatus::Failed);
            let _ = assistant_tool_progress(
                Some(session),
                revision,
                action_name,
                if failed {
                    "tool_failed"
                } else {
                    "tool_completed"
                },
            );
            if session.check_active().is_err() {
                invalidate_assistant_session_previews(session.id())?;
                output.follow_up = None;
                output.continuation = None;
                if let Some(verification) = &mut output.verification {
                    verification.can_continue = false;
                }
            }
            assistant_attach_session_output(&mut output, session);
            let receipt = json!({"action":output.action,"instanceId":output.instance_id,"moduleId":output.module_id,
                "message":output.message,"verification":output.verification,"task":output.task,"fileChange":output.file_change_result,"fileChanges":output.file_changes_result,
                "runtimeResponses":output.runtime_response_texts,"followUpPending":output.follow_up.is_some()});
            if let Err(error) =
                assistant_record_session_event(session, "confirmed_operation", receipt)
            {
                invalidate_assistant_session_previews(session.id())?;
                assistant_unrecorded_operation(&mut output, &error);
                output.completed_operations = completed;
                return Ok(output);
            }
            let saved = async {
                if output
                    .task
                    .as_ref()
                    .is_some_and(|task| task.status == AssistantTaskStatus::Completed)
                {
                    session.set_checkpoint(None)?;
                }
                session.flush().await
            }
            .await;
            if let Err(error) = saved {
                let _ = invalidate_assistant_session_previews(session.id());
                assistant_unrecorded_operation(&mut output, &error);
                output.completed_operations = completed;
                return Ok(output);
            }
        }
        // Inspect the application-owned pending operation before consuming its
        // single-use token. An out-of-scope proposal stays available for review.
        let next = output.follow_up.as_ref().and_then(|next| {
            let grant = authorization.as_ref()?;
            let token = next.confirmation_token.as_ref()?;
            let operations = assistant_pending_operations().lock().ok()?;
            let next_pending = operations.get(token)?;
            (completed.len() < 31
                && assistant_file_set_allows_follow_up(output.file_changes_result.as_ref())
                && grant.allows(next_pending))
            .then(|| (token.clone(), next.plan_summary.clone().unwrap_or_default()))
        });
        let Some((token, summary)) = next else {
            output.completed_operations = completed;
            return Ok(output);
        };
        pending = match take_assistant_pending_operation(&token, &summary, &input.settings) {
            Ok(pending) => pending,
            Err(error) => {
                output.follow_up = None;
                output.completed_operations = completed;
                output.message.push_str(&format!("\nThe next confirmation is no longer valid: {}. Request a fresh preview; the earlier result remains valid.", redact_assistant_provider_text(&error)));
                return Ok(output);
            }
        };
        // Cancellation, expiry and revision changes can race token consumption.
        if !authorization
            .as_ref()
            .is_some_and(|grant| grant.allows(&pending))
        {
            output.follow_up = None;
            output.completed_operations = completed;
            output.message.push_str("\nContinuous authorization ended before the next operation. Request a fresh preview.");
            return Ok(output);
        }
        completed.push(AssistantCompletedOperation {
            action: output.action,
            instance_id: output.instance_id.clone(),
            message: output.message.clone(),
            verification: output.verification.clone(),
            task: output.task.clone(),
            file_change_result: output.file_change_result.clone(),
            file_changes_result: output.file_changes_result.clone(),
        });
    }
}

#[cfg(test)]
#[path = "authorization_tests.rs"]
mod authorization_tests;

#[cfg(test)]
#[path = "authorization_workflow_tests.rs"]
mod authorization_workflow_tests;
