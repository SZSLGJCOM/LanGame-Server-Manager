use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock, Weak};

type PreviewRows = Mutex<Vec<String>>;

fn registry() -> &'static Mutex<HashMap<PathBuf, Weak<PreviewRows>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Weak<PreviewRows>>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

fn poisoned_preview() -> io::Error {
    io::Error::other("The live console preview lock failed; restart the affected server session.")
}

/// Read the durable log and its not-yet-completed terminal rows atomically.
///
/// The callback must read the log at `path` while this lock is held, and use the
/// preview only when that read reaches the current end of the durable log. Rows
/// are a replacement snapshot without newline terminators, not appended events.
/// No active console (including a not-yet-created log) supplies an empty slice.
/// Keep the callback short and do not call this function recursively.
pub fn with_live_console_preview<T>(
    path: &Path,
    read: impl FnOnce(&[String]) -> T,
) -> io::Result<T> {
    let path = std::path::absolute(path)?;
    let preview = {
        let mut registry = registry().lock().map_err(|_| poisoned_preview())?;
        let preview = registry.get(&path).and_then(Weak::upgrade);
        if preview.is_none() {
            registry.remove(&path);
        }
        preview
    };
    match preview {
        Some(preview) => {
            let rows = preview.lock().map_err(|_| poisoned_preview())?;
            Ok(read(&rows))
        }
        None => Ok(read(&[])),
    }
}

/// Only the output reader owns this handle. Its lifetime prevents stale rows
/// from surviving a finished process, and replacing the same path cannot let an
/// older process overwrite the new process's preview.
pub struct LiveConsolePreview {
    rows: Arc<PreviewRows>,
}

impl LiveConsolePreview {
    pub fn register(path: &Path) -> io::Result<Self> {
        let path = std::path::absolute(path)?;
        let rows = Arc::new(Mutex::new(Vec::new()));
        let mut registry = registry().lock().map_err(|_| poisoned_preview())?;
        registry.retain(|_, preview| preview.strong_count() > 0);
        registry.insert(path, Arc::downgrade(&rows));
        Ok(Self { rows })
    }

    /// Persist completed output before publishing its replacement preview, under
    /// the reader's lock. A failed commit leaves the previous preview intact.
    /// Callers must keep rows newline-free and within 128 rows / 64 KiB in total.
    pub fn replace<T>(
        &self,
        rows: Vec<String>,
        commit: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        if rows.len() > 128
            || rows.iter().map(String::len).sum::<usize>() > 64 * 1024
            || rows.iter().any(|row| row.contains(['\r', '\n']))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "The live console preview exceeds its row or byte limit.",
            ));
        }
        let mut current = self.rows.lock().map_err(|_| poisoned_preview())?;
        let result = commit()?;
        *current = rows;
        Ok(result)
    }

    /// The writer must persist and flush completed rows before replacing this
    /// snapshot, under the same lock used by the durable-log reader above.
    #[cfg(any(windows, test))]
    pub(super) fn update<T>(
        &self,
        update: impl FnOnce(&mut Vec<String>) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut rows = self.rows.lock().map_err(|_| poisoned_preview())?;
        update(&mut rows)
    }
}

#[cfg(test)]
#[path = "console_live_output_tests.rs"]
mod tests;
