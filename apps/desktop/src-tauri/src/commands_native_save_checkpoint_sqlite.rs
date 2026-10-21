//! Test-only SQLite checkpoints. A live database must be copied through SQLite,
//! including its committed WAL view; copying the main file is not a checkpoint.
use libsqlite3_sys as ffi;
use sqlx::sqlite::{
    LockedSqliteHandle, SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous,
};
use sqlx::{Connection, SqliteConnection};
use std::ffi::CStr;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

const BUSY_BUDGET: Duration = Duration::from_secs(30);

/// The caller must first guard both paths as ordinary paths in its disposable
/// fixture. `target` must be new, with an existing, exclusively owned parent.
/// Success means a standalone, quick_check-clean physical SQLite snapshot, not
/// proof that the game included the manual save's intended world state.
pub(super) async fn backup(source: &Path, target: &Path) -> Result<(), String> {
    let source = source.to_path_buf();
    let target = target.to_path_buf();
    let runtime = tokio::runtime::Handle::current();
    // Native backup_step performs disk I/O and may wait for SQLite locks. Keep
    // that work off the async executor, including the bounded busy wait.
    tokio::task::spawn_blocking(move || runtime.block_on(backup_owned(&source, &target)))
        .await
        .map_err(|error| format!("SQLite checkpoint worker failed: {error}"))?
}

async fn backup_owned(source: &Path, target: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("SQLite checkpoint source is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || is_reparse(&metadata) {
        return Err("SQLite checkpoint source must be an ordinary existing file".into());
    }
    let mut header = [0_u8; 16];
    fs::File::open(source)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| format!("Read SQLite checkpoint source header: {error}"))?;
    if &header != b"SQLite format 3\0" {
        return Err("SQLite checkpoint source has no SQLite 3 database header".into());
    }
    // Reject orphan companions as well as the main target. They must never be
    // mistaken for journals belonging to this newly reserved database.
    for companion in companions(target) {
        match fs::symlink_metadata(companion) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Inspect SQLite checkpoint target: {error}")),
            Ok(_) => return Err("SQLite checkpoint target has an existing companion".into()),
        }
    }
    let reserved = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| format!("Reserve new SQLite checkpoint target: {error}"))?;
    drop(reserved);

    let result = capture(source, target).await;
    if result.is_err() {
        // Only this create_new file is ours. Never remove the source or an
        // existing target, and preserve cleanup errors alongside the cause.
        return combine(
            result,
            fs::remove_file(target)
                .map_err(|error| format!("Remove failed SQLite checkpoint target: {error}")),
        );
    }
    result
}

async fn capture(source: &Path, target: &Path) -> Result<(), String> {
    let options = SqliteConnectOptions::new()
        .filename(source)
        .read_only(true)
        .create_if_missing(false)
        .busy_timeout(Duration::from_millis(50))
        .pragma("query_only", "ON");
    // Do not use immutable=true: it would ignore the live source's WAL.
    let mut source = SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| format!("Open read-only SQLite checkpoint source: {error}"))?;
    let options = SqliteConnectOptions::new()
        .filename(target)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Delete)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::ZERO);
    let mut target_connection = match SqliteConnection::connect_with(&options).await {
        Ok(connection) => connection,
        Err(error) => {
            return combine(
                Err(format!("Open reserved SQLite checkpoint target: {error}")),
                close(source, "source").await,
            );
        }
    };
    let result = capture_view(&mut source, &mut target_connection).await;
    let result = combine(result, close(source, "source").await);
    let result = combine(result, close(target_connection, "target").await);
    result?;
    // The result must be usable without a WAL companion during restart restore.
    for companion in companions(target) {
        if companion.exists() {
            return Err("SQLite checkpoint retained a journal companion after close".into());
        }
    }
    OpenOptions::new()
        .write(true)
        .open(target)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("Sync completed SQLite checkpoint: {error}"))
}

