//! Durable ownership for the two directory renames in an ordinary save restore.
//! A prepared restore rolls back after interruption; a committed one only cleans
//! its retained rollback copy. Neither state may be used to start a game.
use super::*;
use crate::instance_archive_files as owned;
use crate::instance_isolation::paths::normalize_path;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;

// Match archive purge's bounded cleanup inventory. This only retains cleanup
// copies after a verified restore; backup creation and publication stay unbounded.
const MAX_CLEANUP_ENTRIES: usize = 200_000;
const MAX_CLEANUP_DEPTH: usize = 128;

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    Committed,
    RolledBack,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Directory {
    identity: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    version: u8,
    target: PathBuf,
    staging: PathBuf,
    rollback: PathBuf,
    parent_identity: String,
    original: Directory,
    replacement: Directory,
    phase: Phase,
}

fn journal_path(target: &Path) -> Result<PathBuf, StorageError> {
    let parent = target
        .parent()
        .ok_or_else(|| failure(target, "Save directory has no parent"))?;
    let leaf = target
        .file_name()
        .ok_or_else(|| failure(target, "Save directory has no name"))?;
    let hash = hex_digest(&Sha256::digest(
        leaf.to_string_lossy().to_lowercase().as_bytes(),
    ));
    Ok(parent.join(format!(".langame-backup-restore-{hash}.json")))
}

pub(super) fn ensure_ready(target: &Path) -> Result<(), StorageError> {
    let path = journal_path(target)?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StorageError::ReadPath { path, source }),
        Ok(_) => Err(failure(
            &path,
            "An interrupted backup restore is pending. Keep the server stopped and retry restoring a backup to recover it; all recovery copies were retained",
        )),
    }
}

pub(super) fn ensure_record_ready(
    record: &StoredInstanceRecord,
    saves: &Path,
) -> Result<(), StorageError> {
    for scope in scopes(record, saves) {
        ensure_ready(&scope)?;
    }
    Ok(())
}

fn scopes(record: &StoredInstanceRecord, saves: &Path) -> Vec<PathBuf> {
    let mut scopes = vec![saves.to_owned()];
    if app_core::ark_maps::is_ark(&record.summary.module_id) {
        scopes.push(saves.join(&record.summary.id));
    }
    if record.summary.module_id == "sevendaystodie" {
        scopes.push(instance_root(record).join("GeneratedWorlds"));
    }
    scopes
}

pub(super) fn recover_record(
    record: &StoredInstanceRecord,
    saves: &Path,
    publication: &publication::Publication,
) -> Result<(), StorageError> {
    for scope in scopes(record, saves) {
        recover(&scope, publication)?;
    }
    Ok(())
}

impl Journal {
    pub(super) fn prepare(
        target: &Path,
        staging: &Path,
        rollback: &Path,
    ) -> Result<Self, StorageError> {
        ensure_ready(target)?;
        let target = normalize_path(target)?;
        let staging = normalize_path(staging)?;
        let rollback = normalize_path(rollback)?;
        let parent = target
            .parent()
            .ok_or_else(|| failure(&target, "Save directory has no parent"))?;
        Ok(Self {
            version: 1,
            parent_identity: directory_identity(parent)?,
            original: Directory {
                identity: directory_identity(&target)?,
                sha256: fingerprint(&target)?,
            },
            replacement: Directory {
                identity: directory_identity(&staging)?,
                sha256: fingerprint(&staging)?,
            },
            target,
            staging,
            rollback,
            phase: Phase::Prepared,
        })
    }

    pub(super) fn begin(&self) -> Result<(), StorageError> {
        self.validate()?;
        let path = journal_path(&self.target)?;
        crate::atomic_file::create_file_atomically(&path, &serde_json::to_vec(self)?)
            .map_err(|source| StorageError::WriteConfig { path, source })
    }

    pub(super) fn commit(&mut self) -> Result<(), StorageError> {
        self.set_phase(Phase::Committed)
    }

