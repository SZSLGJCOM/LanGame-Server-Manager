use super::StartupLogCursor;
use app_core::ProcessIdentity;
use app_storage::managed_console_log::ManagedConsoleLog;
use app_storage::startup_sessions::{NativeStartupSource, record_native_source};
use std::path::Path;

#[derive(Default)]
pub(super) struct NativeStartupTranscript {
    preview_limited: bool,
}

impl NativeStartupTranscript {
    pub(super) fn record(
        &mut self,
        startup_path: &Path,
        process_key: &str,
        identity: &ProcessIdentity,
        cursor: &StartupLogCursor,
        lines: &[&String],
        oversized: bool,
    ) -> Result<(), String> {
        if self.preview_limited {
            return Ok(());
        }
        if oversized {
            // The readiness parser intentionally discards oversized records.
            // Keep the last acknowledged boundary so the running reader can
            // still display that record and everything following it.
            self.preview_limited = true;
            let message = format!(
                "[LanGame startup] Native log preview for {process_key} reached its record limit; remaining output is retained in the native log.\n"
            );
            return ManagedConsoleLog::open(startup_path)
                .and_then(|writer| writer.write_all(message.as_bytes()))
                .map_err(|error| {
                    format!("Cannot retain native startup preview diagnostic: {error}")
                });
        }
        if lines.is_empty() {
            return Ok(());
        }
        let mut source = cursor.native_snapshot()?;
        let boundary = NativeStartupSource::capture(
            process_key.to_owned(),
            &source.path,
            identity.creation_time,
            &mut source.file,
            source.offset,
        )
        .map_err(|error| format!("Cannot capture native startup transcript position: {error}"))?;
        let mut text = String::new();
        for line in lines {
            text.push_str(&format!("[game:{process_key}] "));
            text.push_str(&app_core::abiotic_output::redact_modio_token_text(line));
            text.push('\n');
        }
        ManagedConsoleLog::open(startup_path)
            .and_then(|writer| writer.write_all(text.as_bytes()))
            .map_err(|error| format!("Cannot retain native startup output: {error}"))?;
        // A failed checkpoint cannot be treated as a successful handoff: the
        // caller must finish its existing startup cleanup before Running commits.
        record_native_source(startup_path, &boundary)
            .map_err(|error| format!("Cannot retain native startup transcript position: {error}"))
    }
}
