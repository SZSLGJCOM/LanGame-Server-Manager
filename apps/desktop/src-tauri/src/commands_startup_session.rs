use super::*;
use app_storage::startup_sessions::{self, StartupSessionReceipt, StartupSessionStatus};

#[derive(serde::Serialize)]
pub(super) struct StartupSessionDocument {
    #[serde(flatten)]
    receipt: StartupSessionReceipt,
    snapshot: LogTailSnapshot,
    console_source: Option<crate::runtime_log_stream::document::ConsoleSourceDocument>,
    before_cursor: Option<String>,
    retained_start_cursor: String,
    line_generation: String,
}

impl StartupSessionDocument {
    fn from_page(
        receipt: StartupSessionReceipt,
        page: startup_sessions::StartupLogPage,
        source: Option<crate::runtime_log_stream::document::ConsoleSourceDocument>,
        has_before: bool,
    ) -> Self {
        // Readback pages use retained-file positions. Even if an earlier cache
        // refresh succeeded, its clone may predate this page's new diagnostics;
        // it cannot override this independently observed, authoritative page.
        let source = source.filter(|source| !source.requires_readback);
        let line_generation = source.as_ref().map_or_else(
            || format!("retained:{}", page.retained_start_cursor),
            |source| format!("stream:{}", source.stream_id),
        );
        Self {
            receipt,
            snapshot: page.snapshot,
            console_source: if has_before { None } else { source },
            before_cursor: page.before_cursor,
            retained_start_cursor: page.retained_start_cursor,
            line_generation,
        }
    }

    pub(super) fn owns_path(&self, path: &str) -> bool {
        self.receipt
            .source_path
            .replace('\\', "/")
            .eq_ignore_ascii_case(&path.replace('\\', "/"))
    }
}

fn recover_unowned_receipt(
    root: &std::path::Path,
    instance_id: &str,
    receipt: StartupSessionReceipt,
    reservations: &std::sync::Mutex<crate::state::RuntimeStartReservations>,
) -> Result<StartupSessionReceipt, String> {
    if receipt.status != StartupSessionStatus::Starting {
        return Ok(receipt);
    }
    // This runs on the blocking worker. Keep reservation exclusion through the
    // receipt CAS, so a new reserve/begin cannot be mistaken for an orphan.
    // Normal finish retains its reservation until its durable result is saved.
    let ownership = reservations
        .lock()
        .map_err(|_| "Startup reservation lock poisoned")?;
    if ownership.pending_attempt_id(instance_id).is_some() {
        return Ok(receipt);
    }
    let recovered = startup_sessions::recover_interrupted(root, instance_id, &receipt.attempt_id)
        .map_err(|error| error.to_string());
    drop(ownership);
    recovered
}

