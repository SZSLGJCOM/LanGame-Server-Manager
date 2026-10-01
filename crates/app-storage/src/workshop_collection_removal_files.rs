use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{failure, valid_id};
use crate::StorageError;
use crate::atomic_file::{compare_and_swap_optional_file_atomically, write_file_atomically};

const JOURNAL: &str = ".lgsm-workshop-removal.json";
const RETAINED: &str = ".lgsm-workshop-retained";
const COMMITTED: &str = "committed.json";
const CONFIG_LIMIT: u64 = 16 * 1024 * 1024;
const JOURNAL_LIMIT: u64 = 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u8,
    instance_id: String,
    operation_id: String,
    original_sha256: String,
    replacement_sha256: String,
    members: Vec<String>,
}

pub(super) fn remove(
    root: &Path,
    runtime: &Path,
    instance_id: &str,
    original: &Value,
    replacement: &Value,
    members: Vec<String>,
) -> Result<(), StorageError> {
    ensure_no_pending(root)?;
    let config = root.join("config/instance.json");
    let before = read_plain(&config, CONFIG_LIMIT)?
        .ok_or_else(|| failure(&config, "instance configuration is missing"))?;
    let mut document: Value = serde_json::from_slice(&before)?;
    if document.get("instance_id").and_then(Value::as_str) != Some(instance_id)
        || document.get("module_id").and_then(Value::as_str) != Some("squad")
        || document.get("settings") != Some(original)
    {
        return Err(failure(
            &config,
            "instance configuration changed before collection removal",
        ));
    }
    if crate::instance_program_mode(root)? != crate::InstanceProgramMode::Independent
        || crate::resolve_instance_runtime_root(root)? != runtime
    {
        return Err(failure(
            runtime,
            "collection removal requires the instance private runtime",
        ));
    }
    let after = if original == replacement {
        before.clone()
    } else {
        document["settings"] = replacement.clone();
        serde_json::to_vec_pretty(&document)?
    };
    let mods = runtime.join("SquadGame/Plugins/Mods");
    safe_ancestors(&mods)?;
    let mut present = Vec::new();
    let mut budget = ScanBudget::new();
    for id in members {
        let source = mods.join(&id);
        if plain_directory(&source)? {
            budget.inspect(&source)?;
            present.push(id);
        }
    }
    let retained = root.join(RETAINED);
    safe_ancestors(&retained)?;
    if !plain_directory(&retained)? {
        fs::create_dir(&retained).map_err(|error| failure(&retained, error))?;
    }
    let journal = Journal {
        version: 1,
        instance_id: instance_id.into(),
        operation_id: uuid::Uuid::new_v4().to_string(),
        original_sha256: digest(&before),
        replacement_sha256: digest(&after),
        members: present,
    };
    let operation = retained.join(&journal.operation_id);
    // A new directory is the ownership boundary. Never merge an earlier retained package.
    fs::create_dir(&operation).map_err(|error| failure(&operation, error))?;
    let bytes = serde_json::to_vec(&journal)?;
    write_file_atomically(&operation.join("owner.json"), &bytes)
        .map_err(|error| failure(&operation, error))?;
    let journal_path = root.join(JOURNAL);
    cas(&journal_path, None, Some(&bytes))?;
    let result = (|| {
        let started = Instant::now();
        for id in &journal.members {
            if started.elapsed() > Duration::from_secs(30) {
                return Err(failure(
                    &journal_path,
                    "Workshop directory movement exceeded its time budget",
                ));
            }
            move_directory(&mods.join(id), &operation.join(id))?;
        }
        // Changed settings commit with their CAS. A member-only removal leaves
        // every config byte unchanged, so it needs its own durable commit point.
        cas(&config, Some(&before), Some(&after))?;
        if journal.original_sha256 == journal.replacement_sha256 {
            cas(&operation.join(COMMITTED), None, Some(&bytes))?;
        }
        recover(root, false)
    })();
    if let Err(error) = result {
        return match recover(root, false) {
            Ok(()) => Err(error),
            Err(recovery) => Err(failure(
                &journal_path,
                format!("{error}; recovery requires attention: {recovery}"),
            )),
        };
    }
    Ok(())
}

pub(crate) fn ensure_no_pending(root: &Path) -> Result<(), StorageError> {
    match fs::symlink_metadata(root.join(JOURNAL)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(failure(&root.join(JOURNAL), error)),
        Ok(_) => Err(failure(
            &root.join(JOURNAL),
            "pending Workshop collection removal",
        )),
    }
}

