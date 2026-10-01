use crate::runtime_log_stream::file_identity::{LogFileIdentity, log_identity};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CHECKPOINT_BYTES: u64 = 64;
const READ_LIMIT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Default)]
pub(super) struct LogBaseline {
    identity: Option<LogFileIdentity>,
    start: u64,
    observed_length: u64,
    head: Vec<u8>,
    tail: Vec<u8>,
}

impl LogBaseline {
    pub(super) fn capture(path: &Path) -> Result<Self, String> {
        let Some(mut file) = open(path)? else {
            return Ok(Self::default());
        };
        let length = length(&file)?;
        let mut baseline = Self {
            start: length,
            ..Default::default()
        };
        let identity = identity(path, &file)?;
        baseline.observe(&mut file, length, identity)?;
        Ok(baseline)
    }

    fn observe(
        &mut self,
        file: &mut File,
        length: u64,
        identity: LogFileIdentity,
    ) -> Result<(), String> {
        self.identity = Some(identity);
        self.observed_length = length;
        self.head = region(file, 0, length.min(CHECKPOINT_BYTES))?;
        self.tail = region(
            file,
            length.saturating_sub(CHECKPOINT_BYTES),
            length.min(CHECKPOINT_BYTES),
        )?;
        Ok(())
    }
}

pub(super) fn new_log_contains(
    path: &Path,
    baseline: &mut LogBaseline,
    markers: &[String],
) -> Result<bool, String> {
    new_log_matches(path, baseline, |text| {
        markers.iter().any(|marker| text.contains(marker))
    })
}

pub(super) fn new_log_line_matches(
    path: &Path,
    baseline: &mut LogBaseline,
    matches: impl FnMut(&str) -> bool,
) -> Result<bool, String> {
    new_log_matches(path, baseline, |text| text.lines().any(matches))
}

fn new_log_matches(
    path: &Path,
    baseline: &mut LogBaseline,
    matches: impl FnOnce(&str) -> bool,
) -> Result<bool, String> {
    let Some(mut file) = open(path)? else {
        *baseline = LogBaseline::default();
        return Ok(false);
    };
    let length = length(&file)?;
    let identity = identity(path, &file)?;
    let changed_identity = baseline
        .identity
        .as_ref()
        .is_some_and(|previous| previous != &identity);
    let rewritten = changed_identity
        || length < baseline.observed_length
        || region(&mut file, 0, baseline.head.len() as u64)? != baseline.head
        || region(
            &mut file,
            baseline.observed_length.saturating_sub(CHECKPOINT_BYTES),
            baseline.tail.len() as u64,
        )? != baseline.tail;
    if rewritten {
        // Keep the new generation's start after it regrows past the old size.
        // Checkpoints catch same-file truncate/regrow between polls; the file
        // identity also catches replacement with an identical head and tail.
        baseline.start = 0;
    }
    baseline.observe(&mut file, length, identity)?;
    let start = baseline.start.max(length.saturating_sub(READ_LIMIT_BYTES));
    let bytes = region(&mut file, start, READ_LIMIT_BYTES)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(matches(&text))
}

fn open(path: &Path) -> Result<Option<File>, String> {
    match File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("native log cannot be read".into()),
    }
}

fn length(file: &File) -> Result<u64, String> {
    file.metadata()
        .map(|metadata| metadata.len())
        .map_err(|_| "native log metadata cannot be read".into())
}

fn identity(path: &Path, file: &File) -> Result<LogFileIdentity, String> {
    log_identity(path, file).map_err(|_| "native log file identity cannot be read".into())
}

fn region(file: &mut File, start: u64, length: u64) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(start))
        .map_err(|_| "native log seek failed")?;
    let mut bytes = Vec::new();
    file.take(length)
        .read_to_end(&mut bytes)
        .map_err(|_| "native log read failed")?;
    Ok(bytes)
}

#[path = "commands_native_log_readiness_tests.rs"]
mod tests;
