use super::*;
use crate::runtime_log_stream::file_identity::{LogFileIdentity, log_identity};
use app_core::{InstanceProcessState, LogTailSnapshot};
use std::path::Path;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    identity: LogFileIdentity,
    length: u64,
    modified: SystemTime,
}

fn file_stamp(path: &Path) -> Result<FileStamp, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    Ok(FileStamp {
        identity: log_identity(path, &file).map_err(|error| error.to_string())?,
        length: metadata.len(),
        modified: metadata.modified().map_err(|error| error.to_string())?,
    })
}

#[derive(Default)]
struct DocumentCache {
    stamp: Option<Result<FileStamp, String>>,
    snapshot: Option<LogTailSnapshot>,
}

impl DocumentCache {
    fn read(
        &mut self,
        path: &Path,
        process: &InstanceProcessState,
        final_read: bool,
    ) -> Option<app_storage::GameLogSnapshot> {
        let stamp = file_stamp(path);
        if !final_read && self.stamp.as_ref() == Some(&stamp) {
            return None;
        }
        self.stamp = Some(stamp);
        let document =
            app_storage::read_game_log_snapshot(path.to_string_lossy().into_owned(), process, 400);
        let snapshot = &document.snapshot;
        if self.snapshot.as_ref().is_some_and(|previous| {
            previous.lines == snapshot.lines
                && previous.read_error == snapshot.read_error
                && previous.truncated == snapshot.truncated
                && previous.total_lines == snapshot.total_lines
        }) {
            return None;
        }
        self.snapshot = Some(snapshot.clone());
        Some(document)
    }
}

pub(crate) fn start_runtime_game_log_stream(
    app_handle: &tauri::AppHandle,
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
    document: &app_storage::GameLogDocument,
) -> Result<(), String> {
    let (Some(path), Some(console_path)) = (
        document.snapshot.source_path.as_deref(),
        document.console_log_path.as_deref(),
    ) else {
        return Ok(());
    };
    let lease = state
        .runtime_log_streams
        .lock()
        .map_err(|_| String::from("runtime log stream registry lock poisoned"))?
        .reserve_linked(instance_id, path, console_path);
    let Some(lease) = lease else {
        return Ok(());
    };
    let app = app_handle.clone();
    let instance_id = instance_id.to_owned();
    let path = PathBuf::from(path);
    let log_path = path.to_string_lossy().into_owned();
    let process = document.process.clone();
    let mut cache = DocumentCache {
        stamp: None,
        snapshot: Some(document.snapshot.clone()),
    };
    tauri::async_runtime::spawn(async move {
        loop {
            let status = app
                .state::<DesktopState>()
                .runtime_log_streams
                .lock()
                .map(|streams| streams.status(&instance_id, &log_path, lease))
                .unwrap_or(RuntimeLogStreamStatus::Cancelled);
            if status == RuntimeLogStreamStatus::Cancelled {
                break;
            }
            let read_path = path.clone();
            let read_process = process.clone();
            let final_read = status == RuntimeLogStreamStatus::ProducerFinished;
            let read = tokio::task::spawn_blocking(move || {
                let snapshot = cache.read(&read_path, &read_process, final_read);
                (cache, snapshot)
            })
            .await;
            let snapshot = match read {
                Ok((next_cache, snapshot)) => {
                    cache = next_cache;
                    snapshot
                }
                Err(error) => {
                    let document = app_storage::GameLogSnapshot::read_error(
                        log_path.clone(),
                        format!("Game log reader failed: {error}"),
                    );
                    if let Ok(streams) = app.state::<DesktopState>().runtime_log_streams.lock()
                        && streams.is_active(&instance_id, &log_path, lease)
                    {
                        let _ = app.emit(
                            RUNTIME_LOG_STREAM_EVENT,
                            payload(
                                &instance_id,
                                &process,
                                &log_path,
                                document.revision,
                                document.snapshot,
                            ),
                        );
                    }
                    break;
                }
            };
            if let Some(document) = snapshot
                && let Ok(streams) = app.state::<DesktopState>().runtime_log_streams.lock()
                && streams.is_active(&instance_id, &log_path, lease)
            {
                let _ = app.emit(
                    RUNTIME_LOG_STREAM_EVENT,
                    payload(
                        &instance_id,
                        &process,
                        &log_path,
                        document.revision,
                        document.snapshot,
                    ),
                );
            }
            // The game writer has exited. One bounded final document read owns
            // the remaining tail; native snapshots never replay a delta history.
            if final_read {
                break;
            }
            tokio::time::sleep(LIMITS.poll_interval).await;
        }
        if let Ok(mut streams) = app.state::<DesktopState>().runtime_log_streams.lock() {
            streams.release(&instance_id, &log_path, lease);
        }
    });
    Ok(())
}

