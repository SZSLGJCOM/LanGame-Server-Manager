//! Explicit existing-data acceptance. This module never creates a fixture or
//! changes settings; start/stop retain the application's normal save behavior.
use super::readiness::SmokeManifest;
use crate::runtime_service::ExistingClient;
use app_core::{
    InstanceDetails, InstanceRuntimeOverview, InstanceStatus, InstanceSummary, LaunchPlan,
    ProcessIdentity, RuntimeWindowSnapshot, StartInstanceResult, StopInstanceResult,
};
use app_modules::ModuleDescriptor;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[path = "commands_existing_instance_events.rs"]
mod events;
#[path = "commands_existing_instance_evidence.rs"]
mod evidence;
#[path = "commands_existing_instance_observation.rs"]
mod observation;
#[path = "commands_existing_instance_parallel.rs"]
mod parallel;
#[path = "commands_existing_instance_schedule.rs"]
mod schedule;
#[path = "commands_existing_instance_selection.rs"]
mod selection;
#[path = "commands_existing_instance_stop_evidence.rs"]
mod stop_evidence;
#[path = "commands_existing_instance_transcript.rs"]
mod transcript;
use stop_evidence::process_identities;

async fn read<T: DeserializeOwned>(
    client: &ExistingClient,
    command: &str,
    args: serde_json::Value,
) -> Result<T, String> {
    serde_json::from_value(client.request(command, args).await?)
        .map_err(|_| format!("invalid {command} response"))
}

#[derive(Default, Serialize)]
struct Receipt {
    module_id: String,
    instance_id: String,
    elapsed_ms: u128,
    phase: String,
    passed: bool,
    native_ready: bool,
    readiness_failure: Option<observation::ReadinessFailure>,
    startup_windows: observation::WindowEvidence,
    steady_observation: observation::SteadyEvidence,
    health_status: Option<String>,
    health_reason_code: Option<String>,
    native_health_mismatch: bool,
    managed_game_output: bool,
    managed_capture_sound: bool,
    visible_window_count: Option<usize>,
    inspected_process_count: usize,
    window_status: Option<String>,
    host_surface: Option<app_core::ProcessHostSurface>,
    requires_admin: bool,
    run_id: Option<i64>,
    process_run_ids: Vec<i64>,
    started_identity_confirmed: bool,
    stop_api_succeeded: bool,
    normal_stop_succeeded: bool,
    stop_evidence: stop_evidence::StopEvidence,
    stop_elapsed_ms: u128,
    stop_exit_codes: Vec<Option<i32>>,
    owned_processes_exited: bool,
    stopped_state_confirmed: bool,
    settings_unchanged: bool,
    ports_unchanged: bool,
    log_readback_ok: bool,
    logs: Vec<evidence::LogEvidence>,
    events: events::EventEvidence,
    failures: Vec<String>,
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: starts and normally stops explicitly selected existing instances with bounded concurrency; uses their real saves and never changes fixture settings"]
async fn existing_instance_catalog_acceptance() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = super::command_smoke_lock().lock().await;
    let options = selection::Options::from_env()?;
    let descriptors = app_modules::discover_modules(&options.modules_root)?;
    options.validate_catalog(&descriptors)?;
    let client = ExistingClient::new(
        &options.backend_executable,
        options.backend_pid,
        options.backend_creation_time,
    )?;
    let status = client.request("runtime_service_status", json!({})).await?;
    if status["pid"].as_u64() != Some(u64::from(options.backend_pid))
        || status["protocol"].as_u64() != Some(1)
        || status["shutdown_in_progress"].as_bool() != Some(false)
    {
        return Err("selected backend is not ready for acceptance".into());
    }
    let summaries: Vec<InstanceSummary> =
        read(&client, "list_instances_from_storage", json!({})).await?;
    let selected = options.select(&summaries)?;
    let directory = options.create_output_directory()?;
    evidence::write_receipt(&directory, "selection", &options)?;
    parallel::run(client, descriptors, selected, directory)
        .await
        .map_err(Into::into)
}