async fn capture_view(
    source: &mut SqliteConnection,
    target: &mut SqliteConnection,
) -> Result<(), String> {
    let mut view = source
        .begin()
        .await
        .map_err(|error| format!("Begin SQLite checkpoint read transaction: {error}"))?;
    // BEGIN is deferred. An actual read pins this committed WAL transaction
    // before backup_init, so subsequent source commits cannot change the view.
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqlite_schema")
        .fetch_one(&mut *view)
        .await
        .map_err(|error| format!("Pin SQLite checkpoint source view: {error}"))?;
    let result = async {
        let mut source_handle = view
            .lock_handle()
            .await
            .map_err(|error| format!("Lock SQLite checkpoint source handle: {error}"))?;
        let mut target_handle = target
            .lock_handle()
            .await
            .map_err(|error| format!("Lock SQLite checkpoint target handle: {error}"))?;
        copy_locked(&mut source_handle, &mut target_handle)
    }
    .await;
    let result = combine(
        result,
        view.rollback()
            .await
            .map_err(|error| format!("End SQLite checkpoint read transaction: {error}")),
    );
    result?;
    // Copy page images without VACUUM or rebuilding tables: anonymous rowids
    // and all committed WAL pages retain their original values.
    let journal: String = sqlx::query_scalar("PRAGMA journal_mode=DELETE")
        .fetch_one(&mut *target)
        .await
        .map_err(|error| format!("Make SQLite checkpoint standalone: {error}"))?;
    if !journal.eq_ignore_ascii_case("delete") {
        return Err("SQLite checkpoint did not enter standalone DELETE journal mode".into());
    }
    let check: Vec<String> = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_all(target)
        .await
        .map_err(|error| format!("Check SQLite checkpoint integrity: {error}"))?;
    if check.len() != 1 || check[0] != "ok" {
        return Err("SQLite checkpoint failed quick_check".into());
    }
    Ok(())
}

fn copy_locked(
    source: &mut LockedSqliteHandle<'_>,
    target: &mut LockedSqliteHandle<'_>,
) -> Result<(), String> {
    // Both live SQLx handles are exclusively locked for this entire operation;
    // neither background worker can access its connection until finish returns.
    // SQLite's online backup API includes the source read transaction's WAL:
    // https://www.sqlite.org/c3ref/backup_finish.html
    let raw = unsafe {
        ffi::sqlite3_backup_init(
            target.as_raw_handle().as_ptr(),
            c"main".as_ptr(),
            source.as_raw_handle().as_ptr(),
            c"main".as_ptr(),
        )
    };
    let Some(raw) = NonNull::new(raw) else {
        // SAFETY: the destination handle remains locked and alive.
        let code = unsafe { ffi::sqlite3_errcode(target.as_raw_handle().as_ptr()) };
        return Err(native_error("initialize backup", code));
    };
    let mut backup = BackupHandle(Some(raw));
    let deadline = Instant::now() + BUSY_BUDGET;
    let result = loop {
        if Instant::now() >= deadline {
            break Err("SQLite checkpoint exceeded its 30-second backup lock/I/O budget".into());
        }
        // SAFETY: the backup and both exclusive SQLx handle guards remain live.
        let code = unsafe { ffi::sqlite3_backup_step(raw.as_ptr(), 256) };
        match code {
            ffi::SQLITE_DONE => break Ok(()),
            ffi::SQLITE_OK => {}
            ffi::SQLITE_BUSY | ffi::SQLITE_LOCKED => {
                // Continue the same SQLite backup, never resend a game save.
                std::thread::sleep(Duration::from_millis(10));
            }
            code => break Err(native_error("step backup", code)),
        }
    };
    let finish = backup.finish();
    combine(
        result,
        if finish == ffi::SQLITE_OK {
            Ok(())
        } else {
            Err(native_error("finish backup", finish))
        },
    )
}

struct BackupHandle(Option<NonNull<ffi::sqlite3_backup>>);

impl BackupHandle {
    fn finish(&mut self) -> i32 {
        match self.0.take() {
            // SAFETY: this backup is finished exactly once while both locked
            // connections are alive. SQLite releases its destination transaction.
            Some(handle) => unsafe { ffi::sqlite3_backup_finish(handle.as_ptr()) },
            None => ffi::SQLITE_MISUSE,
        }
    }
}

impl Drop for BackupHandle {
    fn drop(&mut self) {
        if self.0.is_some() {
            // Only unwind fallback; the normal path explicitly preserves this
            // return code. Finish also releases a partially copied transaction.
            self.finish();
        }
    }
}

fn native_error(stage: &str, code: i32) -> String {
    // SAFETY: SQLite returns a static NUL-terminated string for every code.
    let message = unsafe { CStr::from_ptr(ffi::sqlite3_errstr(code)) }.to_string_lossy();
    format!("SQLite checkpoint could not {stage}: {message} (code {code})")
}

