const ASSISTANT_TOOL_RESULT_BYTES: usize = 12 * 1024;
// Bound transport and retained evidence independently from model tokenization.
// The native provider rejects token overflow instead of truncating instructions.
const ASSISTANT_INVESTIGATION_PROMPT_BYTES: usize = 64 * 1024;
const ASSISTANT_INVESTIGATION_CONTROL_BYTES: usize = 512;
const ASSISTANT_INVESTIGATION_EVIDENCE_BYTES: usize =
    ASSISTANT_INVESTIGATION_PROMPT_BYTES - ASSISTANT_INVESTIGATION_CONTROL_BYTES;
// Count business evidence once. Native replies retain the same content/calls in
// raw_message together with protocol-only thinking/signatures. That raw envelope
// is separately bounded by the provider's 256 KiB request/response limits; it is
// never truncated, silently stripped or treated as extra observed evidence.
#[derive(Serialize)]
enum AssistantEvidenceMessage<'a> {
    User(&'a str),
    Assistant {
        content: &'a str,
        calls: &'a [AssistantToolCall],
    },
    ToolResult {
        call_id: &'a str,
        name: &'a str,
        content: &'a str,
        is_error: bool,
    },
}

fn assistant_check_conversation_budget(
    messages: &[AssistantToolMessage],
    tools: &[AssistantToolDefinition],
) -> Result<(), String> {
    let evidence = messages
        .iter()
        .map(|message| match message {
            AssistantToolMessage::User(content) => AssistantEvidenceMessage::User(content),
            AssistantToolMessage::Assistant(reply) => AssistantEvidenceMessage::Assistant {
                content: &reply.content,
                calls: &reply.calls,
            },
            AssistantToolMessage::ToolResult {
                call_id,
                name,
                content,
                is_error,
            } => AssistantEvidenceMessage::ToolResult {
                call_id,
                name,
                content,
                is_error: *is_error,
            },
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(evidence, tools))
        .map_err(|error| error.to_string())?
        .len();
    if bytes > ASSISTANT_INVESTIGATION_EVIDENCE_BYTES {
        Err(String::from(
            "Assistant investigation exhausted its context budget; the original request and tool pairs were not truncated.",
        ))
    } else {
        Ok(())
    }
}

fn assistant_tool_result_text(result: &Value) -> String {
    let redacted = redact_assistant_provider_text(&result.to_string());
    // Redaction pretty-prints JSON. Compact it again so indentation cannot make
    // an otherwise bounded dependency page exceed the evidence budget.
    let Ok(redacted) = serde_json::from_str::<Value>(&redacted) else {
        return json!({"ok": false, "error": "Read output could not be safely encoded; no complete evidence was supplied."}).to_string();
    };
    assistant_bounded_tool_result_text(&redacted)
}

// Only call this after executing the validated backend read. History is already
// redacted. Game documentation comes exclusively from the reviewed public-source
// library, never instance files; a second secret/path heuristic would corrupt
// public commands, authority names and the byte offsets of both kinds of page.
// Select by the actual native read type, never by a field in untrusted output.
fn assistant_read_result_text(request: &AssistantReadTool, result: &Value) -> String {
    if matches!(
        request,
        AssistantReadTool::ReadSessionHistory { .. }
            | AssistantReadTool::SearchGameDocs { .. }
            | AssistantReadTool::ReadGameDoc { .. }
    ) && result["ok"] == true
    {
        assistant_bounded_tool_result_text(result)
    } else {
        assistant_tool_result_text(result)
    }
}

fn assistant_bounded_tool_result_text(result: &Value) -> String {
    let encoded = result.to_string();
    // Truncating serialized JSON can hide pagination cursors or turn a partial
    // value into apparently complete evidence. Oversized reads fail explicitly.
    if encoded.len() > ASSISTANT_TOOL_RESULT_BYTES {
        return json!({"ok": false, "error": "Read output exceeds the evidence budget. Request fewer setting keys or log lines, or read configuration file pages. No complete result was supplied."}).to_string();
    }
    encoded
}

#[cfg(test)]
#[path = "investigation_budget_tests.rs"]
mod investigation_budget_tests;

#[cfg(test)]
#[path = "history_transport_tests.rs"]
mod history_transport_tests;
