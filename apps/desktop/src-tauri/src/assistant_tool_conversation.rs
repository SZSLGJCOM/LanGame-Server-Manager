use super::*;
use std::collections::VecDeque;

#[path = "assistant_session_progress.rs"]
mod progress;
pub(crate) use progress::{AssistantProgressSnapshot, AssistantSessionProgress};

pub(crate) type AssistantTextObserver<'a> = dyn Fn(&str) -> Result<(), String> + Send + Sync + 'a;

#[cfg(test)]
#[path = "assistant_tool_conversation_mock.rs"]
mod fixture_protocol;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AssistantToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AssistantToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AssistantToolReply {
    pub content: String,
    pub calls: Vec<AssistantToolCall>,
    pub raw_message: Value,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) enum AssistantToolMessage {
    User(String),
    Assistant(AssistantToolReply),
    ToolResult {
        call_id: String,
        name: String,
        content: String,
        is_error: bool,
    },
}

/// User text and tool evidence must be redacted by the caller before composition.
/// Provider-native assistant blocks are replayed intact, including thinking signatures.
#[cfg(test)]
pub(crate) async fn run_assistant_tool_turn(
    input: &AssistantRunInput,
    system_prompt: &str,
    messages: &[AssistantToolMessage],
    tools: &[AssistantToolDefinition],
) -> Result<AssistantToolReply, String> {
    run_tool_turn(input, system_prompt, messages, tools, None).await
}

/// Only public text is observed. A failed observer or a dropped future closes
/// the response body; no partial tool call is returned to an executor.
pub(crate) async fn run_assistant_tool_turn_streaming(
    input: &AssistantRunInput,
    system_prompt: &str,
    messages: &[AssistantToolMessage],
    tools: &[AssistantToolDefinition],
    observer: &AssistantTextObserver<'_>,
) -> Result<AssistantToolReply, String> {
    run_tool_turn(input, system_prompt, messages, tools, Some(observer)).await
}

async fn run_tool_turn(
    input: &AssistantRunInput,
    system_prompt: &str,
    messages: &[AssistantToolMessage],
    tools: &[AssistantToolDefinition],
    observer: Option<&AssistantTextObserver<'_>>,
) -> Result<AssistantToolReply, String> {
    let protocol = ProviderProtocol::parse(&input.settings.provider)?;
    let model = normalize_required(&input.settings.model, "model")?;
    let system_prompt = persona::system_prompt(&normalize_required(system_prompt, "systemPrompt")?);
    let seen_ids = validate_tool_history(messages)?;
    #[cfg(test)]
    if input.settings.base_url.trim().starts_with("mock://") {
        let reply = fixture_protocol::reply(input, messages, tools)?;
        if let Some(observer) = observer {
            observer(&reply.content)?;
        }
        return Ok(reply);
    }
    let endpoint = protocol.tool_endpoint(&input.settings.base_url)?;
    let mut body = protocol.tool_request_body(&model, &system_prompt, messages, tools)?;
    if observer.is_some() {
        body["stream"] = Value::Bool(true);
    }
    // The application owns its smaller transcript budget; the transport retains
    // a separate hard byte boundary including native protocol/schema overhead.
    if serde_json::to_vec(&body)
        .map_err(|_| String::from("failed to encode assistant tool request"))?
        .len()
        > ASSISTANT_MAX_RESPONSE_BYTES
    {
        return Err(String::from(
            "assistant tool request exceeded the transport byte limit",
        ));
    }
    let api_key =
        resolve_api_key_with_backend(&input.settings, &SecretStoreBackend::SystemKeyring)?;
    let client =
        build_assistant_http_client(&endpoint, assistant_request_timeout(&input.settings))?;
    let response = protocol
        .request(&client, &endpoint, &body, &api_key)
        .send()
        .await
        .map_err(|error| format!("assistant request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = read_assistant_response_body(response).await?;
        return Err(assistant_provider_http_error(status, &body));
    }
    let reply = if let Some(observer) = observer {
        protocol.read_tool_stream(response, observer).await?
    } else {
        let body = read_assistant_response_body(response).await?;
        protocol.decode_tool_response(&body)?
    };
    validate_new_calls(&reply.calls, &seen_ids)?;
    Ok(reply)
}

pub(super) fn validate_tool_identity(id: &str, name: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 || id.chars().any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(String::from("assistant tool call ID is missing or invalid"));
    }
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'))
    {
        return Err(String::from("assistant tool name is missing or invalid"));
    }
    Ok(())
}

pub(super) fn validate_new_calls(
    calls: &[AssistantToolCall],
    previous: &HashSet<String>,
) -> Result<(), String> {
    if calls.len() > 32 {
        return Err(String::from(
            "assistant response exceeded the tool call count limit",
        ));
    }
    let mut seen = previous.clone();
    for call in calls {
        validate_tool_identity(&call.id, &call.name)?;
        if !seen.insert(call.id.clone()) {
            return Err(String::from("assistant returned a duplicate tool call ID"));
        }
    }
    Ok(())
}

fn validate_tool_history(messages: &[AssistantToolMessage]) -> Result<HashSet<String>, String> {
    if messages.is_empty() || messages.len() > 128 {
        return Err(String::from(
            "assistant tool conversation is empty or exceeds its message limit",
        ));
    }
    let mut seen = HashSet::new();
    let mut pending = VecDeque::new();
    for message in messages {
        match message {
            AssistantToolMessage::ToolResult { call_id, name, .. } => {
                // Preserve order as well as identity: native Ollama associates
                // tool outputs by tool name and sequence rather than call ID.
                if pending.pop_front() != Some((call_id.as_str(), name.as_str())) {
                    return Err(String::from(
                        "assistant tool result has no matching pending call",
                    ));
                }
            }
            AssistantToolMessage::User(_) | AssistantToolMessage::Assistant(_) => {
                if !pending.is_empty() {
                    return Err(String::from(
                        "assistant tool conversation has unanswered calls",
                    ));
                }
                if let AssistantToolMessage::Assistant(reply) = message {
                    validate_new_calls(&reply.calls, &seen)?;
                    for call in &reply.calls {
                        seen.insert(call.id.clone());
                        pending.push_back((call.id.as_str(), call.name.as_str()));
                    }
                }
            }
        }
    }
    if !pending.is_empty() {
        return Err(String::from(
            "assistant tool conversation has unanswered calls",
        ));
    }
    Ok(seen)
}

#[cfg(test)]
#[path = "assistant_tool_conversation_tests.rs"]
mod tests;
