fn runtime_action_response_text(
    response: String,
    internal: bool,
) -> Result<Option<String>, String> {
    if !internal {
        return Ok(runtime_command_response_text(response));
    }
    if response.len() > 512 * 1024 {
        return Err(String::from(
            "Runtime action response exceeds the capture limit.",
        ));
    }
    // The preview formatter trims and clips text. Collection needs the complete
    // bounded response, including spaces that belong to canonical player names.
    Ok((!response.trim().is_empty()).then_some(response))
}

#[cfg(test)]
#[path = "../commands_runtime_response_tests.rs"]
mod runtime_response_tests;

fn source_rcon_executor(module_id: &str) -> fn(&str, &str, &str) -> Result<String, String> {
    match module_id {
        "conanexiles" => crate::runtime_transport::conan_rcon_exec,
        "rust" => crate::runtime_transport::rust_rcon_exec,
        "arksurvivalevolved" | "arksurvivalascended" | "squad" => {
            crate::runtime_transport::player_list_rcon_exec
        }
        _ => source_rcon_exec,
    }
}

#[cfg(test)]
#[test]
fn source_rcon_executor_selects_read_only_completion_for_ark_and_squad() {
    for module_id in ["arksurvivalevolved", "arksurvivalascended", "squad"] {
        assert!(std::ptr::fn_addr_eq(
            source_rcon_executor(module_id),
            crate::runtime_transport::player_list_rcon_exec
                as fn(&str, &str, &str) -> Result<String, String>
        ));
    }
    assert!(std::ptr::fn_addr_eq(
        source_rcon_executor("conanexiles"),
        crate::runtime_transport::conan_rcon_exec as fn(&str, &str, &str) -> Result<String, String>
    ));
    assert!(std::ptr::fn_addr_eq(
        source_rcon_executor("minecraft"),
        source_rcon_exec as fn(&str, &str, &str) -> Result<String, String>
    ));
    assert!(std::ptr::fn_addr_eq(
        source_rcon_executor("rust"),
        crate::runtime_transport::rust_rcon_exec as fn(&str, &str, &str) -> Result<String, String>
    ));
}

pub(super) async fn dispatch_source_rcon_command(
    details: &InstanceDetails,
    command: &str,
    port_name: Option<&str>,
    password_setting_key: Option<&str>,
    enabled_setting_key: Option<&str>,
) -> Result<String, String> {
    dispatch_rcon_command(
        details,
        command,
        port_name,
        password_setting_key,
        enabled_setting_key,
        source_rcon_executor(&details.summary.module_id),
    )
    .await
}

async fn dispatch_rcon_command(
    details: &InstanceDetails,
    command: &str,
    port_name: Option<&str>,
    password_setting_key: Option<&str>,
    enabled_setting_key: Option<&str>,
    execute: fn(&str, &str, &str) -> Result<String, String>,
) -> Result<String, String> {
    let (endpoint, password) = prepare_rcon_command(details, port_name, password_setting_key, enabled_setting_key)?;
    let command = command.to_string();
    tokio::task::spawn_blocking(move || execute(&endpoint, &password, &command))
        .await
        .map_err(|error| format!("RCON task failed: {error}"))?
}

pub(super) async fn dispatch_source_rcon_shutdown_command(
    details: &InstanceDetails,
    command: &str,
    port_name: Option<&str>,
    password_setting_key: Option<&str>,
    enabled_setting_key: Option<&str>,
) -> Result<String, crate::runtime_transport::RconCommandFailure> {
    use crate::runtime_transport::{RconCommandFailure, source_rcon_shutdown_exec};
    let (endpoint, password) = prepare_rcon_command(details, port_name, password_setting_key, enabled_setting_key)
        .map_err(RconCommandFailure::before_send)?;
    let command = command.to_string();
    let module_id = details.summary.module_id.clone();
    tokio::task::spawn_blocking(move || source_rcon_shutdown_exec(&module_id, &endpoint, &password, &command))
        .await
        // A task failure cannot establish that its command was never written.
        .map_err(|error| RconCommandFailure::after_send_attempt(format!("RCON task failed: {error}")))?
}

