use std::io;
use std::path::Path;

use crate::STEAMCMD_OUTPUT_LINE_LIMIT_BYTES;
use crate::steamcmd_bootstrap_log::BootstrapLog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LogSource {
    Bootstrap,
    Console,
}

pub(super) struct StreamLogs {
    logs: [BootstrapLog; 2],
    pending: [Vec<u8>; 2],
    truncated: [bool; 2],
    last_snapshot: [String; 2],
}

pub(super) struct LogUpdate {
    pub(super) activity: bool,
    pub(super) lines: Vec<(LogSource, String)>,
}

impl StreamLogs {
    pub(super) async fn before_spawn(root: &Path) -> io::Result<Self> {
        Ok(Self {
            logs: [
                BootstrapLog::before_spawn(root).await?,
                BootstrapLog::snapshot(root.join("logs/console_log.txt")).await?,
            ],
            pending: Default::default(),
            truncated: [false; 2],
            last_snapshot: Default::default(),
        })
    }

    pub(super) async fn poll(&mut self) -> io::Result<LogUpdate> {
        let mut result = LogUpdate {
            activity: false,
            lines: Vec::new(),
        };
        for (index, source) in [LogSource::Bootstrap, LogSource::Console]
            .into_iter()
            .enumerate()
        {
            let update = self.logs[index].poll().await?;
            result.activity |= update.activity;
            if update.reset {
                self.pending[index].clear();
                self.truncated[index] = false;
                self.last_snapshot[index].clear();
            }
            for byte in update.bytes {
                if matches!(byte, b'\r' | b'\n') {
                    if !self.pending[index].is_empty() || self.truncated[index] {
                        let line = super::finish_bounded_output_line(
                            &self.pending[index],
                            self.truncated[index],
                        );
                        if line != self.last_snapshot[index] {
                            result.lines.push((source, line));
                        }
                        self.pending[index].clear();
                        self.truncated[index] = false;
                        self.last_snapshot[index].clear();
                    }
                } else if self.pending[index].len() < STEAMCMD_OUTPUT_LINE_LIMIT_BYTES {
                    self.pending[index].push(byte);
                } else {
                    self.truncated[index] = true;
                }
            }
            // Login waits can stay unterminated for many seconds. Publish only
            // newly read, complete UTF-8, retaining the pending line for its
            // eventual suffix and suppressing an identical completed record.
            if update.activity && !self.pending[index].is_empty() {
                let complete = match std::str::from_utf8(&self.pending[index]) {
                    Ok(_) => self.pending[index].len(),
                    Err(error) if error.error_len().is_none() => error.valid_up_to(),
                    Err(_) => continue,
                };
                let snapshot = super::finish_bounded_output_line(
                    &self.pending[index][..complete],
                    self.truncated[index],
                );
                if !snapshot.is_empty() && snapshot != self.last_snapshot[index] {
                    result.lines.push((source, snapshot.clone()));
                    self.last_snapshot[index] = snapshot;
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "steamcmd_stream_logs_tests.rs"]
mod tests;
