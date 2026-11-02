//! Current-run health for modules whose declared startup evidence is A2S INFO.
use crate::ProcessProbeTarget;
use app_core::startup_readiness::{ModuleStartupReadinessSpec, StartupReadinessProbe};
use app_core::{ActiveInstanceRun, InstanceStatus, PortBinding, RuntimeHealth};

pub(super) fn log_health(
    module_id: &str,
    status: &InstanceStatus,
    snapshot: &app_core::LogTailSnapshot,
    run: Option<&ActiveInstanceRun>,
    ports: &[PortBinding],
    health: RuntimeHealth,
) -> RuntimeHealth {
    if !matches!(health.status.as_str(), "starting" | "ready") {
        return health;
    }
    let (Some(run), Some(path)) = (run, snapshot.source_path.as_deref()) else {
        return result(
            "starting",
            "starting_waiting_logs",
            "Waiting for the current run's final native startup phase.",
        );
    };
    let mut evidence = app_core::LogTailSnapshot {
        source_path: snapshot.source_path.clone(),
        lines: Vec::new(),
        total_lines: 0,
        truncated: false,
        read_error: None,
    };
    match super::console_health_evidence::observe_a2s_world(
        module_id,
        std::path::Path::new(path),
        run,
    ) {
        Ok(lines) => {
            evidence.total_lines = lines.len();
            evidence.lines = lines;
        }
        Err(error) => evidence.read_error = Some(error.to_string()),
    }
    super::analyze_runtime_health(module_id, status, &evidence, Some(run), ports)
}

pub(super) fn apply(
    spec: Option<&ModuleStartupReadinessSpec>,
    status: &InstanceStatus,
    host: &str,
    run: Option<&ActiveInstanceRun>,
    ports: &[PortBinding],
    health: RuntimeHealth,
) -> RuntimeHealth {
    apply_with(
        spec,
        status,
        host,
        run,
        ports,
        health,
        crate::poll_owned_a2s_info,
    )
}

fn apply_with(
    spec: Option<&ModuleStartupReadinessSpec>,
    status: &InstanceStatus,
    host: &str,
    run: Option<&ActiveInstanceRun>,
    ports: &[PortBinding],
    health: RuntimeHealth,
    observe: impl FnOnce(&ProcessProbeTarget, u16) -> Result<bool, String>,
) -> RuntimeHealth {
    // Never hide a fatal log, failed run, warning or an unknown/stopped state.
    if !matches!(status, InstanceStatus::Running | InstanceStatus::Starting)
        || !matches!(health.status.as_str(), "starting" | "ready")
    {
        return health;
    }
    let Some(ModuleStartupReadinessSpec {
        probe:
            StartupReadinessProbe::A2s {
                port_name,
                ready_log,
                ..
            },
        ..
    }) = spec
    else {
        return health;
    };
    // The protocol can answer before the world is loaded. Its success cannot
    // replace the declared final native phase, including failed-start survivors.
    if ready_log.is_some() && health.status != "ready" {
        return health;
    }
    let observed =
        target(host, run, ports, port_name).and_then(|(target, port)| observe(&target, port));
    match observed {
        Ok(true) if ready_log.is_some() => health,
        Ok(true) => result(
            "ready",
            "ready_signal",
            "The current owned server returned a complete A2S INFO response.",
        ),
        Ok(false) => result(
            "starting",
            "starting_tasks",
            "Waiting for a complete A2S INFO response from the current server.",
        ),
        Err(_) => result(
            "warning",
            "native_readiness_unconfirmed",
            "Protocol readiness could not be verified for the current server process.",
        ),
    }
}

fn target(
    host: &str,
    run: Option<&ActiveInstanceRun>,
    ports: &[PortBinding],
    port_name: &str,
) -> Result<(ProcessProbeTarget, u16), String> {
    let run = run.ok_or("The active server run is unavailable.")?;
    if run.run_id <= 0
        || run.process_count != 1
        || run.processes.len() != 1
        || run.session_id.as_deref().is_none_or(str::is_empty)
    {
        return Err("A2S health requires one current primary process generation.".into());
    }
    let process = &run.processes[0];
    if !process.is_primary
        || process.process_key.trim().is_empty()
        || process.run_id != run.run_id
        || process.session_id != run.session_id
        || process.pid != run.pid
        || process.status != "running"
        || process.exit_code.is_some()
        || process.crash_flag
    {
        return Err("The current primary process record changed.".into());
    }
    let pid = process
        .pid
        .filter(|pid| *pid > 0)
        .ok_or("The current process ID is unavailable.")?;
    let identity = process
        .process_identity
        .clone()
        .filter(|identity| identity.creation_time > 0 && !identity.image_path.is_empty())
        .ok_or("The current process creation identity is unavailable.")?;
    let mut matches = ports.iter().filter(|binding| binding.name == port_name);
    let port = matches
        .next()
        .ok_or("The declared A2S port is unavailable.")?;
    if port.port == 0 || !port.protocol.eq_ignore_ascii_case("udp") || matches.next().is_some() {
        return Err("The declared A2S port is invalid or ambiguous.".into());
    }
    Ok((
        ProcessProbeTarget {
            pid,
            identity,
            host: host.into(),
        },
        port.port,
    ))
}

fn result(status: &str, code: &str, summary: &str) -> RuntimeHealth {
    RuntimeHealth {
        status: status.into(),
        summary: summary.into(),
        reason: super::runtime_health_reason(code, &[]),
        matched_line: None,
    }
}

#[cfg(test)]
#[path = "runtime_a2s_health_tests.rs"]
mod tests;
