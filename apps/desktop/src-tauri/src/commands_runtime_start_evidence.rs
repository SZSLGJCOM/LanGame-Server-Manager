#[derive(Debug, Clone)]
pub(super) struct RuntimeStartFailure {
    pub(super) message: String,
    pub(super) log: Option<LogTailSnapshot>,
}

impl From<String> for RuntimeStartFailure {
    fn from(message: String) -> Self {
        Self { message, log: None }
    }
}

fn runtime_start_failure_with_log(
    message: String,
    log: Option<LogTailSnapshot>,
) -> RuntimeStartFailure {
    RuntimeStartFailure { message, log }
}

#[cfg(test)]
fn retain_start_failure_log(path: String, message: &str) -> LogTailSnapshot {
    // World generation can emit hundreds of lines after the startup detector
    // finds a fatal error. Keep that detector's cause in the same attempt's tail.
    let cancelled = serde_json::from_str::<Value>(message)
        .ok()
        .is_some_and(|value| value["code"] == "runtime_start_cancelled");
    let append = append_startup_console_line(
        &path,
        if cancelled {
            "Startup cancelled"
        } else {
            "Startup failed"
        },
        message,
    );
    let mut log = read_log_path_snapshot(path, 200);
    if let Err(error) = append {
        log.read_error = Some(match log.read_error {
            Some(read_error) => format!("{read_error}; could not retain startup failure: {error}"),
            None => format!("Could not retain startup failure: {error}"),
        });
    }
    log
}