pub(super) async fn read(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: &str,
    limit: usize,
    attempt_id: Option<String>,
    before: Option<String>,
) -> Result<Option<StartupSessionDocument>, String> {
    if before.is_some() && attempt_id.is_none() {
        return Err("A startup page cursor requires its attempt ID".into());
    }
    if let Some(cursor) = before.as_deref() {
        cursor
            .parse::<u64>()
            .map_err(|_| "Invalid startup page cursor")?;
    }
    let root = startup_sessions::instance_root(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let reservations = std::sync::Arc::clone(&state.runtime_start_reservations);
    let id = instance_id.to_owned();
    let has_before = before.is_some();
    let receipt = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let Some(receipt) =
            startup_sessions::read_latest(&root, &id).map_err(|error| error.to_string())?
        else {
            return if attempt_id.is_some() {
                Err("The startup attempt is no longer the latest receipt".into())
            } else {
                Ok(None)
            };
        };
        let receipt = recover_unowned_receipt(&root, &id, receipt, &reservations)?;
        if attempt_id
            .as_ref()
            .is_some_and(|attempt| attempt != &receipt.attempt_id)
        {
            return Err("The startup attempt was replaced; its page cursor was not applied".into());
        }
        Ok(Some(receipt))
    })
    .await
    .map_err(|error| format!("Startup session receipt reader failed: {error}"))??;
    let Some(receipt) = receipt else {
        return Ok(None);
    };
    let source = state
        .runtime_log_streams
        .lock()
        .map_err(|_| "Runtime log stream registry lock poisoned")?
        .console_documents(instance_id, Some(&receipt.source_path), 400, None)
        .into_iter()
        .last();
    let anchor = source
        .as_ref()
        .filter(|source| !source.requires_readback)
        .map(|source| (source.reader_byte_offset, source.total_lines));
    let document = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let page = startup_sessions::read_log_page_aligned(
            std::path::Path::new(&receipt.source_path),
            before.as_deref(),
            limit,
            anchor,
        );
        let page = match page {
            Ok(page) => page,
            Err(error) if has_before => return Err(error.to_string()),
            Err(error) => startup_sessions::StartupLogPage {
                snapshot: LogTailSnapshot {
                    source_path: Some(receipt.source_path.clone()),
                    lines: Vec::new(),
                    total_lines: 0,
                    truncated: false,
                    read_error: Some(error.to_string()),
                },
                before_cursor: None,
                retained_start_cursor: "0".into(),
            },
        };
        Ok(StartupSessionDocument::from_page(
            receipt, page, source, has_before,
        ))
    })
    .await
    .map_err(|error| format!("Startup session reader failed: {error}"))??;
    Ok(Some(document))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_log_stream::document::ConsoleSourceDocument;
    use std::sync::{Arc, Barrier, Mutex};

    #[test]
    fn a_read_paused_before_recovery_cannot_interrupt_a_new_reserved_start() {
        let root = std::env::temp_dir().join(format!(
            "langame-startup-read-race-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let (old, old_writer) = startup_sessions::begin(&root, "instance", "old", 1).unwrap();
        let reservations = Arc::new(Mutex::new(crate::state::RuntimeStartReservations::default()));
        let captured = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let worker_root = root.clone();
        let worker_reservations = Arc::clone(&reservations);
        let worker_captured = Arc::clone(&captured);
        let worker_resume = Arc::clone(&resume);
        let reader = std::thread::spawn(move || {
            let receipt = startup_sessions::read_latest(&worker_root, "instance")
                .unwrap()
                .unwrap();
            worker_captured.wait();
            worker_resume.wait();
            recover_unowned_receipt(&worker_root, "instance", receipt, &worker_reservations)
                .unwrap()
        });
        captured.wait();
        let entry = reservations
            .lock()
            .unwrap()
            .reserve("instance", "manual")
            .unwrap();
        let (current, current_writer) =
            startup_sessions::begin(&root, "instance", &entry.attempt_id, 2).unwrap();
        resume.wait();
        assert_eq!(
            reader.join().unwrap().status,
            StartupSessionStatus::Starting
        );
        let latest = startup_sessions::read_latest(&root, "instance")
            .unwrap()
            .unwrap();
        assert_eq!(latest.attempt_id, current.attempt_id);
        assert_eq!(latest.status, StartupSessionStatus::Starting);
        assert!(std::fs::read(&old.source_path).unwrap().is_empty());
        assert!(std::fs::read(&current.source_path).unwrap().is_empty());
        drop((old_writer, current_writer));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_read_paused_before_recovery_cannot_overwrite_a_finished_or_replaced_receipt() {
        for replaced in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "langame-startup-result-race-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let (old, old_writer) = startup_sessions::begin(&root, "instance", "old", 1).unwrap();
            let captured = Arc::new(Barrier::new(2));
            let resume = Arc::new(Barrier::new(2));
            let worker_root = root.clone();
            let worker_captured = Arc::clone(&captured);
            let worker_resume = Arc::clone(&resume);
            let reader = std::thread::spawn(move || {
                let receipt = startup_sessions::read_latest(&worker_root, "instance")
                    .unwrap()
                    .unwrap();
                worker_captured.wait();
                worker_resume.wait();
                recover_unowned_receipt(&worker_root, "instance", receipt, &Mutex::default())
                    .unwrap()
            });
            captured.wait();
            let new_writer = if replaced {
                Some(
                    startup_sessions::begin(&root, "instance", "new", 2)
                        .unwrap()
                        .1,
                )
            } else {
                startup_sessions::finish(
                    &root,
                    "old",
                    StartupSessionStatus::Succeeded,
                    vec![42],
                    None,
                )
                .unwrap();
                None
            };
            resume.wait();
            let result = reader.join().unwrap();
            assert_eq!(result.attempt_id, if replaced { "new" } else { "old" });
            assert_eq!(
                result.status,
                if replaced {
                    StartupSessionStatus::Starting
                } else {
                    StartupSessionStatus::Succeeded
                }
            );
            if !replaced {
                assert_eq!(result.run_ids, [42]);
            }
            assert!(std::fs::read(&old.source_path).unwrap().is_empty());
            let latest = startup_sessions::read_latest(&root, "instance")
                .unwrap()
                .unwrap();
            assert_eq!(latest.status, result.status);
            drop((old_writer, new_writer));
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn startup_readback_document_never_mixes_retained_pages_with_a_stream_cursor() {
        let receipt = StartupSessionReceipt {
            attempt_id: "attempt".into(),
            status: StartupSessionStatus::Failed,
            started_at_unix_ms: 1,
            finished_at_unix_ms: Some(2),
            source_path: "startup.log".into(),
            run_ids: vec![],
            error: Some("original startup failure".into()),
            reader_error: Some("original observer failure".into()),
            native_sources: vec![],
        };
        let mut source = ConsoleSourceDocument::new("startup.log", Some("startup"), None, None, 1);
        source.total_lines = 1000;
        source.lines = vec!["stale stream row".into()];
        source.requires_readback = true;
        let page = startup_sessions::StartupLogPage {
            snapshot: LogTailSnapshot {
                source_path: Some("startup.log".into()),
                lines: vec![
                    "disk retained row".into(),
                    "new persisted diagnostic".into(),
                ],
                total_lines: 400,
                truncated: true,
                read_error: None,
            },
            before_cursor: Some("4096".into()),
            retained_start_cursor: "2048".into(),
        };
        let document = StartupSessionDocument::from_page(receipt, page, Some(source), false);
        let payload = serde_json::to_value(document).unwrap();
        assert!(payload["console_source"].is_null());
        assert_eq!(payload["line_generation"], "retained:2048");
        assert_eq!(payload["snapshot"]["total_lines"], 400);
        assert_eq!(payload["snapshot"]["lines"][1], "new persisted diagnostic");
        assert_eq!(payload["reader_error"], "original observer failure");
        assert_eq!(payload["error"], "original startup failure");
        assert_eq!(payload["before_cursor"], "4096");
    }
}

#[tauri::command]
pub async fn record_startup_session_diagnostic(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    attempt_id: String,
    diagnostic_id: String,
    message: String,
) -> Result<(), String> {
    let _operation = state.begin_storage_context_operation("startup diagnostic")?;
    let storage = state.bootstrap_storage()?;
    let root = startup_sessions::instance_root(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let write_id = instance_id.clone();
    let path = tokio::task::spawn_blocking(move || -> Result<_, String> {
        startup_sessions::record_diagnostic(
            &root,
            &write_id,
            &attempt_id,
            &diagnostic_id,
            &message,
        )
        .map_err(|error| error.to_string())?;
        startup_sessions::read_latest(&root, &write_id)
            .map_err(|error| error.to_string())
            .map(|receipt| receipt.map(|receipt| receipt.source_path))
    })
    .await
    .map_err(|error| format!("Startup diagnostic writer failed: {error}"))??;
    if let Some(path) = path {
        state
            .runtime_log_streams
            .lock()
            .map_err(|_| "Runtime log stream registry lock poisoned")?
            .invalidate_completed_console_path(&instance_id, &path);
    }
    Ok(())
}