pub(crate) fn recover(root: &Path, active: bool) -> Result<(), StorageError> {
    let journal_path = root.join(JOURNAL);
    let Some(bytes) = read_plain(&journal_path, JOURNAL_LIMIT)? else {
        return Ok(());
    };
    if active {
        return Err(failure(
            &journal_path,
            "stop the instance before recovering Workshop collection removal",
        ));
    }
    let journal: Journal =
        serde_json::from_slice(&bytes).map_err(|error| failure(&journal_path, error))?;
    let canonical_uuid = uuid::Uuid::parse_str(&journal.operation_id)
        .ok()
        .map(|id| id.to_string());
    let ids = journal.members.iter().collect::<BTreeSet<_>>();
    if journal.version != 1
        || canonical_uuid.as_deref() != Some(&journal.operation_id)
        || journal.members.len() > 8192
        || ids.len() != journal.members.len()
        || journal.members.iter().any(|id| !valid_id(id))
        || !valid_digest(&journal.original_sha256)
        || !valid_digest(&journal.replacement_sha256)
    {
        return Err(failure(&journal_path, "invalid Workshop removal journal"));
    }
    if crate::instance_program_mode(root)? != crate::InstanceProgramMode::Independent {
        return Err(failure(
            root,
            "Workshop recovery requires instance-owned program files",
        ));
    }
    let runtime = crate::resolve_instance_runtime_root(root)?;
    let mods = runtime.join("SquadGame/Plugins/Mods");
    let operation = root.join(RETAINED).join(&journal.operation_id);
    if read_plain(&operation.join("owner.json"), JOURNAL_LIMIT)?.as_deref()
        != Some(bytes.as_slice())
    {
        return Err(failure(
            &operation,
            "retained Workshop directory ownership does not match the journal",
        ));
    }
    let config = root.join("config/instance.json");
    let current = read_plain(&config, CONFIG_LIMIT)?
        .ok_or_else(|| failure(&config, "instance configuration is missing"))?;
    let document: Value = serde_json::from_slice(&current)?;
    if document.get("instance_id").and_then(Value::as_str) != Some(&journal.instance_id)
        || document.get("module_id").and_then(Value::as_str) != Some("squad")
    {
        return Err(failure(
            &config,
            "Workshop removal journal belongs to another instance",
        ));
    }
    let hash = digest(&current);
    if hash != journal.replacement_sha256 && hash != journal.original_sha256 {
        return Err(failure(
            &config,
            "configuration changed outside the interrupted Workshop removal; retained payloads were preserved",
        ));
    }
    let committed = if journal.original_sha256 == journal.replacement_sha256 {
        match read_plain(&operation.join(COMMITTED), JOURNAL_LIMIT)? {
            None => false,
            Some(marker) if marker == bytes => true,
            Some(_) => {
                return Err(failure(
                    &operation,
                    "Workshop commit marker does not match its owned journal",
                ));
            }
        }
    } else {
        hash == journal.replacement_sha256
    };
    let mut budget = ScanBudget::new();
    let mut rollback = Vec::new();
    for id in &journal.members {
        let source = mods.join(id);
        let saved = operation.join(id);
        let has_source = plain_directory(&source)?;
        let has_saved = plain_directory(&saved)?;
        if has_source == has_saved || (committed && has_source) {
            return Err(failure(
                &source,
                "Workshop recovery found a missing payload or path collision; no directory was overwritten",
            ));
        }
        budget.inspect(if has_saved { &saved } else { &source })?;
        if !committed && has_saved {
            rollback.push((saved, source));
        }
    }
    // Preflight the entire batch before undoing any moves.
    let started = Instant::now();
    for (saved, source) in rollback {
        if started.elapsed() > Duration::from_secs(30) {
            return Err(failure(
                &journal_path,
                "Workshop rollback exceeded its time budget; retry recovery",
            ));
        }
        move_directory(&saved, &source)?;
    }
    cas(&journal_path, Some(&bytes), None)
}

fn move_directory(source: &Path, destination: &Path) -> Result<(), StorageError> {
    if !plain_directory(source)? || plain_directory(destination)? {
        return Err(failure(
            destination,
            "Workshop move source is missing or destination already exists",
        ));
    }
    crate::instance_creation_io::publish_creation_directory(source, destination, None)
}

fn cas(
    path: &Path,
    original: Option<&[u8]>,
    replacement: Option<&[u8]>,
) -> Result<(), StorageError> {
    safe_ancestors(path)?;
    if !compare_and_swap_optional_file_atomically(path, original, replacement)
        .map_err(|error| failure(path, error))?
    {
        return Err(failure(
            path,
            "file changed concurrently during Workshop removal",
        ));
    }
    Ok(())
}

fn safe_ancestors(path: &Path) -> Result<(), StorageError> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || crate::private_runtime::is_reparse_point(ancestor)?
                    || (ancestor != path && !metadata.is_dir())
                {
                    return Err(failure(
                        ancestor,
                        "Workshop removal path is not a plain owned path",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failure(ancestor, error)),
        }
    }
    Ok(())
}

fn plain_directory(path: &Path) -> Result<bool, StorageError> {
    safe_ancestors(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(failure(path, "Workshop payload path is not a directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(failure(path, error)),
    }
}

fn read_plain(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, StorageError> {
    safe_ancestors(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(path, error)),
    };
    if !metadata.is_file() || metadata.len() > limit {
        return Err(failure(
            path,
            "Workshop transaction file is invalid or too large",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| failure(path, error))?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| failure(path, error))?;
    if bytes.len() as u64 > limit {
        return Err(failure(
            path,
            "Workshop transaction file exceeds its size limit",
        ));
    }
    Ok(Some(bytes))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

struct ScanBudget {
    entries: usize,
    started: Instant,
}
impl ScanBudget {
    fn new() -> Self {
        Self {
            entries: 0,
            started: Instant::now(),
        }
    }
    fn inspect(&mut self, root: &Path) -> Result<(), StorageError> {
        let mut pending: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
        while let Some((path, depth)) = pending.pop() {
            self.entries += 1;
            if self.entries > 50_000
                || depth > 32
                || self.started.elapsed() > Duration::from_secs(10)
            {
                return Err(failure(
                    root,
                    "Workshop payload inspection exceeded its bounded budget",
                ));
            }
            safe_ancestors(&path)?;
            let metadata = fs::symlink_metadata(&path).map_err(|error| failure(&path, error))?;
            if metadata.is_dir() {
                for entry in fs::read_dir(&path).map_err(|error| failure(&path, error))? {
                    if pending.len() + self.entries >= 50_000 {
                        return Err(failure(root, "too many Workshop payload entries"));
                    }
                    pending.push((
                        entry.map_err(|error| failure(&path, error))?.path(),
                        depth + 1,
                    ));
                }
            } else if !metadata.is_file() {
                return Err(failure(&path, "unsupported Workshop payload entry"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "workshop_collection_removal_files_tests.rs"]
mod tests;
