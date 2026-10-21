use super::commands_runtime_actions::{find_runtime_action, render_runtime_action_command};
use super::*;
use app_storage::{
    ApplyInstancePlayerAccessMutationInput, PlayerAccessMutationOperation,
    PlayerAccessPersistentMutationResult, PlayerAccessPersistentStatus, PlayerAccessSyncMetadata,
    PlayerAccessSyncMode, apply_instance_player_access_mutation as persist_player_access_mutation,
};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAccessLiveStatus {
    Applied,
    SentUnverified,
    NotRunning,
    RestartRequired,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAccessVerificationStatus {
    Verified,
    Unavailable,
    Failed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInstancePlayerAccessMutationResult {
    pub instance_id: String,
    pub field_key: String,
    pub operation: PlayerAccessMutationOperation,
    pub persistent_status: PlayerAccessPersistentStatus,
    pub live_status: PlayerAccessLiveStatus,
    pub verification_status: PlayerAccessVerificationStatus,
    pub live_target: String,
    pub sync: PlayerAccessSyncMetadata,
    pub live_action_id: Option<String>,
    pub verification_action_id: Option<String>,
    pub live_error: Option<String>,
    pub verification_error: Option<String>,
    pub verification_response: Option<String>,
}

impl ApplyInstancePlayerAccessMutationResult {
    fn after_persistence(persistent: &PlayerAccessPersistentMutationResult) -> Self {
        Self {
            instance_id: persistent.details.summary.id.clone(),
            field_key: persistent.field_key.clone(),
            operation: persistent.operation,
            persistent_status: persistent.persistent_status,
            live_status: PlayerAccessLiveStatus::NotRunning,
            verification_status: PlayerAccessVerificationStatus::Unavailable,
            live_target: persistent.live_target.clone(),
            sync: persistent.sync.clone(),
            live_action_id: None,
            verification_action_id: persistent.sync.verify_action_id.clone(),
            live_error: None,
            verification_error: None,
            verification_response: None,
        }
    }
}

#[tauri::command]
pub async fn apply_instance_player_access_mutation(
    state: tauri::State<'_, DesktopState>,
    input: ApplyInstancePlayerAccessMutationInput,
) -> Result<ApplyInstancePlayerAccessMutationResult, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("player-access mutation")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance_id = input.instance_id.trim().to_string();
    if instance_id.is_empty() {
        return Err(String::from("instance id must not be empty"));
    }
    reconcile_runtime_state(&state).await?;
    let _instance_lock = state.acquire_instance_mutation(&instance_id).await;

    let current = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|e| e.to_string())?;
    if current.summary.module_id == "enshrouded"
        && input.field_key == "banned_player_ids"
        && input.operation == PlayerAccessMutationOperation::Remove
        && current.active_run.is_some()
    {
        return remove_running_enshrouded_ban(&state, &storage, &current, &input).await;
    }

    append_desktop_app_log(
        &storage,
        "info",
        "instance.player_access.request",
        "Player-access mutation requested",
        json!({
            "instance_id": instance_id.as_str(),
            "field_key": input.field_key.as_str(),
            "operation": input.operation,
        }),
    );

    let persistent = persist_player_access_mutation(&storage.paths, input)
        .await
        .map_err(|error| {
            let message = error.to_string();
            append_desktop_app_log(
                &storage,
                "error",
                "instance.player_access.persistence_failed",
                &message,
                json!({ "instance_id": instance_id.as_str() }),
            );
            logged_error_message(&storage, message)
        })?;
    let mut result = ApplyInstancePlayerAccessMutationResult::after_persistence(&persistent);

    match list_instances(&storage.paths).await {
        Ok(instances) => {
            if let Err(message) = update_state_instances(&state, instances) {
                append_desktop_app_log(
                    &storage,
                    "warning",
                    "instance.player_access.state_refresh_failed",
                    &message,
                    json!({ "instance_id": result.instance_id.as_str() }),
                );
            }
        }
        Err(error) => append_desktop_app_log(
            &storage,
            "warning",
            "instance.player_access.instance_refresh_failed",
            &error.to_string(),
            json!({ "instance_id": result.instance_id.as_str() }),
        ),
    }

    if !matches!(persistent.details.summary.status, InstanceStatus::Running) {
        append_player_access_result_log(&storage, &result);
        return Ok(result);
    }
    if persistent.details.summary.module_id == "valheim"
        && matches!(
            persistent.field_key.as_str(),
            "admin_list" | "permitted_list"
        )
    {
        let live_result = match super::commands_valheim_control::is_prepared(&persistent.details) {
            Ok(false) => {
                result.live_status = PlayerAccessLiveStatus::RestartRequired;
                append_player_access_result_log(&storage, &result);
                return Ok(result);
            }
            Ok(true) => super::commands_valheim_control::sync_roster(&storage, &persistent).await,
            Err(error) => Err(error),
        };
        match live_result {
            Ok(()) => {
                result.live_status = PlayerAccessLiveStatus::Applied;
                result.verification_status = PlayerAccessVerificationStatus::Verified;
            }
            Err(error) => {
                result.live_status = PlayerAccessLiveStatus::Failed;
                result.verification_status = PlayerAccessVerificationStatus::Failed;
                result.live_error = Some(error);
            }
        }
        append_player_access_result_log(&storage, &result);
        return Ok(result);
    }
    let Some(action_id) = persistent
        .sync
        .mutation_action_id(persistent.operation)
        .map(str::to_string)
    else {
        result.live_status = PlayerAccessLiveStatus::RestartRequired;
        append_player_access_result_log(&storage, &result);
        return Ok(result);
    };
    result.live_action_id = Some(action_id.clone());

    let descriptors = match discover_modules(&storage.paths.modules_root) {
        Ok(descriptors) => descriptors,
        Err(error) => {
            result.live_status = PlayerAccessLiveStatus::Failed;
            result.live_error = Some(error.to_string());
            append_player_access_result_log(&storage, &result);
            return Ok(result);
        }
    };
    let descriptor = match descriptors
        .iter()
        .find(|descriptor| descriptor.summary.id == persistent.details.summary.module_id)
    {
        Some(descriptor) => descriptor,
        None => {
            result.live_status = PlayerAccessLiveStatus::Failed;
            result.live_error = Some(format!(
                "module `{}` is unavailable after persistence",
                persistent.details.summary.module_id
            ));
            append_player_access_result_log(&storage, &result);
            return Ok(result);
        }
    };
    let Some(action) = find_runtime_action(descriptor, &action_id) else {
        result.live_status = PlayerAccessLiveStatus::Failed;
        result.live_error = Some(format!(
            "declared runtime action `{action_id}` is unavailable after persistence"
        ));
        append_player_access_result_log(&storage, &result);
        return Ok(result);
    };
    let direct = matches!(persistent.sync.mode, PlayerAccessSyncMode::Direct);
    let target = direct.then_some(persistent.live_target.as_str());
    let command = match render_runtime_action_command(action, target, None, direct) {
        Ok(command) => command,
        Err(error) => {
            result.live_status = PlayerAccessLiveStatus::Failed;
            result.live_error = Some(error);
            append_player_access_result_log(&storage, &result);
            return Ok(result);
        }
    };

    if let Err(error) =
        dispatch_player_access_action(&state, &persistent.details, action, &command).await
    {
        result.live_status = PlayerAccessLiveStatus::Failed;
        result.live_error = Some(error);
        append_player_access_result_log(&storage, &result);
        return Ok(result);
    }
    result.live_status = PlayerAccessLiveStatus::SentUnverified;

    let Some(verify_action_id) = persistent.sync.verify_action_id.as_deref() else {
        append_player_access_result_log(&storage, &result);
        return Ok(result);
    };
    let Some(verify_action) = find_runtime_action(descriptor, verify_action_id) else {
        result.verification_status = PlayerAccessVerificationStatus::Failed;
        result.verification_error = Some(format!(
            "declared verification action `{verify_action_id}` is unavailable after persistence"
        ));
        append_player_access_result_log(&storage, &result);
        return Ok(result);
    };
    let verify_target = verify_action
        .command_template
        .contains("{{target}}")
        .then_some(persistent.live_target.as_str());
    let verify_command =
        match render_runtime_action_command(verify_action, verify_target, None, false) {
            Ok(command) => command,
            Err(error) => {
                result.verification_status = PlayerAccessVerificationStatus::Failed;
                result.verification_error = Some(error);
                append_player_access_result_log(&storage, &result);
                return Ok(result);
            }
        };
    match dispatch_player_access_action(&state, &persistent.details, verify_action, &verify_command)
        .await
    {
        Ok(response) => {
            result.verification_response = response.clone();
            match verify_player_access_response(
                persistent.operation,
                &persistent.live_target,
                response.as_deref(),
            ) {
                Ok(()) => {
                    result.live_status = PlayerAccessLiveStatus::Applied;
                    result.verification_status = PlayerAccessVerificationStatus::Verified;
                }
                Err(error) => {
                    result.verification_status = PlayerAccessVerificationStatus::Failed;
                    result.verification_error = Some(error);
                }
            }
        }
        Err(error) => {
            result.verification_status = PlayerAccessVerificationStatus::Failed;
            result.verification_error = Some(error);
        }
    }
    append_player_access_result_log(&storage, &result);
    Ok(result)
}

