use super::*;

#[cfg(test)]
#[path = "commands_runtime_restart_tests.rs"]
mod tests;

pub(in crate::commands) async fn build_runtime_restart_candidate(
    storage: &StorageBootstrap,
    instance_id: &str,
    fallback_instance_name: &str,
    expected_exit: bool,
    exit_code: Option<i32>,
) -> Result<Option<RuntimeRestartCandidate>, String> {
    if expected_exit {
        return Ok(None);
    }

    let (summary, settings_json) =
        app_storage::read_instance_stored_settings(&storage.paths, instance_id)
            .await
            .map_err(|error| error.to_string())?;
    let policy = runtime_restart_policy_from_settings(&settings_json);

    if !policy.enabled {
        return Ok(None);
    }

    if policy.only_nonzero_exit && exit_code == Some(0) {
        append_desktop_app_log(
            storage,
            "info",
            "instance.auto_restart.skipped",
            "Auto restart skipped because the exit code was clean.",
            json!({
                "instance_id": instance_id,
                "instance_name": summary.name.as_str(),
                "exit_code": exit_code,
            }),
        );
        return Ok(None);
    }

    let recent_crash_count = app_storage::read_instance_restart_failure_count(
        &storage.paths,
        instance_id,
        policy.max_restarts + 1,
    )
    .await
    .map_err(|error| error.to_string())?;

    if recent_crash_count > policy.max_restarts {
        append_desktop_app_log(
            storage,
            "warn",
            "instance.auto_restart.blocked",
            "Auto restart blocked by the configured crash restart limit.",
            json!({
                "instance_id": instance_id,
                "instance_name": summary.name.as_str(),
                "exit_code": exit_code,
                "recent_crash_count": recent_crash_count,
                "max_restarts": policy.max_restarts,
            }),
        );
        return Ok(None);
    }

    Ok(Some(RuntimeRestartCandidate {
        instance_id: String::from(instance_id),
        instance_name: if summary.name.trim().is_empty() {
            String::from(fallback_instance_name)
        } else {
            summary.name
        },
        policy,
        recent_crash_count,
        exit_code,
    }))
}

pub(in crate::commands) fn schedule_runtime_restart_candidates(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    candidates: Vec<RuntimeRestartCandidate>,
) -> Result<(), String> {
    for candidate in candidates {
        let scheduled = match state.runtime_restart_scheduler.lock() {
            Ok(mut scheduler) => scheduler.schedule(RuntimeRestartScheduleRequest {
                instance_id: candidate.instance_id.clone(),
                instance_name: candidate.instance_name.clone(),
                backoff: Duration::from_millis(candidate.policy.backoff_ms),
                recent_crash_count: candidate.recent_crash_count,
                exit_code: candidate.exit_code,
            }),
            Err(_) => {
                append_desktop_app_log(
                    storage,
                    "error",
                    "instance.auto_restart.schedule_failed",
                    "Auto restart could not be scheduled because the runtime restart queue is unavailable.",
                    json!({
                        "instance_id": candidate.instance_id,
                        "instance_name": candidate.instance_name,
                    }),
                );
                return Err(String::from("runtime restart scheduler lock poisoned"));
            }
        };
        let Some(scheduled) = scheduled else { continue };

        append_desktop_app_log(
            storage,
            "warn",
            "instance.auto_restart.scheduled",
            "Auto restart scheduled after abnormal process exit.",
            json!({
                "instance_id": scheduled.instance_id.as_str(),
                "instance_name": scheduled.instance_name.as_str(),
                "exit_code": scheduled.exit_code,
                "recent_crash_count": scheduled.recent_crash_count,
                "max_restarts": candidate.policy.max_restarts,
                "backoff_ms": candidate.policy.backoff_ms,
                "due_in_ms": scheduled.due_at.saturating_duration_since(Instant::now()).as_millis(),
            }),
        );
    }
    Ok(())
}

pub(in crate::commands) fn take_due_runtime_restarts(
    state: &tauri::State<'_, DesktopState>,
) -> Vec<RuntimeRestartScheduleEntry> {
    state
        .runtime_restart_scheduler
        .lock()
        .map(|mut scheduler| scheduler.take_due())
        .unwrap_or_default()
}

pub(in crate::commands) fn cancel_pending_runtime_restart(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: &str,
    instance_name: &str,
) {
    let cancelled = state
        .runtime_restart_scheduler
        .lock()
        .ok()
        .and_then(|mut scheduler| scheduler.cancel(instance_id));

    if let Some(cancelled) = cancelled {
        append_desktop_app_log(
            storage,
            "info",
            "instance.auto_restart.cancelled",
            "Pending auto restart was cancelled because the instance was started manually.",
            json!({
                "instance_id": instance_id,
                "instance_name": if instance_name.trim().is_empty() {
                    cancelled.instance_name
                } else {
                    String::from(instance_name)
                },
            }),
        );
    }
}

struct RestartFlight<'a> {
    state: &'a DesktopState,
    entry: RuntimeRestartScheduleEntry,
}

impl Drop for RestartFlight<'_> {
    fn drop(&mut self) {
        if let Ok(mut scheduler) = self.state.runtime_restart_scheduler.lock() {
            scheduler.finish_restart(&self.entry);
        }
    }
}

