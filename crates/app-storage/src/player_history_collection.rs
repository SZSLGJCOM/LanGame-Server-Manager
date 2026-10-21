use super::{StorageError, StoragePaths, connect_pool, dst};
use crate::managed_console_log::{ManagedLogSegment, open_log_segments, owned_fs};
use sqlx::{Row, SqlitePool};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
};

const READ_BYTES: usize = 256 * 1024;
const RUNS_PER_TICK: i64 = 16;
const EVENTS_PER_RUN: usize = 128;

struct Source {
    run_id: i64,
    instance_id: String,
    process_key: String,
    path: PathBuf,
    logs_root: PathBuf,
    started_ms: Option<i64>,
    offset: i64,
    generation: i64,
    last_elapsed: Option<i64>,
    identity: Option<String>,
    issue: Option<String>,
    running: bool,
}

struct Chunk {
    bytes: Vec<u8>,
    start: i64,
    identity: Option<String>,
    discontinuity: bool,
    at_end: bool,
}

/// Called by the runtime heartbeat even while no player view is mounted.
/// Fair round-robin reads bound each tick to 4 MiB / 2048 events. Cursor and events commit
/// together, so restart/retry cannot publish duplicates or lose a partial line.
pub async fn collect_player_history(paths: &StoragePaths) -> Result<(), StorageError> {
    let pool = connect_pool(paths).await?;
    let rows = sqlx::query("SELECT r.id, r.instance_id, COALESCE(r.process_key, 'main') AS process_key,
        r.log_path, i.logs_path, CAST(strftime('%s', r.started_at) AS INTEGER) * 1000 AS started_ms,
        COALESCE(c.byte_offset, 0) AS byte_offset, COALESCE(c.generation, 0) AS generation,
        c.last_elapsed_ms, c.source_identity, c.issue, r.status
        FROM instance_runs r JOIN instances i ON i.id = r.instance_id
        LEFT JOIN player_history_cursors c ON c.run_id = r.id
        WHERE i.module_id = 'dontstarve' AND r.log_path IS NOT NULL AND (r.status = 'running' OR COALESCE(c.completed, 0) = 0)
        ORDER BY COALESCE(c.polled_at, 0), r.id LIMIT ?")
        .bind(RUNS_PER_TICK).fetch_all(&pool).await?;
    for row in rows {
        let source = Source {
            run_id: row.try_get("id")?,
            instance_id: row.try_get("instance_id")?,
            process_key: row.try_get("process_key")?,
            path: PathBuf::from(row.try_get::<String, _>("log_path")?),
            logs_root: PathBuf::from(row.try_get::<String, _>("logs_path")?),
            started_ms: row.try_get("started_ms")?,
            offset: row.try_get("byte_offset")?,
            generation: row.try_get("generation")?,
            last_elapsed: row.try_get("last_elapsed_ms")?,
            identity: row.try_get("source_identity")?,
            issue: row.try_get("issue")?,
            running: row.try_get::<String, _>("status")? == "running",
        };
        let (source, chunk) = tokio::task::spawn_blocking(move || {
            let result = read_chunk(&source);
            (source, result)
        })
        .await
        .map_err(|error| StorageError::ReadPath {
            path: paths.database_path.clone(),
            source: io::Error::other(format!("player history reader failed: {error}")),
        })?;
        match chunk {
            Ok(chunk) => persist_chunk(&pool, source, chunk).await?,
            Err(error) => {
                // Preserve the last good cursor; a subsequent tick can retry.
                // Never persist player chat or full paths in diagnostic text.
                let issue = if error.kind() == io::ErrorKind::NotFound {
                    "source_unavailable"
                } else {
                    "source_read_failed"
                };
                sqlx::query("INSERT INTO player_history_cursors(run_id, polled_at, issue, completed) VALUES (?, ?, ?, ?)
                    ON CONFLICT(run_id) DO UPDATE SET polled_at = excluded.polled_at,
                    issue = CASE WHEN player_history_cursors.issue IN ('log_gap', 'clock_discontinuity') THEN player_history_cursors.issue ELSE excluded.issue END,
                    completed = excluded.completed")
                    .bind(source.run_id).bind(crate::current_unix_ms() as i64).bind(issue)
                    .bind(!source.running && error.kind() == io::ErrorKind::NotFound).execute(&pool).await?;
            }
        }
    }
    pool.close().await;
    Ok(())
}

fn read_chunk(source: &Source) -> io::Result<Chunk> {
    owned_fs::reject_links(&source.path)?;
    let root = std::fs::canonicalize(&source.logs_root)?;
    let parent = source
        .path
        .parent()
        .ok_or_else(|| io::Error::other("history source has no parent"))?;
    if !std::fs::canonicalize(parent)?.starts_with(root) {
        return Err(io::Error::other("history source is outside instance logs"));
    }
    let (mut segments, identity) = match open_log_segments(&source.path)? {
        Some(segments) => (segments, None),
        None => {
            let file = File::open(&source.path)?;
            let identity =
                serde_json::to_string(&owned_fs::identity(&file)?).map_err(io::Error::other)?;
            (
                vec![ManagedLogSegment {
                    path: source.path.clone(),
                    file,
                    start_offset: 0,
                }],
                Some(identity),
            )
        }
    };
    segments.sort_by_key(|segment| segment.start_offset);
    let first = segments
        .first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no retained log segments"))?
        .start_offset as i64;
    let last = segments
        .last()
        .ok_or_else(|| io::Error::other("no log segment"))?;
    let end = (last.start_offset + last.file.metadata()?.len()) as i64;
    let replaced = source.identity.is_some() && source.identity != identity;
    let discontinuity = replaced || source.offset < first || source.offset > end;
    let start = if discontinuity { first } else { source.offset };
    let mut cursor = start;
    let mut bytes = Vec::new();
    for segment in &mut segments {
        let len = segment.file.metadata()?.len();
        let segment_start = segment.start_offset as i64;
        if cursor >= segment_start + len as i64 {
            continue;
        }
        if cursor < segment_start {
            return Err(io::Error::other("gap between retained log segments"));
        }
        segment
            .file
            .seek(SeekFrom::Start((cursor - segment_start) as u64))?;
        let remaining = READ_BYTES - bytes.len();
        let read = (&mut segment.file)
            .take(remaining as u64)
            .read_to_end(&mut bytes)?;
        cursor += read as i64;
        if bytes.len() == READ_BYTES {
            break;
        }
    }
    let complete = if !source.running && cursor == end {
        bytes.len()
    } else {
        bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1)
    };
    if complete == 0 && bytes.len() == READ_BYTES {
        return Err(io::Error::other("history log line exceeds bounded reader"));
    }
    bytes.truncate(complete);
    // Retention may cut through a line. Its incomplete prefix cannot be an event.
    if discontinuity
        && first > 0
        && let Some(end) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes[..=end].fill(b' ');
        bytes[end] = b'\n';
    }
    let at_end = start + bytes.len() as i64 == end;
    Ok(Chunk {
        bytes,
        start,
        identity,
        discontinuity,
        at_end,
    })
}

