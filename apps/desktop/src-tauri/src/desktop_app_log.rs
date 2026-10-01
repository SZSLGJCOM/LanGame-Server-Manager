use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const POLICY: LogPolicy = LogPolicy {
    segment_bytes: 8 * 1024 * 1024,
    total_bytes: 32 * 1024 * 1024,
    record_bytes: 64 * 1024,
};
const OWNER_FILE: &str = "owner";
const SEGMENT_FILE: &str = "entries.jsonl";
const MAX_DIRECTORY_ENTRIES: usize = 4096;
static LOG: OnceLock<AppLog> = OnceLock::new();

#[derive(Clone, Copy)]
struct LogPolicy {
    segment_bytes: u64,
    total_bytes: u64,
    record_bytes: usize,
}

struct AppLog {
    policy: LogPolicy,
    lock: Mutex<LogState>,
}

#[derive(Default)]
struct LogState {
    failure: Option<(PathBuf, String)>,
    dropped_records: u64,
}

struct Segment {
    sequence: u64,
    path: PathBuf,
    bytes: u64,
}

fn logger() -> &'static AppLog {
    LOG.get_or_init(|| AppLog {
        policy: POLICY,
        lock: Mutex::new(LogState::default()),
    })
}

pub(crate) fn append(path: &Path, entry: &Value) -> io::Result<()> {
    logger().append(path, entry)
}

pub(crate) fn failure_summary(path: &Path) -> Option<String> {
    logger().failure_summary(path)
}

pub(crate) fn recent_lines(
    path: &Path,
    previous_log: Option<&Path>,
    max_lines: usize,
    max_bytes: u64,
) -> io::Result<Vec<String>> {
    // Keep generations stable while the bounded query opens and reads them.
    let _guard = logger()
        .lock
        .lock()
        .map_err(|_| io::Error::other("log lock poisoned"))?;
    read_recent_lines(path, previous_log, max_lines, max_bytes)
}

impl AppLog {
    fn failure_summary(&self, path: &Path) -> Option<String> {
        match self.lock.lock() {
            Ok(state) => state.failure.as_ref().filter(|(failed_path, _)| failed_path == path)
                .map(|(_, error)| format!("Application diagnostic log unavailable: {error} ({} record(s) not persisted).", state.dropped_records)),
            Err(_) => Some(String::from("Application diagnostic log unavailable: write lock poisoned.")),
        }
    }

    fn append(&self, path: &Path, entry: &Value) -> io::Result<()> {
        let mut state = self
            .lock
            .lock()
            .map_err(|_| io::Error::other("log lock poisoned"))?;
        let result = self.write_entry(path, entry);
        match &result {
            Ok(()) => state.failure = None,
            Err(error) => {
                state.dropped_records = state.dropped_records.saturating_add(1);
                state.failure = Some((path.to_path_buf(), error.to_string()));
            }
        }
        result
    }