    fn set_phase(&mut self, phase: Phase) -> Result<(), StorageError> {
        let path = journal_path(&self.target)?;
        let expected = serde_json::to_vec(self)?;
        self.phase = phase;
        let bytes = serde_json::to_vec(self)?;
        if !crate::atomic_file::compare_and_swap_file_atomically(&path, &expected, &bytes).map_err(
            |source| StorageError::WriteConfig {
                path: path.clone(),
                source,
            },
        )? {
            return Err(failure(
                &path,
                "Backup restore journal changed; recovery copies were retained",
            ));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), StorageError> {
        let parent = self
            .target
            .parent()
            .ok_or_else(|| failure(&self.target, "Invalid restore parent"))?;
        let sibling = |path: &Path, prefix: &str| {
            path.parent() == Some(parent)
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.strip_prefix(prefix).is_some_and(|suffix| {
                            suffix.len() == 12
                                && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
                        })
                    })
        };
        if self.version != 1
            || !self.target.is_absolute()
            || normalize_path(&self.target)? != self.target
            || !sibling(&self.staging, ".restore-publish-")
            || !sibling(&self.rollback, ".restore-rollback-")
            || self.target == self.staging
            || self.target == self.rollback
            || self.staging == self.rollback
            || ![&self.original.sha256, &self.replacement.sha256]
                .into_iter()
                .all(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(failure(
                &self.target,
                "Invalid backup restore ownership; every recovery copy was retained",
            ));
        }
        if !owned::verify_identity(parent, Some(&self.parent_identity))? {
            return Err(failure(parent, "Backup restore parent disappeared"));
        }
        Ok(())
    }

    fn verify(&self, path: &Path, expected: &Directory) -> Result<(), StorageError> {
        if !owned::verify_identity(path, Some(&expected.identity))?
            || fingerprint(path)? != expected.sha256
        {
            return Err(failure(
                path,
                "Backup recovery contents changed; every recovery copy was retained",
            ));
        }
        Ok(())
    }

    fn cleanup(&self) -> Result<(), StorageError> {
        self.validate()?;
        if self.phase == Phase::Prepared {
            return Err(failure(
                &self.target,
                "An unfinished restore cannot release its recovery marker",
            ));
        }
        let expected = if self.phase == Phase::Committed {
            &self.replacement
        } else {
            &self.original
        };
        let _target = owned::guard_identity(&self.target, &expected.identity)?;
        self.verify(&self.target, expected)?;
        // Cleanup is secondary to the verified terminal target. Never infer
        // that an altered or partially deleted copy is still safe to discard.
        for (path, expected) in [
            (&self.staging, &self.replacement),
            (&self.rollback, &self.original),
        ] {
            let result = (|| {
                if owned::identity(path)?.is_some() {
                    purge_verified_copy(path, expected)?;
                }
                Ok::<_, StorageError>(())
            })();
            if let Err(error) = result {
                let retained = self.retain_completed()?;
                let outcome = if self.phase == Phase::Committed {
                    "Backup restore already completed"
                } else {
                    "Interrupted backup restore was rolled back and the original saves are restored"
                };
                return Err(failure(
                    path,
                    &format!(
                        "{outcome}; cleanup is incomplete. Remaining recovery copies were retained at {} and {}; ownership metadata: {}. The verified save directory is available. Cleanup error: {error}",
                        self.staging.display(),
                        self.rollback.display(),
                        retained.display(),
                    ),
                ));
            }
        }
        self.remove_active_journal()
    }

    fn retain_completed(&self) -> Result<PathBuf, StorageError> {
        let leaf = self
            .rollback
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(".restore-rollback-"))
            .ok_or_else(|| failure(&self.rollback, "Invalid retained restore identity"))?;
        let path = self
            .target
            .parent()
            .unwrap()
            .join(format!(".langame-retained-restore-{leaf}.json"));
        let bytes = serde_json::to_vec(self)?;
        match crate::atomic_file::create_file_atomically(&path, &bytes) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let mut file =
                    owned::native::open_verified_file(&path, false).map_err(|source| {
                        StorageError::ReadPath {
                            path: path.clone(),
                            source,
                        }
                    })?;
                let mut existing = Vec::new();
                file.reader()
                    .take(bytes.len() as u64 + 1)
                    .read_to_end(&mut existing)
                    .map_err(|source| StorageError::ReadPath {
                        path: path.clone(),
                        source,
                    })?;
                if existing != bytes {
                    return Err(failure(
                        &path,
                        "Retained restore metadata changed; recovery remains blocked",
                    ));
                }
            }
            Err(source) => return Err(StorageError::WriteConfig { path, source }),
        }
        // Publication must be durable before removing exactly this generation's
        // active marker. A changed journal or failed CAS remains a hard block.
        self.remove_active_journal()?;
        Ok(path)
    }

    fn remove_active_journal(&self) -> Result<(), StorageError> {
        let path = journal_path(&self.target)?;
        let expected = serde_json::to_vec(self)?;
        if !crate::atomic_file::compare_and_swap_optional_file_atomically(
            &path,
            Some(&expected),
            None,
        )
        .map_err(|source| StorageError::DeletePath {
            path: path.clone(),
            source,
        })? {
            return Err(failure(
                &path,
                "Backup restore journal changed before cleanup",
            ));
        }
        Ok(())
    }
}

