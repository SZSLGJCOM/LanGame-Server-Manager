use super::{
    AssistantProviderSettings, AssistantRunInput, AssistantToolDefinition, AssistantToolMessage,
    AssistantToolReply, ProviderProtocol, run_assistant_tool_turn_streaming,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;
use uuid::Uuid;

const CHECK_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_CONCURRENT_CHECKS: usize = 2;
const MAX_PENDING_CANCELLATIONS: usize = 64;
const CHECK_TOOL: &str = "lan_connection_check";
const CHECK_SYSTEM: &str = "Run a synthetic model connection check. No game, server, file, account or conversation data is available. Follow the current check instructions exactly and keep text brief.";

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantConnectionCheckInput {
    pub request_id: String,
    pub settings: AssistantProviderSettings,
}

impl std::fmt::Debug for AssistantConnectionCheckInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AssistantConnectionCheckInput")
            .field("request_id", &self.request_id)
            .field("settings", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AssistantConnectionStageStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantConnectionStage {
    pub status: AssistantConnectionStageStatus,
    /// A fixed diagnostic code, never provider text or credentials.
    pub diagnostic: Option<String>,
    pub latency_ms: u64,
}

impl AssistantConnectionStage {
    fn skipped(reason: &str) -> Self {
        Self {
            status: AssistantConnectionStageStatus::Skipped,
            diagnostic: Some(reason.into()),
            latency_ms: 0,
        }
    }

    fn completed(started: Instant, failure: Option<&ProbeFailure>) -> Self {
        Self {
            status: if failure.is_some() {
                AssistantConnectionStageStatus::Failed
            } else {
                AssistantConnectionStageStatus::Passed
            },
            diagnostic: failure.map(|failure| failure.diagnostic.into()),
            latency_ms: elapsed_ms(started),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantConnectionCheckOutput {
    pub request_id: String,
    pub provider: String,
    pub model: String,
    pub endpoint_url: String,
    pub chat: AssistantConnectionStage,
    pub tool_call: AssistantConnectionStage,
    pub tool_replay: AssistantConnectionStage,
    pub request_count: u32,
    pub elapsed_ms: u64,
    pub cancelled: bool,
}

#[derive(Default)]
struct CheckRegistry {
    state: Mutex<RegistryState>,
}

#[derive(Default)]
struct RegistryState {
    active: HashMap<Uuid, watch::Sender<bool>>,
    pending_cancellations: HashMap<Uuid, Instant>,
}

impl RegistryState {
    fn expire_pending_cancellations(&mut self) {
        let now = Instant::now();
        self.pending_cancellations
            .retain(|_, deadline| *deadline > now);
    }
}

impl CheckRegistry {
    fn begin(&self, id: Uuid) -> Result<CheckLease<'_>, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "assistant connection check registry is unavailable")?;
        state.expire_pending_cancellations();
        if state.active.contains_key(&id) {
            return Err("assistant connection check request ID is already active".into());
        }
        if state.active.len() >= MAX_CONCURRENT_CHECKS {
            return Err("assistant connection checks are busy; try again when one finishes".into());
        }
        let cancelled = state.pending_cancellations.remove(&id).is_some();
        let (sender, receiver) = watch::channel(cancelled);
        state.active.insert(id, sender);
        Ok(CheckLease {
            registry: self,
            id,
            receiver,
        })
    }

    fn cancel(&self, id: Uuid) -> Result<bool, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "assistant connection check registry is unavailable")?;
        state.expire_pending_cancellations();
        if let Some(sender) = state.active.get(&id) {
            sender.send_replace(true);
        } else {
            // Independent start/cancel IPC requests can arrive out of order.
            // A bounded, short-lived marker belongs only to this check UUID;
            // begin consumes it before any credential lookup or model request.
            if !state.pending_cancellations.contains_key(&id)
                && state.pending_cancellations.len() >= MAX_PENDING_CANCELLATIONS
            {
                return Err("assistant connection cancellation queue is full; try again when pending checks settle".into());
            }
            state
                .pending_cancellations
                .insert(id, Instant::now() + CHECK_TIMEOUT);
        }
        Ok(true)
    }
}

struct CheckLease<'a> {
    registry: &'a CheckRegistry,
    id: Uuid,
    receiver: watch::Receiver<bool>,
}

impl Drop for CheckLease<'_> {
    fn drop(&mut self) {
        let mut state = self
            .registry
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.active.remove(&self.id);
    }
}