    fn write_entry(&self, path: &Path, entry: &Value) -> io::Result<()> {
        let record = encode_record(entry, self.policy.record_bytes)?;
        let directory = path
            .parent()
            .ok_or_else(|| io::Error::other("log directory missing"))?;
        let owner = directory_owner(directory, true)?;
        let header = segment_header(&owner);
        if header.len() as u64 + record.len() as u64 > self.policy.segment_bytes {
            return Err(io::Error::other(
                "diagnostic record exceeds the segment budget",
            ));
        }
        let (mut archives, next_sequence) = segments(directory, &header)?;
        let active_bytes = match owned_file(path, &header)? {
            Some(bytes) => bytes,
            None if path.try_exists()? => {
                return Err(io::Error::other(
                    "active diagnostic log is not owned by LanGame",
                ));
            }
            None => 0,
        };
        let rotate = active_bytes > 0
            && (active_bytes.saturating_add(record.len() as u64) > self.policy.segment_bytes
                || !ends_with_newline(path)?);
        let mut total = archives.iter().try_fold(active_bytes, |total, segment| {
            total
                .checked_add(segment.bytes)
                .ok_or_else(|| io::Error::other("log size overflow"))
        })?;
        let added = record.len() as u64
            + if rotate || active_bytes == 0 {
                header.len() as u64
            } else {
                0
            };
        while total.saturating_add(added) > self.policy.total_bytes {
            let Some(oldest) = archives.first() else {
                return Err(io::Error::other(
                    "diagnostic log budget exhausted; no expired owned segment can be removed",
                ));
            };
            // Recheck ownership immediately before deletion; never clean the directory recursively.
            if owned_file(&oldest.path, &header)? != Some(oldest.bytes) {
                return Err(io::Error::other(
                    "diagnostic archive changed before retention cleanup",
                ));
            }
            fs::remove_file(&oldest.path)?;
            if let Some(parent) = oldest.path.parent() {
                match fs::remove_dir(parent) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
                    Err(error) => return Err(error),
                }
            }
            total = total.saturating_sub(oldest.bytes);
            archives.remove(0);
        }
        if rotate {
            let archive = directory.join(format!("segment-{next_sequence:020}"));
            // A newly created directory reserves the destination without overwriting any file.
            fs::create_dir(&archive)?;
            if let Err(error) = fs::rename(path, archive.join(SEGMENT_FILE)) {
                let _ = fs::remove_dir(&archive);
                return Err(error);
            }
        }
        let mut file = if active_bytes == 0 || rotate {
            publish_new_file(path, &header)?;
            OpenOptions::new().append(true).open(path)?
        } else {
            OpenOptions::new().append(true).open(path)?
        };
        file.write_all(&record)
    }
}

fn segment_header(owner: &str) -> Vec<u8> {
    format!("{{\"action\":\"desktop.log.segment\",\"context\":{{\"owner\":\"{owner}\"}}}}\n")
        .into_bytes()
}

fn publish_new_file(path: &Path, content: &[u8]) -> io::Result<()> {
    publish_new_file_using(path, |file| {
        file.write_all(content).and_then(|()| file.sync_all())
    })
}

fn publish_new_file_using(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("diagnostic file name missing"))?;
    let mut pending_name = name.to_os_string();
    pending_name.push(".pending");
    let temporary = path.with_file_name(pending_name);
    let mut file = File::create_new(&temporary)?;
    let result = write(&mut file);
    drop(file);
    // Only this invocation owns the unpublished temporary file. A complete header
    // is published without replacing any existing name, so interrupted writes
    // cannot leave a permanently invalid owner or active segment.
    let result = result.and_then(|()| fs::hard_link(&temporary, path));
    let cleanup = fs::remove_file(&temporary);
    result.and(cleanup)
}

fn ends_with_newline(path: &Path) -> io::Result<bool> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

fn directory_owner(directory: &Path, create: bool) -> io::Result<String> {
    reject_reparse_ancestors(directory)?;
    if create {
        fs::create_dir_all(directory)?;
    }
    let marker = directory.join(OWNER_FILE);
    if create && !marker.try_exists()? {
        publish_new_file(&marker, uuid::Uuid::new_v4().to_string().as_bytes())?;
    }
    reject_reparse_ancestors(&marker)?;
    if !fs::symlink_metadata(&marker)?.is_file() {
        return Err(io::Error::other(
            "diagnostic directory ownership marker is not a file",
        ));
    }
    let mut owner = String::new();
    File::open(marker)?.take(128).read_to_string(&mut owner)?;
    if owner.len() != 36 || uuid::Uuid::parse_str(&owner).is_err() {
        return Err(io::Error::other(
            "invalid diagnostic directory ownership marker",
        ));
    }
    Ok(owner)
}

fn owned_file(path: &Path, header: &[u8]) -> io::Result<Option<u64>> {
    reject_reparse_ancestors(path)?;
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(None);
    }
    let mut prefix = vec![0; header.len().min(metadata.len() as usize)];
    file.read_exact(&mut prefix)?;
    Ok((prefix == header).then_some(metadata.len()))
}

