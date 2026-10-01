#[cfg(test)]
pub(super) async fn resolve_assistant_task_intent(
    input: &AssistantRequestInput,
    instances: &[InstanceSummary],
    modules: &[ModuleSummary],
    audit_storage: Option<&app_storage::StorageBootstrap>,
    host_state: Option<&DesktopState>,
) -> Result<AssistantIntentResolution, String> {
    resolve_assistant_task_intent_in_session(
        input,
        instances,
        modules,
        audit_storage,
        host_state,
        None,
    )
    .await
}

async fn resolve_assistant_task_intent_in_session(
    input: &AssistantRequestInput,
    instances: &[InstanceSummary],
    modules: &[ModuleSummary],
    audit_storage: Option<&app_storage::StorageBootstrap>,
    host_state: Option<&DesktopState>,
    session: Option<&std::sync::Arc<AssistantSession>>,
) -> Result<AssistantIntentResolution, String> {
    resolve_assistant_task_intent_with_session_tools(
        input,
        AssistantConversationScope { instances, modules, session },
        &ASSISTANT_INTENT_SLOTS,
        ASSISTANT_INTENT_TIMEOUT,
        |messages, tools| async move {
            let response = assistant_model_reply_in_session(
                &AssistantRunInput {
                    settings: input.settings.clone(),
                    prompt_label: String::from("Conversation"),
                    prompt: String::new(),
                    context: String::new(),
                },
                ASSISTANT_INTENT_SYSTEM_PROMPT,
                &messages,
                &tools,
                session,
            )
            .await;
            if let Some(storage) = audit_storage {
                let shape = match &response {
                    Ok(reply) => json!({
                        "outcome":"response",
                        "nativeCallCount":reply.calls.len(),
                        "unexpectedToolCount":reply.calls.iter().filter(|call| !matches!(call.name.as_str(), "resolve_task" | "read_host_info" | "read_session_history")).count(),
                        "hasText":!reply.content.trim().is_empty(),
                        "historyMessageCount":messages.len()
                    }),
                    Err(_) => json!({"outcome":"transport_error", "historyMessageCount":messages.len()}),
                };
                append_desktop_app_log(storage, "info", "assistant.conversation.reply", "Assistant conversation response shape", shape);
            }
            response
        },
        || async {
            match host_state {
                Some(state) => read_assistant_host_info(state).await,
                None => Err(String::from("Host information is unavailable in this conversation environment.")),
            }
        },
    )
    .await
}

#[cfg(test)]
async fn resolve_assistant_task_intent_with<F, Fut>(
    input: &AssistantRequestInput,
    instances: &[InstanceSummary],
    modules: &[ModuleSummary],
    slots: &tokio::sync::Semaphore,
    timeout: Duration,
    reply: F,
) -> Result<AssistantIntentResolution, String>
where
    F: FnMut(
        Vec<crate::assistant::AssistantToolMessage>,
        Vec<crate::assistant::AssistantToolDefinition>,
    ) -> Fut,
    Fut: std::future::Future<Output = Result<crate::assistant::AssistantToolReply, String>>,
{
    resolve_assistant_task_intent_with_tools(
        input,
        instances,
        modules,
        slots,
        timeout,
        reply,
        || async { Err(String::from("No host reader is installed in this test.")) },
    )
    .await
}

#[cfg(test)]
async fn resolve_assistant_task_intent_with_tools<F, Fut, H, HostFut>(
    input: &AssistantRequestInput,
    instances: &[InstanceSummary],
    modules: &[ModuleSummary],
    slots: &tokio::sync::Semaphore,
    timeout: Duration,
    reply: F,
    read_host: H,
) -> Result<AssistantIntentResolution, String>
where
    F: FnMut(
        Vec<crate::assistant::AssistantToolMessage>,
        Vec<crate::assistant::AssistantToolDefinition>,
    ) -> Fut,
    Fut: std::future::Future<Output = Result<crate::assistant::AssistantToolReply, String>>,
    H: FnMut() -> HostFut,
    HostFut: std::future::Future<Output = Result<Value, String>>,
{
    resolve_assistant_task_intent_with_session_tools(
        input,
        AssistantConversationScope {
            instances,
            modules,
            session: None,
        },
        slots,
        timeout,
        reply,
        read_host,
    )
    .await
}

