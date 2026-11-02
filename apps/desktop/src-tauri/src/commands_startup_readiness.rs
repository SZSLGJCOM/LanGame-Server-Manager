//! One pre-registration barrier for every module. Evidence providers cannot
//! commit Running, release process ownership, or bypass cancellation.
use super::startup_log_cursor::StartupLogCursor;
use super::startup_palworld::PalworldReadiness;
use super::startup_probe::{StartupProbe, StartupProcessTarget};
use super::{dst_lifecycle, startup_sources, startup_status};
use crate::state::{
    DesktopState, RuntimeStartReservationLease, spawn_blocking_storage_context_task,
};
use app_core::startup_readiness::{
    ModuleStartupReadinessSpec, StartupReadinessFallback, StartupReadinessProbe,
};
use app_core::{InstanceDetails, ProcessLaunchPlan};
use app_storage::{StartupLogReadiness, StorageBootstrap};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

#[path = "commands_startup_transcript.rs"]
mod transcript;
use transcript::NativeStartupTranscript;

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const CANCELLED: &str = "Server startup readiness was cancelled.";

pub(super) struct StartupContext<'a> {
    pub state: &'a DesktopState,
    pub storage: &'a StorageBootstrap,
    pub instance: &'a InstanceDetails,
    pub source: &'a str,
    pub session_id: &'a str,
    pub reservation: &'a RuntimeStartReservationLease,
    pub first_world_plan: Option<super::super::commands_astroneer_startup::Plan>,
}

pub(super) struct StartupSession {
    timeout: Duration,
    dst: bool,
    probes: Vec<(String, Evidence)>,
}

enum Evidence {
    Log {
        cursor: Box<StartupLogCursor>,
        state: StartupLogReadiness,
        managed: bool,
        fallback: Option<PalworldReadiness>,
        required_probe: Option<StartupProbe>,
    },
    Network(StartupProbe),
    Status {
        root: PathBuf,
        baseline: startup_status::Baseline,
    },
}

struct Target {
    key: String,
    process: StartupProcessTarget,
    evidence: Evidence,
    ready: bool,
    transcript: NativeStartupTranscript,
}

#[derive(Default)]
struct Progress {
    ready: bool,
    more: bool,
}

#[derive(Clone)]
struct NativeLogBinding {
    console_path: PathBuf,
    session_id: String,
}

