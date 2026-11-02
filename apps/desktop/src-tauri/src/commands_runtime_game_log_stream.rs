use super::*;
use crate::runtime_log_stream::document_patch::{
    NativeLogDocument, RuntimeLogDocumentPatch, same_document,
};
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

struct ReadDocument {
    snapshot: LogTailSnapshot,
    revision: u64,
    reset: bool,
}

#[derive(Default)]
struct DocumentCache {
    stamp: Option<Result<FileStamp, String>>,
    snapshot: Option<LogTailSnapshot>,
    retry_at: Option<Instant>,
    retry_delay: Duration,
}

impl DocumentCache {
    #[cfg(test)]
    fn read(
        &mut self,
        path: &Path,
        process: &InstanceProcessState,
        final_read: bool,
    ) -> Option<ReadDocument> {
        self.read_observation(path, final_read, || {
            app_storage::read_game_log_snapshot(path.to_string_lossy().into_owned(), process, 400)
        })
    }

    fn read_after_startup(
        &mut self,
        path: &Path,
        process: &InstanceProcessState,
        boundary: Option<&app_storage::startup_sessions::NativeStartupSource>,
        final_read: bool,
    ) -> Option<ReadDocument> {
        self.read_observation(path, final_read, || {
            app_storage::read_game_log_snapshot_after_startup(
                path.to_string_lossy().into_owned(),
                process,
                400,
                boundary,
            )
        })
    }