fn prepare_rcon_command(
    details: &InstanceDetails,
    port_name: Option<&str>,
    password_setting_key: Option<&str>,
    enabled_setting_key: Option<&str>,
) -> Result<(String, String), String> {
    if details.summary.module_id == "palworld" {
        return Err(String::from(
            "Palworld management uses the authenticated REST API.",
        ));
    }
    let settings: Value = serde_json::from_str(&details.settings_json)
        .map_err(|error| format!("failed to parse instance settings for RCON: {error}"))?;
    let settings = settings.as_object().ok_or_else(|| {
        String::from("instance settings must be a JSON object before sending RCON commands")
    })?;

    if let Some(enabled_key) = enabled_setting_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let enabled = settings
            .get(enabled_key)
            .and_then(|value| json_bool(Some(value)))
            .unwrap_or(false);
        if !enabled {
            return Err(format!(
                "RCON command is disabled because `{enabled_key}` is not enabled for this instance"
            ));
        }
    }

    let password_key = password_setting_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("rcon_password");
    let password = settings
        .get(password_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("RCON password setting `{password_key}` is empty"))?;

    let port_key = port_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("rcon");
    let port = details
        .ports
        .iter()
        .find(|port| port.name.eq_ignore_ascii_case(port_key))
        .ok_or_else(|| format!("instance has no `{port_key}` port for RCON"))?;
    if !port.protocol.eq_ignore_ascii_case("tcp") {
        return Err(format!("RCON port `{port_key}` must be TCP"));
    }

    let host = normalize_query_host(&details.summary.bind_ip);
    let endpoint = host
        .parse::<std::net::IpAddr>()
        .map(|address| std::net::SocketAddr::new(address, port.port).to_string())
        .map_err(|_| String::from("RCON requires an IP-address endpoint"))?;
    Ok((endpoint, password.to_string()))
}

pub(super) async fn dispatch_websocket_rcon_command(
    details: &InstanceDetails,
    command: &str,
    port_name: Option<&str>,
    password_setting_key: Option<&str>,
    enabled_setting_key: Option<&str>,
) -> Result<String, String> {
    let settings: Value = serde_json::from_str(&details.settings_json).map_err(|error| {
        format!("failed to parse instance settings for WebSocket RCON: {error}")
    })?;
    let settings = settings.as_object().ok_or_else(|| {
        String::from(
            "instance settings must be a JSON object before sending WebSocket RCON commands",
        )
    })?;

    if let Some(enabled_key) = enabled_setting_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let enabled = settings
            .get(enabled_key)
            .and_then(|value| json_bool(Some(value)))
            .unwrap_or(false);
        if !enabled {
            return Err(format!(
                "WebSocket RCON command is disabled because `{enabled_key}` is not enabled for this instance"
            ));
        }
    }

    let password_key = password_setting_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("rcon_password");
    let password = settings
        .get(password_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("WebSocket RCON password setting `{password_key}` is empty"))?;

    let port_key = port_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("rcon");
    let port = details
        .ports
        .iter()
        .find(|port| port.name.eq_ignore_ascii_case(port_key))
        .ok_or_else(|| format!("instance has no `{port_key}` port for WebSocket RCON"))?;
    if !port.protocol.eq_ignore_ascii_case("tcp") {
        return Err(format!("WebSocket RCON port `{port_key}` must be TCP"));
    }

    let endpoint = format!(
        "{}:{}",
        normalize_query_host(&details.summary.bind_ip),
        port.port
    );
    let password = password.to_string();
    let command = command.to_string();

    tokio::task::spawn_blocking(move || websocket_rcon_exec(&endpoint, &password, &command))
        .await
        .map_err(|error| format!("WebSocket RCON task failed: {error}"))?
}

pub(super) async fn dispatch_battleye_rcon_command(
    details: &InstanceDetails,
    command: &str,
    port_name: Option<&str>,
    password_setting_key: Option<&str>,
    enabled_setting_key: Option<&str>,
) -> Result<String, String> {
    let settings: Value = serde_json::from_str(&details.settings_json)
        .map_err(|error| format!("failed to parse instance settings for BattlEye RCON: {error}"))?;
    let settings = settings.as_object().ok_or_else(|| {
        String::from(
            "instance settings must be a JSON object before sending BattlEye RCON commands",
        )
    })?;

    if let Some(enabled_key) = enabled_setting_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let enabled = settings
            .get(enabled_key)
            .and_then(|value| json_bool(Some(value)))
            .unwrap_or(false);
        if !enabled {
            return Err(format!(
                "BattlEye RCON command is disabled because `{enabled_key}` is not enabled for this instance"
            ));
        }
    }

    let password_key = password_setting_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("rcon_password");
    let password = settings
        .get(password_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("BattlEye RCON password setting `{password_key}` is empty"))?;

    let port_key = port_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("rcon");
    let port = details
        .ports
        .iter()
        .find(|port| port.name.eq_ignore_ascii_case(port_key))
        .ok_or_else(|| format!("instance has no `{port_key}` port for BattlEye RCON"))?;
    if !port.protocol.eq_ignore_ascii_case("udp") {
        return Err(format!("BattlEye RCON port `{port_key}` must be UDP"));
    }

    let endpoint = format!(
        "{}:{}",
        normalize_query_host(&details.summary.bind_ip),
        port.port
    );
    let password = password.to_string();
    let command = command.to_string();

    tokio::task::spawn_blocking(move || battleye_rcon_exec(&endpoint, &password, &command))
        .await
        .map_err(|error| format!("BattlEye RCON task failed: {error}"))?
}