fn ensure_restart_current(
    state: &DesktopState,
    entry: &RuntimeRestartScheduleEntry,
) -> Result<(), String> {
    let current = state
        .runtime_restart_scheduler
        .lock()
        .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
        .entry_is_current(entry);
    if current && !state.shutdown_in_progress.load(Ordering::SeqCst) {
        Ok(())
    } else {
        Err(String::from(
            "Auto restart cancelled by a stop request, manual start, or application shutdown.",
        ))
    }
}

pub(in crate::commands) fn exit_marks_failed_session(
    expected_exit: bool,
    exit_code: Option<i32>,
    policy: &super::super::commands_runtime_observability::RuntimeRestartPolicy,
) -> bool {
    exit_code.unwrap_or(1) != 0 || (!expected_exit && policy.enabled && !policy.only_nonzero_exit)
}

/// The caller holds the instance mutation lock across validation and save/stop.
/// The injected operation is the existing DST save-ACK/confirmed-exit protocol.
async fn prepare_restart_survivors<F, Fut>(
    state: &DesktopState,
    storage: &StorageBootstrap,
    entry: &RuntimeRestartScheduleEntry,
    stop_survivors: F,
) -> Result<(), String>
where
    F: FnOnce(ActiveInstanceRun) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    ensure_restart_current(state, entry)?;
    let details = read_instance_details(&storage.paths, &entry.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let policy = runtime_restart_policy_from_settings(&details.settings_json);
    if !policy.enabled || (policy.only_nonzero_exit && entry.exit_code == Some(0)) {
        return Err(String::from(
            "Auto restart policy no longer permits this exit.",
        ));
    }
    let failures = app_storage::read_instance_restart_failure_count(
        &storage.paths,
        &entry.instance_id,
        policy.max_restarts + 1,
    )
    .await
    .map_err(|error| error.to_string())?;
    if failures > policy.max_restarts {
        return Err(format!(
            "Auto restart blocked by the configured limit ({} failed sessions, {} restarts).",
            failures, policy.max_restarts
        ));
    }
    let active = read_active_instance_run(&storage.paths, &entry.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(active) = active {
        if details.summary.module_id != "dontstarve"
            || !active.processes.iter().any(|process| process.crash_flag)
        {
            return Err(String::from(
                "Auto restart cannot stop an unrelated active run.",
            ));
        }
        ensure_restart_current(state, entry)?;
        let expected = state
            .runtime_restart_scheduler
            .lock()
            .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
            .expect_survivor_shutdown(
                entry,
                active
                    .processes
                    .iter()
                    .filter(|process| process.status == "running")
                    .map(|process| process.run_id),
            );
        if !expected {
            return Err(String::from(
                "Auto restart cancelled before saving surviving shards.",
            ));
        }
        if let Err(error) = stop_survivors(active).await {
            // A late exit after a failed save acknowledgement must not silently
            // re-arm recovery. The user can retry explicitly after inspecting it.
            state
                .runtime_restart_scheduler
                .lock()
                .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
                .request_stop(&entry.instance_id);
            return Err(error);
        }
        if read_active_instance_run(&storage.paths, &entry.instance_id)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err(String::from(
                "Auto restart requires every surviving shard to finish saving and exit.",
            ));
        }
    }
    ensure_restart_current(state, entry)
}

pub(in crate::commands) async fn execute_due_runtime_restarts(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    restarts: Vec<RuntimeRestartScheduleEntry>,
) {
    // Own all taken tickets up front so cancellation also releases not-yet-run entries.
    let flights = restarts
        .into_iter()
        .map(|entry| RestartFlight { state, entry })
        .collect::<Vec<_>>();
    for flight in flights {
        let restart = &flight.entry;
        let result = async {
            {
                let _instance_lock = state.acquire_instance_mutation(&restart.instance_id).await;
                prepare_restart_survivors(state, storage, restart, |active| async move {
                    super::super::commands_runtime_lifecycle::stop_dst_survivors_for_restart(
                        state,
                        storage,
                        &restart.instance_id,
                        &active,
                    )
                    .await
                })
                .await?;
            }
            // The launch path checks the same ticket again under its instance lock
            // and before spawning, covering user Stop in the lock handoff window.
            ensure_restart_current(state, restart)?;
            start_instance_process_after_reconcile(
                None,
                state,
                storage,
                restart.instance_id.clone(),
                "auto_restart",
                None,
            )
            .await
        }
        .await;
        match result {
            Ok(result) => append_desktop_app_log(
                storage,
                "info",
                "instance.auto_restart.started",
                "Auto restart launched the instance after an unexpected exit.",
                json!({"instance_id":restart.instance_id, "instance_name":restart.instance_name,
                    "run_id":result.run_id, "pid":result.pid, "process_count":result.process_count,
                    "recent_crash_count":restart.recent_crash_count, "previous_exit_code":restart.exit_code,
                    "scheduled_wait_ms":Instant::now().saturating_duration_since(restart.scheduled_at).as_millis()}),
            ),
            Err(error) => append_desktop_app_log(
                storage,
                "error",
                "instance.auto_restart.failed",
                "Auto restart did not launch the instance.",
                json!({"instance_id":restart.instance_id, "instance_name":restart.instance_name,
                    "error":error, "recent_crash_count":restart.recent_crash_count, "previous_exit_code":restart.exit_code}),
            ),
        }
    }
}
