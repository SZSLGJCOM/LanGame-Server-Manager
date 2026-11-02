//! Incremental startup output after a pre-launch baseline, with no game semantics.
use crate::runtime_log_stream::file_identity::{LogFileIdentity, log_identity};
use app_storage::managed_console_log::{ManagedLogSegment, open_log_segments};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const READ_BYTES: usize = 256 * 1024;
const LINE_BYTES: usize = 16 * 1024;
const CHECK_BYTES: u64 = 64;
const MAX_SEGMENTS: usize = 4;

#[derive(Clone)]
pub(super) struct StartupLogCursor {
    path: PathBuf,
    managed: bool,
    offset: u64,
    checkpoint: Option<Checkpoint>,
    line: LineState,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct LogProgress {
    pub lines: Vec<String>,
    /// Readiness deliberately ignores oversized records. Transcript consumers
    /// must not advance their retained boundary past such an omitted record.
    pub oversized_record: bool,
    /// A missing file, replacement, truncation or retention gap invalidated evidence.
    /// Normal managed rotation preserves continuity and does not set this flag.
    pub reset: bool,
    pub more: bool,
}

pub(super) struct NativeLogSnapshot {
    pub path: PathBuf,
    pub file: File,
    pub offset: u64,
    pub discard_fragment: bool,
}

struct Segment {
    file: File,
    start: u64,
    length: u64,
    identity: LogFileIdentity,
}

impl Segment {
    fn end(&self) -> u64 {
        // Validated by open_segments before any cursor can inspect a segment.
        self.start + self.length
    }
}

#[derive(Clone)]
struct Checkpoint {
    start: u64,
    identity: LogFileIdentity,
    offset: u64,
    observed_length: u64,
    head: Vec<u8>,
    anchor: Vec<u8>,
}

impl Checkpoint {
    fn capture(segment: &mut Segment, offset: u64) -> io::Result<Self> {
        Ok(Self {
            start: segment.start,
            identity: segment.identity.clone(),
            offset,
            observed_length: segment.length,
            head: region(&mut segment.file, 0, offset.min(CHECK_BYTES))?,
            anchor: region(
                &mut segment.file,
                offset.saturating_sub(CHECK_BYTES),
                offset.min(CHECK_BYTES),
            )?,
        })
    }

    fn changed(&self, segment: &mut Segment) -> io::Result<bool> {
        Ok(self.identity != segment.identity
            || segment.length < self.observed_length
            || region(&mut segment.file, 0, self.head.len() as u64)? != self.head
            || region(
                &mut segment.file,
                self.offset.saturating_sub(CHECK_BYTES),
                self.anchor.len() as u64,
            )? != self.anchor)
    }
}

impl StartupLogCursor {
    /// Capture before spawning. Existing bytes and a pre-launch partial record
    /// are not evidence produced by the process about to start.
    pub(super) fn capture(path: PathBuf, managed: bool) -> Result<Self, String> {
        let mut cursor = Self {
            path,
            managed,
            offset: 0,
            checkpoint: None,
            line: LineState::default(),
        };
        if let Some(mut segments) =
            open_segments(&cursor.path, managed, &|| false).map_err(|error| cursor.error(error))?
        {
            let last = segments.last().ok_or("startup log has no segments")?;
            cursor.offset = last.end();
            // Rotation can leave an empty active segment. The preceding byte
            // still determines whether the pre-launch record was complete.
            let index = segments
                .iter()
                .rposition(|segment| segment.length > 0)
                .unwrap_or(segments.len() - 1);
            let segment = &mut segments[index];
            let length = segment.length;
            let checkpoint =
                Checkpoint::capture(segment, length).map_err(|error| cursor.error(error))?;
            if let Some(&last) = checkpoint.anchor.last() {
                cursor.line.discard = !matches!(last, b'\r' | b'\n');
                cursor.line.skip_lf = last == b'\r';
            }
            cursor.checkpoint = Some(checkpoint);
        }
        Ok(cursor)
    }

    /// Reads at most 256 KiB of output. Cancellation or an I/O failure leaves
    /// the committed cursor untouched, so a caller may retry without losing lines.
    pub(super) fn poll(&mut self, cancelled: impl Fn() -> bool) -> Result<LogProgress, String> {
        self.poll_inner(&cancelled, || {})
            .map_err(|error| self.error(error))
    }