async fn remove_running_enshrouded_ban(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    input: &ApplyInstancePlayerAccessMutationInput,
) -> Result<ApplyInstancePlayerAccessMutationResult, String> {
    let descriptors = discover_modules(&storage.paths.modules_root).map_err(|e| e.to_string())?;
    let descriptor = find_descriptor(&descriptors, "enshrouded")?;
    validate_native_unban_roster(descriptor)?;
    let value = input
        .value
        .as_str()
        .ok_or("A native accountId string is required.")?
        .trim();
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("A native uint64 accountId is required to remove this ban.".into());
    }
    let account = value
        .parse::<u64>()
        .map_err(|_| "The native accountId is outside uint64.")?;
    let target = account.to_string();
    let run_id = details
        .active_run
        .as_ref()
        .ok_or("The server is stopped.")?
        .run_id;
    state
        .live_player_registry
        .invalidate_instance(&details.summary.id);
    super::commands_assistant_ops::dispatch_declared_runtime_action(
        state,
        super::commands_assistant_ops::DeclaredRuntimeActionRequest {
            instance_id: &details.summary.id,
            expected_run_id: run_id,
            action_id: "unban_player",
            target: Some(&target),
            role: None,
            request_id: None,
            require_target_binding: false,
        },
    )
    .await?;
    match list_instances(&storage.paths).await {
        Ok(instances) => update_state_instances(state, instances)?,
        Err(error) => append_desktop_app_log(
            storage,
            "warning",
            "instance.player_access.instance_refresh_failed",
            "The native unban was confirmed, but refreshing the instance list failed.",
            json!({"instance_id": details.summary.id, "error": error.to_string()}),
        ),
    }
    let result = ApplyInstancePlayerAccessMutationResult {
        instance_id: details.summary.id.clone(),
        field_key: input.field_key.clone(),
        operation: input.operation,
        persistent_status: PlayerAccessPersistentStatus::Updated,
        live_status: PlayerAccessLiveStatus::Applied,
        verification_status: PlayerAccessVerificationStatus::Verified,
        live_target: target,
        sync: PlayerAccessSyncMetadata {
            mode: PlayerAccessSyncMode::Direct,
            add_action_id: None,
            remove_action_id: Some("unban_player".into()),
            action_id: None,
            verify_action_id: None,
            consume_action_ids: Vec::new(),
        },
        live_action_id: Some("unban_player".into()),
        verification_action_id: None,
        live_error: None,
        verification_error: None,
        verification_response: None,
    };
    append_player_access_result_log(storage, &result);
    Ok(result)
}