impl StartupSession {
    pub(super) async fn prepare(
        instance: &InstanceDetails,
        spec: Option<&ModuleStartupReadinessSpec>,
        plans: &[ProcessLaunchPlan],
        reservation: &RuntimeStartReservationLease,
    ) -> Result<Self, String> {
        let spec = spec.cloned().ok_or_else(|| {
            format!(
                "Module `{}` does not declare startup readiness evidence",
                instance.summary.module_id
            )
        })?;
        if plans.is_empty() || plans.len() > 64 || !(1_000..=1_800_000).contains(&spec.timeout_ms) {
            return Err(
                "Startup readiness requires a bounded timeout and nonempty process group".into(),
            );
        }
        let timeout = Duration::from_millis(spec.timeout_ms);
        let instance = instance.clone();
        let plans = plans.to_vec();
        let worker = spawn_blocking_storage_context_task(reservation, move || {
            let dst = matches!(spec.probe, StartupReadinessProbe::DstWorld);
            let mut probes = Vec::new();
            let mut paths = BTreeSet::new();
            for plan in &plans {
                let evidence = match &spec.probe {
                    StartupReadinessProbe::RuntimeLog { source, fallback } => {
                        let (path, managed) = startup_sources::log_path(source, &instance, plan)?;
                        if !paths.insert(
                            path.to_string_lossy()
                                .replace('\\', "/")
                                .to_ascii_lowercase(),
                        ) {
                            return Err("Startup processes must use distinct log sources".into());
                        }
                        Evidence::Log {
                            cursor: Box::new(StartupLogCursor::capture(path, managed)?),
                            state: StartupLogReadiness::new(&instance.summary.module_id)?,
                            managed,
                            required_probe: None,
                            fallback: match fallback {
                                Some(StartupReadinessFallback::PalworldRest) => {
                                    PalworldReadiness::prepare(&instance)?
                                }
                                None => None,
                            },
                        }
                    }
                    StartupReadinessProbe::A2s {
                        port_name,
                        port_argument,
                        ready_log,
                    } => {
                        let probe = StartupProbe::a2s(startup_sources::query_port(
                            &instance,
                            plan,
                            port_name,
                            port_argument.as_deref(),
                            plans.len() > 1,
                        )?);
                        if let Some(source) = ready_log {
                            let (path, managed) =
                                startup_sources::log_path(source, &instance, plan)?;
                            if !paths.insert(
                                path.to_string_lossy()
                                    .replace('\\', "/")
                                    .to_ascii_lowercase(),
                            ) {
                                return Err(
                                    "Startup processes must use distinct log sources".into()
                                );
                            }
                            Evidence::Log {
                                cursor: Box::new(StartupLogCursor::capture(path, managed)?),
                                state: StartupLogReadiness::new(&instance.summary.module_id)?,
                                managed,
                                fallback: None,
                                required_probe: Some(probe),
                            }
                        } else {
                            Evidence::Network(probe)
                        }
                    }
                    StartupReadinessProbe::AstroneerWorld => {
                        Evidence::Network(StartupProbe::astroneer(&instance)?)
                    }
                    StartupReadinessProbe::ReturntomoriaStatus => {
                        if plans.len() != 1 {
                            return Err(
                                "Native status readiness requires a single owned process".into()
                            );
                        }
                        let root = PathBuf::from(&plan.launch_plan.install_root);
                        let baseline = startup_status::capture(&root)?;
                        Evidence::Status { root, baseline }
                    }
                    StartupReadinessProbe::DstWorld => continue,
                };
                probes.push((plan.process_key.clone(), evidence));
            }
            Ok(Self {
                timeout,
                dst,
                probes,
            })
        });
        tokio::select! {
            biased;
            _ = reservation.cancelled() => Err(CANCELLED.into()),
            result = worker => result.map_err(|error| format!("Startup evidence preparation failed: {error}"))?,
        }
    }

