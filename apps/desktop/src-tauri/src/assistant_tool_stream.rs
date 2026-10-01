use super::*;
use crate::assistant::{ASSISTANT_MAX_RESPONSE_BYTES, tool_conversation::AssistantTextObserver};

#[path = "assistant_tool_stream_anthropic.rs"]
mod anthropic;
#[path = "assistant_tool_stream_chat.rs"]
mod chat;

// SSE/NDJSON repeats an envelope for each token. Bound that transport separately
// from the 256 KiB retained response, while limiting individual frames and the
// number of parsed events. The wire ceiling allows ~1 KiB per bounded event;
// ignored metadata and keepalives also consume it.
const MAX_STREAM_EVENTS: usize = 16 * 1024;
const MAX_STREAM_WIRE_BYTES: usize = MAX_STREAM_EVENTS * 1024;
const MAX_STREAM_FRAME_BYTES: usize = ASSISTANT_MAX_RESPONSE_BYTES;

fn invalid() -> String {
    "assistant provider returned an invalid or incomplete stream".into()
}

pub(super) fn retain_bytes(retained: &mut usize, count: usize) -> Result<(), String> {
    let total = retained.checked_add(count).ok_or_else(invalid)?;
    if total > ASSISTANT_MAX_RESPONSE_BYTES {
        return Err(format!(
            "assistant stream exceeded the retained response byte limit (retained_bytes={retained}, incoming_bytes={count}, limit={ASSISTANT_MAX_RESPONSE_BYTES})"
        ));
    }
    *retained = total;
    Ok(())
}

pub(super) fn retain_value(retained: &mut usize, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    retain_bytes(retained, bytes.len())
}

pub(super) fn append_string(
    target: &mut Value,
    field: &str,
    value: &Value,
    retained: &mut usize,
) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let text = value.as_str().ok_or_else(invalid)?;
    retain_bytes(retained, text.len())?;
    if target.get(field).is_none() || target[field].is_null() {
        target[field] = json!("");
    }
    let Value::String(existing) = &mut target[field] else {
        return Err(invalid());
    };
    if existing.len().saturating_add(text.len()) > ASSISTANT_MAX_RESPONSE_BYTES {
        return Err(invalid());
    }
    existing.push_str(text);
    Ok(())
}

enum Accumulator {
    Chat(chat::ChatStream),
    Anthropic(anthropic::AnthropicStream),
}

impl Accumulator {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Chat(stream) => stream.retained_bytes,
            Self::Anthropic(stream) => stream.retained_bytes,
        }
    }

    fn event(&mut self, event: &str, observer: &AssistantTextObserver<'_>) -> Result<(), String> {
        match self {
            Self::Chat(stream) => stream.event(event, observer),
            Self::Anthropic(stream) => stream.event(event, observer),
        }
    }

    fn finish(self) -> Result<Value, String> {
        match self {
            Self::Chat(stream) => stream.finish(),
            Self::Anthropic(stream) => stream.finish(),
        }
    }
}

/// Framing happens on bytes, so split UTF-8 code points are not decoded early.
/// Transport overhead, pending frames and retained payload have separate bounds.
struct ToolStream {
    protocol: ProviderProtocol,
    bytes: usize,
    events: usize,
    line: Vec<u8>,
    event: String,
    accumulator: Accumulator,
}

impl ToolStream {
    fn new(protocol: ProviderProtocol) -> Self {
        Self {
            protocol,
            bytes: 0,
            events: 0,
            line: Vec::new(),
            event: String::new(),
            accumulator: if protocol == ProviderProtocol::AnthropicCompatible {
                Accumulator::Anthropic(anthropic::AnthropicStream::default())
            } else {
                Accumulator::Chat(chat::ChatStream::new(protocol))
            },
        }
    }

    fn push(&mut self, chunk: &[u8], observer: &AssistantTextObserver<'_>) -> Result<(), String> {
        self.bytes = self.bytes.checked_add(chunk.len()).ok_or_else(invalid)?;
        if self.bytes > MAX_STREAM_WIRE_BYTES {
            return Err(self.limit_error("transport byte"));
        }
        for &byte in chunk {
            if byte == b'\n' {
                self.line(observer)?;
            } else {
                if self.line.len() == MAX_STREAM_FRAME_BYTES {
                    return Err(self.limit_error("frame byte"));
                }
                self.line.push(byte);
            }
        }
        Ok(())
    }