fn validate_native_unban_roster(descriptor: &ModuleDescriptor) -> Result<(), String> {
    let schema: Value = serde_json::from_str(
        descriptor
            .schema_json
            .as_deref()
            .ok_or("The native blacklist schema is unavailable.")?,
    )
    .map_err(|_| "The native blacklist schema is invalid.")?;
    let field = schema
        .get("properties")
        .and_then(|properties| properties.get("banned_player_ids"))
        .ok_or("The native blacklist is not declared by this module.")?;
    if descriptor.summary.id != "enshrouded"
        || field.get("type").and_then(Value::as_str) != Some("string")
        || !field
            .get("readOnly")
            .is_none_or(|value| value.as_bool() == Some(false))
        || field
            .get("x-lsgm-player-access-kind")
            .and_then(Value::as_str)
            != Some("block")
        || field
            .get("x-lsgm-player-access-codec")
            .and_then(Value::as_str)
            != Some("uint64")
        || field.get("x-lsgm-source").and_then(Value::as_str) != Some("ban_records")
        || field.get("x-lsgm-source-key").and_then(Value::as_str) != Some("bannedAccounts")
    {
        return Err("This module does not expose a writable native Enshrouded blacklist.".into());
    }
    Ok(())
}

fn verify_player_access_response(
    operation: PlayerAccessMutationOperation,
    live_target: &str,
    response: Option<&str>,
) -> Result<(), String> {
    let response = response
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| String::from("verification action returned no roster response"))?;
    let target = live_target.trim();
    if target.len() != 17 || !target.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(String::from(
            "verification requires a canonical 17-digit Steam64 live target",
        ));
    }

    let normalized_response = response.to_ascii_lowercase();
    const ERROR_PHRASES: &[&str] = &[
        "unknown command",
        "invalid command",
        "command not found",
        "permission denied",
        "not allowed",
    ];
    const ERROR_WORDS: &[&str] = &["error", "failed", "unauthorized", "forbidden", "usage"];
    let error_marker = ERROR_PHRASES
        .iter()
        .copied()
        .find(|phrase| normalized_response.contains(phrase))
        .or_else(|| {
            ERROR_WORDS.iter().copied().find(|word| {
                normalized_response
                    .split(|character: char| !character.is_ascii_alphanumeric())
                    .any(|token| token == *word)
            })
        });
    if let Some(marker) = error_marker {
        return Err(format!(
            "verification action returned an error response containing `{marker}`",
        ));
    }
    if matches!(operation, PlayerAccessMutationOperation::Remove)
        && normalized_response.contains("[langame: response truncated")
    {
        return Err(String::from(
            "verification roster was truncated before absence could be proven",
        ));
    }

    let identities = response
        .split(|character: char| !character.is_ascii_digit())
        .filter(|token| token.len() == 17)
        .collect::<Vec<_>>();
    if matches!(operation, PlayerAccessMutationOperation::Add) && identities.is_empty() {
        return Err(String::from(
            "verification roster contains no recognizable 17-digit Steam64 identities",
        ));
    }
    let contains_target = identities.contains(&target);
    match operation {
        PlayerAccessMutationOperation::Add if contains_target => Ok(()),
        PlayerAccessMutationOperation::Remove if !contains_target => Ok(()),
        PlayerAccessMutationOperation::Add => Err(String::from(
            "verification roster does not contain the added player identity",
        )),
        PlayerAccessMutationOperation::Remove => Err(String::from(
            "verification roster still contains the removed player identity",
        )),
    }
}

