//! Current-launch ASA Mod evidence; downloaded caches never establish freshness.
//!
//! The public CFCore 1.47.0 release logs `Mod valid` only after final
//! installed-set validation and matching the required file revision:
//! https://github.com/curseforge-sdks/cfcore-sdk-ue/releases/tag/1.47.0
//! `client_server_library_shared_impl.cpp` in that release has SHA-256
//! 7e2f6d53d3f83c2927acd09e47aa818f10060aed667682322d54e329782fe934.
//! Native ASA runs also report CFCore 1.47.0. Unit fixtures exercise the
//! documented boundary; the opt-in native acceptance additionally checks each
//! run's records, installed file revisions, game readiness and process cleanup.
//! The release's logger forwards verbosity `Log` to Unreal's `GWarn->Log`, whose
//! normal category output omits a `Log:` tag. Accept that native default as well
//! as explicit `Log:`/`Display:`; other verbosity levels cannot prove success.
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::curseforge_failure::{Failure, named_item_id};
use crate::runtime_log_stream::file_identity::{LogFileIdentity, log_identity};

const MAX_POLL_BYTES: usize = 512 * 1024;
const MAX_PENDING_BYTES: usize = 16 * 1024;
const READ_CHUNK_BYTES: usize = 8 * 1024;
const ANCHOR_BYTES: usize = 256;
// Reserve bounded prefix/tail checks before and after reading the next records.
const MAX_POLL_DATA_BYTES: usize = MAX_POLL_BYTES - 4 * ANCHOR_BYTES;

#[derive(Clone)]
pub(super) struct Evidence {
    expected: BTreeSet<String>,
    created_ms: u64,
    offset: u64,
    identity: Option<LogFileIdentity>,
    prefix: Vec<u8>,
    tail: Vec<u8>,
    pending: Vec<u8>,
    discarding_line: bool,
    caught_up: bool,
    valid: BTreeSet<String>,
    failed: Option<Failure>,
}

impl Evidence {
    pub(super) fn new(expected_ids: Vec<String>, created_ms: u64) -> Self {
        Self {
            expected: expected_ids.into_iter().collect(),
            created_ms,
            offset: 0,
            identity: None,
            prefix: Vec::new(),
            tail: Vec::new(),
            pending: Vec::new(),
            discarding_line: false,
            caught_up: false,
            valid: BTreeSet::new(),
            failed: None,
        }
    }

