use super::commands_runtime_actions::{ResolvedRuntimeCommand, resolve_declared_runtime_action};
use super::commands_stdin_dispatch::{
    RuntimeStdinDispatchBudget, RuntimeStdinDispatchConfirmation, RuntimeStdinWriteCompletion,
    dispatch_managed_stdin_command_with_budget,
};
use super::*;

const SAVE_TICK: Duration = Duration::from_secs(1);
const SAVE_FAILURE_RETRY: Duration = Duration::from_secs(60);
const DEFAULT_SAVE_INTERVAL_SECONDS: u64 = 300;
const MAX_SAVE_INTERVAL_SECONDS: u64 = 86_400;

struct SaveTick {
    at: Instant,
    started: Instant,
}

impl SaveTick {
    fn current(&self) -> Instant {
        self.at + self.started.elapsed()
    }
}

struct SaveSchedule {
    run_id: i64,
    revision: Option<u64>,
    interval: Duration,
    next_due: Option<Instant>,
    completion: Option<RuntimeStdinWriteCompletion>,
}

impl SaveSchedule {
    fn new(run_id: i64) -> Self {
        Self {
            run_id,
            revision: None,
            interval: Duration::ZERO,
            next_due: None,
            completion: None,
        }
    }

    fn configure(&mut self, interval: Duration, revision: u64, now: Instant) {
        if self.revision.is_none() || self.interval != interval {
            self.next_due = (!interval.is_zero()).then(|| now + interval);
        }
        self.interval = interval;
        self.revision = Some(revision);
    }

    fn completed(&mut self, failed: bool, now: Instant) {
        self.completion = None;
        self.next_due = (!self.interval.is_zero()).then(|| {
            now + if failed {
                self.interval.max(SAVE_FAILURE_RETRY)
            } else {
                self.interval
            }
        });
    }
}

#[derive(Default)]
struct ManagedSaveWorker {
    schedules: HashMap<String, SaveSchedule>,
    storage_key: Option<(PathBuf, PathBuf, PathBuf)>,
    action: Option<ResolvedRuntimeCommand>,
    last_error_at: Option<Instant>,
}

pub(super) fn invalidate_instance_policy(state: &DesktopState) {
    state
        .managed_save_policy_revision
        .fetch_add(1, Ordering::Release);
}

/// One worker owns all timers. Slow stdin confirmation never blocks runtime
/// reconciliation, and a pending write prevents another save for the same run.
pub(crate) fn spawn_managed_save_worker(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut worker = ManagedSaveWorker::default();
        let mut tick = tokio::time::interval(SAVE_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let state = app_handle.state::<DesktopState>();
            if state.shutdown_completed.load(Ordering::SeqCst) {
                break;
            }
            if !state.is_storage_ready() || state.shutdown_in_progress.load(Ordering::SeqCst) {
                worker.schedules.clear();
                continue;
            }
            let Ok(_operation) = state.begin_storage_context_operation("periodic world save")
            else {
                continue;
            };
            let storage = match bootstrap_storage() {
                Ok(storage) => storage,
                Err(error) => {
                    if worker.should_log_error(Instant::now()) {
                        eprintln!("periodic world save storage is unavailable: {error}");
                    }
                    continue;
                }
            };
            if let Err(error) = worker.tick(&state, &storage, Instant::now()).await
                && worker.should_log_error(Instant::now())
            {
                append_desktop_app_log(
                    &storage,
                    "error",
                    "instance.periodic_save.failed",
                    &error,
                    json!({}),
                );
            }
        }
    });
}

impl ManagedSaveWorker {
    fn should_log_error(&mut self, now: Instant) -> bool {
        if self
            .last_error_at
            .is_some_and(|last| now.duration_since(last) < SAVE_FAILURE_RETRY)
        {
            return false;
        }
        self.last_error_at = Some(now);
        true
    }

    async fn tick(
        &mut self,
        state: &tauri::State<'_, DesktopState>,
        storage: &StorageBootstrap,
        now: Instant,
    ) -> Result<(), String> {
        let tick = SaveTick {
            at: now,
            started: Instant::now(),
        };
        ensure_storage_context_snapshot_current(state, storage, "periodic world save")?;
        let key = (
            storage.paths.database_path.clone(),
            storage.paths.instances_root.clone(),
            storage.paths.modules_root.clone(),
        );
        if self.storage_key.as_ref() != Some(&key) {
            self.schedules.clear();
            self.action = None;
            self.storage_key = Some(key);
        }
        let tracked: Vec<_> = state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?
            .tracked_instances()
            .into_iter()
            .filter(|instance| {
                instance.summary.module_id == "unturned"
                    && matches!(instance.summary.status, InstanceStatus::Running)
            })
            .collect();
        let live_runs: HashMap<_, _> = tracked
            .iter()
            .map(|instance| (instance.summary.id.as_str(), instance.run_id))
            .collect();
        self.schedules
            .retain(|id, schedule| live_runs.get(id.as_str()) == Some(&schedule.run_id));
        if tracked.is_empty() {
            return Ok(());
        }
        if self.action.is_none() {
            let descriptors =
                discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
            let descriptor = find_descriptor(&descriptors, "unturned")?;
            let action =
                resolve_declared_runtime_action(descriptor, "save_world", None, None, None, false)?;
            if action.transport != "stdin" || action.process_key.as_deref() != Some("main") {
                return Err(String::from(
                    "Unturned periodic save requires its declared main-process stdin save action",
                ));
            }
            self.action = Some(action);
        }
        let Some(action) = self.action.as_ref() else {
            return Ok(());
        };
        let revision = state.managed_save_policy_revision.load(Ordering::Acquire);
        for instance in tracked {
            if state.shutdown_in_progress.load(Ordering::SeqCst) {
                self.schedules.clear();
                break;
            }
            let schedule = self
                .schedules
                .entry(instance.summary.id.clone())
                .or_insert_with(|| SaveSchedule::new(instance.run_id));
            if let Some(completion) = &schedule.completion {
                if let Some(result) = completion.result()? {
                    log_save_result(storage, &instance, &result);
                    schedule.completed(result.is_err(), tick.current());
                }
                continue;
            }
            let due = schedule.next_due.is_some_and(|due| due <= now);
            if schedule.revision == Some(revision) && !due {
                continue;
            }
            let result =
                refresh_and_save(state, storage, &instance, action, schedule, revision, &tick)
                    .await;
            if let Err(error) = result {
                log_save_result(storage, &instance, &Err(error));
                // Retry initialization failures as well as dispatch failures at
                // a bounded cadence; one broken instance does not block others.
                schedule.revision = Some(revision);
                schedule.next_due =
                    Some(tick.current() + schedule.interval.max(SAVE_FAILURE_RETRY));
            }
        }
        Ok(())
    }
}

