use super::commands_runtime_lifecycle::RuntimeTransportDispatch;
use super::*;

#[path = "commands_broadcast_startup.rs"]
mod startup;

#[cfg(test)]
#[path = "commands_broadcast_tests.rs"]
mod tests;

pub(super) fn normalize_broadcast_source(source: Option<&str>) -> String {
    let normalized = source
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual")
        .to_ascii_lowercase()
        .replace('-', "_");
    match normalized.as_str() {
        "manual" | "startup" | "shutdown" | "runtime_health" | "periodic" => normalized,
        _ => String::from("manual"),
    }
}

pub(super) fn normalize_broadcast_initiator(initiator: Option<&str>, source: &str) -> String {
    let normalized = initiator
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(if source == "manual" { "manual" } else { "auto" })
        .to_ascii_lowercase()
        .replace('-', "_");
    match normalized.as_str() {
        "manual" | "auto" | "lifecycle" | "system" => normalized,
        "life_cycle" => String::from("lifecycle"),
        _ if source == "manual" => String::from("manual"),
        _ => String::from("auto"),
    }
}

pub(super) fn normalize_broadcast_policy_snapshot(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| summarize_text(value, 4096))
}

pub(super) fn validate_instance_broadcast_message(message: &str) -> Result<String, String> {
    let normalized = message.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return Err(String::from("Enter a broadcast message first."));
    }
    if trimmed.contains('\n') {
        return Err(String::from("Broadcast messages must be a single line."));
    }
    if trimmed.chars().count() > 240 {
        return Err(String::from(
            "Broadcast messages are limited to 240 characters.",
        ));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(String::from(
            "Broadcast messages cannot contain control characters.",
        ));
    }
    for fragment in [";", "&&", "||", "`", "$(", "{{", "}}"] {
        if trimmed.contains(fragment) {
            return Err(format!(
                "Broadcast message contains unsupported command syntax `{fragment}`."
            ));
        }
    }
    Ok(trimmed.to_string())
}

pub(super) fn find_broadcast_action(
    descriptor: &ModuleDescriptor,
) -> Result<&ModulePlayerActionSpec, String> {
    descriptor
        .runtime
        .player_actions
        .iter()
        .find(|action| action.kind.as_deref() == Some("broadcast"))
        .ok_or_else(|| {
            format!(
                "module `{}` does not declare a broadcast runtime action",
                descriptor.summary.id
            )
        })
}

pub(super) fn encode_broadcast_placeholder(
    action: &ModulePlayerActionSpec,
    message: &str,
) -> String {
    if action.target_encoding.as_deref() == Some("quoted_string") {
        let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        message.to_string()
    }
}

pub(super) fn render_broadcast_command(
    action: &ModulePlayerActionSpec,
    message: &str,
) -> Result<String, String> {
    if action.transport == "palworld_rest" {
        return crate::live_players::palworld_rest::render_command(
            action,
            Some(message),
            None,
            None,
            false,
        );
    }
    let encoded_message = encode_broadcast_placeholder(action, message);
    let command = if action.command_template.contains("{{message}}") {
        action
            .command_template
            .replace("{{message}}", &encoded_message)
    } else if action.command_template.contains("{{target}}") {
        action
            .command_template
            .replace("{{target}}", &encoded_message)
    } else {
        return Err(format!(
            "broadcast action `{}` must contain `{{{{message}}}}` or `{{{{target}}}}`",
            action.id
        ));
    };
    normalize_runtime_command_input(&command)
}

pub(super) fn broadcast_generation_prompt(
    details: &InstanceDetails,
    intent: &str,
    tone: &str,
    source: &str,
) -> String {
    format!(
        "Generate exactly one in-game server broadcast message. Keep it short, single-line, and ready to send. Do not include quotes, markdown, labels, commands, or explanations.\nInstance: {}\nModule: {}\nStatus: {:?}\nSource: {}\nTone: {}\nHost intent: {}",
        details.summary.name,
        details.summary.module_id,
        details.summary.status,
        source,
        tone,
        intent.trim()
    )
}

pub(super) const BROADCAST_SYSTEM_PROMPT: &str = "You generate exactly one in-game server broadcast line. Return only the message text. Use the user's language when clear. Do not include markdown, labels, explanations, command names, quotes, or multiple options.";