pub(super) async fn dispatch_telnet_command(
    details: &InstanceDetails,
    command: &str,
    port_name: Option<&str>,
    password_setting_key: Option<&str>,
    enabled_setting_key: Option<&str>,
) -> Result<String, String> {
    dispatch_telnet_command_using(
        details,
        command,
        port_name,
        password_setting_key,
        enabled_setting_key,
        telnet_exec,
    )
    .await
}

async fn dispatch_telnet_command_using(
    details: &InstanceDetails,
    command: &str,
    port_name: Option<&str>,
    password_setting_key: Option<&str>,
    enabled_setting_key: Option<&str>,
    execute: fn(&str, &str, &str) -> Result<String, String>,
) -> Result<String, String> {
    let settings: Value = serde_json::from_str(&details.settings_json)
        .map_err(|error| format!("failed to parse instance settings for Telnet: {error}"))?;
    let settings = settings.as_object().ok_or_else(|| {
        String::from("instance settings must be a JSON object before sending Telnet commands")
    })?;

    if let Some(enabled_key) = enabled_setting_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let enabled = settings
            .get(enabled_key)
            .and_then(|value| json_bool(Some(value)))
            .unwrap_or(true);
        if !enabled {
            return Err(format!(
                "Telnet command is disabled because `{enabled_key}` is disabled for this instance"
            ));
        }
    }

    let password_key = password_setting_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("telnet_password");
    let password = settings
        .get(password_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    let port_key = port_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("telnet");
    let port = details
        .ports
        .iter()
        .find(|port| port.name.eq_ignore_ascii_case(port_key))
        .ok_or_else(|| format!("instance has no `{port_key}` port for Telnet"))?;
    if !port.protocol.eq_ignore_ascii_case("tcp") {
        return Err(format!("Telnet port `{port_key}` must be TCP"));
    }

    let endpoint = format!(
        "{}:{}",
        normalize_query_host(&details.summary.bind_ip),
        port.port
    );
    let command = command.to_string();

    tokio::task::spawn_blocking(move || execute(&endpoint, &password, &command))
        .await
        .map_err(|error| format!("Telnet task failed: {error}"))?
}

async fn runtime_command_instance_details<'a>(
    storage: &StorageBootstrap,
    instance_id: &str,
    cached: &'a mut Option<InstanceDetails>,
) -> Result<&'a InstanceDetails, String> {
    if cached.is_none() {
        *cached = Some(
            read_instance_details(&storage.paths, instance_id)
                .await
                .map_err(|error| error.to_string())?,
        );
    }
    cached
        .as_ref()
        .ok_or_else(|| String::from("failed to cache runtime command instance details"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRuntimeCommandInput {
    pub instance_id: String,
    pub command: String,
    pub process_key: Option<String>,
    pub transport: Option<String>,
    pub port_name: Option<String>,
    pub password_setting_key: Option<String>,
    pub enabled_setting_key: Option<String>,
    pub runtime_action_id: Option<String>,
    pub runtime_action_target: Option<String>,
    pub runtime_action_role: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DeclaredRuntimeActionRequest<'a> {
    pub(super) instance_id: &'a str,
    pub(super) expected_run_id: i64,
    pub(super) action_id: &'a str,
    pub(super) target: Option<&'a str>,
    pub(super) role: Option<&'a str>,
    pub(super) request_id: Option<&'a str>,
    pub(super) require_target_binding: bool,
}

struct RuntimeCommandDispatchContext<'a> {
    expected_run_id: Option<i64>,
    expected_instance: Option<&'a InstanceDetails>,
    internal_request_id: Option<&'a str>,
    require_target_binding: bool,
    redact_audit_payload: bool,
    reconcile_before_dispatch: bool,
    stdin_dispatch_budget: Option<RuntimeStdinDispatchBudget>,
}

impl RuntimeCommandDispatchContext<'_> {
    fn external_request() -> Self {
        Self {
            expected_run_id: None,
            expected_instance: None,
            internal_request_id: None,
            require_target_binding: false,
            redact_audit_payload: false,
            reconcile_before_dispatch: true,
            stdin_dispatch_budget: None,
        }
    }
}