    fn read_observation(
        &mut self,
        path: &Path,
        final_read: bool,
        read: impl FnOnce() -> app_storage::GameLogSnapshot,
    ) -> Option<ReadDocument> {
        let stamp = file_stamp(path);
        if !final_read && self.stamp.as_ref() == Some(&stamp) {
            let failed = self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.read_error.is_some());
            if !failed
                || self
                    .retry_at
                    .is_some_and(|deadline| Instant::now() < deadline)
            {
                return None;
            }
        }
        let reset = match (self.stamp.as_ref(), &stamp) {
            (Some(Ok(previous)), Ok(current)) => {
                previous.identity != current.identity || current.length < previous.length
            }
            (Some(Err(_)), Ok(_)) => true,
            // The HTTP seed and this file observation do not share a handle.
            // Establish the first observed file as a complete replacement.
            (None, _) => self.snapshot.is_some(),
            _ => false,
        };
        self.stamp = Some(stamp);
        let document = read();
        let snapshot = &document.snapshot;
        if snapshot.read_error.is_some() {
            // A successful stat does not prove its following content read worked.
            // Retry only failed observations, at a bounded rate, for this producer's
            // lifetime; unchanged healthy files retain the metadata fast path.
            self.retry_delay = if self.retry_delay.is_zero() {
                Duration::from_millis(100)
            } else {
                (self.retry_delay * 2).min(Duration::from_secs(2))
            };
            self.retry_at = Some(Instant::now() + self.retry_delay);
        } else {
            self.retry_at = None;
            self.retry_delay = Duration::ZERO;
        }
        if !reset
            && self
                .snapshot
                .as_ref()
                .is_some_and(|previous| same_document(previous, snapshot))
        {
            return None;
        }
        self.snapshot = Some(snapshot.clone());
        Some(ReadDocument {
            snapshot: document.snapshot,
            revision: document.revision,
            reset,
        })
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
    let lease = {
        let mut streams = state
            .runtime_log_streams
            .lock()
            .map_err(|_| String::from("runtime log stream registry lock poisoned"))?;
        let lease = streams.reserve_linked(instance_id, path, console_path);
        if let Some(lease) = lease {
            streams.begin_native_document(
                instance_id,
                path,
                lease,
                NativeLogDocument {
                    snapshot: document.snapshot.clone(),
                    revision: document.snapshot_revision,
                    run_id: document.run_id,
                },
            );
        }
        lease
    };
    let Some(lease) = lease else {
        return Ok(());
    };
    let app = app_handle.clone();
    let instance_id = instance_id.to_owned();
    let path = PathBuf::from(path);
    let log_path = path.to_string_lossy().into_owned();
    let process = document.process.clone();
    let startup_boundary = document.startup_boundary.clone();
    let mut cache = DocumentCache {
        stamp: None,
        snapshot: Some(document.snapshot.clone()),
        ..Default::default()
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
            let read_boundary = startup_boundary.clone();
            let final_read = status == RuntimeLogStreamStatus::ProducerFinished;
            let read = tokio::task::spawn_blocking(move || {
                let snapshot = cache.read_after_startup(
                    &read_path,
                    &read_process,
                    read_boundary.as_ref(),
                    final_read,
                );
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
                    publish(
                        &app,
                        &instance_id,
                        &process,
                        &log_path,
                        lease,
                        ReadDocument {
                            snapshot: document.snapshot,
                            revision: document.revision,
                            reset: true,
                        },
                    );
                    break;
                }
            };
            if let Some(document) = snapshot {
                publish(&app, &instance_id, &process, &log_path, lease, document);
            }
            // The game writer has exited. The final patch and cached full
            // document share a revision so a missed terminal event is recoverable.
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

fn publish(
    app: &tauri::AppHandle,
    instance_id: &str,
    process: &InstanceProcessState,
    log_path: &str,
    lease: RuntimeLogStreamLease,
    document: ReadDocument,
) {
    if let Ok(mut streams) = app.state::<DesktopState>().runtime_log_streams.lock()
        && let Some(patch) = streams.publish_native_document(
            instance_id,
            log_path,
            lease,
            NativeLogDocument {
                snapshot: document.snapshot,
                revision: document.revision,
                run_id: process.run_id,
            },
            document.reset,
        )
    {
        // Publication and API readback share this short lock. Replacement or
        // cancellation cannot interleave between updating the base and emit.
        let _ = app.emit(
            RUNTIME_LOG_STREAM_EVENT,
            payload(instance_id, process, log_path, patch),
        );
    }
}

fn payload(
    instance_id: &str,
    process: &InstanceProcessState,
    path: &str,
    patch: RuntimeLogDocumentPatch,
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
        snapshot: None,
        snapshot_revision: Some(patch.revision),
        stream_error: None,
        console: None,
        document_patch: Some(patch),
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
    fn a_failed_read_is_retried_when_its_successful_file_stamp_has_not_changed() {
        let root =
            std::env::temp_dir().join(format!("langame-native-retry-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("native.log");
        std::fs::write(&path, "[2026.10.01-05.00.01:000][0] recovered\n").unwrap();
        let mut cache = DocumentCache::default();
        let failed = cache
            .read_observation(&path, false, || {
                app_storage::GameLogSnapshot::read_error(
                    path.to_string_lossy().into_owned(),
                    "injected temporary content failure".into(),
                )
            })
            .unwrap();
        assert!(failed.snapshot.read_error.is_some());
        assert!(
            cache
                .read_observation(&path, false, || panic!(
                    "Retry backoff must suppress repeated I/O"
                ))
                .is_none()
        );
        cache.retry_at = Some(Instant::now());
        let recovered = cache.read(&path, &process(), false).unwrap();
        assert!(recovered.snapshot.read_error.is_none());
        assert!(recovered.snapshot.lines[0].ends_with("recovered"));
        assert!(cache.read(&path, &process(), false).is_none());
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

    #[test]
    fn native_file_rotation_with_identical_text_emits_a_complete_replacement_patch() {
        use crate::runtime_log_stream::document_patch::document_patch;
        let root =
            std::env::temp_dir().join(format!("langame-native-patch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("native.log");
        let contents = "[2026.10.01-05.00.01:000][0] same\n";
        std::fs::write(&path, contents).unwrap();
        let mut cache = DocumentCache::default();
        let initial = cache.read(&path, &process(), false).unwrap();
        let initial = NativeLogDocument {
            snapshot: initial.snapshot,
            revision: initial.revision,
            run_id: 1,
        };
        // Preserve the old file object so an identical-size replacement has a
        // distinct identity even when its visible rows and mtime are unchanged.
        std::fs::rename(&path, root.join("previous.log")).unwrap();
        std::fs::write(&path, contents).unwrap();
        let replacement = cache.read(&path, &process(), false).unwrap();
        assert!(replacement.reset);
        let replacement = NativeLogDocument {
            snapshot: replacement.snapshot,
            revision: replacement.revision,
            run_id: 1,
        };
        let patch = document_patch(&initial, &replacement, true);
        assert_eq!((patch.start, patch.delete_count), (0, 1));
        assert_eq!(patch.lines, replacement.snapshot.lines);
        let event = payload("ark", &process(), path.to_str().unwrap(), patch);
        assert!(event.snapshot.is_none());
        assert!(event.lines.is_empty());
        assert!(event.console.is_none());
        assert_eq!(event.snapshot_revision, Some(replacement.revision));
        assert_eq!(
            event.document_patch.unwrap().previous_revision,
            initial.revision
        );

        std::fs::write(&path, "").unwrap();
        let truncated = cache.read(&path, &process(), false).unwrap();
        assert!(truncated.reset);
        assert!(truncated.snapshot.lines.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
