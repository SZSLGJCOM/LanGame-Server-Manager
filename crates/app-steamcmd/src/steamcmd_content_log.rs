use std::io;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const EXCERPT_BYTES: usize = 64 * 1024;
const TRUNCATION_NOTICE: &str = "[content log excerpt truncated]\n";
const WORKSHOP_TRUNCATION_NOTICE: &str = "[workshop log excerpt truncated]\n";
const CHECKPOINT_BYTES: u64 = 64;

pub(super) struct WorkshopLogExcerpt {
    before: WorkshopLogStart,
}

enum WorkshopLogStart {
    Missing(PathBuf),
    Existing {
        file: File,
        length: u64,
        checkpoint: Vec<u8>,
    },
    Unavailable,
}

impl WorkshopLogExcerpt {
    pub(super) async fn before_spawn(root: &Path) -> Self {
        let path = root.join("logs/workshop_log.txt");
        let before = match open_workshop_log(&path).await {
            Ok(mut file) => match file.metadata().await {
                Ok(metadata) if metadata.is_file() => {
                    let length = metadata.len();
                    match read_checkpoint(&mut file, length).await {
                        Ok(checkpoint) => WorkshopLogStart::Existing {
                            file,
                            length,
                            checkpoint,
                        },
                        Err(_) => WorkshopLogStart::Unavailable,
                    }
                }
                _ => WorkshopLogStart::Unavailable,
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                WorkshopLogStart::Missing(path)
            }
            Err(_) => WorkshopLogStart::Unavailable,
        };
        Self { before }
    }

    pub(super) async fn finish(self) -> Option<String> {
        let (mut file, start, checkpoint) = match self.before {
            WorkshopLogStart::Existing {
                file,
                length,
                checkpoint,
            } => (file, length, checkpoint),
            WorkshopLogStart::Missing(path) => {
                (open_workshop_log(&path).await.ok()?, 0, Vec::new())
            }
            WorkshopLogStart::Unavailable => return None,
        };
        let metadata = file.metadata().await.ok()?;
        let length = metadata.len();
        if !metadata.is_file() || length <= start {
            return None;
        }
        if read_checkpoint(&mut file, start).await.ok()? != checkpoint {
            // A truncate-and-regrow can retain the same handle and exceed the
            // original length. Its replacement bytes are not invocation evidence.
            return None;
        }

        // Retain the original open handle: rotating/replacing the path must not
        // import the replacement log's historical records into this invocation.
        let offset = start.max(length.saturating_sub(EXCERPT_BYTES as u64));
        file.seek(std::io::SeekFrom::Start(offset)).await.ok()?;
        let count = length - offset;
        let mut bytes = Vec::with_capacity(count as usize);
        file.take(count).read_to_end(&mut bytes).await.ok()?;
        if bytes.len() as u64 != count {
            return None;
        }
        let truncated = offset > start;
        let continuing_old_line = start > 0
            && !checkpoint
                .last()
                .is_some_and(|byte| matches!(*byte, b'\r' | b'\n'));
        let bytes = if truncated || continuing_old_line {
            match bytes.iter().position(|byte| matches!(*byte, b'\r' | b'\n')) {
                Some(index) if index + 1 < bytes.len() => &bytes[index + 1..],
                _ if continuing_old_line => return None,
                _ => bytes.as_slice(),
            }
        } else {
            bytes.as_slice()
        };
        let excerpt = bounded_excerpt(bytes, truncated, WORKSHOP_TRUNCATION_NOTICE)?;
        if excerpt
            .to_ascii_lowercase()
            .contains("missing decryption key")
        {
            return Some(excerpt);
        }
        // Keep a decisive earlier reason when the line budget rolls forward.
        // Both searches remain inside the same bounded, invocation-only bytes.
        let text = String::from_utf8_lossy(bytes);
        let reason = text
            .lines()
            .find(|line| line.to_ascii_lowercase().contains("missing decryption key"));
        match reason {
            Some(reason) if reason.len() + excerpt.len() < EXCERPT_BYTES => {
                Some(format!("{reason}\n{excerpt}"))
            }
            _ => Some(excerpt),
        }
    }
}

async fn open_workshop_log(path: &Path) -> io::Result<File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        // SteamCMD can append and rotate its log while this diagnostic snapshot
        // holds the original file handle open.
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    options.open(path).await
}

async fn read_checkpoint(file: &mut File, offset: u64) -> io::Result<Vec<u8>> {
    let count = offset.min(CHECKPOINT_BYTES);
    file.seek(std::io::SeekFrom::Start(offset - count)).await?;
    let mut bytes = vec![0; count as usize];
    file.read_exact(&mut bytes).await?;
    Ok(bytes)
}

pub(crate) async fn read_steamcmd_content_log_excerpt(steamcmd_root: &Path) -> Option<String> {
    let mut file = tokio::fs::File::open(steamcmd_root.join("logs/content_log.txt"))
        .await
        .ok()?;
    let length = file.metadata().await.ok()?.len();
    let offset = length.saturating_sub(EXCERPT_BYTES as u64);
    file.seek(std::io::SeekFrom::Start(offset)).await.ok()?;
    let mut bytes = Vec::with_capacity((length - offset) as usize);
    // Take the metadata snapshot's tail only. A writer appending concurrently
    // must not turn an excerpt request into an unbounded read-to-EOF loop.
    file.take(length - offset)
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    let bytes = if offset != 0 {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .filter(|first_line| first_line + 1 < bytes.len())
            .map_or(bytes.as_slice(), |first_line| &bytes[first_line + 1..])
    } else {
        bytes.as_slice()
    };
    bounded_excerpt(bytes, offset != 0, TRUNCATION_NOTICE)
}

fn bounded_excerpt(bytes: &[u8], truncated: bool, truncation_notice: &str) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let lines = text.lines().rev().take(40).collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    let mut excerpt = lines.into_iter().rev().collect::<Vec<_>>().join("\n");
    if truncated || excerpt.len() > EXCERPT_BYTES {
        let mut start = excerpt
            .len()
            .saturating_sub(EXCERPT_BYTES - truncation_notice.len());
        while !excerpt.is_char_boundary(start) {
            start += 1;
        }
        excerpt.drain(..start);
        excerpt.insert_str(0, truncation_notice);
    }
    Some(excerpt)
}

#[cfg(test)]
#[path = "steamcmd_content_log_tests.rs"]
mod tests;