async fn resolve_assistant_task_intent_with_session_tools<F, Fut, H, HostFut>(
    input: &AssistantRequestInput,
    scope: AssistantConversationScope<'_>,
    slots: &tokio::sync::Semaphore,
    timeout: Duration,
    mut reply: F,
    mut read_host: H,
) -> Result<AssistantIntentResolution, String>
where
    F: FnMut(
        Vec<crate::assistant::AssistantToolMessage>,
        Vec<crate::assistant::AssistantToolDefinition>,
    ) -> Fut,
    Fut: std::future::Future<Output = Result<crate::assistant::AssistantToolReply, String>>,
    H: FnMut() -> HostFut,
    HostFut: std::future::Future<Output = Result<Value, String>>,
{
    let _permit = slots.try_acquire().map_err(|_| {
        String::from(
            "Two assistant requests are already being interpreted. Try again when one finishes.",
        )
    })?;
    let AssistantConversationScope {
        instances,
        modules,
        session,
    } = scope;
    let catalog = AssistantIntentCatalog::new(input, instances, modules)?;
    let mut messages = match session {
        Some(session) => {
            session.pin_context(&format!(
                "{}\nCurrent user request:\n{}",
                catalog.prompt(input)?,
                redact_assistant_provider_text(input.prompt.trim())
            ))?;
            let mut history = session.messages()?;
            history.push(crate::assistant::AssistantToolMessage::User(
                catalog.prompt(input)?,
            ));
            history.push(crate::assistant::AssistantToolMessage::User(
                redact_assistant_provider_text(input.prompt.trim()),
            ));
            history
        }
        None => assistant_conversation_messages(input, &catalog)?,
    };
    let mut tools = vec![catalog.tool(), assistant_host_info_tool()];
    if session.is_some() {
        tools.push(assistant_session_history_tool());
    }
    // At most one host read and one protocol correction share the conversation
    // deadline and slot. A rejected batch cannot authorize or execute any work.
    let resolve = async {
        let mut host_read = false;
        let mut corrected = false;
        for _ in 0..12 {
            if let Some(session) = session {
                session.check_active()?;
                session.replace_messages(messages.clone())?;
                messages = session.messages()?;
            }
            let response = reply(messages.clone(), tools.clone()).await?;
            validate_assistant_intent_reply_size(&response)
                .map_err(assistant_intent_protocol_error)?;
            if let Some(session) = session
                && let [call] = response.calls.as_slice()
                && call.name == "read_session_history"
            {
                let read = assistant_session_history_call(session, &call.arguments);
                // Trust comes from the successful local history reader, never
                // from a model-supplied field in a tool argument or response.
                let content = match read {
                    Ok(page) => assistant_bounded_tool_result_text(&json!({"ok":true,"data":page})),
                    Err(error) => assistant_tool_result_text(&json!({"ok":false,"error":error})),
                };
                let feedback = crate::assistant::AssistantToolMessage::ToolResult {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    is_error: serde_json::from_str::<Value>(&content)
                        .ok()
                        .is_none_or(|value| value["ok"] != true),
                    content,
                };
                messages.push(crate::assistant::AssistantToolMessage::Assistant(response));
                messages.push(feedback);
                continue;
            }
            let resolution = if response
                .calls
                .iter()
                .any(|call| call.name == "read_host_info")
            {
                match validate_assistant_host_call(&response, host_read) {
                    Ok(call) => {
                        host_read = true;
                        let result = read_host().await;
                        let (content, is_error) = match result {
                            Ok(data) => (
                                json!({"ok":true,"scope":"manager_host","observedAtUnixMs":unix_timestamp_ms(),"data":data}),
                                false,
                            ),
                            Err(error) => (
                                json!({"ok":false,"error":redact_assistant_provider_text(&error),
                                "guidance":"Host facts are unavailable. Explain this limitation in the user's language; do not substitute guessed values or application defaults."}),
                                true,
                            ),
                        };
                        let feedback = crate::assistant::AssistantToolMessage::ToolResult {
                            call_id: call.id.clone(),
                            name: call.name.clone(),
                            content: content.to_string(),
                            is_error,
                        };
                        messages.push(crate::assistant::AssistantToolMessage::Assistant(response));
                        messages.push(feedback);
                        continue;
                    }
                    Err(reason) => Err(reason),
                }
            } else {
                parse_assistant_intent_response(response.clone(), &catalog)
            };
            match resolution {
                Ok(resolution) => {
                    let calls = response.calls.clone();
                    messages.push(crate::assistant::AssistantToolMessage::Assistant(response));
                    for call in calls {
                        messages.push(crate::assistant::AssistantToolMessage::ToolResult {
                            call_id: call.id, name: call.name,
                            content: json!({"ok":true,"status":"scope_resolved","executed":false,"guidance":"The application will supply the current task scope and tools. This only resolves scope; no server operation has executed."}).to_string(),
                            is_error: false,
                        });
                    }
                    if let Some(session) = session {
                        session.replace_messages(messages.clone())?;
                        session.flush().await?;
                    }
                    return Ok(resolution);
                }
                Err(reason) => {
                    if corrected {
                        return Err(assistant_intent_protocol_error(reason));
                    }
                    let feedback = assistant_intent_error_feedback(&response, &reason)
                        .ok_or_else(|| assistant_intent_protocol_error(reason))?;
                    corrected = true;
                    messages.push(crate::assistant::AssistantToolMessage::Assistant(response));
                    messages.extend(feedback);
                }
            }
        }
        Err(assistant_intent_protocol_error(String::from(
            "Conversation tool round limit reached; no operation was executed.",
        )))
    };
    let outcome = tokio::time::timeout(timeout, resolve)
        .await
        .unwrap_or_else(|_| {
            Err(String::from(
                "Assistant request interpretation timed out; no operation was executed.",
            ))
        });
    if outcome.is_err() {
        assistant_close_pending_tool_results(
            &mut messages,
            outcome.as_ref().err().map(String::as_str),
        );
        if let Some(session) = session {
            session.replace_messages(messages)?;
            session.flush().await?;
        }
    }
    outcome
}
struct AssistantConversationScope<'a> {
    instances: &'a [InstanceSummary],
    modules: &'a [ModuleSummary],
    session: Option<&'a std::sync::Arc<AssistantSession>>,
}
