use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use app_core::{InstanceDetails, InstanceStatus, ProcessIdentity};
use app_platform_win::{WindowInspectionTarget, WindowsPlatform};
use app_runtime::RuntimeSupervisor;

use crate::state::DesktopState;

#[derive(Clone)]
pub(super) struct Endpoint {
    pub(super) instance_id: String,
    pub(super) port: u16,
    pub(super) credential_identity: String,
    pub(super) insecure_local_access: bool,
    pid: u32,
    run_id: i64,
    process_key: String,
    identity: ProcessIdentity,
    supervisor: Arc<Mutex<RuntimeSupervisor>>,
}

pub(super) fn credential_identity(instance: &InstanceDetails) -> Result<String, String> {
    if instance.summary.module_id != "satisfactory" {
        return Err("The instance is not a Satisfactory server.".into());
    }
    let port = api_port(instance)?;
    // A native identity is scoped to the instance and game port. Paths can
    // change during storage relocation; credentials must remain recoverable.
    // A restored/reset native identity rejects stale tokens at verification.
    let identity = format!("{}|{port}", instance.summary.id);
    Ok(super::protocol::sha256_hex(identity.as_bytes()))
}

#[cfg(test)]
#[path = "satisfactory_world_context_tests.rs"]
mod tests;

fn api_port(instance: &InstanceDetails) -> Result<u16, String> {
    let mut ports = instance
        .ports
        .iter()
        .filter(|port| port.name == "game_tcp" && port.protocol.eq_ignore_ascii_case("tcp"));
    let port = ports
        .next()
        .filter(|port| port.port > 0)
        .ok_or_else(|| "The Satisfactory instance has no HTTPS API port.".to_string())?;
    if ports.next().is_some() {
        return Err("The Satisfactory HTTPS API port is ambiguous.".into());
    }
    Ok(port.port)
}

pub(super) fn resolve(
    state: &DesktopState,
    instance: &InstanceDetails,
) -> Result<Option<Endpoint>, String> {
    let credential_identity = credential_identity(instance)?;
    if matches!(instance.summary.status, InstanceStatus::Stopped) && instance.active_run.is_none() {
        return Ok(None);
    }
    if !matches!(
        instance.summary.status,
        InstanceStatus::Starting | InstanceStatus::Running
    ) {
        return Err("Start the Satisfactory server before using its world controls.".into());
    }
    let run = instance
        .active_run
        .as_ref()
        .ok_or_else(|| "The Satisfactory server has no owned active run.".to_string())?;
    let process = run
        .processes
        .iter()
        .find(|process| process.is_primary)
        .filter(|process| {
            run.process_count == 1
                && run.processes.len() == 1
                && instance.summary.active_process_count == 1
                && process.run_id == run.run_id
                && process.status == "running"
                && process.exit_code.is_none()
                && !process.crash_flag
        })
        .ok_or_else(|| "The Satisfactory managed process is unavailable.".to_string())?;
    let identity = process
        .process_identity
        .clone()
        .filter(|identity| identity.creation_time > 0 && !identity.image_path.is_empty())
        .ok_or_else(|| "The Satisfactory process identity is unavailable.".to_string())?;
    let settings: serde_json::Value = serde_json::from_str(&instance.settings_json)
        .map_err(|_| "The Satisfactory instance settings are malformed.".to_string())?;
    Ok(Some(Endpoint {
        instance_id: instance.summary.id.clone(),
        port: api_port(instance)?,
        credential_identity,
        insecure_local_access: settings
            .get("allow_insecure_local_api")
            .and_then(|value| value.as_bool())
            == Some(true),
        pid: process
            .pid
            .filter(|pid| *pid > 0)
            .ok_or_else(|| "The Satisfactory process ID is unavailable.".to_string())?,
        run_id: run.run_id,
        process_key: process.process_key.clone(),
        identity,
        supervisor: state.runtime_supervisor.clone(),
    }))
}

impl Endpoint {
    #[cfg(test)]
    pub(super) fn fixture() -> Self {
        Self {
            instance_id: "fixture".into(),
            port: 1,
            credential_identity: "fixture".into(),
            insecure_local_access: true,
            pid: 1,
            run_id: 1,
            process_key: "fixture".into(),
            identity: ProcessIdentity {
                creation_time: 1,
                image_path: "fixture".into(),
            },
            supervisor: Arc::new(Mutex::new(RuntimeSupervisor::default())),
        }
    }
    pub(super) async fn check(&self) -> Result<(), String> {
        let endpoint = self.clone();
        tokio::task::spawn_blocking(move || endpoint.check_blocking())
            .await
            .map_err(|_| "The Satisfactory process ownership check did not complete.".to_string())?
    }

    fn check_blocking(&self) -> Result<(), String> {
        const ERROR: &str = "The Satisfactory API listener is no longer owned by this instance. Refresh the server state.";
        self.check_process_blocking()?;
        let target = WindowInspectionTarget {
            pid: self.pid,
            process_key: self.process_key.clone(),
            display_name: "Satisfactory".into(),
            process_identity: self.identity.clone(),
        };
        let endpoints = WindowsPlatform::inspect_process_network_endpoints(&[target], &[self.port])
            .map_err(|_| ERROR.to_string())?;
        let pids = endpoints
            .endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.protocol == "tcp"
                    && endpoint.local_port == self.port
                    && matches!(endpoint.local_address.as_str(), "127.0.0.1" | "0.0.0.0")
            })
            .map(|endpoint| endpoint.owning_pid)
            .collect::<BTreeSet<_>>();
        if pids != BTreeSet::from([self.pid]) {
            return Err(ERROR.into());
        }
        Ok(())
    }

    pub(super) async fn check_process(&self) -> Result<(), String> {
        let endpoint = self.clone();
        tokio::task::spawn_blocking(move || endpoint.check_process_blocking())
            .await
            .map_err(|_| "The Satisfactory process ownership check did not complete.".to_string())?
    }

    fn check_process_blocking(&self) -> Result<(), String> {
        const ERROR: &str = "The Satisfactory process is no longer owned by this instance. Refresh the server state.";
        if app_runtime::inspect_process_identity(self.pid)
            .map_err(|_| ERROR.to_string())?
            .as_ref()
            != Some(&self.identity)
        {
            return Err(ERROR.into());
        }
        {
            let mut supervisor = self.supervisor.lock().map_err(|_| ERROR.to_string())?;
            if !supervisor
                .matches_running_process(
                    &self.instance_id,
                    self.run_id,
                    &self.process_key,
                    self.pid,
                )
                .map_err(|_| ERROR.to_string())?
            {
                return Err(ERROR.into());
            }
        }
        Ok(())
    }
}
