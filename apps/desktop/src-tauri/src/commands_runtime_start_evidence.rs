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

fn retain_start_failure_log(path: String, message: &str) -> LogTailSnapshot {
    // World generation can emit hundreds of lines after the startup detector
    // finds a fatal error. Keep that detector's cause in the same attempt's tail.
    let append = append_startup_console_line(&path, "Startup failed", message);
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
    instance_id: &str,
    result: Result<StartInstanceResult, String>,
) -> Result<StartInstanceResult, RuntimeStartFailure> {
    match result {
        Ok(started) => Ok(started),
        Err(message) => {
            // Keep this receipt before the reservation releases its pending log path.
            // Failed world generation may never have produced a database run record.
            let log = match pending_start_console_log_path(state, instance_id) {
                Some(path) => {
                    let cause = message.clone();
                    match tokio::task::spawn_blocking(move || {
                        retain_start_failure_log(path, &cause)
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
            stop_runtime_log_streams_for_instance(state, instance_id);
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
            Ok(capture_runtime_start_result(&worker_state, &worker_instance_id, result).await)
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
    capture_runtime_start_result(state, &instance_id, result).await
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