async fn persist_chunk(
    pool: &SqlitePool,
    source: Source,
    chunk: Chunk,
) -> Result<(), StorageError> {
    let mut tx = pool.begin().await?;
    // Reject a racing collector instead of rewinding its newer cursor.
    let current: Option<i64> =
        sqlx::query_scalar("SELECT byte_offset FROM player_history_cursors WHERE run_id = ?")
            .bind(source.run_id)
            .fetch_optional(&mut *tx)
            .await?;
    if current.unwrap_or(0) != source.offset {
        return Ok(());
    }
    let generation = source.generation + i64::from(chunk.discontinuity);
    let mut elapsed = if chunk.discontinuity {
        None
    } else {
        source.last_elapsed
    };
    let mut issue = if chunk.discontinuity {
        Some("log_gap".to_owned())
    } else {
        source
            .issue
            .filter(|value| value == "log_gap" || value == "clock_discontinuity")
    };
    let mut offset = chunk.start;
    let mut event_count = 0;
    for bytes in chunk.bytes.split_inclusive(|byte| *byte == b'\n') {
        if event_count == EVENTS_PER_RUN {
            break;
        }
        let line_offset = offset;
        offset += bytes.len() as i64;
        let Ok(line) = std::str::from_utf8(bytes) else {
            continue;
        };
        let Some(event) = dst::parse(line) else {
            continue;
        };
        event_count += 1;
        if elapsed.is_some_and(|previous| event.elapsed_ms < previous) {
            issue = Some("clock_discontinuity".to_owned());
        }
        elapsed = Some(event.elapsed_ms);
        let estimated = if issue.as_deref() == Some("clock_discontinuity") {
            None
        } else {
            source
                .started_ms
                .and_then(|base| base.checked_add(event.elapsed_ms))
        };
        let inserted = sqlx::query("INSERT OR IGNORE INTO player_history_events
            (instance_id, run_id, process_key, generation, byte_offset, kind, player_name, account_id, message, log_time, elapsed_ms, estimated_at_unix_ms, session_ambiguous)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&source.instance_id).bind(source.run_id).bind(&source.process_key).bind(generation).bind(line_offset)
            .bind(event.kind).bind(&event.name).bind(event.account_id).bind(event.message).bind(event.log_time)
            .bind(event.elapsed_ms).bind(estimated).bind(issue.as_deref() == Some("clock_discontinuity")).execute(&mut *tx).await?;
        if inserted.rows_affected() == 0 || event.kind == "chat" {
            continue;
        }
        let id = inserted.last_insert_rowid();
        let joins = sqlx::query("SELECT id, elapsed_ms, session_ambiguous FROM player_history_events
            WHERE run_id = ? AND generation = ? AND player_name = ? AND kind = 'join' AND paired_event_id IS NULL AND id < ? ORDER BY id DESC LIMIT 2")
            .bind(source.run_id).bind(generation).bind(&event.name).bind(id).fetch_all(&mut *tx).await?;
        if event.kind == "leave"
            && joins.len() == 1
            && !joins[0].try_get::<bool, _>("session_ambiguous")?
            && issue.as_deref() != Some("clock_discontinuity")
        {
            let joined_at: i64 = joins[0].try_get("elapsed_ms")?;
            let join_id: i64 = joins[0].try_get("id")?;
            if let Some(duration) = event
                .elapsed_ms
                .checked_sub(joined_at)
                .filter(|value| *value >= 0)
            {
                sqlx::query("UPDATE player_history_events SET duration_ms = ?, paired_event_id = CASE WHEN id = ? THEN ? ELSE ? END WHERE id IN (?, ?)")
                    .bind(duration).bind(id).bind(join_id).bind(id).bind(id).bind(join_id).execute(&mut *tx).await?;
            }
        } else if !joins.is_empty() {
            // Duplicate names / missing leave records are ambiguous.
            sqlx::query("UPDATE player_history_events SET session_ambiguous = 1 WHERE run_id = ? AND generation = ? AND player_name = ? AND paired_event_id IS NULL AND kind IN ('join', 'leave')")
                .bind(source.run_id).bind(generation).bind(&event.name).execute(&mut *tx).await?;
            if event.kind == "leave" {
                // Consume one ambiguous arrival without assigning a duration or
                // claiming an account match; otherwise all future namesakes
                // would remain blocked by this incomplete session forever.
                let join_id: i64 = joins[0].try_get("id")?;
                sqlx::query("UPDATE player_history_events SET paired_event_id = CASE WHEN id = ? THEN ? ELSE ? END WHERE id IN (?, ?)")
                    .bind(id).bind(join_id).bind(id).bind(id).bind(join_id).execute(&mut *tx).await?;
            }
        }
    }
    sqlx::query("INSERT INTO player_history_cursors(run_id, byte_offset, generation, last_elapsed_ms, source_identity, polled_at, issue, completed)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(run_id) DO UPDATE SET byte_offset = excluded.byte_offset,
        generation = excluded.generation, last_elapsed_ms = excluded.last_elapsed_ms, source_identity = excluded.source_identity,
        polled_at = excluded.polled_at, issue = excluded.issue, completed = excluded.completed")
        .bind(source.run_id).bind(offset).bind(generation).bind(elapsed).bind(chunk.identity)
        .bind(crate::current_unix_ms() as i64).bind(issue)
        .bind(!source.running && chunk.at_end && offset == chunk.start + chunk.bytes.len() as i64).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