fn payload(
    instance_id: &str,
    process: &InstanceProcessState,
    path: &str,
    revision: u64,
    snapshot: LogTailSnapshot,
) -> RuntimeLogStreamPayload {
    RuntimeLogStreamPayload {
        instance_id: instance_id.into(),
        process_key: Some(process.process_key.clone()),
        display_name: Some(process.display_name.clone()),
        run_id: Some(process.run_id),
        log_path: path.into(),
        lines: vec![],
        byte_offset: 0,
        emitted_at_unix_ms: current_unix_ms(),
        snapshot: Some(snapshot),
        snapshot_revision: Some(revision),
        stream_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn process() -> InstanceProcessState {
        InstanceProcessState {
            run_id: 1,
            session_id: Some("fixture".into()),
            process_key: "main".into(),
            display_name: "Island".into(),
            pid: Some(123),
            process_identity: Some(app_core::ProcessIdentity {
                creation_time: 134_353_044_000_000_000,
                image_path: "synthetic.exe".into(),
            }),
            status: "running".into(),
            started_at: None,
            stopped_at: None,
            exit_code: None,
            crash_flag: false,
            log_path: None,
            is_primary: true,
        }
    }

    #[test]
    fn game_log_document_cache_replaces_partial_lines_and_emits_only_changes() {
        let root = std::env::temp_dir().join(format!(
            "langame-ark-document-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("native.log");
        let process = process();
        let mut cache = DocumentCache::default();
        std::fs::write(&path, "[2026.10.01-04.59.59:999][0] previous run ready\n").unwrap();
        assert!(
            cache
                .read(&path, &process, false)
                .unwrap()
                .snapshot
                .lines
                .is_empty()
        );
        assert!(cache.read(&path, &process, false).is_none());
        std::fs::write(&path, "[2026.10.01-05.00.01:000][0] startup part").unwrap();
        assert!(
            cache.read(&path, &process, false).unwrap().snapshot.lines[0].ends_with("startup part")
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"ial complete\n")
            .unwrap();
        let complete = cache.read(&path, &process, false).unwrap();
        assert_eq!(
            complete.snapshot.lines,
            ["[2026.10.01-05.00.01:000][0] startup partial complete"]
        );
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(file.metadata().unwrap().modified().unwrap() + Duration::from_secs(1))
            .unwrap();
        assert!(
            cache.read(&path, &process, false).is_none(),
            "Metadata-only changes must not duplicate the document"
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"[2026.10.01-05.00.02:000][0] final unterminated")
            .unwrap();
        let final_doc = cache.read(&path, &process, true).unwrap();
        assert!(
            final_doc
                .snapshot
                .lines
                .last()
                .unwrap()
                .ends_with("final unterminated")
        );
        assert!(cache.read(&path, &process, true).is_none());
        drop(file);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn game_log_document_cache_detects_replacement_and_bounds_the_visible_tail() {
        let root = std::env::temp_dir().join(format!(
            "langame-ark-document-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("native.log");
        let process = process();
        let mut cache = DocumentCache::default();
        std::fs::write(&path, "[2026.10.01-05.00.01:000][0] old\n").unwrap();
        cache.read(&path, &process, false).unwrap();
        std::fs::rename(&path, root.join("previous.log")).unwrap();
        std::fs::write(&path, "[2026.10.01-05.00.01:000][0] new\n").unwrap();
        assert!(cache.read(&path, &process, false).unwrap().snapshot.lines[0].ends_with("new"));
        let lines = (0..500)
            .map(|index| format!("[2026.10.01-05.00.02:000][0] line {index}\n"))
            .collect::<String>();
        std::fs::write(&path, lines).unwrap();
        let snapshot = cache.read(&path, &process, false).unwrap().snapshot;
        assert_eq!(snapshot.lines.len(), 400);
        assert!(snapshot.truncated);
        assert!(snapshot.lines[0].ends_with("line 100"));
        assert!(snapshot.lines[399].ends_with("line 499"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
