use super::*;

pub(super) struct WorldSaveRequest<'a> {
    pub(super) command: &'a str,
    pub(super) port_name: Option<&'a str>,
    pub(super) password_setting_key: Option<&'a str>,
    pub(super) enabled_setting_key: Option<&'a str>,
    pub(super) expected_run_id: Option<i64>,
    pub(super) redact_audit_payload: bool,
    pub(super) submitted_at_unix_ms: u128,
}

struct PreparedMapSave {
    process_key: String,
    endpoint: String,
    password: String,
}

/// Validate every configured live map before performing any network write. A
/// missing target must not silently turn an instance save into a partial save.
fn prepare_targets(
    details: &InstanceDetails,
    request: &WorldSaveRequest<'_>,
) -> Result<Vec<PreparedMapSave>, String> {
    app_core::ark_maps::processes(details)?
        .into_iter()
        .map(|map| {
            let projected = crate::commands::commands_runtime_ark::project_running_command(
                details,
                Some(&map.process_key),
            )?
            .ok_or("ARK world save requires an ARK instance")?;
            let (endpoint, password) = prepare_rcon_command(
                &projected,
                request.port_name,
                request.password_setting_key,
                request.enabled_setting_key,
            )
            .map_err(|error| format!("ARK map `{}`: {error}", map.process_key))?;
            Ok(PreparedMapSave {
                process_key: map.process_key,
                endpoint,
                password,
            })
        })
        .collect()
}

fn dispatch_failure(
    targets: &[PreparedMapSave],
    index: usize,
    delivery_unknown: bool,
    error: &str,
) -> String {
    let keys = |maps: &[PreparedMapSave]| {
        if maps.is_empty() {
            String::from("none")
        } else {
            maps.iter()
                .map(|map| map.process_key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    let failed = &targets[index].process_key;
    let delivery = if delivery_unknown {
        "command delivery and save completion are unknown"
    } else {
        "command was not sent"
    };
    format!(
        "ARK world save stopped at `{failed}` ({delivery}): {error}. Commands already sent: {}. Maps not attempted after this target: {}. Disk completion is not confirmed.",
        keys(&targets[..index]),
        keys(&targets[index + 1..]),
    )
}

async fn dispatch_using(
    details: &InstanceDetails,
    request: &WorldSaveRequest<'_>,
    execute: fn(&str, &str, &str) -> Result<String, String>,
    mut before_send: impl FnMut() -> Result<(), String>,
    mut record_sent: impl FnMut(&str, String),
) -> Result<Vec<String>, String> {
    let targets = prepare_targets(details, request).map_err(|error| {
        format!("ARK world-save preflight failed; no save commands were sent: {error}")
    })?;
    let mut sent = Vec::with_capacity(targets.len());
    for (index, target) in targets.iter().enumerate() {
        before_send().map_err(|error| dispatch_failure(&targets, index, false, &error))?;
        // Each target is attempted once. A transport error cannot establish
        // whether the server already executed this side-effecting command.
        let response = dispatch_prepared_rcon_command(
            target.endpoint.clone(),
            target.password.clone(),
            request.command,
            execute,
        )
        .await
        .map_err(|error| dispatch_failure(&targets, index, true, &error))?;
        super::super::commands_world_save::network_save_completion(
            &details.summary.module_id,
            "source_rcon",
            Some("save_world"),
            request.command,
            &response,
        )
        .map_err(|error| dispatch_failure(&targets, index, true, &error))?;
        record_sent(&target.process_key, response);
        sent.push(target.process_key.clone());
    }
    Ok(sent)
}

pub(super) async fn save_world(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    request: WorldSaveRequest<'_>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let sent = dispatch_using(
        details,
        &request,
        source_rcon_executor(&details.summary.module_id),
        || ensure_expected_supervisor_state(state, &details.summary.id, request.expected_run_id),
        |process_key, response| {
            append_desktop_app_log(
                storage,
                "info",
                "instance.runtime_command.rcon_sent",
                "Runtime save command sent through RCON to an ARK map",
                json!({
                    "instance_id": details.summary.id.as_str(),
                    "transport": "source_rcon",
                    "process_key": process_key,
                    "port_name": request.port_name.unwrap_or("rcon"),
                    "command": (!request.redact_audit_payload).then_some(request.command),
                    "response_text": (!request.redact_audit_payload)
                        .then(|| runtime_command_response_text(response)).flatten(),
                    "runtime_action_id": "save_world",
                }),
            );
        },
    )
    .await
    .inspect_err(|error| {
        append_desktop_app_log(
            storage,
            "warning",
            "instance.runtime_command.ark_world_save_failed",
            "ARK instance save did not send successfully to every enabled map",
            json!({
                "instance_id": details.summary.id.as_str(),
                "runtime_action_id": "save_world",
                "error": error.as_str(),
            }),
        );
    })?;
    Ok(InstanceRuntimeCommandResult {
        instance_id: details.summary.id.clone(),
        process_key: String::from("source_rcon"),
        display_name: String::from("ARK RCON (all enabled maps)"),
        pid: 0,
        command: request.command.to_owned(),
        response_text: Some(format!(
            "Native save request sent to {} ARK map(s): {}.",
            sent.len(),
            sent.join(", "),
        )),
        write_confirmation_pending: false,
        save_completion: Some(WorldSaveCompletion::Requested),
        submitted_at_unix_ms: request.submitted_at_unix_ms,
    })
}

#[cfg(test)]
#[path = "ark_world_save_tests.rs"]
mod tests;