    /// Transfer only bytes already observed by this startup. The receiving
    /// health reader resumes incomplete records instead of replaying the file.
    pub(super) fn native_snapshot(&self) -> Result<NativeLogSnapshot, String> {
        if self.managed {
            return Err("Native log handoff cannot replace a managed transcript".into());
        }
        let previous = self
            .checkpoint
            .as_ref()
            .ok_or("Native log handoff requires an observed file")?;
        let mut segments = open_segments(&self.path, false, &|| false)
            .map_err(|error| self.error(error))?
            .ok_or("Native log disappeared before health handoff")?;
        if segments.len() != 1 || segments[0].start != 0 {
            return Err("Native log handoff requires one unrotated file".into());
        }
        let mut segment = segments.remove(0);
        if previous
            .changed(&mut segment)
            .map_err(|error| self.error(error))?
        {
            return Err("Native log changed before health handoff".into());
        }
        let offset = self
            .offset
            .checked_sub(self.line.pending.len() as u64)
            .ok_or("Native log handoff has an invalid complete-line boundary")?;
        Ok(NativeLogSnapshot {
            path: self.path.clone(),
            file: segment.file,
            offset,
            discard_fragment: self.line.discard,
        })
    }

    fn error(&self, error: io::Error) -> String {
        format!(
            "Failed to read startup log ({}): {error}",
            self.path.display()
        )
    }

    fn poll_inner(
        &mut self,
        cancelled: &impl Fn() -> bool,
        after_read: impl FnOnce(),
    ) -> io::Result<LogProgress> {
        check_cancelled(cancelled)?;
        let Some(mut segments) = open_segments(&self.path, self.managed, cancelled)? else {
            check_cancelled(cancelled)?;
            // Invalidate cached readiness immediately, but retain the committed
            // cursor to avoid replaying pre-launch bytes if the same file returns.
            return Ok(LogProgress {
                reset: self.checkpoint.is_some(),
                ..Default::default()
            });
        };
        let mut staged = self.clone();
        let mut progress = staged.poll_segments(&mut segments, cancelled)?;
        // Tests can replace the path here while the original handles are open.
        after_read();
        let Some(mut current) = open_segments(&self.path, self.managed, cancelled)? else {
            check_cancelled(cancelled)?;
            return Ok(LogProgress {
                reset: true,
                ..Default::default()
            });
        };
        // A successful read of an old handle must not publish readiness after
        // its path is replaced. Managed rotation retains the same segment ID.
        let stable = match &staged.checkpoint {
            Some(previous) => match current.iter_mut().find(|part| part.start == previous.start) {
                Some(segment) => !previous.changed(segment)?,
                None => current
                    .first()
                    .is_some_and(|part| staged.offset == part.start && previous.start < part.start),
            },
            None => false,
        };
        check_cancelled(cancelled)?;
        if !stable {
            return Ok(LogProgress {
                reset: true,
                ..Default::default()
            });
        }
        progress.more |= current
            .last()
            .is_some_and(|part| staged.offset < part.end());
        *self = staged;
        Ok(progress)
    }