pub(super) fn normalize_ai_broadcast_candidate(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");

    normalized
        .lines()
        .map(normalize_ai_broadcast_line)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

pub(super) fn normalize_ai_broadcast_line(line: &str) -> String {
    let mut candidate = unwrap_ai_broadcast_markdown_label(line.trim());

    for _ in 0..4 {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        if let Some(stripped) = strip_ai_broadcast_label_prefix(trimmed) {
            candidate = stripped.trim().to_string();
            continue;
        }

        return trim_broadcast_wrappers(trimmed).to_string();
    }

    trim_broadcast_wrappers(candidate.trim()).to_string()
}

pub(super) fn unwrap_ai_broadcast_markdown_label(line: &str) -> String {
    let trimmed = line.trim().trim_start_matches('#').trim();
    if let Some(after_marker) = trimmed.strip_prefix("**")
        && let Some(end_index) = after_marker.find("**")
    {
        let (label, rest_with_marker) = after_marker.split_at(end_index);
        let rest = &rest_with_marker[2..];
        return format!("{label}{rest}");
    }

    trimmed.to_string()
}

pub(super) fn strip_ai_broadcast_label_prefix(line: &str) -> Option<&str> {
    const LABELS: &[&str] = &[
        "conclusion",
        "broadcast",
        "broadcast message",
        "message",
        "server message",
        "output",
        "result",
        "final",
        "\u{7ed3}\u{8bba}",
        "\u{5e7f}\u{64ad}",
        "\u{6d88}\u{606f}",
        "\u{5185}\u{5bb9}",
    ];

    LABELS
        .iter()
        .find_map(|label| strip_broadcast_label(line, label))
}

pub(super) fn strip_broadcast_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let trimmed = line.trim();
    let has_label = if label.is_ascii() {
        trimmed.to_ascii_lowercase().starts_with(label)
    } else {
        trimmed.starts_with(label)
    };
    if !has_label {
        return None;
    }

    let rest = trimmed.get(label.len()..)?.trim_start();
    if rest.is_empty() {
        return Some("");
    }

    let mut chars = rest.chars();
    let delimiter = chars.next()?;
    if matches!(delimiter, ':' | '\u{ff1a}' | '-') {
        return Some(chars.as_str().trim_start());
    }

    None
}

pub(super) fn trim_broadcast_wrappers(value: &str) -> &str {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .trim()
}

