//! Optional observation after an already failed stop. Never sends a command,
//! changes the production allowance, or converts the failed stop into success.
use super::NativeRuntime;
use app_core::InstanceDetails;
use std::path::Path;
use std::time::{Duration, Instant};

fn budget(module: &str, value: Option<&str>) -> Result<Option<Duration>, &'static str> {
    let Some(value) = value else {
        return Ok(None);
    };
    if module != "arksurvivalascended" {
        return Err("wrong_module");
    }
    let seconds: u64 = value.parse().map_err(|_| "invalid_seconds")?;
    if !(1..=120).contains(&seconds) {
        return Err("seconds_out_of_range");
    }
    Ok(Some(Duration::from_secs(seconds)))
}

fn label(state: Option<bool>) -> &'static str {
    match state {
        Some(true) => "running",
        Some(false) => "exited",
        None => "unknown",
    }
}

pub(super) async fn observe<R: tauri::Runtime>(
    runtime: &NativeRuntime<'_, '_, R>,
    expected: &InstanceDetails,
    stop_elapsed: Duration,
) -> Result<(), &'static str> {
    let seconds = std::env::var("LANGAME_NATIVE_ASA_STOP_OBSERVE_SECONDS").ok();
    let Some(allowance) = budget(&expected.summary.module_id, seconds.as_deref())? else {
        return Ok(());
    };
    let fixture = runtime
        .package
        .root
        .canonicalize()
        .map_err(|_| "fixture_unavailable")?;
    let config = Path::new(&expected.config_file_path)
        .canonicalize()
        .map_err(|_| "config_unavailable")?;
    let install = runtime
        .effective_install_root
        .canonicalize()
        .map_err(|_| "runtime_unavailable")?;
    if !config.starts_with(&fixture) || !install.starts_with(&fixture) {
        return Err("outside_fixture");
    }
    let run = expected.active_run.as_ref().ok_or("run_missing")?;
    let process = run
        .processes
        .first()
        .filter(|process| {
            run.processes.len() == 1
                && run.process_count == 1
                && process.is_primary
                && process.run_id == run.run_id
                && process.session_id == run.session_id
                && run
                    .session_id
                    .as_ref()
                    .is_some_and(|session| !session.is_empty())
                && process.status == "running"
                && !process.crash_flag
                && process.exit_code.is_none()
        })
        .ok_or("run_ambiguous")?;
    let pid = process
        .pid
        .filter(|pid| *pid > 0 && Some(*pid) == run.pid)
        .ok_or("pid_missing")?;
    let identity = process
        .process_identity
        .as_ref()
        .filter(|identity| identity.creation_time > 0)
        .ok_or("identity_missing")?;
    let image = Path::new(&identity.image_path)
        .canonicalize()
        .map_err(|_| "image_unavailable")?;
    if image != install.join("ShooterGame/Binaries/Win64/ArkAscendedServer.exe") {
        return Err("wrong_image");
    }
    let current = app_storage::read_instance_details(&runtime.storage.paths, &expected.summary.id)
        .await
        .map_err(|_| "instance_read_failed")?;
    let current_run = current.active_run.as_ref().ok_or("current_run_missing")?;
    if current_run.run_id != run.run_id
        || current_run.session_id != run.session_id
        || current_run.pid != run.pid
        || current_run.processes.len() != 1
        || current_run.processes[0].process_identity != process.process_identity
        || current_run.processes[0].process_key != process.process_key
    {
        return Err("generation_changed");
    }
    let started = Instant::now();
    let mut last = None;
    let mut next_report = Duration::ZERO;
    loop {
        let supervisor = runtime.state.runtime_supervisor.clone();
        let instance = expected.summary.id.clone();
        let key = process.process_key.clone();
        let run_id = run.run_id;
        let observation = tokio::task::spawn_blocking(move || {
            let mut supervisor = supervisor.lock().map_err(|_| "supervisor_lock")?;
            let root = supervisor
                .owned_process_is_running(&instance, run_id, &key, pid)
                .map_err(|_| "root_inspection")?;
            let tree = supervisor
                .owned_process_tree_is_running(&instance, run_id, &key, pid)
                .map_err(|_| "tree_inspection")?;
            Ok::<_, &'static str>((root, tree))
        })
        .await
        .map_err(|_| "inspection_worker")??;
        let elapsed = started.elapsed();
        if last != Some(observation) || elapsed >= next_report || elapsed >= allowance {
            eprintln!(
                "NATIVE_ASA_STOP_OBSERVATION elapsed_ms={} stop_elapsed_ms={} root={} tree={} original_stop_error=retained command_replayed=false",
                elapsed.as_millis(),
                (stop_elapsed + elapsed).as_millis(),
                label(observation.0),
                label(observation.1)
            );
            let logs = super::super::diagnostics::failed_run_logs(expected, &install, &fixture)?;
            super::print_native_failure(
                "post_stop_observation",
                "original stop failure retained",
                &logs,
                runtime.settings,
                &fixture,
            );
            last = Some(observation);
            next_report = elapsed + Duration::from_secs(15);
        }
        if observation == (Some(false), Some(false)) {
            crate::commands::commands_runtime_supervision::reconcile_runtime_state(runtime.state)
                .await
                .map_err(|_| "reconcile_failed")?;
            let overview = app_storage::read_instance_runtime_overview(
                &runtime.storage.paths,
                &expected.summary.id,
            )
            .await
            .map_err(|_| "history_read_failed")?;
            let completed = overview
                .recent_runs
                .iter()
                .find(|record| record.run_id == run.run_id)
                .ok_or("history_run_missing")?;
            for process in &completed.processes {
                eprintln!(
                    "NATIVE_ASA_STOP_OBSERVATION outcome=natural_exit run_id={} exit_code={:?} crash_flag={} original_stop_error=retained",
                    process.run_id, process.exit_code, process.crash_flag
                );
            }
            // Reconciliation consumed the original owner only after both its
            // root handle and complete Job were observed exited above.
            runtime
                .package
                .cleanup_allowed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return Ok(());
        }
        if elapsed >= allowance {
            eprintln!(
                "NATIVE_ASA_STOP_OBSERVATION outcome=budget_exhausted original_stop_error=retained"
            );
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500).min(allowance.saturating_sub(elapsed))).await;
    }
}

#[test]
fn post_stop_observation_is_off_by_default_closed_to_asa_and_at_most_120_seconds() {
    assert_eq!(budget("arksurvivalascended", None), Ok(None));
    assert_eq!(budget("minecraft", None), Ok(None));
    assert_eq!(
        budget("arksurvivalascended", Some("120")),
        Ok(Some(Duration::from_secs(120)))
    );
    for value in ["0", "121", "-1", "true", " 10"] {
        assert!(budget("arksurvivalascended", Some(value)).is_err());
    }
    for module in ["arksurvivalevolved", "minecraft", "squad"] {
        assert!(budget(module, Some("120")).is_err());
    }
    assert_eq!(label(None), "unknown");
    assert_ne!(label(None), label(Some(false)));
}