    fn poll_segments(
        &mut self,
        segments: &mut [Segment],
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<LogProgress> {
        let first_start = segments
            .first()
            .ok_or_else(|| io::Error::other("startup log has no segments"))?
            .start;
        let end = segments
            .last()
            .ok_or_else(|| io::Error::other("startup log has no segments"))?
            .end();
        let mut reset = match &self.checkpoint {
            Some(previous) => match segments
                .iter_mut()
                .find(|part| part.start == previous.start)
            {
                Some(segment) => previous.changed(segment)?,
                // A fully consumed segment may expire exactly at the next
                // segment boundary. Its pending UTF-8/line bytes remain valid.
                None => self.offset != first_start || previous.start >= first_start,
            },
            None => true,
        } || self.offset < first_start
            || self.offset > end;
        let mut offset = if reset { first_start } else { self.offset };
        let mut line = if reset {
            LineState::after_gap(first_start != 0)
        } else {
            self.line.clone()
        };
        let mut bytes = Vec::new();
        let mut touched = None;
        for (index, segment) in segments.iter_mut().enumerate() {
            check_cancelled(cancelled)?;
            if offset >= segment.end() {
                continue;
            }
            if bytes.len() == READ_BYTES {
                break;
            }
            if offset < segment.start {
                if !bytes.is_empty() {
                    // Return the preceding contiguous range first. Otherwise
                    // reset would discard records delivered in the same poll.
                    break;
                }
                offset = segment.start;
                line = LineState::after_gap(true);
                reset = true;
            }
            segment.file.seek(SeekFrom::Start(offset - segment.start))?;
            let count = (segment.end() - offset).min((READ_BYTES - bytes.len()) as u64) as usize;
            read_bytes(&mut segment.file, count, &mut bytes, cancelled)?;
            offset += count as u64;
            touched = Some(index);
        }
        let lines = line.consume(&bytes, cancelled)?;
        let index = touched.or_else(|| {
            segments
                .iter()
                .rposition(|part| part.start <= offset && offset <= part.end())
        });
        let checkpoint = if let Some(index) = index {
            let segment = &mut segments[index];
            let local_offset = offset - segment.start;
            Some(Checkpoint::capture(segment, local_offset)?)
        } else {
            self.checkpoint.clone()
        };
        check_cancelled(cancelled)?;
        self.offset = offset;
        self.checkpoint = checkpoint;
        self.line = line;
        Ok(LogProgress {
            lines,
            oversized_record: self.line.oversized_record,
            reset,
            more: offset < end,
        })
    }
}

fn open_segments(
    path: &Path,
    managed: bool,
    cancelled: &impl Fn() -> bool,
) -> io::Result<Option<Vec<Segment>>> {
    check_cancelled(cancelled)?;
    let parts = match (|| {
        if managed && let Some(parts) = open_log_segments(path)? {
            return Ok(parts);
        }
        Ok::<_, io::Error>(vec![ManagedLogSegment {
            path: path.to_path_buf(),
            file: File::open(path)?,
            start_offset: 0,
        }])
    })() {
        Ok(parts) => parts,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if parts.is_empty() || parts.len() > MAX_SEGMENTS {
        return Err(io::Error::other(
            "startup log segment count is outside its limit",
        ));
    }
    let mut segments = Vec::with_capacity(parts.len());
    let mut previous_end = None;
    for part in parts {
        check_cancelled(cancelled)?;
        let metadata = part.file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::other("startup log is not a regular file"));
        }
        let end = part
            .start_offset
            .checked_add(metadata.len())
            .ok_or_else(|| io::Error::other("startup log offset overflow"))?;
        if previous_end.is_some_and(|previous| part.start_offset < previous) {
            return Err(io::Error::other("startup log segments overlap"));
        }
        // The logical path is stable when managed rotation renames a segment.
        // Its OS file identity still identifies the particular opened object.
        let identity = match log_identity(path, &part.file) {
            Ok(identity) => identity,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        segments.push(Segment {
            file: part.file,
            start: part.start_offset,
            length: metadata.len(),
            identity,
        });
        previous_end = Some(end);
    }
    Ok(Some(segments))
}

fn region(file: &mut File, offset: u64, length: u64) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_bytes(
    file: &mut File,
    mut remaining: usize,
    bytes: &mut Vec<u8>,
    cancelled: &impl Fn() -> bool,
) -> io::Result<()> {
    let mut buffer = [0; 8192];
    while remaining > 0 {
        check_cancelled(cancelled)?;
        let count = remaining.min(buffer.len());
        let read = file.read(&mut buffer[..count])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "startup log changed while its current range was being read",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        remaining -= read;
    }
    Ok(())
}

fn check_cancelled(cancelled: &impl Fn() -> bool) -> io::Result<()> {
    if cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "startup log read was cancelled",
        ))
    } else {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct LineState {
    pending: Vec<u8>,
    discard: bool,
    skip_lf: bool,
    oversized_record: bool,
}

impl LineState {
    fn after_gap(discard: bool) -> Self {
        Self {
            discard,
            ..Default::default()
        }
    }

    fn consume(&mut self, bytes: &[u8], cancelled: &impl Fn() -> bool) -> io::Result<Vec<String>> {
        self.oversized_record = false;
        let mut lines = Vec::new();
        for chunk in bytes.chunks(8192) {
            check_cancelled(cancelled)?;
            for &byte in chunk {
                if self.skip_lf {
                    self.skip_lf = false;
                    if byte == b'\n' {
                        continue;
                    }
                }
                if matches!(byte, b'\r' | b'\n') {
                    if !self.discard {
                        let text = String::from_utf8_lossy(&self.pending);
                        // Oversized records are discarded as whole records,
                        // never truncated into an apparent readiness message.
                        if text.len() <= LINE_BYTES {
                            lines.push(text.into_owned());
                        }
                    }
                    self.pending.clear();
                    self.discard = false;
                    self.skip_lf = byte == b'\r';
                } else if !self.discard {
                    if self.pending.len() == LINE_BYTES {
                        self.oversized_record = true;
                        self.pending.clear();
                        self.discard = true;
                    } else {
                        self.pending.push(byte);
                    }
                }
            }
        }
        Ok(lines)
    }
}

#[cfg(test)]
#[path = "commands_startup_log_cursor_tests.rs"]
mod tests;