    /// The owner calls this bounded synchronous reader from its blocking worker.
    pub(super) fn read(&mut self, path: &Path) -> Result<(), String> {
        self.caught_up = false;
        self.check_failed()?;
        let Some(mut file) = open_regular(path)? else {
            if self.identity.take().is_some() {
                self.reset_cursor();
            }
            return Ok(());
        };
        let identity = log_identity(path, &file).map_err(|error| log_error("identify", error))?;
        let length = file
            .metadata()
            .map_err(|error| log_error("inspect", error))?
            .len();
        if self
            .identity
            .as_ref()
            .is_none_or(|known| known != &identity)
            || length < self.offset
            || !self.anchors_match(&mut file)?
        {
            self.reset_cursor();
        }
        self.identity = Some(identity);
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|error| log_error("seek", error))?;
        let mut remaining = MAX_POLL_DATA_BYTES;
        let mut buffer = [0_u8; READ_CHUNK_BYTES];
        while remaining > 0 {
            let count = file
                .read(&mut buffer[..remaining.min(READ_CHUNK_BYTES)])
                .map_err(|error| log_error("read", error))?;
            if count == 0 {
                break;
            }
            self.retain_anchors(&buffer[..count]);
            self.offset = self
                .offset
                .checked_add(count as u64)
                .ok_or("ASA CurseForge startup log cursor overflow")?;
            remaining -= count;
            self.consume(&buffer[..count]);
            self.check_failed()?;
        }
        let current_length = file
            .metadata()
            .map_err(|error| log_error("inspect", error))?
            .len();
        if current_length < self.offset || !self.anchors_match(&mut file)? {
            self.reset_cursor();
            return Ok(());
        }
        self.caught_up =
            self.offset >= current_length && self.pending.is_empty() && !self.discarding_line;
        Ok(())
    }

    pub(super) fn is_complete(&self) -> bool {
        self.failed.is_none()
            && self.caught_up
            && !self.expected.is_empty()
            && self.expected.iter().all(|id| self.valid.contains(id))
    }

    pub(super) fn valid_count(&self) -> usize {
        self.valid.len()
    }

    fn reset_cursor(&mut self) {
        self.offset = 0;
        self.pending.clear();
        self.prefix.clear();
        self.tail.clear();
        self.discarding_line = false;
        self.valid.clear();
        self.caught_up = false;
    }

    fn retain_anchors(&mut self, bytes: &[u8]) {
        let prefix_count = (ANCHOR_BYTES - self.prefix.len()).min(bytes.len());
        self.prefix.extend_from_slice(&bytes[..prefix_count]);
        if bytes.len() >= ANCHOR_BYTES {
            self.tail.clear();
            self.tail
                .extend_from_slice(&bytes[bytes.len() - ANCHOR_BYTES..]);
        } else {
            let excess = (self.tail.len() + bytes.len()).saturating_sub(ANCHOR_BYTES);
            self.tail.drain(..excess);
            self.tail.extend_from_slice(bytes);
        }
    }

    fn anchors_match(&self, file: &mut File) -> Result<bool, String> {
        Ok(matches_anchor(file, 0, &self.prefix)?
            && matches_anchor(file, self.offset - self.tail.len() as u64, &self.tail)?)
    }

    fn check_failed(&self) -> Result<(), String> {
        self.failed.map_or(Ok(()), |failure| Err(failure.error()))
    }

    fn consume(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if byte == b'\n' {
                if !self.discarding_line {
                    let line = std::mem::take(&mut self.pending);
                    if let Ok(line) = std::str::from_utf8(&line) {
                        self.observe_line(line.trim_end_matches('\r'));
                    }
                }
                self.pending.clear();
                self.discarding_line = false;
            } else if !self.discarding_line {
                if self.pending.len() == MAX_PENDING_BYTES {
                    self.pending.clear();
                    self.discarding_line = true;
                } else {
                    self.pending.push(byte);
                }
            }
        }
    }

    fn observe_line(&mut self, line: &str) {
        let Some((timestamp, message)) = app_storage::ark_native_log_message(line) else {
            return;
        };
        if timestamp < self.created_ms {
            return;
        }
        // ASA 94.29 reports missing requested IDs after CFCore validates only
        // the returned projects. The game error must still reject this run.
        if message
            == "ASAMods: Error: Not all mods were installed. Check the log for CFCore errors."
        {
            self.failed = self.failed.or(Some(Failure::Install));
            return;
        }
        let Some(record) = message.strip_prefix("LogCFCore: ") else {
            return;
        };
        let (level, detail) = [
            "Display",
            "Log",
            "Warning",
            "Error",
            "Verbose",
            "VeryVerbose",
            "Fatal",
        ]
        .into_iter()
        .find_map(|level| {
            record
                .strip_prefix(level)
                .and_then(|rest| rest.strip_prefix(": "))
                .map(|detail| (level, detail))
        })
        .unwrap_or(("Log", record));
        // A level alone is not terminal: the SDK also emits ordinary warnings.
        if matches!(level, "Display" | "Log" | "Warning" | "Error") {
            self.failed = self.failed.or_else(|| Failure::from_native(detail));
        }
        if matches!(level, "Log" | "Display")
            && let Some(id) = final_valid_item(detail)
            && self.expected.contains(id)
        {
            self.valid.insert(id.to_owned());
        }
    }
}

fn matches_anchor(file: &mut File, offset: u64, expected: &[u8]) -> Result<bool, String> {
    if expected.is_empty() {
        return Ok(true);
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| log_error("seek", error))?;
    let mut buffer = [0_u8; ANCHOR_BYTES];
    match file.read_exact(&mut buffer[..expected.len()]) {
        Ok(()) => Ok(&buffer[..expected.len()] == expected),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(log_error("read", error)),
    }
}

fn final_valid_item(detail: &str) -> Option<&str> {
    named_item_id(detail.strip_prefix("Mod valid: ")?)
}

fn open_regular(path: &Path) -> Result<Option<File>, String> {
    // Keep reparse points out of the entire reader path, including its parents.
    for ancestor in path
        .ancestors()
        .filter(|entry| !entry.as_os_str().is_empty())
    {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(log_error("inspect", error)),
        };
        if reparse(&metadata) || (ancestor != path && !metadata.is_dir()) {
            return Err("ASA CurseForge startup log must use an ordinary file path".into());
        }
        if ancestor == path && !metadata.is_file() {
            return Err("ASA CurseForge startup log must be a regular file".into());
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .share_mode(0x1 | 0x2 | 0x4)
            .custom_flags(0x0020_0000);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(log_error("open", error)),
    };
    let metadata = file
        .metadata()
        .map_err(|error| log_error("inspect", error))?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err("ASA CurseForge startup log must be a regular file".into());
    }
    Ok(Some(file))
}

fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn log_error(action: &str, error: std::io::Error) -> String {
    format!("Cannot {action} ASA CurseForge startup log: {error}")
}

#[cfg(test)]
#[path = "commands_asa_curseforge_evidence_tests.rs"]
mod tests;