fn registry() -> &'static CheckRegistry {
    static REGISTRY: OnceLock<CheckRegistry> = OnceLock::new();
    REGISTRY.get_or_init(CheckRegistry::default)
}

fn parse_request_id(value: &str) -> Result<Uuid, String> {
    let parsed = Uuid::parse_str(value)
        .map_err(|_| "assistant connection check request ID must be a UUID")?;
    if parsed.is_nil() || !parsed.to_string().eq_ignore_ascii_case(value) {
        return Err(
            "assistant connection check request ID must be a nonempty canonical UUID".into(),
        );
    }
    Ok(parsed)
}

/// Checks the production streaming protocol with three isolated synthetic turns.
/// Dropping this future closes its HTTP body and releases its local admission.
pub async fn check_assistant_connection(
    input: &AssistantConnectionCheckInput,
) -> Result<AssistantConnectionCheckOutput, String> {
    check_with_registry(input, registry(), CHECK_TIMEOUT).await
}

/// True acknowledges active cancellation or a bounded marker for a start request
/// that has not arrived yet. Each new check must use a fresh request UUID.
pub fn cancel_assistant_connection_check(request_id: &str) -> Result<bool, String> {
    registry().cancel(parse_request_id(request_id)?)
}

async fn check_with_registry(
    input: &AssistantConnectionCheckInput,
    registry: &CheckRegistry,
    timeout: Duration,
) -> Result<AssistantConnectionCheckOutput, String> {
    let id = parse_request_id(&input.request_id)?;
    let protocol = ProviderProtocol::parse(&input.settings.provider)?;
    let model = input.settings.model.trim();
    if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
        return Err("assistant connection check requires a valid model name".into());
    }
    let endpoint_url = protocol.tool_endpoint(&input.settings.base_url)?;
    let mut lease = registry.begin(id)?;
    let started = Instant::now();
    let deadline = started + timeout;
    let mut output = AssistantConnectionCheckOutput {
        request_id: id.to_string(),
        provider: protocol.as_str().into(),
        model: model.into(),
        endpoint_url,
        chat: AssistantConnectionStage::skipped("chat_not_ready"),
        tool_call: AssistantConnectionStage::skipped("chat_not_ready"),
        tool_replay: AssistantConnectionStage::skipped("tool_call_not_ready"),
        request_count: 0,
        elapsed_ms: 0,
        cancelled: false,
    };
    let run_input = AssistantRunInput {
        settings: input.settings.clone(),
        prompt_label: "connection-check".into(),
        prompt: String::new(),
        context: String::new(),
    };
    run_check(&run_input, &mut lease.receiver, deadline, &mut output).await;
    output.elapsed_ms = elapsed_ms(started);
    Ok(output)
}

struct ProbeFailure {
    diagnostic: &'static str,
    cancelled: bool,
}

impl ProbeFailure {
    fn new(diagnostic: &'static str) -> Self {
        Self {
            diagnostic,
            cancelled: false,
        }
    }