#[tauri::command]
pub async fn send_instance_runtime_command(
    state: tauri::State<'_, DesktopState>,
    input: InstanceRuntimeCommandInput,
) -> Result<InstanceRuntimeCommandResult, String> {
    Box::pin(send_instance_runtime_command_inner(
        &state,
        input,
        RuntimeCommandDispatchContext::external_request(),
    ))
    .await
}

pub(super) async fn send_assistant_runtime_command(
    state: tauri::State<'_, DesktopState>,
    input: InstanceRuntimeCommandInput,
    expected: &InstanceDetails,
) -> Result<InstanceRuntimeCommandResult, String> {
    Box::pin(send_instance_runtime_command_inner(
        &state,
        input,
        RuntimeCommandDispatchContext {
            expected_run_id: expected.active_run.as_ref().map(|run| run.run_id),
            expected_instance: Some(expected),
            ..RuntimeCommandDispatchContext::external_request()
        },
    ))
    .await
}

#[tauri::command]
pub async fn send_instance_gm_command(
    state: tauri::State<'_, DesktopState>,
    input: InstanceRuntimeCommandInput,
) -> Result<InstanceRuntimeCommandResult, String> {
    Box::pin(send_instance_runtime_command_inner(
        &state,
        input,
        RuntimeCommandDispatchContext::external_request(),
    ))
    .await
}

pub(super) async fn dispatch_declared_runtime_action(
    state: &tauri::State<'_, DesktopState>,
    request: DeclaredRuntimeActionRequest<'_>,
) -> Result<InstanceRuntimeCommandResult, String> {
    dispatch_declared_runtime_action_with_budget(state, request, None).await
}

pub(super) async fn dispatch_declared_runtime_action_until(
    state: &tauri::State<'_, DesktopState>,
    request: DeclaredRuntimeActionRequest<'_>,
    confirmation_deadline: Instant,
    submission_tracker: app_runtime::RuntimeCommandSubmissionTracker,
) -> Result<InstanceRuntimeCommandResult, String> {
    dispatch_declared_runtime_action_with_budget(
        state,
        request,
        Some(RuntimeStdinDispatchBudget::tracked_until(
            confirmation_deadline,
            submission_tracker,
        )),
    )
    .await
}

async fn dispatch_declared_runtime_action_with_budget(
    state: &tauri::State<'_, DesktopState>,
    request: DeclaredRuntimeActionRequest<'_>,
    stdin_dispatch_budget: Option<RuntimeStdinDispatchBudget>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let DeclaredRuntimeActionRequest {
        instance_id,
        expected_run_id,
        action_id,
        target,
        role,
        request_id,
        require_target_binding,
    } = request;
    // Runtime dispatch owns the transport and storage futures. Keep that state
    // on the heap instead of embedding it in every player-collection layer.
    Box::pin(send_instance_runtime_command_inner(
        state,
        InstanceRuntimeCommandInput {
            instance_id: instance_id.to_string(),
            command: String::new(),
            process_key: None,
            transport: None,
            port_name: None,
            password_setting_key: None,
            enabled_setting_key: None,
            runtime_action_id: Some(action_id.to_string()),
            runtime_action_target: target.map(str::to_string),
            runtime_action_role: role.map(str::to_string),
        },
        RuntimeCommandDispatchContext {
            expected_run_id: Some(expected_run_id),
            expected_instance: None,
            internal_request_id: request_id,
            require_target_binding,
            redact_audit_payload: true,
            reconcile_before_dispatch: false,
            stdin_dispatch_budget,
        },
    ))
    .await
}

pub(super) struct RuntimeActionRejectionAudit {
    pub(super) message: &'static str,
    pub(super) payload: Value,
    pub(super) public_error: String,
}