pub(super) fn recover(
    target: &Path,
    publication: &publication::Publication,
) -> Result<(), StorageError> {
    let path = journal_path(target)?;
    let metadata = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(StorageError::ReadPath { path, source }),
        Ok(metadata) => metadata,
    };
    require_plain_file(&path, target.parent().unwrap_or(target), &metadata)?;
    if metadata.len() > 64 * 1024 {
        return Err(failure(
            &path,
            "Backup restore journal exceeds its metadata limit",
        ));
    }
    let mut file = owned::native::open_verified_file(&path, false).map_err(|source| {
        StorageError::ReadPath {
            path: path.clone(),
            source,
        }
    })?;
    let mut bytes = Vec::new();
    file.reader()
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
    drop(file);
    let mut journal: Journal = serde_json::from_slice(&bytes)?;
    if journal.target != normalize_path(target)? {
        return Err(failure(
            &path,
            "Backup restore journal belongs to a different save directory",
        ));
    }
    journal.validate()?;
    let _parent =
        owned::guard_identity(journal.target.parent().unwrap(), &journal.parent_identity)?;
    if journal.phase == Phase::Prepared {
        let at_target = owned::identity(&journal.target)?;
        let rollback = owned::identity(&journal.rollback)?;
        let staging = owned::identity(&journal.staging)?;
        let already_original = at_target.as_deref() == Some(&journal.original.identity);
        if already_original {
            journal.verify(&journal.target, &journal.original)?;
            if rollback.is_some() {
                return Err(failure(
                    &path,
                    "Conflicting original restore directories were retained",
                ));
            }
        } else {
            journal.verify(&journal.rollback, &journal.original)?;
            if at_target.is_some() {
                journal.verify(&journal.target, &journal.replacement)?;
                if staging.is_some() {
                    return Err(failure(
                        &path,
                        "Conflicting replacement restore directories were retained",
                    ));
                }
            }
        }
        if staging.is_some() {
            journal.verify(&journal.staging, &journal.replacement)?;
        }
        let stamp = publication::Stamp::directories(&[
            journal.target.clone(),
            journal.staging.clone(),
            journal.rollback.clone(),
        ])?;
        publication.publish(&stamp, || {
            if !already_original {
                if at_target.is_some() {
                    owned::move_directory(
                        &journal.target,
                        &journal.staging,
                        &journal.replacement.identity,
                    )?;
                }
                owned::move_directory(
                    &journal.rollback,
                    &journal.target,
                    &journal.original.identity,
                )?;
            }
            journal.set_phase(Phase::RolledBack)
        })?;
    }
    journal.cleanup()
}

fn directory_identity(path: &Path) -> Result<String, StorageError> {
    owned::identity(path)?.ok_or_else(|| failure(path, "Backup directory is missing"))
}

// Stream names and file contents without imposing the bounded preview hash's
// tree-size limit on ordinary backups. Only one directory's entries are sorted.
fn fingerprint(root: &Path) -> Result<String, StorageError> {
    scan_tree(root, None)
}

struct CleanupEntry {
    path: PathBuf,
    identity: String,
    sha256: Option<String>,
    // Keep every directory pinned until its children have been removed.
    guard: Option<owned::native::OwnedNode>,
}

