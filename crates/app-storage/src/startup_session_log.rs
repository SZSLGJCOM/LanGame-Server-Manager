use crate::managed_console_log::open_log_segments;
use app_core::LogTailSnapshot;
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

pub struct StartupLogPage {
    pub snapshot: LogTailSnapshot,
    pub before_cursor: Option<String>,
    pub retained_start_cursor: String,
}

fn scan(
    path: &Path,
    before: Option<u64>,
    mut line: impl FnMut(u64, u64, &[u8], bool) -> io::Result<()>,
) -> io::Result<(u64, u64)> {
    let segments =
        open_log_segments(path)?.ok_or_else(|| io::Error::other("Startup log is not managed"))?;
    let first = segments.first().map_or(0, |part| part.start_offset);
    let end = segments
        .last()
        .map(|part| {
            part.file
                .metadata()
                .map(|meta| part.start_offset + meta.len())
        })
        .transpose()?
        .unwrap_or(first);
    let before = before.unwrap_or(end);
    if before < first || before > end {
        return Err(io::Error::other(
            "Startup page cursor expired or is outside the retained log",
        ));
    }
    let mut pending = Vec::new();
    let mut line_start = first;
    for segment in segments {
        if segment.start_offset >= before {
            break;
        }
        let mut offset = segment.start_offset;
        let count = (before - offset).min(segment.file.metadata()?.len());
        let mut reader = BufReader::new(segment.file.take(count));
        loop {
            let read = reader.read_until(b'\n', &mut pending)?;
            if read == 0 {
                break;
            }
            offset += read as u64;
            if pending.len() > 512 * 1024 {
                return Err(io::Error::other(
                    "Startup log row exceeds the 512 KiB page limit",
                ));
            }
            if pending.last() == Some(&b'\n') {
                pending.pop();
                if pending.last() == Some(&b'\r') {
                    pending.pop();
                }
                line(line_start, offset, &pending, true)?;
                pending.clear();
                line_start = offset;
            }
        }
    }
    if !pending.is_empty() {
        if before != end {
            return Err(io::Error::other(
                "Startup page cursor is not a row boundary",
            ));
        }
        line(line_start, before, &pending, false)?;
    }
    Ok((first, before))
}

pub fn read_log_page(
    path: &Path,
    before: Option<&str>,
    max_lines: usize,
) -> io::Result<StartupLogPage> {
    read_log_page_aligned(path, before, max_lines, None)
}

/// Align pages with the running reader's cumulative row positions even when
/// retention has removed segments that reader already consumed. The anchor is
/// captured with its revision under the stream registry lock.
pub fn read_log_page_aligned(
    path: &Path,
    before: Option<&str>,
    max_lines: usize,
    anchor: Option<(u64, u64)>,
) -> io::Result<StartupLogPage> {
    let before = before
        .map(str::parse::<u64>)
        .transpose()
        .map_err(|_| io::Error::other("Invalid startup page cursor"))?;
    let mut rows = VecDeque::new();
    let mut total_lines = 0usize;
    let mut bytes = 0;
    let mut anchor_rows = 0u64;
    // Scan retained segments once. Output is bounded, and rows after the page
    // boundary only establish the captured reader's retained-prefix count.
    let mut before_is_boundary = before.is_none();
    let (first, end) = scan(path, None, |start, end, data, complete| {
        before_is_boundary |= before == Some(start) || before == Some(end);
        if complete && anchor.is_some_and(|(offset, _)| end <= offset) {
            anchor_rows += 1;
        }
        if before.is_some_and(|before| end > before) {
            return Ok(());
        }
        let text = String::from_utf8_lossy(data).into_owned();
        bytes += text.len();
        rows.push_back((start, text));
        total_lines += 1;
        while rows.len() > max_lines.clamp(1, 400) || bytes > 512 * 1024 {
            if let Some((_, text)) = rows.pop_front() {
                bytes -= text.len();
            }
        }
        Ok(())
    })?;
    if let Some(before) = before
        && (before < first || before > end || (!before_is_boundary && before != first))
    {
        return Err(io::Error::other(
            "Startup page cursor expired or is not a retained row boundary",
        ));
    }
    if let Some((offset, total)) = anchor {
        if offset < first || anchor_rows > total {
            return Err(io::Error::other(
                "Startup reader position no longer matches the retained log; refresh the page",
            ));
        }
        total_lines = total_lines
            .checked_add(
                usize::try_from(total - anchor_rows)
                    .map_err(|_| io::Error::other("Startup row position overflow"))?,
            )
            .ok_or_else(|| io::Error::other("Startup row position overflow"))?;
    }
    let before_cursor = rows
        .front()
        .filter(|(start, _)| *start > first)
        .map(|(start, _)| start.to_string());
    let truncated = first > 0 || total_lines > rows.len();
    Ok(StartupLogPage {
        snapshot: LogTailSnapshot {
            source_path: Some(path.to_string_lossy().into_owned()),
            lines: rows.into_iter().map(|(_, line)| line).collect(),
            total_lines,
            truncated,
            read_error: None,
        },
        before_cursor,
        retained_start_cursor: first.to_string(),
    })
}

pub(super) fn contains_line(path: &Path, expected: &str) -> io::Result<bool> {
    let mut found = false;
    scan(path, None, |_, _, line, _| {
        found |= line == expected.as_bytes();
        Ok(())
    })?;
    Ok(found)
}
