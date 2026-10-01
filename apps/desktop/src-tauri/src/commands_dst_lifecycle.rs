use super::*;
use app_runtime::StartupConsoleWriter;

#[path = "dst_console_protocol.rs"]
mod protocol;

#[path = "dst_startup_watchdog.rs"]
mod startup_watchdog;

const STOP_TIMEOUT: Duration = Duration::from_secs(90);
const PROBE_INTERVAL: Duration = Duration::from_secs(2);

type ConsoleWrite = tokio::task::JoinHandle<(StartupConsoleWriter, Result<(), String>)>;

struct StartupProbe {
    writer: Option<StartupConsoleWriter>,
    pending: Option<ConsoleWrite>,
    log: ProbeLog,
    ready: bool,
    next_write: Instant,
}

struct ProbeLog {
    path: PathBuf,
    tail: RuntimeLogTailState,
}

impl ProbeLog {
    fn new(path: &str) -> Self {
        Self {
            path: PathBuf::from(path),
            tail: RuntimeLogTailState::default(),
        }
    }

    async fn skip_existing(&mut self) -> Result<(), String> {
        let path = self.path.clone();
        self.tail = tokio::task::spawn_blocking(move || {
            crate::runtime_log_stream::runtime_log_tail_at_end(&path)
        })
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("Could not locate DST save confirmation log: {error}"))?;
        Ok(())
    }

    async fn lines(&mut self) -> Result<Vec<String>, String> {
        let path = self.path.clone();
        let mut tail = std::mem::take(&mut self.tail);
        let (tail, result) = tokio::task::spawn_blocking(move || {
            let result = read_runtime_log_delta_bounded(&path, &mut tail, 256 * 1024, 16 * 1024);
            (tail, result)
        })
        .await
        .map_err(|error| format!("DST log reader failed: {error}"))?;
        self.tail = tail;
        match result {
            Ok(delta) => match delta.stream_error {
                Some(error) => Err(format!("DST process log capture is incomplete: {error}")),
                None => Ok(delta.lines),
            },
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(format!(
                "Could not read DST process log {}: {error}",
                self.path.display()
            )),
        }
    }
}

