use super::commands_runtime_lifecycle::normalize_runtime_command_input;
use super::*;

#[derive(Clone, Copy)]
pub(super) struct RuntimeCommandResolutionInput<'a> {
    pub command: &'a str,
    pub process_key: Option<&'a str>,
    pub transport: Option<&'a str>,
    pub port_name: Option<&'a str>,
    pub password_setting_key: Option<&'a str>,
    pub enabled_setting_key: Option<&'a str>,
    pub runtime_action_id: Option<&'a str>,
    pub runtime_action_target: Option<&'a str>,
    pub runtime_action_role: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedRuntimeCommand {
    pub command: String,
    pub process_key: Option<String>,
    pub transport: String,
    pub port_name: Option<String>,
    pub password_setting_key: Option<String>,
    pub enabled_setting_key: Option<String>,
    pub runtime_action_id: Option<String>,
}

pub(super) fn runtime_action_fields_present(input: &RuntimeCommandResolutionInput<'_>) -> bool {
    input.runtime_action_id.is_some()
        || input.runtime_action_target.is_some()
        || input.runtime_action_role.is_some()
}

pub(super) fn resolve_runtime_command(
    descriptor: Option<&ModuleDescriptor>,
    input: RuntimeCommandResolutionInput<'_>,
) -> Result<ResolvedRuntimeCommand, String> {
    if !runtime_action_fields_present(&input) {
        return Ok(ResolvedRuntimeCommand {
            command: normalize_runtime_command_input(input.command)?,
            process_key: input.process_key.map(str::to_string),
            transport: input
                .transport
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("stdin")
                .to_string(),
            port_name: input.port_name.map(str::to_string),
            password_setting_key: input.password_setting_key.map(str::to_string),
            enabled_setting_key: input.enabled_setting_key.map(str::to_string),
            runtime_action_id: None,
        });
    }

    let descriptor = descriptor.ok_or_else(|| {
        String::from("runtime action metadata requires the instance module descriptor")
    })?;
    let action_id = input
        .runtime_action_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            String::from("runtimeActionId is required when runtime action metadata is provided")
        })?;
    let mut resolved = resolve_declared_runtime_action(
        descriptor,
        action_id,
        input.runtime_action_target,
        input.runtime_action_role,
        None,
        false,
    )?;
    // ARK map selection is configuration-backed and checked against the live
    // process by its RCON dispatcher. Transport and command metadata stay native.
    if app_core::ark_maps::is_ark(&descriptor.summary.id)
        && let Some(process_key) = input.process_key
    {
        resolved.process_key = Some(process_key.to_owned());
    }
    Ok(resolved)
}

pub(super) fn resolve_declared_runtime_action(
    descriptor: &ModuleDescriptor,
    action_id: &str,
    target: Option<&str>,
    role: Option<&str>,
    request_id: Option<&str>,
    require_target_binding: bool,
) -> Result<ResolvedRuntimeCommand, String> {
    let action = find_runtime_action(descriptor, action_id).ok_or_else(|| {
        format!(
            "runtime action `{action_id}` is not declared by module `{}`",
            descriptor.summary.id
        )
    })?;
    let transport = action.transport.trim();
    if transport.is_empty() {
        return Err(format!(
            "runtime action `{}` must declare a transport",
            action.id
        ));
    }
    if transport == "palworld_rest" && descriptor.summary.id != "palworld" {
        return Err(String::from(
            "Palworld REST actions require the Palworld module.",
        ));
    }

    if descriptor.summary.id == "humanitz"
        && matches!(
            action.id.as_str(),
            "kick_player" | "ban_player" | "unban_player"
        )
        && !target.is_some_and(app_core::is_humanitz_net_id)
    {
        return Err(String::from(
            "HumanitZ player actions require the complete NetID (EpicAccountId|ProductUserId or |ProductUserId); short IDs and Steam64 IDs are not accepted.",
        ));
    }

    Ok(ResolvedRuntimeCommand {
        command: render_runtime_action_command_with_request_id(
            action,
            target,
            role,
            require_target_binding,
            request_id,
        )?,
        process_key: normalized_optional_metadata(action.process_key.as_deref()),
        transport: transport.to_string(),
        port_name: normalized_optional_metadata(action.port_name.as_deref()),
        password_setting_key: normalized_optional_metadata(action.password_setting_key.as_deref()),
        enabled_setting_key: normalized_optional_metadata(action.enabled_setting_key.as_deref()),
        runtime_action_id: Some(action.id.clone()),
    })
}

pub(super) fn find_runtime_action<'a>(
    descriptor: &'a ModuleDescriptor,
    action_id: &str,
) -> Option<&'a ModulePlayerActionSpec> {
    descriptor
        .runtime
        .player_actions
        .iter()
        .find(|action| action.id == action_id)
}

