use super::*;
use crate::assistant::{AssistantToolCall, tool_conversation::validate_new_calls};

fn invalid() -> String {
    String::from("assistant provider returned an invalid tool response")
}

pub(super) fn decode(
    protocol: ProviderProtocol,
    body: &[u8],
) -> Result<AssistantToolReply, String> {
    let payload: Value = serde_json::from_slice(body).map_err(|error| {
        format!(
            "failed to decode assistant tool response: {:?} at line {}, column {}",
            error.classify(),
            error.line(),
            error.column()
        )
    })?;
    if payload.get("error").is_some_and(|error| !error.is_null()) {
        return Err(invalid());
    }
    let (message, finish) = match protocol {
        ProviderProtocol::OpenAiCompatible => {
            let choices = payload["choices"].as_array().ok_or_else(invalid)?;
            if choices.len() != 1 {
                return Err(invalid());
            }
            (
                choices[0]["message"].clone(),
                choices[0]["finish_reason"].as_str(),
            )
        }
        ProviderProtocol::Ollama => {
            if payload["done"] != true {
                return Err(String::from("assistant response did not complete normally"));
            }
            (payload["message"].clone(), payload["done_reason"].as_str())
        }
        ProviderProtocol::AnthropicCompatible => (
            json!({"role":payload["role"],"content":payload["content"]}),
            payload["stop_reason"].as_str(),
        ),
    };
    if matches!(finish, Some("length" | "max_tokens")) {
        return Err(String::from(
            "assistant response exceeded its output token limit",
        ));
    }
    let normal = match protocol {
        ProviderProtocol::OpenAiCompatible => matches!(finish, Some("stop" | "tool_calls")),
        ProviderProtocol::Ollama => finish == Some("stop"),
        ProviderProtocol::AnthropicCompatible => matches!(finish, Some("end_turn" | "tool_use")),
    };
    if !normal || message["role"] != "assistant" {
        return Err(String::from("assistant response did not complete normally"));
    }
    let mut calls = Vec::new();
    let mut content = match protocol {
        ProviderProtocol::AnthropicCompatible => decode_anthropic_blocks(&message, &mut calls)?,
        ProviderProtocol::OpenAiCompatible | ProviderProtocol::Ollama => {
            if let Some(raw_calls) = message.get("tool_calls").filter(|calls| !calls.is_null()) {
                let raw_calls = raw_calls.as_array().ok_or_else(invalid)?;
                if raw_calls.len() > 32 {
                    return Err(String::from(
                        "assistant response exceeded the tool call count limit",
                    ));
                }
                let mut missing_id_nonce = None;
                for (index, raw) in raw_calls.iter().enumerate() {
                    if raw.get("type").is_some_and(|value| value != "function")
                        || (protocol == ProviderProtocol::OpenAiCompatible
                            && raw["type"] != "function")
                    {
                        return Err(invalid());
                    }
                    let id = match raw.get("id") {
                        None | Some(Value::Null) if protocol == ProviderProtocol::Ollama => {
                            // A compacted history can keep the same length for
                            // many turns. Identity belongs to this response.
                            let nonce = missing_id_nonce.get_or_insert_with(uuid::Uuid::new_v4);
                            format!("ollama_{}_{index}", nonce.simple())
                        }
                        Some(Value::String(id)) => id.clone(),
                        _ => return Err(invalid()),
                    };
                    let name = raw["function"]["name"]
                        .as_str()
                        .ok_or_else(invalid)?
                        .to_owned();
                    let arguments = raw["function"]
                        .get("arguments")
                        .cloned()
                        .unwrap_or(Value::Null);
                    let arguments = if protocol == ProviderProtocol::OpenAiCompatible {
                        match arguments {
                            Value::String(raw) => {
                                serde_json::from_str(&raw).unwrap_or(Value::String(raw))
                            }
                            _ => return Err(invalid()),
                        }
                    } else {
                        arguments
                    };
                    calls.push(AssistantToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
            }
            match message.get("content") {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(content)) => content.clone(),
                Some(Value::Array(parts)) if protocol == ProviderProtocol::OpenAiCompatible => {
                    let mut texts = Vec::new();
                    for part in parts {
                        if part["type"] != "text" {
                            return Err(invalid());
                        }
                        texts.push(part["text"].as_str().ok_or_else(invalid)?);
                    }
                    texts.join("\n")
                }
                _ => return Err(invalid()),
            }
        }
    };
    if protocol == ProviderProtocol::OpenAiCompatible {
        let refusal = match message.get("refusal") {
            None | Some(Value::Null) => "",
            Some(Value::String(text)) => text,
            _ => return Err(invalid()),
        };
        if !refusal.trim().is_empty() {
            if !calls.is_empty() {
                return Err(String::from(
                    "assistant response included both refusal text and tool calls",
                ));
            }
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(refusal);
        }
    }
    if protocol != ProviderProtocol::Ollama
        && matches!(finish, Some("tool_calls" | "tool_use")) == calls.is_empty()
    {
        return Err(String::from(
            "assistant response stop reason does not match its tool calls",
        ));
    }
    if content.trim().is_empty() && calls.is_empty() {
        return Err(String::from(
            "assistant response did not include message text or tool calls",
        ));
    }
    validate_new_calls(&calls, &HashSet::new())?;
    Ok(AssistantToolReply {
        content,
        calls,
        raw_message: message,
    })
}

fn decode_anthropic_blocks(
    message: &Value,
    calls: &mut Vec<AssistantToolCall>,
) -> Result<String, String> {
    let blocks = message["content"].as_array().ok_or_else(invalid)?;
    let mut texts = Vec::new();
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => texts.push(block["text"].as_str().ok_or_else(invalid)?),
            Some("tool_use") => calls.push(AssistantToolCall {
                id: block["id"].as_str().ok_or_else(invalid)?.to_owned(),
                name: block["name"].as_str().ok_or_else(invalid)?.to_owned(),
                arguments: block.get("input").cloned().unwrap_or(Value::Null),
            }),
            Some("thinking") => {
                if !block["thinking"].is_string()
                    || block["signature"].as_str().is_none_or(str::is_empty)
                {
                    return Err(invalid());
                }
            }
            Some("redacted_thinking") => {
                if block["data"].as_str().is_none_or(str::is_empty) {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    Ok(texts.join("\n"))
}