    fn skip_reason(&self, default: &'static str) -> &'static str {
        if self.cancelled || self.diagnostic == "overall_timeout" {
            self.diagnostic
        } else {
            default
        }
    }
}

async fn run_check(
    input: &AssistantRunInput,
    cancellation: &mut watch::Receiver<bool>,
    deadline: Instant,
    output: &mut AssistantConnectionCheckOutput,
) {
    let chat_started = Instant::now();
    let chat_messages = [AssistantToolMessage::User(
        "Reply with one short sentence confirming that you can answer this synthetic connection check. Do not call any tool.".into(),
    )];
    let chat = probe_turn(input, &chat_messages, &[], cancellation, deadline, output).await;
    let chat = chat.and_then(|reply| {
        if reply.calls.is_empty() && !reply.content.trim().is_empty() {
            Ok(())
        } else {
            Err(ProbeFailure::new("chat_response_invalid"))
        }
    });
    output.chat = AssistantConnectionStage::completed(chat_started, chat.as_ref().err());
    if let Err(failure) = chat {
        output.cancelled = failure.cancelled;
        output.tool_call = AssistantConnectionStage::skipped(failure.skip_reason("chat_not_ready"));
        output.tool_replay =
            AssistantConnectionStage::skipped(failure.skip_reason("chat_not_ready"));
        return;
    }

    let nonce = Uuid::new_v4().to_string();
    let tool = AssistantToolDefinition {
        name: CHECK_TOOL.into(),
        description: "A synthetic connection check that echoes a nonce and returns a fresh receipt. It has no side effects and accesses no external data.".into(),
        parameters: json!({"type":"object","properties":{"nonce":{"type":"string","const":nonce}},"required":["nonce"],"additionalProperties":false}),
    };
    let mut messages = vec![AssistantToolMessage::User(format!(
        "Call {CHECK_TOOL} exactly once with nonce {nonce}. Do not substitute text for the call. After the tool result arrives, reply with exactly its receipt value and no other text. The receipt is not available until the tool returns it."
    ))];
    let tools = [tool];
    let call_started = Instant::now();
    let reply = probe_turn(input, &messages, &tools, cancellation, deadline, output).await;
    let reply = reply.and_then(|reply| {
        if reply.calls.is_empty() {
            return Err(ProbeFailure::new("tool_not_called"));
        }
        if reply.calls.len() != 1
            || reply.calls[0].name != CHECK_TOOL
            || reply.calls[0].arguments != json!({"nonce":nonce})
        {
            return Err(ProbeFailure::new("tool_call_invalid"));
        }
        Ok(reply)
    });
    output.tool_call = AssistantConnectionStage::completed(call_started, reply.as_ref().err());
    let reply = match reply {
        Ok(reply) => reply,
        Err(failure) => {
            output.cancelled = failure.cancelled;
            output.tool_replay =
                AssistantConnectionStage::skipped(failure.skip_reason("tool_call_not_ready"));
            return;
        }
    };

    // This second challenge exists only in the tool result. Echoing the initial
    // call nonce cannot falsely prove that native tool-result replay succeeded.
    let receipt = Uuid::new_v4().to_string();
    let call_id = reply.calls[0].id.clone();
    messages.push(AssistantToolMessage::Assistant(reply));
    messages.push(AssistantToolMessage::ToolResult {
        call_id,
        name: CHECK_TOOL.into(),
        content: json!({"ok":true,"nonce":nonce,"receipt":receipt}).to_string(),
        is_error: false,
    });
    let replay_started = Instant::now();
    let replay = probe_turn(input, &messages, &tools, cancellation, deadline, output).await;
    let replay = replay.and_then(|reply| {
        if !reply.calls.is_empty() || reply.content.trim() != receipt {
            Err(ProbeFailure::new("tool_result_not_consumed"))
        } else {
            Ok(())
        }
    });
    output.tool_replay = AssistantConnectionStage::completed(replay_started, replay.as_ref().err());
    if let Err(failure) = replay {
        output.cancelled = failure.cancelled;
    }
}

async fn probe_turn(
    input: &AssistantRunInput,
    messages: &[AssistantToolMessage],
    tools: &[AssistantToolDefinition],
    cancellation: &mut watch::Receiver<bool>,
    deadline: Instant,
    output: &mut AssistantConnectionCheckOutput,
) -> Result<AssistantToolReply, ProbeFailure> {
    if *cancellation.borrow() {
        return Err(ProbeFailure {
            diagnostic: "request_cancelled",
            cancelled: true,
        });
    }
    if Instant::now() >= deadline {
        return Err(ProbeFailure::new("overall_timeout"));
    }
    output.request_count += 1;
    let observer = |_text: &str| Ok(());
    tokio::select! {
        biased;
        _ = cancellation.changed() => Err(ProbeFailure {
            diagnostic: "request_cancelled",
            cancelled: true,
        }),
        _ = tokio::time::sleep_until(deadline) => Err(ProbeFailure::new("overall_timeout")),
        reply = run_assistant_tool_turn_streaming(input, CHECK_SYSTEM, messages, tools, &observer) => {
            reply.map_err(|error| ProbeFailure::new(safe_diagnostic(&error)))
        }
    }
}

fn safe_diagnostic(error: &str) -> &'static str {
    if let Some(status) = error.strip_prefix("assistant provider returned ") {
        return match status.split_whitespace().next() {
            Some("401" | "403") => "authentication_rejected",
            Some("404") => "endpoint_not_found",
            Some("429") => "rate_limited",
            Some(code) if code.starts_with('5') => "provider_unavailable",
            _ => "request_failed",
        };
    }
    if error.starts_with("failed to read assistant credential:")
        || error.starts_with("failed to create assistant credential entry:")
    {
        return "credential_unavailable";
    }
    if error.starts_with("assistant request failed:")
        || error.starts_with("assistant stream failed:")
        || error.starts_with("failed to read assistant provider response:")
    {
        return if error.contains("timed out") || error.contains("timeout") {
            "request_timeout"
        } else {
            "request_failed"
        };
    }
    "invalid_response"
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "assistant_connection_tests.rs"]
mod tests;