pub(super) fn runtime_action_requires_live_player_service(
    descriptor: &ModuleDescriptor,
    action_id: &str,
) -> bool {
    descriptor
        .runtime
        .player_list
        .as_ref()
        .is_some_and(|player_list| {
            player_list.action_id.as_deref() == Some(action_id)
                || player_list
                    .player_action_ids
                    .iter()
                    .any(|candidate| candidate == action_id)
        })
}

pub(super) fn render_runtime_action_command(
    action: &ModulePlayerActionSpec,
    target: Option<&str>,
    role: Option<&str>,
    require_target_binding: bool,
) -> Result<String, String> {
    render_runtime_action_command_with_request_id(
        action,
        target,
        role,
        require_target_binding,
        None,
    )
}

pub(super) fn render_runtime_action_command_with_request_id(
    action: &ModulePlayerActionSpec,
    target: Option<&str>,
    role: Option<&str>,
    require_target_binding: bool,
    request_id: Option<&str>,
) -> Result<String, String> {
    if action.transport == "palworld_rest" {
        return crate::live_players::palworld_rest::render_command(
            action,
            target,
            role,
            request_id,
            require_target_binding,
        );
    }
    let template = action.command_template.trim();
    if template.is_empty() || template.chars().any(char::is_control) {
        return Err(format!(
            "runtime action `{}` must declare one command line",
            action.id
        ));
    }

    let uses_target = template.contains("{{target}}");
    let uses_role = template.contains("{{role}}");
    let uses_request_id = template.contains("{{request_id}}");
    if require_target_binding && !uses_target {
        return Err(format!(
            "runtime action `{}` must bind the canonical live target",
            action.id
        ));
    }

    let target = validated_runtime_action_value(target, "target")?;
    // Snapshot targets are server identities. Trimming a quoted username can
    // select a different account; only operator-entered values are normalized.
    let target = if require_target_binding {
        target
    } else {
        target.trim()
    };
    if (action.target_required || uses_target) && target.is_empty() {
        return Err(format!(
            "runtime action `{}` requires a non-empty target",
            action.id
        ));
    }
    if !uses_target && !target.is_empty() {
        return Err(format!(
            "runtime action `{}` does not accept a target",
            action.id
        ));
    }

    let role = validated_runtime_action_value(role, "role")?.trim();
    if uses_role {
        if role.is_empty() {
            return Err(format!("runtime action `{}` requires a role", action.id));
        }
        if !role
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
        {
            return Err(format!(
                "runtime action `{}` requires a single-token role",
                action.id
            ));
        }
        if !action.role_values.iter().any(|value| value == role) {
            return Err(format!(
                "runtime action `{}` does not declare the requested role",
                action.id
            ));
        }
    } else if !role.is_empty() {
        return Err(format!(
            "runtime action `{}` does not accept a role",
            action.id
        ));
    }

    let request_id = request_id.unwrap_or_default().trim();
    if uses_request_id {
        if !valid_internal_request_id(request_id) {
            return Err(format!(
                "runtime action `{}` requires an internal 32-character lowercase request id",
                action.id
            ));
        }
    } else if !request_id.is_empty() {
        return Err(format!(
            "runtime action `{}` does not accept a request id",
            action.id
        ));
    }

    let encoded_target = encode_runtime_action_target(action, target)?;
    let rendered = template
        .replace("{{target}}", &encoded_target)
        .replace("{{role}}", role)
        .replace("{{request_id}}", request_id);
    if rendered.contains("{{") || rendered.contains("}}") {
        return Err(format!(
            "runtime action `{}` contains an unresolved template placeholder",
            action.id
        ));
    }
    normalize_runtime_command_input(&rendered)
}

fn valid_internal_request_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validated_runtime_action_value<'a>(
    value: Option<&'a str>,
    field: &str,
) -> Result<&'a str, String> {
    let value = value.unwrap_or_default();
    if value.chars().any(char::is_control)
        || value.contains("{{")
        || value.contains("}}")
        || [";", "&&", "||", "`", "$("]
            .iter()
            .any(|fragment| value.contains(fragment))
    {
        return Err(format!(
            "runtime action {field} contains unsupported command syntax"
        ));
    }
    Ok(value)
}

fn encode_runtime_action_target(
    action: &ModulePlayerActionSpec,
    target: &str,
) -> Result<String, String> {
    match action.target_encoding.as_deref().map(str::trim) {
        Some("quoted_string") => Ok(format!(
            "\"{}\"",
            target.replace('\\', "\\\\").replace('"', "\\\"")
        )),
        None | Some("") | Some("raw") => {
            if target.chars().any(char::is_whitespace) {
                return Err(format!(
                    "runtime action `{}` requires a single-token target",
                    action.id
                ));
            }
            Ok(target.to_string())
        }
        Some(encoding) => Err(format!(
            "runtime action `{}` declares unsupported target encoding `{encoding}`",
            action.id
        )),
    }
}

fn normalized_optional_metadata(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
#[path = "commands_runtime_actions_tests.rs"]
mod tests;
