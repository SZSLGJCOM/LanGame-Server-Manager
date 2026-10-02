use std::io::{self, Write};

use serde_json::{Value, json};

pub(super) fn record(level: &str, action: &str, message: &str) {
    record_with(
        &mut io::stderr().lock(),
        |entry| {
            let paths = app_storage::StoragePaths::resolve_default().map_err(io::Error::other)?;
            crate::desktop_app_log::append(&paths.app_log_path(), entry)
        },
        level,
        action,
        message,
    );
}

fn record_with(
    stderr: &mut impl Write,
    persist: impl FnOnce(&Value) -> io::Result<()>,
    level: &str,
    action: &str,
    message: &str,
) {
    let entry = json!({
        "ts_unix_ms": super::current_unix_ms(),
        "level": level,
        "action": action,
        "message": message,
        "context": { "pid": std::process::id() },
    });
    // The detached runtime can inherit a console pipe whose reader has closed.
    // Diagnostic output must not terminate the discovery worker in that case.
    // Persist first so its root cause survives even when stderr is unavailable.
    let persisted = persist(&entry);
    let _ = writeln!(stderr, "{message}");
    if let Err(error) = persisted {
        // AppLog retains its failure status; do not recurse into the same logger.
        let _ = writeln!(
            stderr,
            "LanGame LAN directory diagnostic log unavailable: {error}"
        );
    }
}

#[cfg(test)]
mod tests;
