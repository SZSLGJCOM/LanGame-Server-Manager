use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use app_steamcmd::{InstallConsoleOutput, InstallOutputSource, InstallProgressUpdate};

use super::progress::PrestartProgressLog;

const MAX_PENDING_BYTES: usize = 60 * 1024;
const MAX_OUTPUT_STREAMS: usize = 16;

struct RawStream {
    source: InstallOutputSource,
    offset: u64,
    pending: Vec<u8>,
}

/// Each pipe owns its UTF-8 and record boundary. Interleaved reads must never
/// splice half a character from stdout into stderr in the shared durable log.
pub(in crate::commands) struct PrestartConsole {
    log: app_storage::managed_console_log::ManagedConsoleLog,
    preview: app_runtime::LiveConsolePreview,
    progress: PrestartProgressLog,
    progress_preview: Option<String>,
    streams: BTreeMap<u64, RawStream>,
    // Keep bytes until the atomic durable/preview publication succeeds. A
    // preview validation error must not consume already completed records.
    uncommitted: Vec<u8>,
    diagnostic_log: Option<std::path::PathBuf>,
    finished: bool,
}

impl PrestartConsole {
    pub(in crate::commands) fn open(path: &str) -> io::Result<Self> {
        let path = Path::new(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self {
            log: app_storage::managed_console_log::ManagedConsoleLog::open(path)?,
            preview: app_runtime::LiveConsolePreview::register(path)?,
            progress: PrestartProgressLog::default(),
            progress_preview: None,
            streams: BTreeMap::new(),
            uncommitted: Vec::new(),
            diagnostic_log: None,
            finished: false,
        })
    }

    pub(in crate::commands) fn open_for_startup(
        path: &str,
        storage: &app_storage::StorageBootstrap,
    ) -> io::Result<Self> {
        let mut console = Self::open(path)?;
        console.diagnostic_log = Some(super::super::desktop_app_log_path(storage));
        Ok(console)
    }

    pub(in crate::commands) fn update(&mut self, update: &InstallProgressUpdate) -> io::Result<()> {
        if let Some(InstallConsoleOutput::Raw {
            stream_id,
            source,
            offset,
            bytes,
            finished,
        }) = &update.console_output
        {
            self.raw(*stream_id, *source, *offset, bytes, *finished)
        } else {
            let projection = self.progress.update(update);
            self.progress_preview = projection.preview;
            self.publish(stage_bytes(projection.durable))
        }
    }

    fn raw(
        &mut self,
        id: u64,
        source: InstallOutputSource,
        offset: u64,
        bytes: &[u8],
        finished: bool,
    ) -> io::Result<()> {
        if !bytes.is_empty() {
            self.progress_preview = None;
        }
        if !self.streams.contains_key(&id) && self.streams.len() >= MAX_OUTPUT_STREAMS {
            return Err(io::Error::other("Too many active startup output streams."));
        }
        let stream = self.streams.entry(id).or_insert_with(|| RawStream {
            source,
            offset: 0,
            pending: Vec::new(),
        });
        if stream.offset != offset || stream.source != source {
            return Err(io::Error::other(
                "Startup output byte position changed; refusing to duplicate or skip log data.",
            ));
        }
        stream.offset += bytes.len() as u64;
        stream.pending.extend_from_slice(bytes);
        let mut consumed = 0;
        for (index, byte) in stream.pending.iter().enumerate() {
            if *byte == b'\n' || (*byte == b'\r' && (index + 1 < stream.pending.len() || finished))
            {
                consumed = index + 1;
            }
        }
        let mut durable = stream.pending.drain(..consumed).collect::<Vec<_>>();
        if finished {
            durable.append(&mut stream.pending);
            if !durable.is_empty() && !matches!(durable.last(), Some(b'\n' | b'\r')) {
                durable.push(b'\n');
            }
            self.streams.remove(&id);
        }
        if self
            .streams
            .values()
            .map(|stream| stream.pending.len())
            .sum::<usize>()
            > MAX_PENDING_BYTES
        {
            self.commit(durable, Vec::new())?;
            return Err(io::Error::other(
                "Startup output has more than 60 KiB of unfinished records; output cannot be safely displayed.",
            ));
        }
        self.publish(durable)
    }

    fn publish(&mut self, bytes: Vec<u8>) -> io::Result<()> {
        let mut rows = self
            .streams
            .values()
            .filter_map(|stream| {
                let bytes = stream
                    .pending
                    .strip_suffix(b"\r")
                    .unwrap_or(&stream.pending);
                let complete = match std::str::from_utf8(bytes) {
                    Err(error) if error.error_len().is_none() => error.valid_up_to(),
                    _ => bytes.len(),
                };
                (complete > 0).then(|| String::from_utf8_lossy(&bytes[..complete]).into_owned())
            })
            .collect::<Vec<_>>();
        if let Some(progress) = &self.progress_preview {
            rows.push(format!("[LanGame startup update] {progress}"));
        }
        self.commit(bytes, rows)
    }

    fn commit(&mut self, bytes: Vec<u8>, rows: Vec<String>) -> io::Result<()> {
        self.uncommitted.extend(bytes);
        self.preview
            .replace(rows, || self.log.write_all(&self.uncommitted))?;
        self.uncommitted.clear();
        Ok(())
    }

    pub(in crate::commands) fn finish(mut self) -> io::Result<()> {
        let result = self.close();
        self.finished = true;
        result
    }

    fn close(&mut self) -> io::Result<()> {
        let mut bytes = Vec::new();
        for stream in self.streams.values_mut() {
            if !stream.pending.is_empty() {
                bytes.append(&mut stream.pending);
                if !matches!(bytes.last(), Some(b'\n' | b'\r')) {
                    bytes.push(b'\n');
                }
            }
        }
        self.streams.clear();
        bytes.extend(stage_bytes(self.progress.finish().durable));
        self.progress_preview = None;
        self.publish(bytes)
    }
}

impl Drop for PrestartConsole {
    fn drop(&mut self) {
        if !self.finished
            && let Err(error) = self.close()
        {
            let message = format!("Startup console final output could not be recorded: {error}");
            if let Some(path) = &self.diagnostic_log {
                let entry = serde_json::json!({
                    "ts_unix_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_millis()).unwrap_or(0),
                    "level": "error", "action": "instance.startup_console.finish_failed", "message": message,
                });
                if let Err(log_error) = crate::desktop_app_log::append(path, &entry) {
                    eprintln!("{message}; diagnostic log failed: {log_error}");
                }
            } else {
                eprintln!("{message}");
            }
        }
    }
}

fn stage_bytes(lines: Vec<String>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for line in lines {
        for row in line.split(['\r', '\n']).filter(|row| !row.is_empty()) {
            bytes.extend_from_slice(format!("[LanGame startup update] {row}\n").as_bytes());
        }
    }
    bytes
}

#[cfg(test)]
#[path = "commands_runtime_prestart_console_tests.rs"]
mod tests;
