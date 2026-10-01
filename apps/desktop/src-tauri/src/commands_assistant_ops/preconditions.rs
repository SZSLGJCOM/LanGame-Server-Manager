#[derive(Debug, Clone)]
pub(super) struct AssistantOperationPrecondition {
    instance_id: String,
    module_id: String,
    config_file_path: String,
    saves_path: String,
    backup_policy: (bool, bool, u32),
    settings_json: String,
    ports: Vec<(String, String, u16)>,
    bind_ip: String,
    status: std::mem::Discriminant<InstanceStatus>,
    active_process_count: usize,
    active_run: Option<AssistantRunIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssistantRunIdentity {
    run_id: i64,
    session_id: Option<String>,
    pid: Option<u32>,
    process_count: usize,
    processes: Vec<AssistantRunProcessIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssistantRunProcessIdentity {
    process_key: String,
    run_id: i64,
    session_id: Option<String>,
    pid: Option<u32>,
    process_identity: Option<ProcessIdentity>,
    status: String,
}

impl AssistantOperationPrecondition {
    pub(super) fn from_details(details: &InstanceDetails) -> Self {
        Self {
            instance_id: details.summary.id.clone(),
            module_id: details.summary.module_id.clone(),
            config_file_path: details.config_file_path.clone(),
            saves_path: details.saves_path.clone(),
            backup_policy: (
                details.backup_uses_declared_saves_path,
                details.auto_backup_on_stop,
                details.backup_retention_count,
            ),
            settings_json: details.settings_json.clone(),
            ports: assistant_port_snapshot(&details.ports),
            bind_ip: details.summary.bind_ip.clone(),
            status: std::mem::discriminant(&details.summary.status),
            active_process_count: details.summary.active_process_count,
            active_run: assistant_run_identity(details.active_run.as_ref()),
        }
    }

    pub(super) fn validate(&self, details: &InstanceDetails) -> Result<(), String> {
        if self.instance_id != details.summary.id
            || self.module_id != details.summary.module_id
            || self.config_file_path != details.config_file_path
            || self.saves_path != details.saves_path
            || self.backup_policy
                != (
                    details.backup_uses_declared_saves_path,
                    details.auto_backup_on_stop,
                    details.backup_retention_count,
                )
            || self.settings_json != details.settings_json
            || self.ports != assistant_port_snapshot(&details.ports)
            || self.bind_ip != details.summary.bind_ip
            || self.status != std::mem::discriminant(&details.summary.status)
            || self.active_process_count != details.summary.active_process_count
            || self.active_run != assistant_run_identity(details.active_run.as_ref())
        {
            return Err(String::from(
                "The server configuration or runtime changed after the assistant preview. Request a new preview before applying it.",
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn expected_settings_json(&self) -> &str {
        &self.settings_json
    }
}

fn assistant_run_identity(run: Option<&ActiveInstanceRun>) -> Option<AssistantRunIdentity> {
    run.map(|run| {
        let mut processes = run
            .processes
            .iter()
            .map(|process| AssistantRunProcessIdentity {
                process_key: process.process_key.clone(),
                run_id: process.run_id,
                session_id: process.session_id.clone(),
                pid: process.pid,
                process_identity: process.process_identity.clone(),
                status: process.status.clone(),
            })
            .collect::<Vec<_>>();
        processes.sort_by(|left, right| {
            left.process_key
                .cmp(&right.process_key)
                .then(left.run_id.cmp(&right.run_id))
        });
        AssistantRunIdentity {
            run_id: run.run_id,
            session_id: run.session_id.clone(),
            pid: run.pid,
            process_count: run.process_count,
            processes,
        }
    })
}

fn assistant_port_snapshot(ports: &[PortBinding]) -> Vec<(String, String, u16)> {
    let mut bindings = ports
        .iter()
        .map(|port| (port.name.clone(), port.protocol.clone(), port.port))
        .collect::<Vec<_>>();
    bindings.sort();
    bindings
}

pub(super) fn verify_assistant_settings_result(
    details: &InstanceDetails,
    expected: &Value,
    keys: &[String],
) -> Result<(), String> {
    let actual = serde_json::from_str::<Value>(&details.settings_json).map_err(|_| {
        String::from("Cannot verify saved settings: stored configuration is not valid JSON.")
    })?;
    let expected_object = expected.as_object().ok_or_else(|| {
        String::from("Cannot verify saved settings: expected configuration must be an object.")
    })?;
    let actual_object = actual.as_object().ok_or_else(|| {
        String::from("Cannot verify saved settings: stored configuration must be an object.")
    })?;
    let mut seen = HashSet::new();
    if keys.is_empty()
        || keys.iter().any(|key| {
            !seen.insert(key.as_str())
                || !expected_object.contains_key(key)
                || !actual_object.contains_key(key)
        })
    {
        return Err(String::from(
            "Cannot verify saved settings: the applied field set is empty, duplicated, or missing.",
        ));
    }
    // Compare the complete object as well as the applied fields, so preserving
    // the requested values cannot hide an unrelated setting being lost.
    if actual != *expected {
        return Err(String::from(
            "Saved settings do not match the assistant's expected result. The repair is not verified.",
        ));
    }
    Ok(())
}

pub(super) fn verify_assistant_ports_result(
    details: &InstanceDetails,
    expected: &[PortBinding],
    names: &[String],
) -> Result<(), String> {
    let mut seen = HashSet::new();
    if names.is_empty()
        || names.iter().any(|name| {
            !seen.insert(name.as_str())
                || expected.iter().filter(|port| port.name == *name).count() != 1
                || details
                    .ports
                    .iter()
                    .filter(|port| port.name == *name)
                    .count()
                    != 1
        })
    {
        return Err(String::from(
            "Cannot verify saved ports: the applied port set is empty, duplicated, or missing.",
        ));
    }
    if assistant_port_snapshot(&details.ports) != assistant_port_snapshot(expected) {
        return Err(String::from(
            "Saved ports do not match the assistant's expected result. The repair is not verified.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "preconditions_tests.rs"]
mod preconditions_tests;
