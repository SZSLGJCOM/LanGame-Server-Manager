use app_storage::managed_console_log::ManagedLogSegment;
use std::io::{self, Read, Seek, SeekFrom};

pub(super) struct SegmentDelta {
    pub bytes: Vec<u8>,
    pub offset: u64,
    pub more: bool,
    pub gap: bool,
}

/// A cursor is a run's cumulative byte position, independent of the active
/// filename. Retained open files preserve old tails while the writer rotates.
pub(super) fn read(
    segments: Vec<ManagedLogSegment>,
    mut offset: u64,
    max_bytes: usize,
) -> io::Result<SegmentDelta> {
    let mut bytes = Vec::new();
    let mut gap = false;
    let mut more = false;
    let mut previous_end = None;
    for mut segment in segments {
        let length = segment.file.metadata()?.len();
        let end = segment
            .start_offset
            .checked_add(length)
            .ok_or_else(|| io::Error::other("console log cursor overflow"))?;
        if previous_end.is_some_and(|previous| segment.start_offset < previous) {
            return Err(io::Error::other("console log segments overlap"));
        }
        previous_end = Some(end);
        if offset >= end {
            continue;
        }
        if bytes.len() == max_bytes {
            more = true;
            break;
        }
        if offset < segment.start_offset {
            if !bytes.is_empty() {
                // Deliver the preceding contiguous range first. The next poll
                // reports the gap and discards only its unfinished line.
                more = true;
                break;
            }
            offset = segment.start_offset;
            gap = true;
        }
        segment
            .file
            .seek(SeekFrom::Start(offset - segment.start_offset))?;
        let count = (end - offset).min((max_bytes - bytes.len()) as u64);
        let before = bytes.len();
        segment.file.take(count).read_to_end(&mut bytes)?;
        let read = (bytes.len() - before) as u64;
        if read != count {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "console log segment changed during the read",
            ));
        }
        offset += read;
        more = offset < end;
    }
    Ok(SegmentDelta {
        bytes,
        offset,
        more,
        gap,
    })
}

#[cfg(test)]
#[path = "managed_segments_tests.rs"]
mod tests;