#[tauri::command]
pub async fn generate_instance_broadcast(
    state: tauri::State<'_, DesktopState>,
    input: GenerateInstanceBroadcastInput,
) -> Result<GenerateInstanceBroadcastOutput, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("broadcast event generation")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;

    let details = read_instance_details(&storage.paths, &input.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let source = normalize_broadcast_source(input.source.as_deref());
    let initiator = normalize_broadcast_initiator(input.initiator.as_deref(), &source);
    let policy_snapshot_json =
        normalize_broadcast_policy_snapshot(input.policy_snapshot_json.as_deref());
    let tone = input
        .tone
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("short");
    let intent = input.intent.trim();
    if intent.is_empty() {
        return Err(String::from("Enter a broadcast intent first."));
    }

    let prompt = broadcast_generation_prompt(&details, intent, tone, &source);
    let assistant_input = AssistantRunInput {
        settings: input.settings,
        prompt_label: String::from("AI Broadcast"),
        prompt,
        context: String::from("Generate one safe in-game broadcast line."),
    };

    let output =
        match run_assistant_with_system_prompt(&assistant_input, BROADCAST_SYSTEM_PROMPT).await {
            Ok(output) => output,
            Err(message) => {
                let _ = insert_instance_broadcast_event(
                    &storage.paths,
                    InsertInstanceBroadcastEventInput {
                        instance_id: details.summary.id.clone(),
                        module_id: details.summary.module_id.clone(),
                        source: source.clone(),
                        rule_id: input.rule_id.clone(),
                        message: summarize_text(intent, 240),
                        ai_provider: Some(assistant_input.settings.provider.clone()),
                        ai_model: Some(assistant_input.settings.model.clone()),
                        action_id: None,
                        transport: None,
                        command_preview: None,
                        status: String::from("failed"),
                        response_text: None,
                        error_message: Some(message.clone()),
                        initiator: Some(initiator.clone()),
                        policy_snapshot_json: policy_snapshot_json.clone(),
                    },
                )
                .await;
                append_desktop_app_log(
                    &storage,
                    "error",
                    "instance.broadcast.generate_failed",
                    &message,
                    json!({
                        "instance_id": details.summary.id.as_str(),
                        "module_id": details.summary.module_id.as_str(),
                        "source": source.as_str(),
                    }),
                );
                return Err(logged_error_message(&storage, message));
            }
        };
    let candidate = normalize_ai_broadcast_candidate(&output.content);
    let message = match validate_instance_broadcast_message(&candidate) {
        Ok(message) => message,
        Err(message) => {
            let _ = insert_instance_broadcast_event(
                &storage.paths,
                InsertInstanceBroadcastEventInput {
                    instance_id: details.summary.id.clone(),
                    module_id: details.summary.module_id.clone(),
                    source: source.clone(),
                    rule_id: input.rule_id.clone(),
                    message: summarize_text(&candidate, 240),
                    ai_provider: Some(output.provider.clone()),
                    ai_model: Some(output.model.clone()),
                    action_id: None,
                    transport: None,
                    command_preview: None,
                    status: String::from("failed"),
                    response_text: Some(summarize_text(&output.content, 240)),
                    error_message: Some(message.clone()),
                    initiator: Some(initiator.clone()),
                    policy_snapshot_json: policy_snapshot_json.clone(),
                },
            )
            .await;
            return Err(message);
        }
    };

    let event = insert_instance_broadcast_event(
        &storage.paths,
        InsertInstanceBroadcastEventInput {
            instance_id: details.summary.id.clone(),
            module_id: details.summary.module_id.clone(),
            source,
            rule_id: input.rule_id,
            message: message.clone(),
            ai_provider: Some(output.provider.clone()),
            ai_model: Some(output.model.clone()),
            action_id: None,
            transport: None,
            command_preview: None,
            status: String::from("generated"),
            response_text: None,
            error_message: None,
            initiator: Some(initiator),
            policy_snapshot_json,
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    append_desktop_app_log(
        &storage,
        "info",
        "instance.broadcast.generated",
        "AI broadcast message generated",
        json!({
            "instance_id": details.summary.id.as_str(),
            "module_id": details.summary.module_id.as_str(),
            "source": event.source.as_str(),
            "message_excerpt": summarize_text(&message, 160),
        }),
    );

    Ok(GenerateInstanceBroadcastOutput {
        message,
        provider: output.provider,
        model: output.model,
        endpoint_url: output.endpoint_url,
        event,
    })
}

#[tauri::command]
pub async fn send_instance_broadcast(
    state: tauri::State<'_, DesktopState>,
    input: SendInstanceBroadcastInput,
) -> Result<SendInstanceBroadcastOutput, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("broadcast event dispatch")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;

    let source = normalize_broadcast_source(input.source.as_deref());
    let initiator = normalize_broadcast_initiator(input.initiator.as_deref(), &source);
    let wait_for_startup_channel = startup::should_wait_for_broadcast_channel(&source, &initiator);
    // Ordinary broadcasts share the stop lock through transport confirmation.
    // Startup broadcasts acquire it only after their readiness wait completes.
    let _instance_lock = if wait_for_startup_channel {
        None
    } else {
        Some(state.acquire_instance_mutation(&input.instance_id).await)
    };
    let details = read_instance_details(&storage.paths, &input.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if !matches!(details.summary.status, InstanceStatus::Running) {
        return Err(String::from(
            "Start the instance before sending a broadcast.",
        ));
    }

    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &details.summary.module_id)?;
    let action = find_broadcast_action(descriptor)?;
    let message = validate_instance_broadcast_message(&input.message)?;
    let command = render_broadcast_command(action, &message)?;
    let policy_snapshot_json =
        normalize_broadcast_policy_snapshot(input.policy_snapshot_json.as_deref());
    let transport = action.transport.clone();

    // Persist before submission. An interrupted application leaves an honest
    // pending record and never retries a potentially delivered broadcast.
    let mut event = insert_instance_broadcast_event(
        &storage.paths,
        InsertInstanceBroadcastEventInput {
            instance_id: details.summary.id.clone(),
            module_id: details.summary.module_id.clone(),
            source,
            rule_id: input.rule_id,
            message,
            ai_provider: input.ai_provider,
            ai_model: input.ai_model,
            action_id: Some(action.id.clone()),
            transport: Some(transport.clone()),
            command_preview: Some(command.clone()),
            status: String::from("pending"),
            response_text: None,
            error_message: None,
            initiator: Some(initiator),
            policy_snapshot_json,
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    let request = RuntimeTransportRequest {
        command: &command,
        transport: &transport,
        process_key: action.process_key.as_deref(),
        port_name: action.port_name.as_deref(),
        password_setting_key: action.password_setting_key.as_deref(),
        enabled_setting_key: action.enabled_setting_key.as_deref(),
    };
    let dispatch_result = if wait_for_startup_channel {
        startup::dispatch_startup_broadcast(&state, &details, descriptor, &request).await
    } else {
        dispatch_instance_runtime_transport(&state, &details, &request).await
    };

    match dispatch_result {
        Ok(RuntimeTransportDispatch::Confirmed) => {
            complete_broadcast_event(&storage, &mut event, Ok(())).await?;
        }
        Ok(RuntimeTransportDispatch::Pending(completion)) => {
            let pending_storage = storage.clone();
            let mut pending_event = event.clone();
            // Keep this storage context alive until the original writer finishes.
            // The retained completion also covers a writer finishing before spawn.
            tauri::async_runtime::spawn(async move {
                let _storage_context_operation = _storage_context_operation;
                let result = completion.wait().await;
                // This function logs persistence failures; no command is resent.
                let _ =
                    complete_broadcast_event(&pending_storage, &mut pending_event, result).await;
            });
            append_desktop_app_log(
                &storage,
                "info",
                "instance.broadcast.pending",
                "Broadcast submitted; stdin write confirmation is pending",
                json!({ "instance_id": event.instance_id, "event_id": event.event_id }),
            );
        }
        Err(error) => {
            complete_broadcast_event(&storage, &mut event, Err(error.clone())).await?;
            return Err(logged_error_message(&storage, error));
        }
    }

    Ok(SendInstanceBroadcastOutput {
        event,
        action_id: action.id.clone(),
        transport,
        command_preview: command,
    })
}

pub(super) fn broadcast_dispatch_message(output: &SendInstanceBroadcastOutput) -> String {
    if output.event.status == "pending" {
        format!(
            "Broadcast submitted via {}; process input confirmation is pending. Check broadcast history and game logs before resending.",
            output.transport,
        )
    } else {
        format!(
            "Broadcast sent via {}. Game delivery is not independently confirmed.",
            output.transport
        )
    }
}

async fn complete_broadcast_event(
    storage: &StorageBootstrap,
    event: &mut InstanceBroadcastEvent,
    result: Result<(), String>,
) -> Result<(), String> {
    let updated = app_storage::complete_instance_broadcast_event(
        &storage.paths,
        &event.instance_id,
        &event.event_id,
        result.as_ref().map(|_| ()).map_err(String::as_str),
    )
    .await;
    match updated {
        Ok(true) => {}
        Ok(false) => {
            return Err(logged_error_message(
                storage,
                format!(
                    "Broadcast {} was submitted but its pending history record could not be finalized. Check game logs before resending.",
                    event.event_id,
                ),
            ));
        }
        Err(error) => {
            append_desktop_app_log(
                storage,
                "error",
                "instance.broadcast.history_update_failed",
                "Could not persist broadcast completion; the command will not be resent",
                json!({
                    "instance_id": event.instance_id,
                    "event_id": event.event_id,
                    "transport_confirmed": result.is_ok(),
                    "dispatch_error": result.as_ref().err(),
                    "storage_error": error.to_string(),
                }),
            );
            return Err(format!(
                "Broadcast submitted, but its history update failed: {error}. Check game logs before resending."
            ));
        }
    }
    event.status = if result.is_ok() { "sent" } else { "failed" }.into();
    event.error_message = result.err();
    append_desktop_app_log(
        storage,
        if event.error_message.is_some() {
            "error"
        } else {
            "info"
        },
        if event.error_message.is_some() {
            "instance.broadcast.failed"
        } else {
            "instance.broadcast.sent"
        },
        "Broadcast transport completed; game delivery is not independently confirmed",
        json!({
            "instance_id": event.instance_id,
            "event_id": event.event_id,
            "transport": event.transport,
            "status": event.status,
            "error": event.error_message,
        }),
    );
    Ok(())
}

#[tauri::command]
pub async fn read_instance_broadcast_policy(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<InstanceBroadcastPolicy, String> {
    let _operation = state.begin_storage_context_operation("broadcast policy read")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    read_instance_broadcast_policy_snapshot(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn update_instance_broadcast_policy(
    state: tauri::State<'_, DesktopState>,
    input: UpdateInstanceBroadcastPolicyInput,
) -> Result<InstanceBroadcastPolicy, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("broadcast policy update")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    upsert_instance_broadcast_policy_snapshot(&storage.paths, input)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_instance_broadcast_events(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    limit: Option<usize>,
) -> Result<Vec<InstanceBroadcastEvent>, String> {
    let _operation = state.begin_storage_context_operation("broadcast event listing")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    list_instance_broadcast_events_snapshot(&storage.paths, &instance_id, limit.unwrap_or(50))
        .await
        .map_err(|error| error.to_string())
}