/// The native listener can exist before map initialization. Ask the running Lua
/// VM instead; the primary also confirms every enabled secondary's connection.
pub(super) async fn wait_until_ready(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    processes: &mut [(ProcessLaunchPlan, app_runtime::SpawnedProcess)],
    source: &str,
    reservation: &RuntimeStartReservationLease,
) -> Result<(), String> {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let settings: serde_json::Value =
        serde_json::from_str(&instance.settings_json).map_err(|error| error.to_string())?;
    let required_peers = app_core::dst_shards::dst_shards(&settings)?
        .into_iter()
        .filter(|shard| shard.process_key != "master")
        .map(|shard| shard.directory)
        .collect::<Vec<_>>();
    let mut probes = processes
        .iter_mut()
        .map(|(plan, process)| {
            Ok(StartupProbe {
                writer: Some(
                    process
                        .startup_console()
                        .map_err(|error| error.to_string())?,
                ),
                pending: None,
                log: ProbeLog::new(&plan.log_path),
                ready: false,
                next_write: Instant::now(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let started = Instant::now();
    let mut watchdog = startup_watchdog::StartupWatchdog::new(started);

    loop {
        ensure_runtime_start_allowed(state, storage, &instance.summary.id, source, reservation)?;
        for ((plan, process), probe) in processes.iter_mut().zip(&mut probes) {
            if let Some(status) = process
                .child
                .as_mut()
                .ok_or_else(|| format!("DST {} lost its process handle", plan.display_name))?
                .try_wait()
                .map_err(|error| error.to_string())?
            {
                return Err(format!(
                    "DST {} exited before its world was ready (code {:?}); see {}",
                    plan.display_name,
                    status.code(),
                    plan.log_path
                ));
            }
            if probe
                .pending
                .as_ref()
                .is_some_and(|write| write.is_finished())
            {
                let pending = probe
                    .pending
                    .take()
                    .ok_or("DST startup writer disappeared")?;
                let (writer, result) = pending
                    .await
                    .map_err(|error| format!("DST startup writer failed: {error}"))?;
                probe.writer = Some(writer);
                result?;
            }
            for line in probe.log.lines().await? {
                // Download failure can be followed by a playable vanilla world.
                // Any failure in this batch must win over its READY marker.
                if let Some(error) = protocol::startup_failure(&line, &nonce) {
                    return Err(format!(
                        "DST {} failed during startup: {error}; see {}",
                        plan.display_name, plan.log_path
                    ));
                }
                probe.ready |= protocol::is_ack(&line, "READY", &nonce);
                if protocol::world_generation_progress(&line) {
                    watchdog.observe_progress(Instant::now());
                }
            }
            if !probe.ready && probe.pending.is_none() && Instant::now() >= probe.next_write {
                let mut writer = probe
                    .writer
                    .take()
                    .ok_or("DST startup console is unavailable")?;
                let command = protocol::ready_command(
                    &nonce,
                    if plan.process_key == "master" {
                        &required_peers
                    } else {
                        &[]
                    },
                );
                probe.pending = Some(tokio::task::spawn_blocking(move || {
                    let result = writer
                        .write_line(&command)
                        .map_err(|error| error.to_string());
                    (writer, result)
                }));
                probe.next_write = Instant::now() + PROBE_INTERVAL;
            }
        }
        if probes
            .iter()
            .all(|probe| probe.ready && probe.pending.is_none())
        {
            return Ok(());
        }
        if watchdog.expired(Instant::now()) {
            let waiting = processes
                .iter()
                .zip(&probes)
                .filter(|(_, probe)| !probe.ready)
                .map(|((plan, _), _)| plan.display_name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "DST world initialization timed out ({waiting}; {} seconds elapsed, {} seconds without generation progress or {} seconds total). Check shard logs, world settings, mods and the enabled shard connections.",
                started.elapsed().as_secs(),
                startup_watchdog::IDLE_LIMIT.as_secs(),
                startup_watchdog::TOTAL_LIMIT.as_secs(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// A failed save/exit confirmation leaves the process under supervision. The
/// caller must not follow this error with forced cleanup or an automatic backup.
pub(super) async fn save_and_shutdown(
    state: &DesktopState,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
) -> Result<(), String> {
    save_and_shutdown_with_timeout(state, storage, details, STOP_TIMEOUT).await
}

async fn save_and_shutdown_with_timeout(
    state: &DesktopState,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    timeout: Duration,
) -> Result<(), String> {
    let Some(active) = read_active_instance_run(&storage.paths, &details.summary.id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let running = active
        .processes
        .iter()
        .filter(|process| process.status == "running")
        .collect::<Vec<_>>();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let command = protocol::shutdown_command(&nonce);
    let mut logs = running
        .iter()
        .map(|process| {
            process
                .log_path
                .as_deref()
                .map(ProbeLog::new)
                .ok_or_else(|| {
                    format!(
                        "DST {} has no log for save confirmation",
                        process.display_name
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut saved = vec![false; running.len()];
    let deadline = Instant::now() + timeout;

    for log in &mut logs {
        log.skip_existing().await?;
    }

    // Send once to each surviving shard. Each native save callback acknowledges
    // that shard; a dead Master must not prevent surviving secondaries saving.
    for process in &running {
        super::dispatch_managed_stdin_command(
            state, &details.summary.id, Some(&process.process_key), &command, None,
        ).await.map_err(|error| format!("DST {} save/stop command failed: {error}. The remaining server processes have not been forcibly stopped.", process.display_name))?;
    }
    loop {
        let mut all_exited = true;
        for ((process, log), acknowledged) in running.iter().zip(&mut logs).zip(&mut saved) {
            for line in log.lines().await? {
                *acknowledged |= protocol::is_ack(&line, "SAVED", &nonce);
            }
            let alive = shutdown_process_is_running(
                state,
                &details.summary.id,
                process,
                process_matches_identity,
            )?;
            all_exited &= !alive;
            if !alive && !*acknowledged {
                // Drain the last buffered output once more after observing exit.
                for line in log.lines().await? {
                    *acknowledged |= protocol::is_ack(&line, "SAVED", &nonce);
                }
                if !*acknowledged {
                    return Err(format!(
                        "DST {} exited without confirming its save. Automatic backup was skipped; inspect its log before restarting.",
                        process.display_name
                    ));
                }
            }
        }
        if all_exited && saved.iter().all(|saved| *saved) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "DST did not confirm saving and exiting within {} seconds. Remaining processes are still managed and were not forcibly stopped; inspect the shard logs and retry Stop.",
                timeout.as_secs()
            ));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn shutdown_process_is_running(
    state: &DesktopState,
    instance_id: &str,
    process: &app_core::InstanceProcessState,
    inspect: impl FnOnce(
        u32,
        &app_core::ProcessIdentity,
    ) -> Result<bool, app_runtime::RuntimeProcessError>,
) -> Result<bool, String> {
    let pid = process.pid.ok_or("DST process has no recorded PID")?;
    // Poll the original process object during shutdown. Reopening by PID can
    // fail while the process is exiting, even when its owned handle is valid.
    let owned = state
        .runtime_supervisor
        .lock()
        .map_err(|_| "runtime supervisor lock poisoned")?
        .owned_process_is_running(instance_id, process.run_id, &process.process_key, pid)
        .map_err(|error| error.to_string())?;
    if let Some(alive) = owned {
        return Ok(alive);
    }
    let identity = process
        .process_identity
        .as_ref()
        .ok_or("DST process has no recorded identity")?;
    inspect(pid, identity).map_err(|error| error.to_string())
}

pub(in crate::commands) async fn finish_confirmed_stop(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance_id: &str,
    active: &ActiveInstanceRun,
) -> Result<Vec<app_core::StoppedProcess>, String> {
    let managed = state
        .runtime_supervisor
        .lock()
        .map_err(|_| "runtime supervisor lock poisoned")?
        .take_running_for_stop(instance_id);
    let mut exits = HashMap::new();
    if let Some(mut managed) = managed {
        // Job termination and ConPTY cleanup can wait for kernel objects. Keep
        // that work off the async executor, and return ownership on failure.
        let observed = tokio::task::spawn_blocking(move || {
            let observed = managed
                .processes
                .iter_mut()
                .map(|process| {
                    let child = process
                        .child
                        .as_mut()
                        .ok_or("DST exit handle is unavailable")?;
                    let status = child
                        .try_wait()
                        .map_err(|error| error.to_string())?
                        .ok_or("DST process remains alive after save confirmation")?;
                    child.finish_process_tree().map_err(|error| {
                        format!(
                            "DST {} process tree cleanup failed: {error}",
                            process.display_name
                        )
                    })?;
                    Ok((process.run_id, status.code()))
                })
                .collect::<Result<Vec<_>, String>>();
            match observed {
                Ok(observed) => {
                    drop(managed);
                    Ok(observed)
                }
                Err(error) => Err((Box::new(managed), error)),
            }
        })
        .await
        .map_err(|error| format!("DST process tree cleanup task failed: {error}"))?;
        match observed {
            Ok(observed) => {
                for (run_id, _) in &observed {
                    if let Some(path) = active
                        .processes
                        .iter()
                        .find(|process| process.run_id == *run_id)
                        .and_then(|process| process.log_path.as_deref())
                    {
                        super::finish_runtime_log_stream_for_process(state, instance_id, path);
                    }
                }
                exits.extend(observed);
            }
            Err((managed, error)) => {
                state
                    .runtime_supervisor
                    .lock()
                    .map_err(|_| "runtime supervisor lock poisoned")?
                    .restore_running_after_failed_stop(*managed);
                return Err(error);
            }
        }
    }

    // The normal reconciler may have collected an exited shard while we waited
    // for another one. Keep its actual stored exit code instead of overwriting
    // that history with an unknown, supposedly successful forced stop.
    let expected = active
        .processes
        .iter()
        .filter(|process| process.status == "running")
        .collect::<Vec<_>>();
    let reconciliation_deadline = Instant::now() + Duration::from_secs(5);
    while expected
        .iter()
        .any(|process| !exits.contains_key(&process.run_id))
    {
        let overview = read_instance_runtime_overview(&storage.paths, instance_id)
            .await
            .map_err(|error| error.to_string())?;
        for run in &overview.recent_runs {
            for process in &run.processes {
                if process.status != "running"
                    && expected
                        .iter()
                        .any(|expected| expected.run_id == process.run_id)
                {
                    exits.entry(process.run_id).or_insert(process.exit_code);
                }
            }
        }
        if expected
            .iter()
            .all(|process| exits.contains_key(&process.run_id))
        {
            break;
        }
        // Reaping releases the supervisor lock before its asynchronous database
        // update. Wait for that update without inventing an exit code.
        if Instant::now() >= reconciliation_deadline {
            return Err(String::from(
                "DST exit records did not finish synchronizing; automatic backup was skipped. Inspect the runtime history before restarting.",
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut stopped = Vec::new();
    let mut abnormal = Vec::new();
    for process in active
        .processes
        .iter()
        .filter(|process| process.status == "running")
    {
        let exit_code = *exits.get(&process.run_id).ok_or_else(|| {
            format!(
                "DST {} exit could not be confirmed; automatic backup was skipped",
                process.display_name
            )
        })?;
        let failed = exit_code != Some(0);
        mark_instance_process_stopped(
            &storage.paths,
            instance_id,
            process.run_id,
            exit_code,
            failed,
        )
        .await
        .map_err(|error| error.to_string())?;
        if failed {
            abnormal.push(process.display_name.as_str());
        }
        stopped.push(app_core::StoppedProcess {
            run_id: process.run_id,
            process_key: process.process_key.clone(),
            display_name: process.display_name.clone(),
            pid: process.pid,
            log_path: process.log_path.clone(),
            exit_code,
        });
    }
    if !abnormal.is_empty() {
        return Err(format!(
            "DST {} did not exit normally after saving; automatic backup was skipped",
            abnormal.join(", ")
        ));
    }
    Ok(stopped)
}

#[cfg(all(test, windows))]
#[path = "commands_dst_lifecycle_tests.rs"]
mod tests;