async fn run_one(
    client: &ExistingClient,
    descriptor: &ModuleDescriptor,
    id: &str,
    directory: &Path,
    reservation: &InstanceDetails,
    ownership: &parallel::Ownership,
) -> (Receipt, bool) {
    let began = Instant::now();
    let mut report = Receipt {
        module_id: descriptor.summary.id.clone(),
        instance_id: id.into(),
        phase: "preflight".into(),
        ..Default::default()
    };
    let prepared = prepare(client, descriptor, id, reservation, ownership).await;
    let (before, plan, smoke, baselines, native_baselines, expected_world, prior_run_ids) =
        match prepared {
            Ok(prepared) => prepared,
            Err(reason) => {
                report.failures.push(reason);
                report.elapsed_ms = began.elapsed().as_millis();
                // Other reserved workers may be running. This target must still
                // be stopped, and no unowned instance may have become active.
                let safe = ownership.verify_stopped_target(client, id).await.is_ok();
                return (report, safe);
            }
        };
    report.host_surface = Some(plan.host_surface.clone());
    report.requires_admin = plan.requires_admin;
    let mut cursor = events::Observer::new(id);
    if cursor.establish(client).await.is_err() {
        report.failures.push("event_baseline_unavailable".into());
        return (report, true);
    }
    report.phase = "start_requested".into();
    if evidence::write_receipt(
        directory,
        &format!("{}.start-requested", report.module_id),
        &report,
    )
    .is_err()
    {
        ownership.halt();
        report.failures.push("cannot_persist_start_intent".into());
        return (report, true);
    }
    let mut args = json!({"instanceId": id});
    if let Some(preview) = expected_world {
        args["expectedWorldStart"] = preview;
    }
    // Recheck after event baselining, immediately before sending the start. A
    // failed sibling closes admission without cancelling workers already running.
    if let Err(reason) = ownership.verify_stopped_target(client, id).await {
        report.failures.push(reason);
        return (report, false);
    }
    if !ownership.mark_start_requested(id).await {
        report
            .failures
            .push("batch_admission_halted_before_start".into());
        return (report, true);
    }
    let started: Result<StartInstanceResult, String> = cursor
        .watch(
            client,
            observation::watch_windows(
                client,
                id,
                &mut report.startup_windows,
                read(client, "start_instance_process", args),
            ),
        )
        .await;
    let mut identities = Vec::<(u32, ProcessIdentity)>::new();
    let mut paths = Vec::<PathBuf>::new();
    let mut current = None;
    let mut started_scope = None;
    match started {
        Ok(started) => {
            let scope = stop_evidence::StartedScope::from_result(id, &report.module_id, &started);
            report.run_id = Some(started.run_id);
            report.process_run_ids = started.processes.iter().map(|item| item.run_id).collect();
            paths.extend(
                started
                    .processes
                    .iter()
                    .map(|item| PathBuf::from(&item.log_path)),
            );
            if paths.is_empty() {
                paths.push(PathBuf::from(&started.log_path));
            }
            match read::<InstanceDetails>(
                client,
                "read_instance_details_from_storage",
                json!({"instanceId": id}),
            )
            .await
            {
                Ok(instance) => {
                    identities = process_identities(&instance);
                    report.started_identity_confirmed = scope.matches_instance(&instance)
                        && identities.len() == started.process_count;
                    report.phase = "native_readiness".into();
                    let readiness_deadline = Instant::now() + smoke.readiness_budget();
                    if evidence::write_receipt(
                        directory,
                        &format!("{}.started", report.module_id),
                        &report,
                    )
                    .is_err()
                    {
                        ownership.halt();
                        report
                            .failures
                            .push("cannot_persist_started_identity".into());
                    } else if report.started_identity_confirmed {
                        let ready = cursor
                            .watch(
                                client,
                                observation::watch_windows(
                                    client,
                                    id,
                                    &mut report.startup_windows,
                                    smoke.wait_ready_existing(
                                        descriptor,
                                        &instance,
                                        Path::new(&plan.install_root),
                                        baselines,
                                    ),
                                ),
                            )
                            .await;
                        report.native_ready = ready.is_ok();
                        if let Err(reason) = ready {
                            report.readiness_failure =
                                Some(observation::ReadinessFailure::from_reason(&reason));
                        }
                    }
                    if !report.native_ready {
                        report.failures.push("native_readiness_failed".into());
                    } else {
                        report.phase = "steady_observation".into();
                        // Keep consuming the real backend event stream throughout
                        // the observation, including an otherwise quiet server.
                        if let Err(reason) = cursor
                            .watch(
                                client,
                                observation::observe_steady(
                                    client,
                                    &instance,
                                    readiness_deadline,
                                    &mut report.steady_observation,
                                ),
                            )
                            .await
                        {
                            report.failures.push(reason);
                        }
                    }
                    current = Some(instance);
                }
                Err(_) => report
                    .failures
                    .push("started_instance_readback_failed".into()),
            }
            started_scope = Some(scope);
        }
        Err(_) => report
            .failures
            .push("start_failed_or_outcome_uncertain".into()),
    }
    if !report.startup_windows.sound() {
        report
            .failures
            .push("startup_window_observation_failed".into());
    }
    // Always attempt normal stop once after a start request, including a
    // rejected/uncertain response. Never retry start or force-kill a process.
    if let Ok(overview) = read::<InstanceRuntimeOverview>(
        client,
        "read_instance_runtime_overview_from_storage",
        json!({"instanceId": id}),
    )
    .await
    {
        // Failed starts (including Windrose bootstrap) may publish a run before
        // rejecting the request. Retain only newly observed run log paths.
        for run in &overview.recent_runs {
            for process in &run.processes {
                if !prior_run_ids.contains(&process.run_id)
                    && let Some(path) = &process.log_path
                {
                    paths.push(PathBuf::from(path));
                }
            }
            if !prior_run_ids.contains(&run.run_id)
                && let Some(path) = &run.log_path
            {
                paths.push(PathBuf::from(path));
            }
        }
        report.health_status = Some(overview.health.status);
        report.health_reason_code = Some(overview.health.reason.code);
    } else {
        report.failures.push("health_readback_failed".into());
    }
    report.native_health_mismatch =
        report.native_ready && report.health_status.as_deref() != Some("ready");
    if let Ok(window) = read::<RuntimeWindowSnapshot>(
        client,
        "read_instance_runtime_window_snapshot",
        json!({"instanceId": id}),
    )
    .await
    {
        report.visible_window_count = Some(window.windows.len());
        report.inspected_process_count = window.inspected_process_count;
        report.window_status = Some(window.status);
    } else {
        report.failures.push("window_readback_failed".into());
    }
    if let Some(run_id) = report.run_id {
        report.log_readback_ok = read::<app_core::LogTailSnapshot>(
            client,
            "read_instance_log_document_from_storage",
            json!({"instanceId": id, "runId": run_id, "maxLines": 160}),
        )
        .await
        .is_ok_and(|document| {
            document.read_error.is_none()
                && document.source_path.is_some()
                && document.lines.iter().any(|line| evidence::game_line(line))
        });
    }
    stop_evidence::record(
        client,
        stop_evidence::StopBaseline {
            before: &before,
            scope: started_scope.as_ref(),
            identities: &identities,
            prior_run_ids: &prior_run_ids,
        },
        &mut cursor,
        &mut report,
        &mut paths,
    )
    .await;
    paths.extend(cursor.pending_paths());
    paths.sort();
    paths.dedup();
    report.logs.extend(
        paths
            .iter()
            .map(|path| evidence::inspect(path, "managed_run")),
    );
    for (path, baseline) in native_baselines {
        if !paths.contains(&path) {
            report.logs.push(evidence::inspect_since(&path, baseline));
        }
    }
    report.managed_game_output = !paths.is_empty()
        && report
            .logs
            .iter()
            .filter(|log| log.source == "managed_run")
            .all(|log| log.game_lines > 0);
    report.managed_capture_sound = !paths.is_empty()
        && report
            .logs
            .iter()
            .filter(|log| log.source == "managed_run")
            .all(evidence::LogEvidence::sound);
    report.events = cursor.finish(client, &report.logs).await;
    // A successfully stopped instance with a failed start still cannot prove
    // that an unpublished child was cleaned up. Stop the suite conservatively.
    let safe = (report.normal_stop_succeeded || report.stop_evidence.safe_after_late_exit())
        && report.started_identity_confirmed
        && current.is_some();
    report.passed = safe
        && report.normal_stop_succeeded
        && report.native_ready
        && report.startup_windows.sound()
        && report.steady_observation.passed
        && report.health_status.as_deref() == Some("ready")
        && report.managed_game_output
        && report.managed_capture_sound
        && report.log_readback_ok
        && report.visible_window_count == Some(0)
        && report.window_status.as_deref() == Some("clear")
        && report.inspected_process_count > 0
        && report.settings_unchanged
        && report.ports_unchanged
        && report.events.sound_for(&report.process_run_ids)
        && report.failures.is_empty();
    report.phase = if safe { "completed" } else { "aborted" }.into();
    report.elapsed_ms = began.elapsed().as_millis();
    (report, safe)
}

