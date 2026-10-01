use super::*;

static GAME_LOG_READ_SEQUENCE: std::sync::Mutex<u64> = std::sync::Mutex::new(0);

pub struct GameLogSnapshot {
    pub snapshot: LogTailSnapshot,
    pub revision: u64,
}

fn ordered_snapshot(read: impl FnOnce() -> LogTailSnapshot) -> GameLogSnapshot {
    // Only bounded file reads share this lock. Database access and server
    // lifecycle operations must never hold it, so shutdown logs keep flowing.
    // The counter is the only protected state; a failed reader cannot corrupt it.
    let mut sequence = GAME_LOG_READ_SEQUENCE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let snapshot = read();
    *sequence = sequence.saturating_add(1);
    GameLogSnapshot {
        snapshot,
        revision: *sequence,
    }
}

impl GameLogSnapshot {
    pub fn read_error(source_path: String, message: String) -> Self {
        ordered_snapshot(|| LogTailSnapshot {
            source_path: Some(source_path),
            lines: vec![],
            total_lines: 0,
            truncated: false,
            read_error: Some(message),
        })
    }
}

/// The console source owns the producer lifetime of this map's native file.
pub struct GameLogDocument {
    pub snapshot: LogTailSnapshot,
    pub snapshot_revision: u64,
    pub process_key: String,
    pub display_name: String,
    pub run_id: i64,
    pub console_log_path: Option<String>,
    pub process: InstanceProcessState,
}

pub(super) async fn load_instance_run_by_id(
    pool: &SqlitePool,
    instance_id: &str,
    run_id: i64,
) -> Result<StoredInstanceRunRow, StorageError> {
    let row = sqlx::query("SELECT * FROM instance_runs WHERE instance_id = ? AND id = ?")
        .bind(instance_id)
        .bind(run_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| StorageError::MissingInstanceRun {
            instance_id: instance_id.into(),
            run_id,
        })?;
    Ok(map_instance_run_row(&row))
}

/// Stable map filenames may still contain the previous startup. UE timestamped
/// lines establish the segment belonging to the captured OS process creation;
/// undated continuation lines are retained only inside that current segment.
pub fn read_game_log_snapshot(
    source_path: String,
    process: &InstanceProcessState,
    max_lines: usize,
) -> GameLogSnapshot {
    ordered_snapshot(|| read_current_game_log_snapshot(source_path, process, max_lines))
}

fn read_current_game_log_snapshot(
    source_path: String,
    process: &InstanceProcessState,
    max_lines: usize,
) -> LogTailSnapshot {
    let mut snapshot = read_log_snapshot(Some(source_path), RUNTIME_STARTUP_SCAN_LINE_LIMIT);
    let mut current = false;
    snapshot.lines.retain(|line| {
        if ark_readiness::native_line_timestamp(line).is_some() {
            current = ark_readiness::native_line_is_current(process, line);
        }
        current
    });
    snapshot.total_lines = snapshot.lines.len();
    let limit = max_lines.clamp(1, 400);
    snapshot.truncated |= snapshot.lines.len() > limit;
    if snapshot.lines.len() > limit {
        snapshot.lines.drain(..snapshot.lines.len() - limit);
    }
    snapshot
}

pub async fn read_instance_game_log_document(
    paths: &StoragePaths,
    instance_id: &str,
    max_lines: usize,
    run_id: i64,
) -> Result<GameLogDocument, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let record = fetch_instance_record(&pool, instance_id).await?;
        let invalid = |reason: String| StorageError::InvalidGameLogSource {
            instance_id: instance_id.into(), run_id, reason,
        };
        if !app_core::ark_maps::is_ark(&record.summary.module_id) {
            return Err(invalid("Game source selection requires an ARK map".into()));
        }
        let run = load_instance_run_by_id(&pool, instance_id, run_id).await?;
        // A native path is reused. Only the latest recorded run of this map may
        // display its current file; older run documents retain their own stdout.
        let latest: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(id) FROM instance_runs WHERE instance_id = ? AND COALESCE(process_key, 'main') = ?"
        ).bind(instance_id).bind(&run.process_key).fetch_one(&pool).await?;
        if latest != Some(run_id) {
            return Err(invalid("This map has a newer run; select its retained console output for the older run".into()));
        }
        if run.process_identity.as_ref().is_none_or(|identity| identity.creation_time < 116_444_736_000_000_000) {
            return Err(invalid("The retained run has no precise process creation time for its native log".into()));
        }
        let process = map_process_state(&run);
        let native_path = app_core::ark_maps::native_log_path(
            &record.summary.module_id, &instance_logs_dir_from_record(&record), &run.process_key,
        ).map_err(invalid)?;
        let read_process = process.clone();
        let document = tokio::task::spawn_blocking(move || {
            read_game_log_snapshot(native_path.to_string_lossy().into_owned(), &read_process, max_lines)
        }).await.map_err(|error| StorageError::BlockingTaskFailed {
            operation: "game log document read", message: error.to_string(),
        })?;
        Ok(GameLogDocument {
            snapshot: document.snapshot, snapshot_revision: document.revision,
            process_key: run.process_key, display_name: run.display_name,
            run_id, console_log_path: run.log_path, process,
        })
    }.await;
    pool.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn ark_game_log_revision_orders_snapshot_reads_across_final_drain() {
        let root =
            std::env::temp_dir().join(format!("langame-ark-read-order-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("native.log");
        std::fs::write(&path, "startup\n").unwrap();
        let captured = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let first_path = path.clone();
        let first_captured = captured.clone();
        let first_release = release.clone();
        let first = std::thread::spawn(move || {
            ordered_snapshot(|| {
                let snapshot =
                    read_log_snapshot(Some(first_path.to_string_lossy().into_owned()), 400);
                first_captured.wait();
                first_release.wait();
                snapshot
            })
        });
        captured.wait();
        let serialized_during_read = matches!(
            GAME_LOG_READ_SEQUENCE.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        );
        std::fs::write(&path, "startup\nfinal shutdown\n").unwrap();
        let final_path = path.clone();
        let final_read = std::thread::spawn(move || {
            ordered_snapshot(|| {
                read_log_snapshot(Some(final_path.to_string_lossy().into_owned()), 400)
            })
        });
        release.wait();
        let older = first.join().unwrap();
        let newer = final_read.join().unwrap();
        assert!(
            serialized_during_read,
            "The revision boundary must include the complete file read"
        );
        assert!(older.revision < newer.revision);
        assert_eq!(older.snapshot.lines, ["startup"]);
        assert_eq!(newer.snapshot.lines, ["startup", "final shutdown"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
