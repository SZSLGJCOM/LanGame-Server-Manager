use super::native_line_is_current;
use crate::atomic_file::compare_and_swap_optional_file_atomically;
use crate::managed_console_log::owned_fs::{FileIdentity, identity, reject_links};
use app_core::InstanceProcessState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const SCAN_BYTES: u64 = 1024 * 1024;
const STATE_BYTES: u64 = 64 * 1024;
const MAX_LINE_BYTES: u64 = 16 * 1024;
const CHECK_BYTES: u64 = 64;
const MAX_MAPS: usize = 16;
const CAS_ATTEMPTS: usize = 3;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    group_run_id: i64,
    entries: BTreeMap<String, Entry>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    run_id: i64,
    pid: u32,
    creation_time: u64,
    file_identity: FileIdentity,
    offset: u64,
    observed_length: u64,
    head: Vec<u8>,
    anchor: Vec<u8>,
    partial: PartialLine,
    ready: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialLine {
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    length: u64,
    marker: bool,
    skip: bool,
}

impl Entry {
    fn owns(&self, process: &InstanceProcessState) -> bool {
        self.run_id == process.run_id
            && Some(self.pid) == process.pid
            && process
                .process_identity
                .as_ref()
                .is_some_and(|identity| identity.creation_time == self.creation_time)
    }

    fn valid(&self) -> bool {
        self.head.len() <= CHECK_BYTES as usize
            && self.anchor.len() <= CHECK_BYTES as usize
            && self.partial.prefix.len() <= 25
            && self.partial.suffix.len() <= 32
            && self.partial.length <= MAX_LINE_BYTES + 1
            && self.offset <= self.observed_length
    }
}

/// One bounded checkpoint belongs to the instance's current process group. Each
/// entry also belongs to an OS process, so restart and PID reuse cannot inherit
/// readiness, and an older group snapshot cannot prune a newer map's evidence.
pub(super) fn observe(
    native_path: &Path,
    process: &InstanceProcessState,
    group_run_id: i64,
    enabled_keys: &[String],
    recent_ready: bool,
) -> io::Result<bool> {
    observe_with(
        native_path,
        process,
        group_run_id,
        enabled_keys,
        recent_ready,
        |_| {},
    )
}

fn observe_with(
    native_path: &Path,
    process: &InstanceProcessState,
    group_run_id: i64,
    enabled_keys: &[String],
    recent_ready: bool,
    mut before_cas: impl FnMut(usize),
) -> io::Result<bool> {
    if group_run_id <= 0
        || enabled_keys.len() > MAX_MAPS
        || !enabled_keys.contains(&process.process_key)
    {
        return Err(io::Error::other("invalid ARK evidence map ownership"));
    }
    let token = process
        .process_identity
        .as_ref()
        .ok_or_else(|| io::Error::other("ARK process creation identity is missing"))?;
    let pid = process
        .pid
        .ok_or_else(|| io::Error::other("ARK process PID is missing"))?;
    reject_links(native_path)?;
    let mut file = File::open(native_path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("ARK native log is not a file"));
    }
    let file_identity = identity(&file)?;
    let state_path = native_path
        .parent()
        .ok_or_else(|| io::Error::other("ARK native log has no owned directory"))?
        .join(".ark-readiness.json");
    let (mut checkpoint, mut expected) = load(&state_path)?;
    if !claim_group(&mut checkpoint, group_run_id) {
        return Ok(false);
    }
    let mut entry = checkpoint
        .entries
        .get(&process.process_key)
        .filter(|entry| entry.owns(process))
        .cloned()
        .unwrap_or_else(|| Entry {
            run_id: process.run_id,
            pid,
            creation_time: token.creation_time,
            file_identity: file_identity.clone(),
            offset: 0,
            observed_length: 0,
            head: Vec::new(),
            anchor: Vec::new(),
            partial: PartialLine::default(),
            ready: false,
        });
    entry.ready |= recent_ready;
    advance(&mut file, file_identity, &mut entry, process)?;
    for attempt in 0..CAS_ATTEMPTS {
        if attempt > 0 {
            (checkpoint, expected) = load(&state_path)?;
            if !claim_group(&mut checkpoint, group_run_id) {
                return Ok(false);
            }
        }
        if let Some(current) = checkpoint.entries.get(&process.process_key) {
            // A request captured before restart must not overwrite the newer run.
            if current.run_id > process.run_id {
                return Ok(false);
            }
            if current.owns(process) {
                entry.ready |= current.ready;
                if current.file_identity == entry.file_identity
                    && current.head == entry.head
                    && current.offset > entry.offset
                    && current.offset <= file.metadata()?.len()
                    && region(
                        &mut file,
                        current.offset.saturating_sub(CHECK_BYTES),
                        current.anchor.len() as u64,
                    )? == current.anchor
                {
                    let ready = entry.ready;
                    entry = current.clone();
                    entry.ready |= ready;
                }
            }
        }
        // A replacement while this handle was scanned belongs to another log
        // generation. Leave its cursor to the next request rather than regress it.
        reject_links(native_path)?;
        if identity(&File::open(native_path)?)? != entry.file_identity {
            return Ok(entry.ready);
        }
        checkpoint
            .entries
            .retain(|key, _| enabled_keys.contains(key));
        checkpoint
            .entries
            .insert(process.process_key.clone(), entry.clone());
        let replacement = serde_json::to_vec(&checkpoint).map_err(io::Error::other)?;
        if replacement.len() as u64 > STATE_BYTES {
            return Err(io::Error::other(
                "ARK readiness evidence exceeds its size limit",
            ));
        }
        reject_links(&state_path)?;
        before_cas(attempt);
        if expected.as_deref() == Some(replacement.as_slice())
            || compare_and_swap_optional_file_atomically(
                &state_path,
                expected.as_deref(),
                Some(&replacement),
            )?
        {
            return Ok(entry.ready);
        }
    }
    Err(io::Error::other(
        "ARK readiness evidence changed during concurrent inspection",
    ))
}

