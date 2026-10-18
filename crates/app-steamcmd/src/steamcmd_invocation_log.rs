//! Read only complete records appended after the invocation snapshot. Every
//! poll is bounded; rotation or rewriting permanently disables this reader.
use super::{Snapshot, checkpoint, identity, open_log, read_snapshot};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

const READ_BYTES: u64 = 64 * 1024;

pub(crate) struct InvocationLog {
    state: Option<State>,
}

struct State {
    path: PathBuf,
    anchor: Option<Snapshot>,
    current: Option<Snapshot>,
    pending: Vec<u8>,
    discard_line: bool,
    usable: bool,
}

#[derive(Default)]
pub(crate) struct LogUpdate {
    pub(crate) activity: bool,
    pub(crate) lines: Vec<String>,
}

impl InvocationLog {
    pub(crate) async fn before_spawn(path: PathBuf) -> Self {
        let state = tokio::task::spawn_blocking(move || {
            let snapshot = read_snapshot(&path);
            let usable = snapshot.is_ok();
            let current = snapshot.ok().flatten();
            let discard_line = current.as_ref().is_some_and(|before| {
                before.length > 0
                    && !before
                        .checkpoint
                        .last()
                        .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
            });
            State {
                path,
                anchor: current.clone(),
                current,
                pending: Vec::new(),
                discard_line,
                usable,
            }
        })
        .await
        .ok();
        Self { state }
    }

    pub(crate) async fn poll(&mut self) -> LogUpdate {
        let Some(mut state) = self.state.take() else {
            return LogUpdate::default();
        };
        if let Ok((state, update)) = tokio::task::spawn_blocking(move || {
            let update = state.read_new_lines().unwrap_or_else(|_| {
                state.usable = false;
                LogUpdate::default()
            });
            (state, update)
        })
        .await
        {
            self.state = Some(state);
            update
        } else {
            LogUpdate::default()
        }
    }
}

impl State {
    fn read_new_lines(&mut self) -> io::Result<LogUpdate> {
        if !self.usable {
            return Ok(LogUpdate::default());
        }
        let mut file = match open_log(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound && self.current.is_none() => {
                return Ok(LogUpdate::default());
            }
            Err(error) => return Err(error),
        };
        let length = file.metadata()?.len();
        let id = identity(&file)?;
        for snapshot in [&self.anchor, &self.current].into_iter().flatten() {
            if id != snapshot.identity
                || length < snapshot.length
                || checkpoint(&mut file, snapshot.length)? != snapshot.checkpoint
            {
                self.usable = false;
                self.pending.clear();
                return Ok(LogUpdate::default());
            }
        }
        let offset = self.current.as_ref().map_or(0, |snapshot| snapshot.length);
        let count = (length - offset).min(READ_BYTES);
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; count as usize];
        file.read_exact(&mut bytes)?;
        let next = Snapshot {
            identity: id,
            length: offset + count,
            checkpoint: checkpoint(&mut file, offset + count)?,
        };
        if self.anchor.is_none() {
            self.anchor = Some(next.clone());
        }
        self.current = Some(next);
        let mut update = LogUpdate {
            activity: count > 0,
            lines: Vec::new(),
        };
        for byte in bytes {
            if matches!(byte, b'\r' | b'\n') {
                if !self.discard_line
                    && !self.pending.is_empty()
                    && let Ok(line) = std::str::from_utf8(&self.pending)
                {
                    update.lines.push(line.to_owned());
                }
                self.pending.clear();
                self.discard_line = false;
            } else if !self.discard_line {
                if self.pending.len() < crate::STEAMCMD_OUTPUT_LINE_LIMIT_BYTES {
                    self.pending.push(byte);
                } else {
                    self.pending.clear();
                    self.discard_line = true;
                }
            }
        }
        Ok(update)
    }
}
