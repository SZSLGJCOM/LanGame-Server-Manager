//! The latest accepted startup owns its receipt and log independently of game runs.
use crate::managed_console_log::{ManagedConsoleLog, owned_fs::reject_links};
use crate::{StorageError, StoragePaths};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[path = "startup_session_log.rs"]
mod log_page;
pub use log_page::{StartupLogPage, read_log_page, read_log_page_aligned};
#[path = "startup_native_source.rs"]
mod native_source;
pub use native_source::NativeStartupSource;

const RECEIPT_NAME: &str = "startup-session.json";
const MAX_RECEIPT_BYTES: u64 = 512 * 1024;
static RECEIPTS: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartupSessionStatus {
    Starting,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StartupSessionReceipt {
    pub attempt_id: String,
    pub status: StartupSessionStatus,
    pub started_at_unix_ms: u128,
    pub finished_at_unix_ms: Option<u128>,
    pub source_path: String,
    pub run_ids: Vec<i64>,
    pub error: Option<String>,
    pub reader_error: Option<String>,
    #[serde(default)]
    pub native_sources: Vec<NativeStartupSource>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Diagnostic {
    id: String,
    message: String,
    at: u128,
    written: bool,
}

#[derive(Deserialize, Serialize)]
struct StoredSession {
    format: u32,
    instance_id: String,
    receipt: StartupSessionReceipt,
    #[serde(default)]
    diagnostics: Vec<Diagnostic>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |value| value.as_millis())
}

pub async fn instance_root(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<PathBuf, StorageError> {
    let pool = crate::storage_db::connect_pool(paths).await?;
    let record = crate::storage_db::fetch_instance_record(&pool, instance_id).await;
    pool.close().await;
    let record = record?;
    crate::instance_reconciliation::checked_instance_root(paths, &record.config_dir)
}

fn receipt_path(root: &Path) -> io::Result<PathBuf> {
    reject_links(root)?;
    let path = root.join("logs").join(RECEIPT_NAME);
    reject_links(&path)?;
    Ok(path)
}

fn expected_log(root: &Path, started_at: u128) -> PathBuf {
    root.join("logs")
        .join("managed-console")
        .join(format!("run-{started_at}-startup.log"))
}

fn read_stored(root: &Path) -> io::Result<Option<StoredSession>> {
    let path = receipt_path(root)?;
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if file.metadata()?.len() > MAX_RECEIPT_BYTES {
        return Err(invalid("Startup session receipt exceeds its limit"));
    }
    let stored: StoredSession = serde_json::from_reader(file)?;
    let expected = expected_log(root, stored.receipt.started_at_unix_ms);
    if stored.format != 1
        || stored.receipt.attempt_id.is_empty()
        || stored.receipt.attempt_id.len() > 128
        || Path::new(&stored.receipt.source_path) != expected
        || stored.diagnostics.len() > 64
    {
        return Err(invalid(
            "Startup session receipt has an invalid identity or owned path",
        ));
    }
    reject_links(&expected)?;
    Ok(Some(stored))
}

fn save(root: &Path, stored: &StoredSession) -> io::Result<()> {
    let path = receipt_path(root)?;
    let bytes = serde_json::to_vec_pretty(stored)?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(invalid("Startup session receipt exceeds its limit"));
    }
    crate::atomic_file::write_file_atomically(&path, &bytes)
}

pub fn read_latest(root: &Path, instance_id: &str) -> io::Result<Option<StartupSessionReceipt>> {
    let root = fs::canonicalize(root)?;
    let root = root.as_path();
    let _guard = RECEIPTS
        .lock()
        .map_err(|_| invalid("Startup receipt lock poisoned"))?;
    let stored = read_stored(root)?;
    if stored
        .as_ref()
        .is_some_and(|value| value.instance_id != instance_id)
    {
        return Err(invalid("Startup receipt belongs to another instance"));
    }
    Ok(stored.map(|value| value.receipt))
}

/// Call only after rejecting duplicate starts and resolving the owned instance root.
pub fn begin(
    root: &Path,
    instance_id: &str,
    attempt_id: &str,
    reserved_at: u128,
) -> io::Result<(StartupSessionReceipt, ManagedConsoleLog)> {
    let root = fs::canonicalize(root)?;
    let root = root.as_path();
    if attempt_id.is_empty() || attempt_id.len() > 128 {
        return Err(invalid("Invalid startup attempt identity"));
    }
    let _guard = RECEIPTS
        .lock()
        .map_err(|_| invalid("Startup receipt lock poisoned"))?;
    let previous = read_stored(root)?;
    let started_at = previous.as_ref().map_or(reserved_at, |old| {
        reserved_at.max(old.receipt.started_at_unix_ms.saturating_add(1))
    });
    let path = expected_log(root, started_at);
    let writer = ManagedConsoleLog::open(&path)?;
    let receipt = StartupSessionReceipt {
        attempt_id: attempt_id.into(),
        status: StartupSessionStatus::Starting,
        started_at_unix_ms: started_at,
        finished_at_unix_ms: None,
        source_path: path.to_string_lossy().into_owned(),
        run_ids: Vec::new(),
        error: None,
        reader_error: None,
        native_sources: Vec::new(),
    };
    save(
        root,
        &StoredSession {
            format: 1,
            instance_id: instance_id.into(),
            receipt: receipt.clone(),
            diagnostics: Vec::new(),
        },
    )?;
    Ok((receipt, writer))
}

pub fn finish(
    root: &Path,
    attempt_id: &str,
    status: StartupSessionStatus,
    run_ids: Vec<i64>,
    error: Option<String>,
) -> io::Result<StartupSessionReceipt> {
    let root = fs::canonicalize(root)?;
    let root = root.as_path();
    let _guard = RECEIPTS
        .lock()
        .map_err(|_| invalid("Startup receipt lock poisoned"))?;
    let mut stored = read_stored(root)?.ok_or_else(|| invalid("Startup receipt is missing"))?;
    if stored.receipt.attempt_id != attempt_id {
        return Err(invalid("Startup receipt was replaced by a later attempt"));
    }
    stored.receipt.status = status;
    stored.receipt.finished_at_unix_ms = Some(now_ms());
    stored.receipt.run_ids = run_ids;
    stored.receipt.error = error;
    save(root, &stored)?;
    Ok(stored.receipt)
}

/// The caller holds the instance's reservation exclusion until this returns.
/// Recheck the durable identity/status as well: a previously observed receipt
/// must never overwrite a completed result or a later accepted attempt.
pub fn recover_interrupted(
    root: &Path,
    instance_id: &str,
    observed_attempt_id: &str,
) -> io::Result<StartupSessionReceipt> {
    let root = fs::canonicalize(root)?;
    let _guard = RECEIPTS
        .lock()
        .map_err(|_| invalid("Startup receipt lock poisoned"))?;
    let mut stored = read_stored(&root)?.ok_or_else(|| invalid("Startup receipt is missing"))?;
    if stored.instance_id != instance_id {
        return Err(invalid("Startup receipt belongs to another instance"));
    }
    if stored.receipt.attempt_id != observed_attempt_id
        || stored.receipt.status != StartupSessionStatus::Starting
    {
        return Ok(stored.receipt);
    }
    let at = now_ms();
    let mut message = String::from(
        "The runtime session ended before the startup result was recorded; inspect the retained logs and runtime state.",
    );
    let append = ManagedConsoleLog::open(&stored.receipt.source_path).and_then(|mut writer| {
        writeln!(
            writer,
            "[{at}] [LanGameCMD/startup] Startup interrupted: {message}"
        )
    });
    if let Err(error) = append {
        message.push_str(&format!(" Could not append the interruption: {error}"));
    }
    stored.receipt.status = StartupSessionStatus::Interrupted;
    stored.receipt.finished_at_unix_ms = Some(at);
    stored.receipt.error = Some(message);
    save(&root, &stored)?;
    Ok(stored.receipt)
}

/// Separate receipt storage avoids recursively writing into the log that failed to read.
pub fn record_reader_error(log_path: &Path, message: &str) -> io::Result<()> {
    if log_path
        .parent()
        .and_then(Path::file_name)
        .is_none_or(|name| name != "managed-console")
        || log_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| !name.ends_with("-startup.log"))
    {
        return Ok(());
    }
    let Some(root) = log_path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
    else {
        return Ok(());
    };
    let root = fs::canonicalize(root)?;
    let root = root.as_path();
    let _guard = RECEIPTS
        .lock()
        .map_err(|_| invalid("Startup receipt lock poisoned"))?;
    let Some(mut stored) = read_stored(root)? else {
        return Ok(());
    };
    if Path::new(&stored.receipt.source_path).file_name() != log_path.file_name() {
        return Ok(());
    }
    stored.receipt.reader_error = Some(format!(
        "[{}] [LanGameCMD/reader] {}",
        now_ms(),
        message.chars().take(4096).collect::<String>()
    ));
    save(root, &stored)
}