pub(super) fn runtime_action_rejection_audit(
    redact_source_details: bool,
    instance_id: &str,
    module_id: &str,
    runtime_action: Option<&ModulePlayerActionSpec>,
    source_error: &str,
) -> RuntimeActionRejectionAudit {
    let mut payload = json!({
        "instance_id": instance_id,
        "module_id": module_id,
        "runtime_action_id": runtime_action.map(|action| action.id.as_str()),
        "error_kind": "semantic_validation_rejected",
    });
    if !redact_source_details {
        payload["error"] = Value::String(source_error.to_owned());
    }

    RuntimeActionRejectionAudit {
        message: "Runtime action request rejected by server-side semantic validation",
        payload,
        public_error: if redact_source_details {
            String::from("The declared runtime action failed semantic validation.")
        } else {
            source_error.to_owned()
        },
    }
}

pub(super) async fn acquire_runtime_command_instance_mutation(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance_id: &str,
    expected: Option<&InstanceDetails>,
    cached: &mut Option<InstanceDetails>,
) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    let guard = state.acquire_instance_mutation(instance_id).await;
    if let Some(expected) = expected {
        let current = runtime_command_instance_details(storage, instance_id, cached).await?;
        AssistantOperationPrecondition::from_details(expected).validate(current)?;
    }
    // Keep this guard through dispatch so no managed restart or configuration
    // edit can replace the validated target while the command waits for I/O.
    Ok(guard)
}