    fn dispatch(
        &mut self,
        event: &str,
        observer: &AssistantTextObserver<'_>,
    ) -> Result<(), String> {
        if self.events == MAX_STREAM_EVENTS {
            return Err(self.limit_error("event count"));
        }
        self.events += 1;
        self.accumulator.event(event, observer).map_err(|error| {
            if error.starts_with("assistant stream exceeded the retained response byte limit") {
                format!("{error}; wire_bytes={}, events={}", self.bytes, self.events)
            } else {
                error
            }
        })
    }

    fn limit_error(&self, limit: &str) -> String {
        format!(
            "assistant stream exceeded the {limit} limit (wire_bytes={}, retained_bytes={}, pending_frame_bytes={}, events={})",
            self.bytes,
            self.accumulator.retained_bytes(),
            self.line.len() + self.event.len(),
            self.events
        )
    }

    fn line(&mut self, observer: &AssistantTextObserver<'_>) -> Result<(), String> {
        let bytes = std::mem::take(&mut self.line);
        let line = std::str::from_utf8(&bytes)
            .map_err(|_| invalid())?
            .trim_end_matches('\r');
        if self.protocol == ProviderProtocol::Ollama {
            if !line.trim().is_empty() {
                self.dispatch(line, observer)?;
            }
        } else if line.is_empty() {
            if !self.event.is_empty() {
                let event = std::mem::take(&mut self.event);
                self.dispatch(event.trim_end_matches('\n'), observer)?;
            }
        } else if let Some(data) = line.strip_prefix("data:") {
            let data = data.strip_prefix(' ').unwrap_or(data);
            if self
                .event
                .len()
                .saturating_add(data.len())
                .saturating_add(1)
                > MAX_STREAM_FRAME_BYTES
            {
                return Err(self.limit_error("frame byte"));
            }
            self.event.push_str(data);
            self.event.push('\n');
        } else if !line.starts_with(':')
            && !line.starts_with("event:")
            && !line.starts_with("id:")
            && !line.starts_with("retry:")
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn finish(
        mut self,
        observer: &AssistantTextObserver<'_>,
    ) -> Result<AssistantToolReply, String> {
        if !self.line.is_empty() {
            self.line(observer)?;
        }
        if !self.event.is_empty() {
            let event = std::mem::take(&mut self.event);
            self.dispatch(event.trim_end_matches('\n'), observer)?;
        }
        let payload = self.accumulator.finish()?;
        let bytes = serde_json::to_vec(&payload).map_err(|_| invalid())?;
        if bytes.len() > ASSISTANT_MAX_RESPONSE_BYTES {
            return Err(format!(
                "assistant stream exceeded the retained response byte limit (wire_bytes={}, response_bytes={}, events={})",
                self.bytes,
                bytes.len(),
                self.events
            ));
        }
        let reply = self.protocol.decode_tool_response(&bytes)?;
        // A finished stream cannot expose a partial argument string as a call.
        if reply.calls.iter().any(|call| !call.arguments.is_object()) {
            return Err(invalid());
        }
        Ok(reply)
    }
}

impl ProviderProtocol {
    pub(in crate::assistant) async fn read_tool_stream(
        self,
        mut response: reqwest::Response,
        observer: &AssistantTextObserver<'_>,
    ) -> Result<AssistantToolReply, String> {
        // Some compatible gateways return a completed JSON response even when
        // stream=true. Accept that explicit media type without retrying a request.
        if self != Self::Ollama
            && response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    value
                        .split(';')
                        .next()
                        .is_some_and(|mime| mime.trim() == "application/json")
                })
        {
            let body = crate::assistant::read_assistant_response_body(response).await?;
            let reply = self.decode_tool_response(&body)?;
            if !reply.content.is_empty() {
                observer(&reply.content)?;
            }
            return Ok(reply);
        }
        let mut stream = ToolStream::new(self);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("assistant stream failed: {error}"))?
        {
            stream.push(&chunk, observer)?;
        }
        stream.finish(observer)
    }
}

#[cfg(test)]
#[path = "assistant_tool_stream_tests.rs"]
mod tests;