pub fn record_native_source(
    startup_log_path: &Path,
    binding: &NativeStartupSource,
) -> io::Result<()> {
    let root = startup_log_path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| invalid("Startup log root is missing"))?;
    let root = fs::canonicalize(root)?;
    let _guard = RECEIPTS
        .lock()
        .map_err(|_| invalid("Startup receipt lock poisoned"))?;
    let mut stored = read_stored(&root)?.ok_or_else(|| invalid("Startup receipt is missing"))?;
    if Path::new(&stored.receipt.source_path).file_name() != startup_log_path.file_name() {
        return Err(invalid(
            "Native startup binding belongs to an earlier attempt",
        ));
    }
    if let Some(existing) = stored.receipt.native_sources.iter_mut().find(|source| {
        source.process_key == binding.process_key && source.source_path == binding.source_path
    }) {
        *existing = binding.clone();
    } else {
        if stored.receipt.native_sources.len() >= 32 {
            return Err(invalid("Startup native source limit reached"));
        }
        stored.receipt.native_sources.push(binding.clone());
    }
    save(&root, &stored)
}

pub fn record_diagnostic(
    root: &Path,
    instance_id: &str,
    attempt_id: &str,
    id: &str,
    message: &str,
) -> io::Result<()> {
    let root = fs::canonicalize(root)?;
    let root = root.as_path();
    if id.is_empty() || id.len() > 128 || message.is_empty() || message.len() > 4096 {
        return Err(invalid(
            "Startup diagnostic requires an ID of 1–128 bytes and message of 1–4096 bytes",
        ));
    }
    let redacted = app_core::abiotic_output::redact_modio_token_text(message);
    let message = redacted.as_str();
    let _guard = RECEIPTS
        .lock()
        .map_err(|_| invalid("Startup receipt lock poisoned"))?;
    let mut stored =
        read_stored(root)?.ok_or_else(|| invalid("Startup receipt is not available yet"))?;
    if stored.instance_id != instance_id || stored.receipt.attempt_id != attempt_id {
        return Err(invalid("Startup diagnostic belongs to a different attempt"));
    }
    let index = if let Some(index) = stored.diagnostics.iter().position(|entry| entry.id == id) {
        let entry = &stored.diagnostics[index];
        if entry.message != message {
            return Err(invalid("Diagnostic ID was reused with different content"));
        }
        if entry.written {
            return Ok(());
        }
        index
    } else {
        if stored.diagnostics.len() == 64 {
            return Err(invalid("Startup diagnostic limit reached"));
        }
        stored.diagnostics.push(Diagnostic {
            id: id.into(),
            message: message.into(),
            at: now_ms(),
            written: false,
        });
        save(root, &stored)?;
        stored.diagnostics.len() - 1
    };
    let entry = &stored.diagnostics[index];
    // A JSON record is one physical line. Its durable intent and exact line let a
    // retry repair a failed append without duplicating an append whose ACK was lost.
    let line = format!(
        "[{}] [LanGameCMD/frontend] {}",
        entry.at,
        serde_json::to_string(&serde_json::json!({"id":entry.id,"message":entry.message}))?
    );
    if !log_page::contains_line(Path::new(&stored.receipt.source_path), &line)? {
        let mut writer = ManagedConsoleLog::open(&stored.receipt.source_path)?;
        writeln!(writer, "{line}")?;
    }
    stored.diagnostics[index].written = true;
    save(root, &stored)
}

#[cfg(test)]
#[path = "startup_sessions_tests.rs"]
mod tests;