async fn close(connection: SqliteConnection, name: &str) -> Result<(), String> {
    connection
        .close()
        .await
        .map_err(|error| format!("Close SQLite checkpoint {name}: {error}"))
}

fn combine(first: Result<(), String>, second: Result<(), String>) -> Result<(), String> {
    match (first, second) {
        (Err(first), Err(second)) => Err(format!("{first}; additionally: {second}")),
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn companions(path: &Path) -> [PathBuf; 3] {
    ["-wal", "-shm", "-journal"].map(|suffix| {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    })
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sqlite_checkpoint_copies_committed_wal_excludes_uncommitted_and_preserves_rowids() {
        let root = crate::commands::tests::temp_test_dir("sqlite-checkpoint");
        let source = root.join("live.db");
        let target = root.join("snapshot.db");
        let mut writer = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&source)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .pragma("wal_autocheckpoint", "0"),
        )
        .await
        .unwrap();
        sqlx::query("CREATE TABLE world (value TEXT NOT NULL)")
            .execute(&mut writer)
            .await
            .unwrap();
        sqlx::query("INSERT INTO world (rowid, value) VALUES (7, 'deleted'), (1001, 'committed'), (9001, 'retained')")
            .execute(&mut writer).await.unwrap();
        sqlx::query("DELETE FROM world WHERE rowid=7")
            .execute(&mut writer)
            .await
            .unwrap();
        let wal = &companions(&source)[0];
        assert!(fs::metadata(wal).unwrap().len() > 0);
        let mut pending = writer.begin().await.unwrap();
        sqlx::query("UPDATE world SET value='uncommitted' WHERE rowid=1001")
            .execute(&mut *pending)
            .await
            .unwrap();
        sqlx::query("INSERT INTO world (rowid, value) VALUES (42, 'uncommitted')")
            .execute(&mut *pending)
            .await
            .unwrap();
        let source_bytes = fs::read(&source).unwrap();
        let wal_bytes = fs::read(wal).unwrap();
        backup(&source, &target).await.unwrap();
        assert_eq!(fs::read(&source).unwrap(), source_bytes);
        assert_eq!(fs::read(wal).unwrap(), wal_bytes);
        assert!(companions(&target).iter().all(|path| !path.exists()));
        let mut snapshot = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&target)
                .read_only(true),
        )
        .await
        .unwrap();
        let rows: Vec<(i64, String)> =
            sqlx::query_as("SELECT rowid, value FROM world ORDER BY rowid")
                .fetch_all(&mut snapshot)
                .await
                .unwrap();
        assert_eq!(
            rows,
            vec![(1001, "committed".into()), (9001, "retained".into())]
        );
        snapshot.close().await.unwrap();
        pending.rollback().await.unwrap();
        writer.close().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn sqlite_checkpoint_refuses_existing_target_and_missing_or_invalid_source() {
        let root = crate::commands::tests::temp_test_dir("sqlite-guard");
        let missing = root.join("missing.db");
        let target = root.join("target.db");
        assert!(backup(&missing, &target).await.is_err());
        assert!(!target.exists());
        let source = root.join("live.db");
        let mut writer = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&source)
                .create_if_missing(true),
        )
        .await
        .unwrap();
        sqlx::query("CREATE TABLE world (value TEXT)")
            .execute(&mut writer)
            .await
            .unwrap();
        writer.close().await.unwrap();
        fs::write(&target, b"retained target").unwrap();
        assert!(backup(&source, &target).await.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"retained target");
        fs::remove_file(&target).unwrap();
        fs::write(&companions(&target)[0], b"retained orphan WAL").unwrap();
        assert!(backup(&source, &target).await.is_err());
        assert!(!target.exists());
        assert_eq!(
            fs::read(&companions(&target)[0]).unwrap(),
            b"retained orphan WAL"
        );
        fs::remove_file(&companions(&target)[0]).unwrap();
        let corrupt = root.join("corrupt.db");
        let mut corrupt_bytes = vec![0_u8; 512];
        corrupt_bytes[..16].copy_from_slice(b"SQLite format 3\0");
        fs::write(&corrupt, &corrupt_bytes).unwrap();
        assert!(backup(&corrupt, &target).await.is_err());
        assert!(!target.exists());
        assert_eq!(fs::read(&corrupt).unwrap(), corrupt_bytes);
        fs::remove_dir_all(root).unwrap();
    }
}
