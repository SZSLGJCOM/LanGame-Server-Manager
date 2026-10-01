use super::commands_runtime_supervision::StrictBindEvaluation;
use super::*;
use app_core::ModulePortGroupSpec;

pub(super) fn port_groups(
    instance: &InstanceDetails,
    base: &[ModulePortGroupSpec],
) -> Result<Vec<ModulePortGroupSpec>, String> {
    if !app_core::ark_maps::is_ark(&instance.summary.module_id) {
        return Ok(base.to_vec());
    }
    let settings =
        serde_json::from_str(&instance.settings_json).map_err(|error| error.to_string())?;
    app_core::ark_maps::port_groups(&instance.summary.module_id, &settings, base)
}

pub(super) fn build_launch_plans(
    settings: &AppSettings,
    module: &ModuleDetails,
    instance: &InstanceDetails,
    install_root: Option<&str>,
) -> Result<Vec<ProcessLaunchPlan>, String> {
    app_core::ark_maps::processes(instance)?
        .into_iter()
        .map(|map| {
            let projected = app_core::ark_maps::project_process(instance, Some(&map.process_key))?;
            Ok(ProcessLaunchPlan {
                process_key: map.process_key,
                display_name: map.display_name,
                log_path: String::new(),
                launch_plan: build_launch_plan_with_override(
                    settings,
                    module,
                    &projected,
                    install_root,
                )
                .map_err(|error| error.to_string())?,
            })
        })
        .collect()
}

/// Configuration identifies a map; an active run identifies the owned process
/// that may receive a command. Both must agree before selecting its RCON port.
pub(super) fn project_running_command(
    instance: &InstanceDetails,
    process_key: Option<&str>,
) -> Result<Option<InstanceDetails>, String> {
    if !app_core::ark_maps::is_ark(&instance.summary.module_id) {
        return Ok(None);
    }
    let projected = app_core::ark_maps::project_process(instance, process_key)?;
    let key = process_key.unwrap_or("main");
    if !running_target(instance, key) {
        return Err(format!("ARK map process `{key}` is not running."));
    }
    Ok(Some(projected))
}

fn running_target(instance: &InstanceDetails, key: &str) -> bool {
    instance.active_run.as_ref().is_some_and(|run| {
        if run.processes.is_empty() {
            key == "main" && run.pid.is_some()
        } else {
            run.processes.iter().any(|process| {
                process.process_key == key && process.status == "running" && process.pid.is_some()
            })
        }
    })
}

pub(super) fn has_multiple_maps(instance: &InstanceDetails) -> Result<bool, String> {
    if !app_core::ark_maps::is_ark(&instance.summary.module_id) {
        return Ok(false);
    }
    Ok(app_core::ark_maps::processes(instance)?.len() > 1)
}

fn map_bind_ports(
    instance: &InstanceDetails,
    policy: &ModuleBindAddressSpec,
    process_key: &str,
) -> Result<Vec<PortBinding>, String> {
    let projected = app_core::ark_maps::project_process(instance, Some(process_key))?;
    policy
        .port_names
        .iter()
        .map(|name| {
            projected
                .ports
                .iter()
                .find(|port| port.name == *name)
                .cloned()
                .ok_or_else(|| format!("ARK map `{process_key}` is missing bind port `{name}`."))
        })
        .collect()
}

pub(super) fn required_bind_ports(
    instance: &InstanceDetails,
    policy: &ModuleBindAddressSpec,
) -> Result<Vec<PortBinding>, String> {
    let mut required = Vec::new();
    for map in app_core::ark_maps::processes(instance)? {
        for mut port in map_bind_ports(instance, policy, &map.process_key)? {
            port.name = format!("{}:{}", map.process_key, port.name);
            required.push(port);
        }
    }
    Ok(required)
}

#[cfg(test)]
fn evaluate_bind_endpoints(
    instance: &InstanceDetails,
    policy: &ModuleBindAddressSpec,
    expected_address: &str,
    endpoints: &[ProcessNetworkEndpoint],
) -> Result<StrictBindEvaluation, String> {
    let required = required_bind_ports(instance, policy)?;
    Ok(evaluate_bind_ports(&required, expected_address, endpoints))
}

pub(super) fn evaluate_bind_ports(
    required_ports: &[PortBinding],
    expected_address: &str,
    endpoints: &[ProcessNetworkEndpoint],
) -> StrictBindEvaluation {
    let wildcard = super::commands_runtime_supervision::is_wildcard_bind_address(expected_address);
    let mut missing = Vec::new();
    for port in required_ports {
        let Some((process_key, _)) = port.name.split_once(':') else {
            return StrictBindEvaluation::Failed {
                message: format!("ARK listener `{}` has no map ownership.", port.name),
            };
        };
        let matching = endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.process_key == process_key
                    && endpoint.local_port == port.port
                    && endpoint.protocol.eq_ignore_ascii_case(&port.protocol)
            })
            .collect::<Vec<_>>();
        if !wildcard
            && let Some(wrong) = matching
                .iter()
                .find(|endpoint| endpoint.local_address != expected_address)
        {
            return StrictBindEvaluation::Failed {
                message: format!(
                    "ARK listener `{}` ({}/{}) is bound to {} instead of selected address {} (pid {}).",
                    port.name,
                    port.protocol,
                    port.port,
                    wrong.local_address,
                    expected_address,
                    wrong.owning_pid,
                ),
            };
        }
        if matching.is_empty() {
            missing.push(port.name.clone());
        }
    }
    if missing.is_empty() {
        StrictBindEvaluation::Ready
    } else {
        StrictBindEvaluation::Pending {
            missing_port_names: missing,
        }
    }
}

/// Send each phase to every live map, then wait once for the phase. This saves
/// all worlds before any map exits without multiplying grace delays by map count.
pub(super) fn shutdown_for_running_maps(
    instance: &InstanceDetails,
    shutdown: &ModuleShutdownSpec,
) -> Result<ModuleShutdownSpec, String> {
    let maps = app_core::ark_maps::processes(instance)?;
    let keys = maps
        .iter()
        .filter(|map| running_target(instance, &map.process_key))
        .map(|map| map.process_key.as_str())
        .collect::<Vec<_>>();
    let mut result = shutdown.clone();
    result.commands.clear();
    for command in &shutdown.commands {
        for (index, key) in keys.iter().enumerate() {
            let mut mapped = command.clone();
            mapped.process_key = Some((*key).to_owned());
            mapped.wait_after_ms = if index + 1 == keys.len() {
                command.wait_after_ms
            } else {
                0
            };
            result.commands.push(mapped);
        }
    }
    Ok(result)
}

#[cfg(test)]
#[path = "commands_runtime_ark_tests.rs"]
mod tests;