async fn dispatch_player_access_action(
    state: &DesktopState,
    details: &InstanceDetails,
    action: &ModulePlayerActionSpec,
    command: &str,
) -> Result<Option<String>, String> {
    let transport = action.transport.trim();
    if transport.eq_ignore_ascii_case("source_rcon") {
        return dispatch_source_rcon_command(
            details,
            command,
            action.port_name.as_deref(),
            action.password_setting_key.as_deref(),
            action.enabled_setting_key.as_deref(),
        )
        .await
        .map(runtime_command_response_text);
    }
    if transport.eq_ignore_ascii_case("websocket_rcon") {
        return dispatch_websocket_rcon_command(
            details,
            command,
            action.port_name.as_deref(),
            action.password_setting_key.as_deref(),
            action.enabled_setting_key.as_deref(),
        )
        .await
        .map(runtime_command_response_text);
    }
    if transport.eq_ignore_ascii_case("battleye_rcon") {
        return dispatch_battleye_rcon_command(
            details,
            command,
            action.port_name.as_deref(),
            action.password_setting_key.as_deref(),
            action.enabled_setting_key.as_deref(),
        )
        .await
        .map(runtime_command_response_text);
    }
    if transport.eq_ignore_ascii_case("telnet") {
        return dispatch_telnet_command(
            details,
            command,
            action.port_name.as_deref(),
            action.password_setting_key.as_deref(),
            action.enabled_setting_key.as_deref(),
        )
        .await
        .map(runtime_command_response_text);
    }

    dispatch_instance_runtime_transport(
        state,
        details,
        &RuntimeTransportRequest {
            command,
            transport,
            process_key: action.process_key.as_deref(),
            port_name: action.port_name.as_deref(),
            password_setting_key: action.password_setting_key.as_deref(),
            enabled_setting_key: action.enabled_setting_key.as_deref(),
        },
    )
    .await
    .map(|_| None)
}

fn append_player_access_result_log(
    storage: &StorageBootstrap,
    result: &ApplyInstancePlayerAccessMutationResult,
) {
    let level = if matches!(result.live_status, PlayerAccessLiveStatus::Failed)
        || matches!(
            result.verification_status,
            PlayerAccessVerificationStatus::Failed
        ) {
        "warning"
    } else {
        "info"
    };
    append_desktop_app_log(
        storage,
        level,
        "instance.player_access.completed",
        "Player-access mutation completed",
        json!({
            "instance_id": result.instance_id.as_str(),
            "field_key": result.field_key.as_str(),
            "operation": result.operation,
            "persistent_status": result.persistent_status,
            "live_status": result.live_status,
            "verification_status": result.verification_status,
            "live_action_id": result.live_action_id.as_deref(),
            "verification_action_id": result.verification_action_id.as_deref(),
            "live_error": result.live_error.as_deref(),
            "verification_error": result.verification_error.as_deref(),
        }),
    );
}

#[cfg(test)]
#[path = "commands_player_access_tests.rs"]
mod tests;