fn segments(directory: &Path, header: &[u8]) -> io::Result<(Vec<Segment>, u64)> {
    let mut result = Vec::new();
    let mut last_sequence = 0;
    for (index, entry) in fs::read_dir(directory)?.enumerate() {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err(io::Error::other(
                "diagnostic directory entry limit exceeded",
            ));
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(digits) = name.to_str().and_then(|name| name.strip_prefix("segment-")) else {
            continue;
        };
        if digits.len() != 20 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(sequence) = digits.parse::<u64>() else {
            continue;
        };
        last_sequence = last_sequence.max(sequence);
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_dir() || is_reparse(&metadata) {
            continue;
        }
        let path = entry.path().join(SEGMENT_FILE);
        if let Some(bytes) = owned_file(&path, header)? {
            result.push(Segment {
                sequence,
                path,
                bytes,
            });
        }
    }
    result.sort_unstable_by_key(|segment| segment.sequence);
    let next = last_sequence
        .checked_add(1)
        .ok_or_else(|| io::Error::other("diagnostic generation overflow"))?;
    Ok((result, next))
}

fn reject_reparse_ancestors(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_reparse(&metadata) => {
                return Err(io::Error::other(
                    "diagnostic path contains a link or reparse point",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn encode_record(entry: &Value, limit: usize) -> io::Result<Vec<u8>> {
    struct BoundedRecord {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for BoundedRecord {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(io::Error::other("record limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut record = BoundedRecord {
        bytes: Vec::new(),
        limit: limit.saturating_sub(1),
    };
    if serde_json::to_writer(&mut record, entry).is_err() {
        let mut summary = json!({
            "level": short_text(&entry["level"], 32),
            "action": short_text(&entry["action"], 256),
            "message": short_text(&entry["message"], 4096),
            "context": { "omitted": true },
            "log_record_truncated": true,
        });
        if let Some(timestamp) = entry["ts_unix_ms"].as_u64() {
            summary["ts_unix_ms"] = json!(timestamp);
        }
        if entry["context"]["instance_id"].is_string() {
            summary["context"]["instance_id"] =
                json!(short_text(&entry["context"]["instance_id"], 1024));
        }
        record.bytes.clear();
        serde_json::to_writer(&mut record, &summary).map_err(io::Error::other)?;
    }
    record.bytes.push(b'\n');
    Ok(record.bytes)
}

fn short_text(value: &Value, bytes: usize) -> &str {
    let text = value.as_str().unwrap_or_default();
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn read_recent_lines(
    path: &Path,
    previous_log: Option<&Path>,
    max_lines: usize,
    max_bytes: u64,
) -> io::Result<Vec<String>> {
    if max_lines == 0 || max_bytes == 0 {
        return Ok(Vec::new());
    }
    let mut paths = vec![path.to_path_buf()];
    if let Some(directory) = path.parent() {
        match directory_owner(directory, false) {
            Ok(owner) => {
                let (archives, _) = segments(directory, &segment_header(&owner))?;
                paths.extend(archives.into_iter().rev().map(|segment| segment.path));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if let Some(previous_log) = previous_log {
        paths.push(previous_log.to_path_buf());
    }
    let mut remaining = max_bytes;
    let mut lines = Vec::new();
    for path in paths {
        if remaining == 0 || lines.len() == max_lines {
            break;
        }
        reject_reparse_ancestors(&path)?;
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let size = file.metadata()?.len();
        let count = size.min(remaining);
        file.seek(SeekFrom::Start(size - count))?;
        let mut bytes = Vec::new();
        file.take(count).read_to_end(&mut bytes)?;
        remaining = remaining.saturating_sub(bytes.len() as u64);
        let complete = if size > count {
            bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|index| &bytes[index + 1..])
                .unwrap_or_default()
        } else {
            &bytes
        };
        let content = String::from_utf8_lossy(complete);
        lines.extend(
            content
                .lines()
                .rev()
                .take(max_lines - lines.len())
                .map(String::from),
        );
    }
    lines.reverse();
    Ok(lines)
}

#[cfg(test)]
#[path = "desktop_app_log_tests.rs"]
mod tests;
