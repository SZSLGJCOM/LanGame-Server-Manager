const ASSISTANT_NETWORK_TARGETS: usize = 16;
const ASSISTANT_NETWORK_ENDPOINTS: usize = 64;

pub(super) async fn read_assistant_network_endpoints(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance_id: &str,
) -> Result<Value, String> {
    let _lease = state.begin_storage_context_operation("assistant network inspection")?;
    ensure_storage_context_snapshot_current(state, storage, "assistant network inspection")?;
    let before = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    let targets = assistant_network_targets(&before)?;
    let inspection = run_assistant_evidence_read(state, move || {
        validate_assistant_network_processes(&targets)?;
        // Empty ports means all endpoints owned by the verified instance process
        // tree, so a wrong native bind port remains observable.
        let result = WindowsPlatform::inspect_process_network_endpoints(&targets, &[])?;
        validate_assistant_network_processes(&targets)?;
        Ok(result)
    })
    .await?;
    let after = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    assistant_network_evidence(&before, &after, inspection)
}

fn assistant_network_targets(
    instance: &InstanceDetails,
) -> Result<Vec<WindowInspectionTarget>, String> {
    let run = instance
        .active_run
        .as_ref()
        .ok_or("No active instance run is registered; endpoint ownership is unknown.")?;
    if run.processes.is_empty()
        || run.processes.len() > ASSISTANT_NETWORK_TARGETS
        || run.process_count != run.processes.len()
    {
        return Err(String::from(
            "The active run has no bounded complete process identity set.",
        ));
    }
    run.processes
        .iter()
        .map(|process| {
            if process.run_id != run.run_id || process.session_id != run.session_id {
                return Err(String::from(
                    "The registered process belongs to another run or session.",
                ));
            }
            Ok(WindowInspectionTarget {
                pid: process
                    .pid
                    .ok_or("A registered process has no PID; endpoint ownership is unknown.")?,
                process_identity: process.process_identity.clone().ok_or(
                    "A registered process has no creation identity; endpoint ownership is unknown.",
                )?,
                process_key: process.process_key.clone(),
                display_name: process.display_name.clone(),
            })
        })
        .collect()
}

fn validate_assistant_network_processes(targets: &[WindowInspectionTarget]) -> Result<(), String> {
    for target in targets {
        if !inspect_process_identity(target.pid)
            .map_err(|error| error.to_string())?
            .as_ref()
            .is_some_and(|current| process_identities_match(&target.process_identity, current))
        {
            return Err(String::from(
                "An instance process exited or changed identity during network inspection; repeat the observation.",
            ));
        }
    }
    Ok(())
}

fn assistant_network_evidence(
    before: &InstanceDetails,
    after: &InstanceDetails,
    inspection: app_platform_win::ProcessNetworkInspectionResult,
) -> Result<Value, String> {
    if inspection.inspected_process_count == 0 {
        return Err(String::from(
            "No process identity was verified by endpoint inspection; empty endpoints are not a successful observation.",
        ));
    }
    if !assistant_runtime_observation_is_current(before, None, after, None)
        || serde_json::to_value(&before.ports).map_err(|error| error.to_string())?
            != serde_json::to_value(&after.ports).map_err(|error| error.to_string())?
    {
        return Err(String::from(
            "The instance run or declared ports changed during network inspection; evidence was discarded.",
        ));
    }
    let mut endpoints = inspection.endpoints;
    endpoints.sort_by(|left, right| {
        left.local_port
            .cmp(&right.local_port)
            .then_with(|| left.protocol.cmp(&right.protocol))
            .then_with(|| left.owning_pid.cmp(&right.owning_pid))
    });
    let omitted = endpoints.len().saturating_sub(ASSISTANT_NETWORK_ENDPOINTS);
    endpoints.truncate(ASSISTANT_NETWORK_ENDPOINTS);
    Ok(json!({
        "scope":"verified_instance_process_tree", "instanceId":before.summary.id,
        "runId": before.active_run.as_ref().map(|run| run.run_id),
        "inspectedProcessCount":inspection.inspected_process_count,
        "endpoints":endpoints.iter().map(|endpoint| json!({
            "protocol":endpoint.protocol, "localAddress":endpoint.local_address,
            "localPort":endpoint.local_port, "pid":endpoint.owning_pid,
            "processKey":redact_assistant_provider_text(&endpoint.process_key), "relation":endpoint.relation,
        })).collect::<Vec<_>>(),
        "omittedEndpointCount":omitted,
        "guidance":"Observed local TCP/UDP endpoints only. An endpoint is not proof of game readiness, firewall reachability or remote connectivity. A missing endpoint is not proof of a configuration error.",
    }))
}

#[cfg(test)]
#[path = "workspace_diagnostics_tests.rs"]
mod workspace_diagnostics_tests;

include!("workspace_validation.rs");
