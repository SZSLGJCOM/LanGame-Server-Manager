use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const EXCERPT_BYTES: usize = 64 * 1024;
const TRUNCATION_NOTICE: &str = "[content log excerpt truncated]\n";

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
    let text = String::from_utf8_lossy(bytes);
    let lines = text.lines().rev().take(40).collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    let mut excerpt = lines.into_iter().rev().collect::<Vec<_>>().join("\n");
    if offset != 0 || excerpt.len() > EXCERPT_BYTES {
        let mut start = excerpt
            .len()
            .saturating_sub(EXCERPT_BYTES - TRUNCATION_NOTICE.len());
        while !excerpt.is_char_boundary(start) {
            start += 1;
        }
        excerpt.drain(..start);
        excerpt.insert_str(0, TRUNCATION_NOTICE);
    }
    Some(excerpt)
}

#[cfg(test)]
#[path = "steamcmd_content_log_tests.rs"]
mod tests;
