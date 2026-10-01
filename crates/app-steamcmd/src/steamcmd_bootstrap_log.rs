use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const READ_LIMIT_BYTES: usize = 4 * 1024;
const CHECKPOINT_BYTES: usize = 64;

pub(super) struct BootstrapLogUpdate {
    pub(super) bytes: Vec<u8>,
    pub(super) activity: bool,
    pub(super) reset: bool,
}

pub(super) struct BootstrapLog {
    path: PathBuf,
    offset: u64,
    created: Option<SystemTime>,
    checkpoint: Vec<u8>,
    utf8_pending: Vec<u8>,
}

impl BootstrapLog {
    pub(super) async fn before_spawn(root: &Path) -> io::Result<Self> {
        Self::snapshot(root.join("logs").join("bootstrap_log.txt")).await
    }

    pub(super) async fn snapshot(path: PathBuf) -> io::Result<Self> {
        let mut log = Self {
            path,
            offset: 0,
            created: None,
            checkpoint: Vec::new(),
            utf8_pending: Vec::new(),
        };
        match File::open(&log.path).await {
            Ok(mut file) => {
                let metadata = file.metadata().await?;
                log.offset = metadata.len();
                log.created = metadata.created().ok();
                log.checkpoint = read_checkpoint(&mut file, log.offset).await?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(log)
    }

    pub(super) async fn poll(&mut self) -> io::Result<BootstrapLogUpdate> {
        let mut file = match File::open(&self.path).await {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.reset();
                return Ok(BootstrapLogUpdate {
                    bytes: Vec::new(),
                    activity: false,
                    reset: true,
                });
            }
            Err(error) => return Err(error),
        };
        let metadata = file.metadata().await?;
        let created = metadata.created().ok();
        // The checkpoint also detects truncate-and-regrow between polls, when
        // the inode/creation time and the new length alone are insufficient.
        let reset = metadata.len() < self.offset
            || (self.created.is_some() && self.created != created)
            || (!self.checkpoint.is_empty()
                && read_checkpoint(&mut file, self.offset).await? != self.checkpoint);
        if reset {
            self.reset();
        }
        self.created = created;
        file.seek(std::io::SeekFrom::Start(self.offset)).await?;
        let mut bytes = [0_u8; READ_LIMIT_BYTES];
        let read = file.read(&mut bytes).await?;
        self.offset += read as u64;
        self.checkpoint.extend_from_slice(&bytes[..read]);
        let keep_from = self.checkpoint.len().saturating_sub(CHECKPOINT_BYTES);
        self.checkpoint.drain(..keep_from);
        self.utf8_pending.extend_from_slice(&bytes[..read]);
        let complete = match std::str::from_utf8(&self.utf8_pending) {
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            _ => self.utf8_pending.len(),
        };
        let bytes = self.utf8_pending.drain(..complete).collect();
        Ok(BootstrapLogUpdate {
            bytes,
            activity: read > 0,
            reset,
        })
    }

    fn reset(&mut self) {
        self.offset = 0;
        self.created = None;
        self.checkpoint.clear();
        self.utf8_pending.clear();
    }
}

async fn read_checkpoint(file: &mut File, offset: u64) -> io::Result<Vec<u8>> {
    let length = offset.min(CHECKPOINT_BYTES as u64) as usize;
    file.seek(std::io::SeekFrom::Start(offset - length as u64))
        .await?;
    let mut bytes = vec![0_u8; length];
    let mut read = 0;
    while read < length {
        let count = file.read(&mut bytes[read..]).await?;
        if count == 0 {
            break;
        }
        read += count;
    }
    bytes.truncate(read);
    Ok(bytes)
}

#[cfg(test)]
#[path = "steamcmd_bootstrap_log_tests.rs"]
mod tests;