type Prepared = (
    InstanceDetails,
    LaunchPlan,
    SmokeManifest,
    super::readiness::ProbeBaselines,
    Vec<(PathBuf, evidence::NativeBaseline)>,
    Option<serde_json::Value>,
    Vec<i64>,
);

async fn prepare(
    client: &ExistingClient,
    descriptor: &ModuleDescriptor,
    id: &str,
    reservation: &InstanceDetails,
    ownership: &parallel::Ownership,
) -> Result<Prepared, String> {
    ownership.verify_stopped_target(client, id).await?;
    let before: InstanceDetails = read(
        client,
        "read_instance_details_from_storage",
        json!({"instanceId": id}),
    )
    .await?;
    if before.summary.module_id != descriptor.summary.id
        || !selection::is_inactive(&before.summary)
        || before.active_run.is_some()
    {
        return Err("selected_instance_not_stopped".into());
    }
    if before.settings_json != reservation.settings_json
        || before.summary.bind_ip != reservation.summary.bind_ip
        || serde_json::to_value(&before.ports).ok() != serde_json::to_value(&reservation.ports).ok()
    {
        return Err("selected_instance_changed_after_port_reservation".into());
    }
    let previous: InstanceRuntimeOverview = read(
        client,
        "read_instance_runtime_overview_from_storage",
        json!({"instanceId": id}),
    )
    .await?;
    let prior_run_ids = previous
        .recent_runs
        .iter()
        .flat_map(|run| {
            std::iter::once(run.run_id).chain(run.processes.iter().map(|process| process.run_id))
        })
        .collect();
    let plan: LaunchPlan =
        read(client, "preview_instance_launch", json!({"instanceId": id})).await?;
    if !plan.ready_to_launch || !plan.executable_exists {
        return Err("launch_preflight_failed".into());
    }
    let smoke = SmokeManifest::load(descriptor)?;
    selection::preconditions(&before, &smoke)?;
    let install = Path::new(&plan.install_root);
    let baselines = smoke.log_baselines(&before, install)?;
    let native_baselines = smoke
        .evidence_log_paths(&before, install)?
        .into_iter()
        .map(|path| {
            let baseline = evidence::NativeBaseline::capture(&path);
            (path, baseline)
        })
        .collect();
    let expected_world = if descriptor.summary.id == "dontstarve" {
        Some(
            client
                .request("preview_dontstarve_world_start", json!({"instanceId": id}))
                .await?,
        )
    } else {
        None
    };
    Ok((
        before,
        plan,
        smoke,
        baselines,
        native_baselines,
        expected_world,
        prior_run_ids,
    ))
}

async fn all_inactive(client: &ExistingClient) -> Result<(), String> {
    let instances: Vec<InstanceSummary> =
        read(client, "list_instances_from_storage", json!({})).await?;
    for summary in instances {
        if !selection::is_inactive(&summary) {
            return Err("another_instance_is_not_inactive".into());
        }
        if matches!(summary.status, InstanceStatus::Error) {
            let details: InstanceDetails = read(
                client,
                "read_instance_details_from_storage",
                json!({"instanceId": summary.id}),
            )
            .await?;
            if !selection::is_inactive(&details.summary) || details.active_run.is_some() {
                return Err("failed_instance_still_has_an_active_run".into());
            }
        }
    }
    Ok(())
}