async fn refresh_and_save(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance: &app_runtime::TrackedInstance,
    action: &ResolvedRuntimeCommand,
    schedule: &mut SaveSchedule,
    revision: u64,
    tick: &SaveTick,
) -> Result<(), String> {
    let details = read_instance_details(&storage.paths, &instance.summary.id)
        .await
        .map_err(|error| error.to_string())?;
    let interval = save_interval(&details.settings_json)?;
    schedule.configure(interval, revision, tick.at);
    if !schedule.next_due.is_some_and(|due| due <= tick.at) {
        return Ok(());
    }
    let _instance_lock = tokio::time::timeout(
        Duration::from_millis(100),
        state.acquire_instance_mutation(&instance.summary.id),
    )
    .await
    .map_err(|_| {
        String::from("Periodic save deferred because an instance operation is in progress")
    })?;
    if state.shutdown_in_progress.load(Ordering::SeqCst) {
        schedule.next_due = None;
        return Ok(());
    }
    ensure_storage_context_snapshot_current(state, storage, "periodic world save")?;
    let details = read_instance_details(&storage.paths, &instance.summary.id)
        .await
        .map_err(|error| error.to_string())?;
    let current_interval = save_interval(&details.settings_json)?;
    if current_interval != interval {
        schedule.configure(current_interval, revision, tick.at);
        return Ok(());
    }
    if !matches!(details.summary.status, InstanceStatus::Running)
        || !details
            .active_run
            .as_ref()
            .is_some_and(|run| run.run_id == instance.run_id && run.pid == instance.pid)
    {
        schedule.next_due = None;
        return Ok(());
    }
    let running = state
        .runtime_supervisor
        .lock()
        .map_err(|_| String::from("runtime supervisor lock poisoned"))?
        .matches_running_process(
            &instance.summary.id,
            instance.run_id,
            "main",
            instance.pid.unwrap_or(0),
        )
        .map_err(|error| error.to_string())?;
    if !running {
        schedule.next_due = None;
        return Ok(());
    }
    let completion = RuntimeStdinWriteCompletion::default();
    let receipt = dispatch_managed_stdin_command_with_budget(
        state,
        &instance.summary.id,
        action.process_key.as_deref(),
        &action.command,
        Some(instance.run_id),
        RuntimeStdinDispatchBudget::observed(completion.clone()),
    )
    .await?;
    match receipt.confirmation {
        RuntimeStdinDispatchConfirmation::Confirmed => {
            log_save_result(storage, instance, &Ok(()));
            schedule.completed(false, tick.current());
        }
        RuntimeStdinDispatchConfirmation::Pending => {
            schedule.completion = Some(completion);
            schedule.next_due = None;
            append_desktop_app_log(
                storage,
                "warn",
                "instance.periodic_save.pending",
                "Automatic Save command is still awaiting stdin write confirmation; another save will not overlap it",
                json!({ "instance_id": instance.summary.id, "run_id": instance.run_id }),
            );
        }
    }
    Ok(())
}

fn save_interval(settings_json: &str) -> Result<Duration, String> {
    let settings: Value = serde_json::from_str(settings_json)
        .map_err(|error| format!("Invalid periodic-save settings: {error}"))?;
    let interval = match settings.get("managed_save_interval_seconds") {
        None => DEFAULT_SAVE_INTERVAL_SECONDS,
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= MAX_SAVE_INTERVAL_SECONDS)
            .ok_or_else(|| {
                String::from("managed_save_interval_seconds must be an integer from 0 to 86400")
            })?,
    };
    Ok(Duration::from_secs(interval))
}

fn log_save_result(
    storage: &StorageBootstrap,
    instance: &app_runtime::TrackedInstance,
    result: &Result<(), String>,
) {
    let (level, action, message) = match result {
        Ok(()) => (
            "info",
            "instance.periodic_save.command_written",
            "Automatic Save command written to the managed server console",
        ),
        Err(error) => ("error", "instance.periodic_save.failed", error.as_str()),
    };
    append_desktop_app_log(
        storage,
        level,
        action,
        message,
        json!({ "instance_id": instance.summary.id, "run_id": instance.run_id, "module_id": "unturned" }),
    );
}

#[cfg(test)]
#[path = "commands_managed_save_tests.rs"]
mod tests;