    pub(super) async fn wait_until_ready(
        self,
        processes: &mut [(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
        context: StartupContext<'_>,
    ) -> Result<(), String> {
        // Log baselines precede spawn, but this budget starts after the bounded
        // module preparation/listener barriers. Cold IL2CPP, mod downloads and
        // serialized maps retain their own validated progress budgets.
        let deadline = tokio::time::Instant::now() + self.timeout;
        if let Some(plan) = context.first_world_plan {
            let (_, process) = processes
                .iter()
                .find(|(plan, _)| plan.process_key == "main")
                .ok_or("First-world initialization has no primary process")?;
            if process.child.is_none() {
                return Err("First-world initialization lost its owned process handle".into());
            }
            let expected = super::super::commands_astroneer_startup::SpawnedTarget {
                pid: process.pid,
                identity: process.process_identity.clone(),
            };
            tokio::select! {
                biased;
                _ = context.reservation.cancelled() => return Err(CANCELLED.into()),
                _ = tokio::time::sleep_until(deadline) => return Err("First-world startup timed out; existing data is retained and creation will not be resubmitted automatically".into()),
                result = super::super::commands_astroneer_startup::initialize_spawned(
                    context.state, context.storage, context.instance, plan, context.reservation, expected
                ) => result?,
            }
        }
        if self.dst {
            // DST's nonce-scoped Lua provider proves all worlds and shard links.
            // It runs under the same deadline, reservation and cleanup as logs/API.
            return tokio::select! {
                biased;
                _ = context.reservation.cancelled() => Err(CANCELLED.into()),
                _ = tokio::time::sleep_until(deadline) => Err("Server startup timed out waiting for shard worlds".into()),
                result = dst_lifecycle::wait_until_ready(context.state, context.storage, context.instance,
                    processes, context.source, context.reservation) => result,
            };
        }
        if self.probes.len() != processes.len() {
            return Err("Startup evidence does not match the owned process group".into());
        }
        let host = match context.instance.summary.bind_ip.as_str() {
            "0.0.0.0" | "" => "127.0.0.1",
            "::" => "::1",
            value => value,
        };
        let targets = self
            .probes
            .into_iter()
            .zip(processes.iter())
            .map(|((key, evidence), (plan, process))| {
                if key != plan.process_key || process.child.is_none() {
                    return Err("Startup evidence lost its owned process handle".into());
                }
                Ok(Target {
                    key,
                    process: StartupProcessTarget {
                        pid: process.pid,
                        identity: process.process_identity.clone(),
                        host: host.into(),
                    },
                    evidence,
                    ready: false,
                    transcript: NativeStartupTranscript::default(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let native_log_binding = (context.instance.summary.module_id == "valheim"
            && targets
                .iter()
                .any(|target| matches!(&target.evidence, Evidence::Log { managed: false, .. })))
        .then(|| NativeLogBinding {
            console_path: PathBuf::from(&processes[0].0.log_path),
            session_id: context.session_id.into(),
        });
        wait_for_targets(
            targets,
            deadline,
            self.timeout,
            context.reservation,
            native_log_binding,
        )
        .await
    }
}

async fn wait_for_targets(
    mut targets: Vec<Target>,
    deadline: tokio::time::Instant,
    timeout: Duration,
    reservation: &RuntimeStartReservationLease,
    native_log_binding: Option<NativeLogBinding>,
) -> Result<(), String> {
    let startup_transcript_path = reservation
        .startup_session()?
        .map(|(_, receipt)| PathBuf::from(receipt.source_path));
    loop {
        if reservation.is_cancelled() {
            return Err(CANCELLED.into());
        }
        let pending = targets
            .iter()
            .filter(|target| !target.ready)
            .map(|target| target.key.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let expired = || {
            format!(
                "Server startup timed out after {} seconds waiting for readiness from: {pending}",
                timeout.as_secs()
            )
        };
        if tokio::time::Instant::now() >= deadline {
            return Err(expired());
        }
        let worker_reservation = reservation.clone();
        let binding = native_log_binding.clone();
        let transcript_path = startup_transcript_path.clone();
        let worker = spawn_blocking_storage_context_task(reservation, move || {
            let progress = poll_targets_with_transcript(
                &mut targets,
                |target| {
                    app_runtime::process_matches_identity(
                        target.process.pid,
                        &target.process.identity,
                    )
                    .map_err(|error| error.to_string())
                },
                || worker_reservation.is_cancelled(),
                transcript_path.as_deref(),
            )?;
            if progress.ready
                && let Some(binding) = binding
            {
                publish_native_log_binding(&mut targets, &binding, &worker_reservation)?;
            }
            Ok::<_, String>((targets, progress))
        });
        let (observed, progress) = tokio::select! {
            biased;
            _ = reservation.cancelled() => return Err(CANCELLED.into()),
            _ = tokio::time::sleep_until(deadline) => return Err(expired()),
            result = worker => result.map_err(|error| format!("Startup readiness worker failed: {error}"))??,
        };
        targets = observed;
        if reservation.is_cancelled() {
            return Err(CANCELLED.into());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(expired());
        }
        if progress.ready {
            return Ok(());
        }
        if progress.more {
            tokio::task::yield_now().await;
        } else {
            tokio::select! {
                biased;
                _ = reservation.cancelled() => return Err(CANCELLED.into()),
                _ = tokio::time::sleep_until(deadline) => return Err(expired()),
                _ = tokio::time::sleep(POLL_INTERVAL) => {},
            }
        }
    }
}

fn publish_native_log_binding(
    targets: &mut [Target],
    binding: &NativeLogBinding,
    reservation: &RuntimeStartReservationLease,
) -> Result<(), String> {
    if targets.len() != 1 || binding.session_id.is_empty() {
        return Err("Valheim health handoff requires one owned session process".into());
    }
    let target = &mut targets[0];
    let mut is_running = |target: &Target| {
        app_runtime::process_matches_identity(target.process.pid, &target.process.identity)
            .map_err(|error| error.to_string())
    };
    check_process(target, &mut is_running, &|| reservation.is_cancelled())?;
    let Evidence::Log {
        cursor,
        state,
        managed: false,
        ..
    } = &target.evidence
    else {
        return Err("Valheim health handoff lost its native log evidence".into());
    };
    let mut snapshot = cursor.native_snapshot()?;
    app_storage::record_valheim_native_log(
        app_storage::ValheimLogHandoff {
            console_path: &binding.console_path,
            native_path: &snapshot.path,
            file: &mut snapshot.file,
            offset: snapshot.offset,
            discard_fragment: snapshot.discard_fragment,
            session_id: &binding.session_id,
            process_key: &target.key,
            pid: target.process.pid,
            identity: &target.process.identity,
        },
        state,
    )
    .map_err(|error| format!("Cannot retain the current Valheim log source: {error}"))?;
    check_process(target, &mut is_running, &|| reservation.is_cancelled())
}

#[cfg(test)]
fn poll_targets(
    targets: &mut [Target],
    is_running: impl FnMut(&Target) -> Result<bool, String>,
    cancelled: impl Fn() -> bool,
) -> Result<Progress, String> {
    poll_targets_with_transcript(targets, is_running, cancelled, None)
}

fn poll_targets_with_transcript(
    targets: &mut [Target],
    mut is_running: impl FnMut(&Target) -> Result<bool, String>,
    cancelled: impl Fn() -> bool,
    startup_path: Option<&std::path::Path>,
) -> Result<Progress, String> {
    let mut progress = Progress::default();
    for target in targets.iter_mut() {
        check_process(target, &mut is_running, &cancelled)?;
        // Continue observing already-ready maps: a later loading/offline event
        // must revoke their evidence while another process is still starting.
        target.ready = match &mut target.evidence {
            Evidence::Log {
                cursor,
                state,
                managed,
                fallback,
                required_probe,
            } => {
                let observed = cursor.poll(&cancelled)?;
                if observed.reset {
                    state.reset();
                }
                let current_lines = observed
                    .lines
                    .iter()
                    .filter(|line| {
                        *managed
                            || app_storage::startup_log_line_is_current(
                                &target.process.identity,
                                line,
                            )
                    })
                    .collect::<Vec<_>>();
                if !*managed && let Some(startup_path) = startup_path {
                    target.transcript.record(
                        startup_path,
                        &target.key,
                        &target.process.identity,
                        cursor,
                        &current_lines,
                        observed.oversized_record,
                    )?;
                }
                for line in current_lines {
                    state.observe(line);
                }
                progress.more |= observed.more;
                if state.has_failed() || observed.more {
                    false
                } else if state.is_ready() {
                    match required_probe {
                        Some(probe) => probe.poll(&target.process)?,
                        None => true,
                    }
                } else if let Some(probe) = fallback {
                    probe.poll(&target.process)?
                } else {
                    false
                }
            }
            Evidence::Network(probe) => probe.poll(&target.process)?,
            Evidence::Status { root, baseline } => {
                startup_status::ready_for_process(root, baseline, &target.process.identity)?
            }
        };
    }
    // I/O can overlap a child exiting or PID reuse; cached evidence must never
    // outlive the original process identity, including earlier-ready siblings.
    for target in targets.iter() {
        check_process(target, &mut is_running, &cancelled)?;
    }
    progress.ready = !targets.is_empty() && targets.iter().all(|target| target.ready);
    Ok(progress)
}

fn check_process(
    target: &Target,
    is_running: &mut impl FnMut(&Target) -> Result<bool, String>,
    cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    if cancelled() {
        return Err(CANCELLED.into());
    }
    if is_running(target)? {
        Ok(())
    } else {
        Err(format!(
            "Process `{}` exited or changed identity before startup completed",
            target.key
        ))
    }
}

#[cfg(test)]
#[path = "commands_startup_readiness_tests.rs"]
mod tests;
