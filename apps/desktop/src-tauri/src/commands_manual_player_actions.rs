use app_core::{
    ExecuteInstanceManualPlayerActionInput, ExecuteInstancePlayerActionResult,
    RuntimeLivePlayerActionStatus,
};
use serde_json::Value;

use super::commands_assistant_ops::{
    DeclaredRuntimeActionRequest, dispatch_declared_runtime_action,
};
use super::commands_live_players::{
    audit_reference, dispatch_after_live_player_authorization_revocation,
    load_persisted_live_player_context,
};
use super::commands_runtime_actions::find_runtime_action;
use super::commands_runtime_supervision::reconcile_runtime_state;
use super::*;

#[tauri::command]
pub async fn execute_instance_manual_player_action(
    state: tauri::State<'_, DesktopState>,
    input: ExecuteInstanceManualPlayerActionInput,
) -> Result<ExecuteInstancePlayerActionResult, String> {
    let audit_instance_id = audit_reference(&input.instance_id);
    let audit_action_id = audit_reference(&input.action_id);
    let result = execute_instance_manual_player_action_inner(&state, input).await;
    audit_manual_player_action(
        &audit_instance_id,
        &audit_action_id,
        result.as_ref().map(|_| ()).map_err(String::as_str),
    );
    result
}

async fn execute_instance_manual_player_action_inner(
    state: &tauri::State<'_, DesktopState>,
    input: ExecuteInstanceManualPlayerActionInput,
) -> Result<ExecuteInstancePlayerActionResult, String> {
    let instance_id = validated_field(&input.instance_id, "instance_id", 128)?;
    let action_id = validated_field(&input.action_id, "action_id", 128)?;
    let target = validated_field(&input.target, "target", 256)?;
    let role = input
        .role
        .as_deref()
        .map(|value| validated_field(value, "role", 64))
        .transpose()?;

    reconcile_runtime_state(state)
        .await
        .map_err(|_| String::from("Unable to validate the current server run."))?;
    let _mutation = state.acquire_instance_mutation(instance_id).await;
    let (_, details, descriptor) = load_persisted_live_player_context(instance_id)
        .await
        .map_err(|_| String::from("Unable to load the current manual player-action contract."))?;
    if details.active_run.is_none() {
        return Err(String::from("The server is not running."));
    }
    if descriptor.runtime.player_list.as_ref().is_some_and(|list| {
        list.action_id.as_deref() == Some(action_id)
            || list.player_action_ids.iter().any(|id| id == action_id)
    }) {
        return Err(String::from(
            "Structured player actions require an authoritative player snapshot.",
        ));
    }
    if descriptor
        .runtime
        .player_management
        .as_ref()
        .is_some_and(|management| management.status == "pending_adapter")
    {
        return Err(String::from(
            "This module does not expose verified manual player actions.",
        ));
    }
    let action = find_runtime_action(&descriptor, action_id)
        .ok_or_else(|| String::from("The requested manual player action is not declared."))?;
    if action.kind.as_deref() == Some("broadcast")
        || !action.command_template.contains("{{target}}")
    {
        return Err(String::from(
            "The requested runtime action is not an identity-bound player action.",
        ));
    }
    if schema_consumes_action(descriptor.schema_json.as_deref(), action_id)? {
        return Err(String::from(
            "This action is owned by the persistent access-control workflow.",
        ));
    }
    let run_id = details
        .active_run
        .as_ref()
        .map(|run| run.run_id)
        .ok_or_else(|| String::from("The server is not running."))?;

    dispatch_after_live_player_authorization_revocation(
        &state.live_player_registry,
        instance_id,
        dispatch_declared_runtime_action(
            state,
            DeclaredRuntimeActionRequest {
                instance_id,
                expected_run_id: run_id,
                action_id,
                target: Some(target),
                role,
                request_id: None,
                require_target_binding: true,
            },
        ),
    )
    .await
    .map_err(|_| String::from("The server could not complete the declared player action."))?;

    Ok(ExecuteInstancePlayerActionResult {
        action_id: action_id.to_owned(),
        status: RuntimeLivePlayerActionStatus::Sent,
        executed_at_unix_ms: state.live_player_registry.now_unix_ms(),
        summary: String::from("The declared manual player action was accepted for delivery."),
    })
}

fn validated_field<'a>(value: &'a str, field: &str, max_chars: usize) -> Result<&'a str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > max_chars
        || trimmed.chars().any(char::is_control)
    {
        return Err(format!("{field} is invalid"));
    }
    Ok(trimmed)
}

fn schema_consumes_action(schema_json: Option<&str>, action_id: &str) -> Result<bool, String> {
    let Some(schema_json) = schema_json else {
        return Ok(false);
    };
    let schema: Value = serde_json::from_str(schema_json)
        .map_err(|_| String::from("The module access-control contract is unavailable."))?;
    Ok(value_consumes_action(&schema, action_id))
}

fn value_consumes_action(value: &Value, action_id: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.get("consume_action_ids").is_some_and(|value| {
                value.as_array().is_some_and(|values| {
                    values
                        .iter()
                        .any(|candidate| candidate.as_str() == Some(action_id))
                })
            }) || object
                .values()
                .any(|value| value_consumes_action(value, action_id))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| value_consumes_action(value, action_id)),
        _ => false,
    }
}

fn audit_manual_player_action(instance_id: &str, action_id: &str, result: Result<(), &str>) {
    let Ok(storage) = bootstrap_storage() else {
        return;
    };
    let (level, outcome) = match result {
        Ok(()) => ("info", "sent"),
        Err(_) => ("warning", "rejected_or_failed"),
    };
    append_desktop_app_log(
        &storage,
        level,
        "instance.live_players.manual_action",
        "Manual identity-bound player action completed",
        json!({
            "instance_id": instance_id,
            "action_id": action_id,
            "outcome": outcome,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_player_action_schema_consumption_is_detected() {
        let schema = r#"{"properties":{"admins":{"x-lsgm-player-access-sync":{"consume_action_ids":["grant","revoke"]}}}}"#;
        assert!(schema_consumes_action(Some(schema), "grant").expect("valid schema"));
        assert!(!schema_consumes_action(Some(schema), "kick").expect("valid schema"));
    }

    #[test]
    fn manual_player_action_fields_are_bounded_and_control_free() {
        assert_eq!(validated_field(" Player1 ", "target", 16), Ok("Player1"));
        assert!(validated_field("Player\n1", "target", 16).is_err());
        assert!(validated_field(&"x".repeat(17), "target", 16).is_err());
    }
}