fn claim_group(checkpoint: &mut Checkpoint, group_run_id: i64) -> bool {
    if checkpoint.group_run_id > group_run_id {
        return false;
    }
    if checkpoint.group_run_id < group_run_id {
        checkpoint.entries.clear();
        checkpoint.group_run_id = group_run_id;
    }
    true
}

fn load(path: &Path) -> io::Result<(Checkpoint, Option<Vec<u8>>)> {
    reject_links(path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Checkpoint::default(), None));
        }
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > STATE_BYTES {
        return Err(io::Error::other(
            "ARK readiness evidence is not a bounded file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(STATE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > STATE_BYTES {
        return Err(io::Error::other(
            "ARK readiness evidence grew beyond its limit",
        ));
    }
    let checkpoint: Checkpoint = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if checkpoint.group_run_id < 0
        || (checkpoint.group_run_id == 0 && !checkpoint.entries.is_empty())
        || checkpoint.entries.len() > MAX_MAPS
        || checkpoint.entries.values().any(|entry| !entry.valid())
    {
        return Err(io::Error::other(
            "ARK readiness evidence has invalid bounded state",
        ));
    }
    Ok((checkpoint, Some(bytes)))
}

fn advance(
    file: &mut File,
    file_identity: FileIdentity,
    entry: &mut Entry,
    process: &InstanceProcessState,
) -> io::Result<()> {
    let length = file.metadata()?.len();
    let rewritten = file_identity != entry.file_identity
        || length < entry.observed_length
        || region(file, 0, entry.head.len() as u64)? != entry.head
        || region(
            file,
            entry.offset.saturating_sub(CHECK_BYTES),
            entry.anchor.len() as u64,
        )? != entry.anchor;
    if rewritten {
        entry.offset = 0;
        entry.partial = PartialLine::default();
    }
    entry.file_identity = file_identity;
    if entry.ready {
        if rewritten {
            entry.observed_length = length;
            entry.head = region(file, 0, length.min(CHECK_BYTES))?;
            entry.anchor.clear();
        }
        return Ok(());
    }
    let bytes = region(file, entry.offset, SCAN_BYTES)?;
    for fragment in bytes.split_inclusive(|byte| *byte == b'\n') {
        scan_fragment(&mut entry.partial, fragment, process, &mut entry.ready);
    }
    entry.offset += bytes.len() as u64;
    entry.observed_length = file.metadata()?.len().max(entry.offset);
    entry.head = region(file, 0, entry.observed_length.min(CHECK_BYTES))?;
    entry.anchor = region(
        file,
        entry.offset.saturating_sub(CHECK_BYTES),
        entry.offset.min(CHECK_BYTES),
    )?;
    Ok(())
}

fn scan_fragment(
    partial: &mut PartialLine,
    fragment: &[u8],
    process: &InstanceProcessState,
    ready: &mut bool,
) {
    let complete = fragment.last() == Some(&b'\n');
    partial.length = partial
        .length
        .saturating_add(fragment.len() as u64)
        .min(MAX_LINE_BYTES + 1);
    partial.skip |= partial.length > MAX_LINE_BYTES;
    if !partial.skip {
        let needed = 25 - partial.prefix.len();
        partial
            .prefix
            .extend_from_slice(&fragment[..needed.min(fragment.len())]);
        let mut combined = std::mem::take(&mut partial.suffix);
        combined.extend_from_slice(fragment);
        let text = String::from_utf8_lossy(&combined).to_ascii_lowercase();
        partial.marker |=
            text.contains("has successfully started") || text.contains("advertising for join");
        partial.suffix = combined[combined.len().saturating_sub(32)..].to_vec();
    }
    if complete {
        if !partial.skip
            && partial.marker
            && native_line_is_current(process, &String::from_utf8_lossy(&partial.prefix))
        {
            *ready = true;
        }
        *partial = PartialLine::default();
    }
}

fn region(file: &mut File, offset: u64, limit: u64) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "runtime_ark_log_evidence_tests.rs"]
mod tests;