fn scan_tree(
    root: &Path,
    mut inventory: Option<&mut Vec<CleanupEntry>>,
) -> Result<String, StorageError> {
    canonical_plain_directory(root, root)?;
    let mut digest = Sha256::new();
    let mut pending = vec![(root.to_owned(), 0_usize)];
    let mut buffer = [0_u8; 65536];
    while let Some((path, depth)) = pending.pop() {
        if inventory.as_ref().is_some_and(|entries| {
            entries.len() >= MAX_CLEANUP_ENTRIES || depth > MAX_CLEANUP_DEPTH
        }) {
            return Err(failure(
                root,
                "Recovery cleanup traversal limit exceeded; remaining copies were retained",
            ));
        }
        let metadata = fs::symlink_metadata(&path).map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| failure(&path, "Backup entry escaped its root"))?;
        let name = relative.as_os_str().as_encoded_bytes();
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name);
        if metadata.is_dir() {
            require_plain_directory(&path, root, &metadata)?;
            if let Some(entries) = inventory.as_deref_mut() {
                let identity = directory_identity(&path)?;
                let guard = owned::guard_identity(&path, &identity)?;
                entries.push(CleanupEntry {
                    path: path.clone(),
                    identity,
                    sha256: None,
                    guard: Some(guard),
                });
            }
            digest.update([0]);
            let reader = fs::read_dir(&path).map_err(|source| StorageError::ReadDirectory {
                path: path.clone(),
                source,
            })?;
            let mut children = Vec::new();
            for child in reader {
                if inventory.as_ref().is_some_and(|entries| {
                    entries.len() + pending.len() + children.len() >= MAX_CLEANUP_ENTRIES
                }) {
                    return Err(failure(
                        root,
                        "Recovery cleanup traversal limit exceeded; remaining copies were retained",
                    ));
                }
                children.push(
                    child
                        .map_err(|source| StorageError::ReadDirectory {
                            path: path.clone(),
                            source,
                        })?
                        .path(),
                );
            }
            children.sort();
            pending.extend(children.into_iter().rev().map(|path| (path, depth + 1)));
        } else {
            require_plain_file(&path, root, &metadata)?;
            digest.update([1]);
            digest.update(metadata.len().to_le_bytes());
            let mut file = owned::native::open_verified_file(&path, false).map_err(|source| {
                StorageError::ReadPath {
                    path: path.clone(),
                    source,
                }
            })?;
            let mut file_digest = inventory.is_some().then(Sha256::new);
            let mut copied = 0_u64;
            loop {
                let count =
                    file.reader()
                        .read(&mut buffer)
                        .map_err(|source| StorageError::ReadPath {
                            path: path.clone(),
                            source,
                        })?;
                if count == 0 {
                    break;
                }
                copied += count as u64;
                digest.update(&buffer[..count]);
                if let Some(digest) = file_digest.as_mut() {
                    digest.update(&buffer[..count]);
                }
            }
            if copied != metadata.len() {
                return Err(failure(
                    &path,
                    "Backup contents changed during verification",
                ));
            }
            if let Some(entries) = inventory.as_deref_mut() {
                let identity = serde_json::to_string(&file.identity().map_err(|source| {
                    StorageError::ReadPath {
                        path: path.clone(),
                        source,
                    }
                })?)?;
                entries.push(CleanupEntry {
                    path,
                    identity,
                    sha256: file_digest.map(|digest| hex_digest(&digest.finalize())),
                    guard: None,
                });
            }
        }
    }
    Ok(hex_digest(&digest.finalize()))
}

fn purge_verified_copy(root: &Path, expected: &Directory) -> Result<(), StorageError> {
    let mut root_guard = Some(owned::guard_identity(root, &expected.identity)?);
    let mut entries = Vec::new();
    if scan_tree(root, Some(&mut entries))? != expected.sha256 {
        return Err(failure(
            root,
            "Recovery copy changed; no cleanup was performed",
        ));
    }
    let mut buffer = [0_u8; 65536];
    for mut entry in entries.into_iter().rev() {
        if entry.path == root {
            drop(root_guard.take());
        }
        drop(entry.guard.take());
        let mut node = match &entry.sha256 {
            Some(_) => owned::native::open_verified_file(&entry.path, true),
            None => owned::native::open(&entry.path, true, true),
        }
        .map_err(|source| StorageError::DeletePath {
            path: entry.path.clone(),
            source,
        })?;
        let identity =
            serde_json::to_string(&node.identity().map_err(|source| StorageError::ReadPath {
                path: entry.path.clone(),
                source,
            })?)?;
        if identity != entry.identity {
            return Err(failure(
                &entry.path,
                "Recovery entry identity changed; remaining copies were retained",
            ));
        }
        if let Some(expected) = &entry.sha256 {
            let mut digest = Sha256::new();
            loop {
                let count =
                    node.reader()
                        .read(&mut buffer)
                        .map_err(|source| StorageError::ReadPath {
                            path: entry.path.clone(),
                            source,
                        })?;
                if count == 0 {
                    break;
                }
                digest.update(&buffer[..count]);
            }
            if hex_digest(&digest.finalize()) != *expected {
                return Err(failure(
                    &entry.path,
                    "Recovery file changed; remaining copies were retained",
                ));
            }
        }
        // This handle denies writers and replacement through verification and
        // deletion. New children cause directory removal to fail, not recurse.
        node.remove().map_err(|source| StorageError::DeletePath {
            path: entry.path,
            source,
        })?;
    }
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn failure(path: &Path, message: &str) -> StorageError {
    backup_confirmation_error(path, message)
}
