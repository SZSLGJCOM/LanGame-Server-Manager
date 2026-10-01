use crate::runtime_log_stream::{
    RuntimeLogTailState,
    file_identity::{LogFileIdentity, log_identity},
    read_runtime_log_delta_bounded, runtime_log_tail_at_end,
};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const READ_BUDGET: usize = 1024 * 1024;
const LINE_BUDGET: usize = 4096;

pub(super) struct WorldReadiness {
    cursor: RuntimeLogTailState,
    plain_identity: Option<LogFileIdentity>,
    ready: bool,
}

impl WorldReadiness {
    /// Capture before spawning: a retained world and earlier ready output are
    /// valid recovery data, but are not evidence about the new native process.
    pub(super) fn before_spawn(path: &Path) -> io::Result<Self> {
        let mut cursor = runtime_log_tail_at_end(path)?;
        let (mut last, plain_identity) =
            match app_storage::managed_console_log::open_log_segments(path)? {
                Some(mut segments) => (
                    segments
                        .pop()
                        .ok_or_else(|| io::Error::other("bootstrap log has no segment"))?
                        .file,
                    None,
                ),
                None => {
                    let file = File::open(path)?;
                    let identity = log_identity(path, &file)?;
                    (file, Some(identity))
                }
            };
        if last.metadata()?.len() > 0 {
            last.seek(SeekFrom::End(-1))?;
            let mut byte = [0];
            last.read_exact(&mut byte)?;
            if !matches!(byte[0], b'\r' | b'\n') {
                cursor.pending_text = String::from("[pre-spawn partial line] ");
            }
        }
        Ok(Self {
            cursor,
            plain_identity,
            ready: false,
        })
    }

    pub(super) fn poll(&mut self, path: &Path) -> io::Result<bool> {
        self.check_generation(path)?;
        let before = self.cursor.byte_offset;
        let delta =
            read_runtime_log_delta_bounded(path, &mut self.cursor, READ_BUDGET, LINE_BUDGET)?;
        self.check_generation(path)?;
        if let Some(error) = delta.stream_error {
            return Err(io::Error::other(format!(
                "Windrose bootstrap log stream is incomplete: {error}"
            )));
        }
        if delta.byte_offset != before.saturating_add(delta.bytes_read as u64) {
            return Err(io::Error::other(
                "bootstrap log history expired before readiness was observed",
            ));
        }
        for line in delta.lines {
            match app_storage::windrose_native_stage(&line) {
                Some(app_storage::WindroseNativeStage::HostReady) => self.ready = true,
                Some(
                    app_storage::WindroseNativeStage::Loading
                    | app_storage::WindroseNativeStage::Stopping,
                ) => self.ready = false,
                None => {}
            }
        }
        // A marker in an incomplete scan cannot establish the latest native
        // stage. Preserve the bounded cursor and finish the scan next poll.
        Ok(self.ready && !delta.limit_exhausted && self.cursor.pending_text.is_empty())
    }

    fn check_generation(&self, path: &Path) -> io::Result<()> {
        if let Some(identity) = &self.plain_identity {
            let file = File::open(path)?;
            if log_identity(path, &file)? != *identity
                || file.metadata()?.len() < self.cursor.byte_offset
            {
                return Err(io::Error::other("bootstrap log was replaced or truncated"));
            }
        } else if runtime_log_tail_at_end(path)?.byte_offset < self.cursor.byte_offset {
            return Err(io::Error::other("bootstrap managed log was truncated"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "commands_windrose_bootstrap_readiness_tests.rs"]
mod tests;