async fn capture_runtime_start_result(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: &str,
    reservation: &RuntimeStartReservationLease,
    result: Result<StartInstanceResult, String>,
) -> Result<StartInstanceResult, RuntimeStartFailure> {
    let active_result = if result.is_err() {
        read_active_instance_run(&storage.paths, instance_id).await
    } else {
        Ok(None)
    };
    let active = active_result.as_ref().ok().and_then(Option::as_ref);
    let run_ids = result
        .as_ref()
        .map(|started| {
            started
                .processes
                .iter()
                .map(|process| process.run_id)
                .collect()
        })
        .unwrap_or_else(|_| {
            active
                .as_ref()
                .map(|run| run.processes.iter().map(|process| process.run_id).collect())
                .unwrap_or_default()
        });
    let persistence =
        finish_owned_startup_session(state, instance_id, reservation, &result, run_ids).await;
    if result.is_err() && active_result.is_ok() {
        // Cleanup has settled before this receipt. Keep any registered survivors'
        // observers alive; every other source owns its final drain, not cancellation.
        let keep: Vec<_> = active
            .as_ref()
            .map(|run| {
                run.processes
                    .iter()
                    .filter_map(|process| process.log_path.as_ref())
                    .map(|path| path.replace('\\', "/").to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        if let Ok(mut streams) = state.runtime_log_streams.lock() {
            for (id, path) in streams.paths() {
                if id == instance_id && !keep.iter().any(|kept| kept == &path) {
                    streams.finish_path(instance_id, &path);
                }
            }
        }
    }
    if let Err(error) = active_result {
        append_desktop_app_log(
            storage,
            "error",
            "instance.startup.observer_cleanup_unconfirmed",
            &error.to_string(),
            json!({"instance_id":instance_id}),
        );
    }
    match result {
        Ok(mut started) => {
            if let Err(error) = persistence {
                append_desktop_app_log(
                    storage,
                    "error",
                    "instance.startup.receipt_failed",
                    &error,
                    json!({"instance_id":instance_id}),
                );
                started.warnings.push(app_core::RuntimeStartWarning {
                    code: app_core::RuntimeStartWarningCode::StartupLogPersistenceFailed,
                    provider: None,
                    reason: None,
                    item_ids: Vec::new(),
                    message: error,
                    log_path: reservation
                        .startup_session()
                        .ok()
                        .flatten()
                        .map(|(_, receipt)| receipt.source_path),
                    map_key: None,
                    http_status: None,
                });
            }
            Ok(started)
        }
        Err(message) => {
            // Keep this receipt before the reservation releases its pending log path.
            // Failed world generation may never have produced a database run record.
            let owned_path = reservation
                .startup_session()
                .ok()
                .flatten()
                .map(|(_, receipt)| receipt.source_path);
            let log = match owned_path
                .or_else(|| pending_start_console_log_path(state, instance_id))
            {
                Some(path) => {
                    let cause = message.clone();
                    match tokio::task::spawn_blocking(move || {
                        let mut log = read_log_path_snapshot(path, 400);
                        if let Err(error) = persistence {
                            log.read_error =
                                Some(format!("{error}; original startup failure: {cause}"));
                        }
                        log
                    })
                    .await
                    {
                        Ok(log) => Some(log),
                        Err(error) => Some(LogTailSnapshot {
                            source_path: None,
                            lines: Vec::new(),
                            total_lines: 0,
                            truncated: false,
                            read_error: Some(format!("Failed-start log reader failed: {error}")),
                        }),
                    }
                }
                None => None,
            };
            Err(runtime_start_failure_with_log(message, log))
        }
    }
}

pub(super) async fn start_instance_process_with_preconditions(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: String,
    source: &str,
    preconditions: RuntimeStartPreconditions,
) -> Result<StartInstanceResult, String> {
    start_instance_process_with_evidence(
        app_handle,
        state,
        storage,
        instance_id,
        source,
        preconditions,
    )
    .await
    .map_err(|failure| failure.message)
}

pub(super) async fn start_instance_process_with_evidence(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: String,
    source: &str,
    preconditions: RuntimeStartPreconditions,
) -> Result<StartInstanceResult, RuntimeStartFailure> {
    let runtime_start_reservation =
        reserve_runtime_instance_start(state, storage, &instance_id, source)?;
    start_instance_process_with_reserved_evidence(
        app_handle,
        state,
        storage,
        instance_id,
        source,
        preconditions,
        runtime_start_reservation,
    )
    .await
}

async fn start_instance_process_with_reserved_evidence(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: String,
    source: &str,
    preconditions: RuntimeStartPreconditions,
    runtime_start_reservation: RuntimeStartReservationLease,
) -> Result<StartInstanceResult, RuntimeStartFailure> {
    ensure_runtime_start_allowed(
        state,
        storage,
        &instance_id,
        source,
        &runtime_start_reservation,
    )?;
    if source == "manual" {
        state
            .runtime_restart_scheduler
            .lock()
            .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
            .reset_for_manual_start(&instance_id);
    }
    if source != super::commands_autostart::AUTOSTART_SOURCE {
        state.autostart.cancel(&instance_id)?;
    }
    if let Some(app_handle) = app_handle {
        let worker_app_handle = app_handle.clone();
        let worker_storage = storage.clone();
        let worker_instance_id = instance_id.clone();
        let worker_source = source.to_owned();
        let worker_keepalive = runtime_start_reservation.clone();
        return run_runtime_start_worker_to_completion(&worker_keepalive, async move {
            let worker_state = worker_app_handle.state::<DesktopState>();
            let result = start_instance_process_after_reconcile_reserved(
                Some(&worker_app_handle),
                &worker_state,
                &worker_storage,
                worker_instance_id.clone(),
                &worker_source,
                &runtime_start_reservation,
                preconditions,
            )
            .await;
            Ok(capture_runtime_start_result(
                &worker_state,
                &worker_storage,
                &worker_instance_id,
                &runtime_start_reservation,
                result,
            )
            .await)
        })
        .await
        .map_err(RuntimeStartFailure::from)?;
    }
    let result = start_instance_process_after_reconcile_reserved(
        None,
        state,
        storage,
        instance_id.clone(),
        source,
        &runtime_start_reservation,
        preconditions,
    )
    .await;
    capture_runtime_start_result(
        state,
        storage,
        &instance_id,
        &runtime_start_reservation,
        result,
    )
    .await
}

async fn begin_owned_startup_session(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    instance: &InstanceDetails,
    reservation: &RuntimeStartReservationLease,
) -> Result<(), String> {
    let root = super::commands_program_storage::instance_root(instance)?.to_path_buf();
    let instance_id = instance.summary.id.clone();
    let (attempt_id, started_at) = reservation.startup_identity();
    let attempt_id = attempt_id.to_owned();
    let write_root = root.clone();
    let write_id = instance_id.clone();
    let (receipt, writer) = tokio::task::spawn_blocking(move || {
        app_storage::startup_sessions::begin(&write_root, &write_id, &attempt_id, started_at)
    })
    .await
    .map_err(|error| format!("Startup session writer failed: {error}"))?
    .map_err(|error| format!("Could not create startup session: {error}"))?;
    let path = receipt.source_path.clone();
    reservation.own_startup_session(root, receipt, writer)?;
    publish_pending_start_console_log_path(
        app_handle,
        state,
        &instance_id,
        &path,
        Some("startup"),
        Some("Startup"),
        None,
    )?;
    append_owned_startup_lines(
        &path,
        &format!("Preparing {} startup...", instance.summary.name),
        "",
    )
}

fn append_owned_startup_lines(path: &str, detail: &str, error: &str) -> Result<(), String> {
    use std::io::Write;
    let mut writer = app_storage::managed_console_log::ManagedConsoleLog::open(path)
        .map_err(|error| error.to_string())?;
    let at = current_unix_ms();
    for line in [detail, error]
        .into_iter()
        .flat_map(|text| text.split(['\r', '\n']))
        .filter(|line| !line.is_empty())
    {
        writeln!(
            writer,
            "[{at}] [LanGameCMD/startup] {}",
            app_core::abiotic_output::redact_modio_token_text(line)
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn finish_owned_startup_session(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
    reservation: &RuntimeStartReservationLease,
    result: &Result<StartInstanceResult, String>,
    run_ids: Vec<i64>,
) -> Result<(), String> {
    let Some((root, receipt)) = reservation.startup_session()? else {
        return Ok(());
    };
    let path = receipt.source_path.clone();
    let status = match result {
        Ok(_) => app_storage::startup_sessions::StartupSessionStatus::Succeeded,
        Err(_) if reservation.cancelled_by_user() => {
            app_storage::startup_sessions::StartupSessionStatus::Cancelled
        }
        Err(_) => app_storage::startup_sessions::StartupSessionStatus::Failed,
    };
    let error = result
        .as_ref()
        .err()
        .map(|error| app_core::abiotic_output::redact_modio_token_text(error));
    let outcome = match status {
        app_storage::startup_sessions::StartupSessionStatus::Succeeded => "Startup succeeded",
        app_storage::startup_sessions::StartupSessionStatus::Cancelled => "Startup cancelled",
        _ => "Startup failed",
    };
    let write_path = path.clone();
    let finish = tokio::task::spawn_blocking(move || {
        let append =
            append_owned_startup_lines(&write_path, outcome, error.as_deref().unwrap_or_default());
        let saved = app_storage::startup_sessions::finish(
            &root,
            &receipt.attempt_id,
            status,
            run_ids,
            error,
        )
        .map_err(|error| format!("Could not persist startup result: {error}"));
        append.and(saved.map(|_| ()))
    })
    .await
    .map_err(|error| format!("Startup result writer failed: {error}"))
    .and_then(|result| result);
    let close = reservation.close_startup_output();
    finish_runtime_log_stream_for_process(state, instance_id, &path);
    finish.and(close)
}

#[cfg(test)]
mod start_evidence_tests {
    use super::*;

    #[test]
    fn failed_start_cause_survives_a_noisy_native_log_tail() {
        let root = std::env::temp_dir().join(format!("langame-start-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("run-123-master.log");
        let cause =
            "DST Master failed during startup: ERROR: Failed to load ../worldgenoverride.lua";
        let content = format!("{cause}\n{}", "World generation progress\n".repeat(410));
        std::fs::write(&path, &content).unwrap();
        let snapshot = retain_start_failure_log(path.to_string_lossy().into_owned(), cause);
        assert!(snapshot.read_error.is_none());
        assert!(snapshot.truncated);
        assert_eq!(
            snapshot.lines.last().unwrap(),
            &format!("[LanGame startup] {cause}")
        );
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with(&content)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_start_log_write_failure_is_an_evidence_gap() {
        let root = std::env::temp_dir().join(format!("langame-start-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let snapshot =
            retain_start_failure_log(root.to_string_lossy().into_owned(), "native error");
        assert!(snapshot.lines.is_empty());
        assert!(
            snapshot
                .read_error
                .unwrap()
                .contains("could not retain startup failure")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_start_receipt_keeps_the_original_error_and_its_unregistered_log() {
        let log = LogTailSnapshot {
            source_path: Some(String::from("instance/logs/failed-start.log")),
            lines: vec![String::from("Missing server mod dependency")],
            total_lines: 1,
            truncated: false,
            read_error: None,
        };
        let failure =
            runtime_start_failure_with_log(String::from("world generation failed"), Some(log));
        assert_eq!(failure.message, "world generation failed");
        assert_eq!(
            failure.log.unwrap().lines,
            ["Missing server mod dependency"]
        );
    }
}