async fn send_instance_runtime_command_inner(
    state: &tauri::State<'_, DesktopState>,
    input: InstanceRuntimeCommandInput,
    context: RuntimeCommandDispatchContext<'_>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let RuntimeCommandDispatchContext {
        mut expected_run_id,
        expected_instance,
        internal_request_id,
        require_target_binding,
        redact_audit_payload,
        reconcile_before_dispatch,
        stdin_dispatch_budget,
    } = context;
    let InstanceRuntimeCommandInput {
        instance_id,
        command,
        process_key,
        transport,
        port_name,
        password_setting_key,
        enabled_setting_key,
        runtime_action_id,
        runtime_action_target,
        runtime_action_role,
    } = input;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    if reconcile_before_dispatch {
        reconcile_runtime_state(state).await?;
    }
    let mut instance_details = None;
    let _instance_mutation = if reconcile_before_dispatch || expected_instance.is_some() {
        Some(
            acquire_runtime_command_instance_mutation(
                state,
                &storage,
                &instance_id,
                expected_instance,
                &mut instance_details,
            )
            .await?,
        )
    } else {
        None
    };

    let resolution_input = RuntimeCommandResolutionInput {
        command: &command,
        process_key: process_key.as_deref(),
        transport: transport.as_deref(),
        port_name: port_name.as_deref(),
        password_setting_key: password_setting_key.as_deref(),
        enabled_setting_key: enabled_setting_key.as_deref(),
        runtime_action_id: runtime_action_id.as_deref(),
        runtime_action_target: runtime_action_target.as_deref(),
        runtime_action_role: runtime_action_role.as_deref(),
    };
    let resolved = if runtime_action_fields_present(&resolution_input) {
        let details =
            runtime_command_instance_details(&storage, &instance_id, &mut instance_details).await?;
        if expected_run_id.is_none() {
            expected_run_id = Some(
                details
                    .active_run
                    .as_ref()
                    .map(|run| run.run_id)
                    .ok_or_else(|| String::from("the server is not running"))?,
            );
        }
        ensure_expected_runtime_run(details, expected_run_id)?;
        let descriptors =
            discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
        let descriptor = find_descriptor(&descriptors, &details.summary.module_id)?;
        if !redact_audit_payload
            && runtime_action_id.as_deref().is_some_and(|action_id| {
                runtime_action_requires_live_player_service(descriptor, action_id)
            })
        {
            append_desktop_app_log(
                &storage,
                "warning",
                "instance.runtime_action.player_service_required",
                "Structured live-player action rejected outside the snapshot service",
                json!({
                    "instance_id": instance_id.as_str(),
                    "module_id": details.summary.module_id.as_str(),
                    "runtime_action_id": runtime_action_id.as_deref(),
                }),
            );
            return Err(String::from(
                "structured live-player actions must be dispatched through the live-player service",
            ));
        }
        let resolution = if internal_request_id.is_some() || require_target_binding {
            let action_id = runtime_action_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| String::from("internal runtime action requires an action id"))?;
            resolve_declared_runtime_action(
                descriptor,
                action_id,
                runtime_action_target.as_deref(),
                runtime_action_role.as_deref(),
                internal_request_id,
                require_target_binding,
            )
        } else {
            resolve_runtime_command(Some(descriptor), resolution_input)
        };
        let canonical_audit_action = runtime_action_id
            .as_deref()
            .and_then(|action_id| find_runtime_action(descriptor, action_id));
        resolution.map_err(|error| {
            let audit = runtime_action_rejection_audit(
                redact_audit_payload,
                &details.summary.id,
                &details.summary.module_id,
                canonical_audit_action,
                &error,
            );
            append_desktop_app_log(
                &storage,
                "warning",
                "instance.runtime_action.rejected",
                audit.message,
                audit.payload,
            );
            audit.public_error
        })?
    } else {
        resolve_runtime_command(None, resolution_input)?
    };
    let ResolvedRuntimeCommand {
        command: normalized_command,
        process_key,
        transport: normalized_transport,
        port_name,
        password_setting_key,
        enabled_setting_key,
        runtime_action_id,
    } = resolved;
    ensure_expected_supervisor_state(state, &instance_id, expected_run_id)?;
    let audited_command = (!redact_audit_payload).then_some(normalized_command.as_str());
    let submitted_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| String::from("failed to compute runtime command timestamp"))?
        .as_millis();
    if normalized_transport.eq_ignore_ascii_case("palworld_rest") {
        if runtime_action_id.is_none() {
            return Err(String::from(
                "Palworld REST requests must use a declared runtime action.",
            ));
        }
        let details =
            runtime_command_instance_details(&storage, &instance_id, &mut instance_details).await?;
        let response_text =
            crate::live_players::palworld_rest::execute(details, &normalized_command).await?;
        append_desktop_app_log(
            &storage,
            "info",
            "instance.runtime_command.rest_sent",
            "Palworld REST API confirmed the declared runtime action",
            json!({
                "instance_id": instance_id.as_str(),
                "transport": "palworld_rest",
                "runtime_action_id": runtime_action_id.as_deref(),
            }),
        );
        return Ok(InstanceRuntimeCommandResult {
            instance_id,
            process_key: normalized_transport,
            display_name: String::from("Palworld REST API"),
            pid: 0,
            command: normalized_command,
            response_text: (!redact_audit_payload).then_some(response_text),
            write_confirmation_pending: false,
            submitted_at_unix_ms,
        });
    }
    if normalized_transport.eq_ignore_ascii_case("source_rcon")
        || normalized_transport.eq_ignore_ascii_case("humanitz_rcon")
    {
        let details =
            runtime_command_instance_details(&storage, &instance_id, &mut instance_details).await?;
        let projected = crate::commands::commands_runtime_ark::project_running_command(
            details,
            process_key.as_deref(),
        )?;
        let result_process_key = if projected.is_some() {
            process_key.clone().unwrap_or_else(|| String::from("main"))
        } else {
            normalized_transport.clone()
        };
        let target_pid = projected
            .as_ref()
            .and_then(|target| {
                target.active_run.as_ref().and_then(|run| {
                    run.processes
                        .iter()
                        .find(|process| process.process_key == result_process_key)
                        .and_then(|process| process.pid)
                        .or(run.pid.filter(|_| result_process_key == "main"))
                })
            })
            .unwrap_or(0);
        let execute = if normalized_transport.eq_ignore_ascii_case("humanitz_rcon") {
            crate::runtime_transport_humanitz::humanitz_rcon_info
                as fn(&str, &str, &str) -> Result<String, String>
        } else {
            source_rcon_executor(&details.summary.module_id)
        };
        let response_text = runtime_action_response_text(
            dispatch_rcon_command(
                projected.as_ref().unwrap_or(details),
                &normalized_command,
                port_name.as_deref(),
                password_setting_key.as_deref(),
                enabled_setting_key.as_deref(),
                execute,
            )
            .await?,
            redact_audit_payload,
        )?;
        let audited_response = (!redact_audit_payload)
            .then_some(response_text.as_deref())
            .flatten();

        append_desktop_app_log(
            &storage,
            "info",
            "instance.runtime_command.rcon_sent",
            "Runtime command sent through RCON",
            json!({
                "instance_id": instance_id.as_str(),
                "transport": normalized_transport.as_str(),
                "process_key": result_process_key.as_str(),
                "port_name": port_name.as_deref().unwrap_or("rcon"),
                "command": audited_command,
                "response_text": audited_response,
                "runtime_action_id": runtime_action_id.as_deref(),
            }),
        );

        return Ok(InstanceRuntimeCommandResult {
            instance_id,
            process_key: result_process_key,
            display_name: String::from("RCON"),
            pid: target_pid,
            command: normalized_command,
            response_text,
            write_confirmation_pending: false,
            submitted_at_unix_ms,
        });
    }

    if normalized_transport.eq_ignore_ascii_case("websocket_rcon") {
        let details =
            runtime_command_instance_details(&storage, &instance_id, &mut instance_details).await?;
        let response_text = runtime_action_response_text(
            dispatch_websocket_rcon_command(
                details,
                &normalized_command,
                port_name.as_deref(),
                password_setting_key.as_deref(),
                enabled_setting_key.as_deref(),
            )
            .await?,
            redact_audit_payload,
        )?;
        let audited_response = (!redact_audit_payload)
            .then_some(response_text.as_deref())
            .flatten();

        append_desktop_app_log(
            &storage,
            "info",
            "instance.runtime_command.websocket_rcon_sent",
            "Runtime command sent through WebSocket RCON",
            json!({
                "instance_id": instance_id.as_str(),
                "transport": "websocket_rcon",
                "port_name": port_name.as_deref().unwrap_or("rcon"),
                "command": audited_command,
                "response_text": audited_response,
                "runtime_action_id": runtime_action_id.as_deref(),
            }),
        );

        return Ok(InstanceRuntimeCommandResult {
            instance_id,
            process_key: String::from("websocket_rcon"),
            display_name: String::from("WebSocket RCON"),
            pid: 0,
            command: normalized_command,
            response_text,
            write_confirmation_pending: false,
            submitted_at_unix_ms,
        });
    }

    if normalized_transport.eq_ignore_ascii_case("telnet") {
        let details =
            runtime_command_instance_details(&storage, &instance_id, &mut instance_details).await?;
        let execute = if require_target_binding
            && details.summary.module_id == "sevendaystodie"
            && runtime_action_id.as_deref() == Some("ban_player")
        {
            crate::runtime_transport::seven_days_ban_telnet_exec
                as fn(&str, &str, &str) -> Result<String, String>
        } else {
            telnet_exec
        };
        let response_text = runtime_action_response_text(
            dispatch_telnet_command_using(
                details,
                &normalized_command,
                port_name.as_deref(),
                password_setting_key.as_deref(),
                enabled_setting_key.as_deref(),
                execute,
            )
            .await?,
            redact_audit_payload,
        )?;
        let audited_response = (!redact_audit_payload)
            .then_some(response_text.as_deref())
            .flatten();

        append_desktop_app_log(
            &storage,
            "info",
            "instance.runtime_command.telnet_sent",
            "Runtime command sent through Telnet",
            json!({
                "instance_id": instance_id.as_str(),
                "transport": "telnet",
                "port_name": port_name.as_deref().unwrap_or("telnet"),
                "command": audited_command,
                "response_text": audited_response,
                "runtime_action_id": runtime_action_id.as_deref(),
            }),
        );

        return Ok(InstanceRuntimeCommandResult {
            instance_id,
            process_key: String::from("telnet"),
            display_name: String::from("Telnet"),
            pid: 0,
            command: normalized_command,
            response_text,
            write_confirmation_pending: false,
            submitted_at_unix_ms,
        });
    }

    if normalized_transport.eq_ignore_ascii_case("battleye_rcon") {
        let details =
            runtime_command_instance_details(&storage, &instance_id, &mut instance_details).await?;
        let response_text = runtime_action_response_text(
            dispatch_battleye_rcon_command(
                details,
                &normalized_command,
                port_name.as_deref(),
                password_setting_key.as_deref(),
                enabled_setting_key.as_deref(),
            )
            .await?,
            redact_audit_payload,
        )?;
        let audited_response = (!redact_audit_payload)
            .then_some(response_text.as_deref())
            .flatten();

        append_desktop_app_log(
            &storage,
            "info",
            "instance.runtime_command.battleye_rcon_sent",
            "Runtime command sent through BattlEye RCON",
            json!({
                "instance_id": instance_id.as_str(),
                "transport": "battleye_rcon",
                "port_name": port_name.as_deref().unwrap_or("rcon"),
                "command": audited_command,
                "response_text": audited_response,
                "runtime_action_id": runtime_action_id.as_deref(),
            }),
        );

        return Ok(InstanceRuntimeCommandResult {
            instance_id,
            process_key: String::from("battleye_rcon"),
            display_name: String::from("BattlEye RCON"),
            pid: 0,
            command: normalized_command,
            response_text,
            write_confirmation_pending: false,
            submitted_at_unix_ms,
        });
    }

    if normalized_transport.eq_ignore_ascii_case("console_ctrl_c")
        || normalized_transport.eq_ignore_ascii_case("ctrl_c")
        || normalized_transport.eq_ignore_ascii_case("window_close")
    {
        let window_close = normalized_transport.eq_ignore_ascii_case("window_close");
        if window_close && normalized_command != "WM_CLOSE" {
            return Err("window_close transport only accepts WM_CLOSE".into());
        }
        let dispatched = {
            let mut runtime = state
                .runtime_supervisor
                .lock()
                .map_err(|_| String::from("runtime supervisor lock poisoned"))?;
            ensure_expected_supervisor_run(&runtime, &instance_id, expected_run_id)?;
            if window_close {
                runtime.request_window_close(&instance_id, process_key.as_deref())
            } else {
                runtime.request_console_interrupt(&instance_id, process_key.as_deref())
            }
            .map_err(|error| error.to_string())?
        };

        append_desktop_app_log(
            &storage,
            "info",
            if window_close {
                "instance.runtime_command.window_close_requested"
            } else {
                "instance.runtime_command.console_interrupt_sent"
            },
            if window_close {
                "Window close queued for managed process; exit remains unconfirmed"
            } else {
                "Runtime console interrupt sent to managed process"
            },
            json!({
                "instance_id": instance_id.as_str(),
                "process_key": dispatched.process_key.as_str(),
                "display_name": dispatched.display_name.as_str(),
                "pid": dispatched.pid,
                "command": audited_command,
                "transport": normalized_transport,
                "runtime_action_id": runtime_action_id.as_deref(),
            }),
        );

        return Ok(InstanceRuntimeCommandResult {
            instance_id,
            process_key: dispatched.process_key,
            display_name: dispatched.display_name,
            pid: dispatched.pid,
            command: normalized_command,
            response_text: None,
            write_confirmation_pending: false,
            submitted_at_unix_ms,
        });
    }

    if !normalized_transport.eq_ignore_ascii_case("stdin") {
        return Err(format!(
            "unsupported runtime command transport `{normalized_transport}`"
        ));
    }

    let dispatched = match stdin_dispatch_budget {
        Some(budget) => {
            dispatch_managed_stdin_command_with_budget(
                state,
                &instance_id,
                process_key.as_deref(),
                &normalized_command,
                expected_run_id,
                budget,
            )
            .await?
        }
        None => {
            dispatch_managed_stdin_command(
                state,
                &instance_id,
                process_key.as_deref(),
                &normalized_command,
                expected_run_id,
            )
            .await?
        }
    };
    let confirmation_pending = dispatched.confirmation == RuntimeStdinDispatchConfirmation::Pending;
    let response_text = confirmation_pending
        .then(|| String::from("Runtime command accepted; stdin write confirmation is pending."));

    append_desktop_app_log(
        &storage,
        "info",
        if confirmation_pending {
            "instance.runtime_command.accepted"
        } else {
            "instance.runtime_command.sent"
        },
        if confirmation_pending {
            "Runtime command accepted by the managed stdin writer"
        } else {
            "Runtime command sent to managed process"
        },
        json!({
            "instance_id": instance_id.as_str(),
            "process_key": dispatched.target.process_key.as_str(),
            "display_name": dispatched.target.display_name.as_str(),
            "pid": dispatched.target.pid,
            "command": audited_command,
            "runtime_action_id": runtime_action_id.as_deref(),
            "write_confirmation_pending": confirmation_pending,
        }),
    );
    let dispatched = dispatched.target;

    Ok(InstanceRuntimeCommandResult {
        instance_id,
        process_key: dispatched.process_key,
        display_name: dispatched.display_name,
        pid: dispatched.pid,
        command: normalized_command,
        response_text,
        write_confirmation_pending: confirmation_pending,
        submitted_at_unix_ms,
    })
}

fn ensure_expected_runtime_run(
    details: &InstanceDetails,
    expected_run_id: Option<i64>,
) -> Result<(), String> {
    let Some(expected_run_id) = expected_run_id else {
        return Ok(());
    };
    let active_run_id = details.active_run.as_ref().map(|run| run.run_id);
    if active_run_id != Some(expected_run_id) {
        return Err(String::from(
            "the server run changed before the live-player action could be dispatched",
        ));
    }
    Ok(())
}
