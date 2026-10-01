use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use super::{ExternalFile, ExternalProgramPlan, PAYLOAD};
use crate::StorageError;
use crate::instance_archive_files::{self as files, native};
use crate::instance_archive_store::{self as store, Archive, Snapshot, invalid};
use crate::instance_isolation::paths::normalize_path;
use crate::instance_settings_lock::InstanceSettingsLock;

type ProgramInventory = (BTreeMap<String, (String, u64)>, BTreeSet<String>);

fn read(path: &Path, mutate: bool) -> Result<native::OwnedNode, StorageError> {
    native::open_verified_file(path, mutate).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })
}

fn hash(node: &mut native::OwnedNode, path: &Path) -> Result<(String, u64), StorageError> {
    let mut sha = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 256 * 1024];
    loop {
        let n = node
            .reader()
            .read(&mut buffer)
            .map_err(|source| StorageError::ReadPath {
                path: path.to_owned(),
                source,
            })?;
        if n == 0 {
            break;
        }
        #[cfg(test)]
        crate::instance_archive::read_probe::record(path, n as u64);
        sha.update(&buffer[..n]);
        bytes += n as u64;
    }
    let digest = sha
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((digest, bytes))
}

fn check(
    node: &mut native::OwnedNode,
    path: &Path,
    expected: &ExternalFile,
) -> Result<(), StorageError> {
    if hash(node, path)? != (expected.sha256.clone(), expected.bytes) {
        return Err(invalid(
            path,
            "Program or instance data changed after archive admission; the existing file was preserved.",
        ));
    }
    Ok(())
}

pub(super) fn digest_path(path: &Path) -> Result<(String, u64), StorageError> {
    hash(&mut read(path, false)?, path)
}

fn admit_empty_directory(path: &Path) -> Result<String, StorageError> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(source) => {
            return Err(StorageError::CreatePath {
                path: path.to_owned(),
                source,
            });
        }
    }
    let directory = native::open(path, true, false).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    if fs::read_dir(path)
        .map_err(|source| StorageError::ReadDirectory {
            path: path.to_owned(),
            source,
        })?
        .next()
        .transpose()
        .map_err(|source| StorageError::ReadDirectory {
            path: path.to_owned(),
            source,
        })?
        .is_some()
    {
        return Err(invalid(
            path,
            "Unrecognized non-empty archive staging was retained; no files were changed.",
        ));
    }
    Ok(serde_json::to_string(&directory.identity().map_err(
        |source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        },
    )?)?)
}

pub(super) fn inventory(root: &Path) -> Result<ProgramInventory, StorageError> {
    let mut pending = vec![(root.to_owned(), 0usize)];
    let mut entries = BTreeMap::new();
    let mut directories = BTreeSet::new();
    while let Some((directory, depth)) = pending.pop() {
        if depth > 128 || entries.len() + directories.len() + pending.len() > 200_000 {
            return Err(invalid(
                root,
                "External program inventory exceeds its traversal limit.",
            ));
        }
        let _guard =
            native::open(&directory, true, false).map_err(|source| StorageError::ReadPath {
                path: directory.clone(),
                source,
            })?;
        for entry in fs::read_dir(&directory).map_err(|source| StorageError::ReadDirectory {
            path: directory.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| StorageError::ReadDirectory {
                path: directory.clone(),
                source,
            })?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid(&path, "Program inventory escapes its root."))?
                .to_str()
                .ok_or_else(|| invalid(&path, "Program path is not valid text."))?
                .replace('\\', "/");
            crate::private_runtime_refresh::validated_relative_path(&relative)?;
            let metadata =
                fs::symlink_metadata(&path).map_err(|source| StorageError::ReadPath {
                    path: path.clone(),
                    source,
                })?;
            if metadata.is_dir() {
                if !files::plain_directory(&path)? {
                    return Err(invalid(&path, "Program directory disappeared."));
                }
                directories.insert(relative);
                pending.push((path, depth + 1));
            } else {
                entries.insert(relative, hash(&mut read(&path, false)?, &path)?);
            }
            if entries.len() + directories.len() > 200_000 {
                return Err(invalid(
                    root,
                    "External program inventory exceeds 200000 entries.",
                ));
            }
        }
    }
    Ok((entries, directories))
}

pub(super) fn owned_inventory(
    root: &Path,
    owned: &[std::path::PathBuf],
) -> Result<ProgramInventory, StorageError> {
    let mut entries = BTreeMap::new();
    let mut directories = BTreeSet::new();
    for path in owned {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(StorageError::ReadPath {
                    path: path.clone(),
                    source,
                });
            }
        };
        let prefix = path
            .strip_prefix(root)
            .map_err(|_| invalid(path, "Owned data escapes its installation."))?
            .to_str()
            .ok_or_else(|| invalid(path, "Owned path is not text."))?
            .replace('\\', "/");
        if metadata.is_dir() {
            let (children, child_dirs) = inventory(path)?;
            entries.extend(
                children
                    .into_iter()
                    .map(|(key, value)| (format!("{prefix}/{key}"), value)),
            );
            directories.insert(prefix.clone());
            directories.extend(child_dirs.into_iter().map(|key| format!("{prefix}/{key}")));
        } else {
            entries.insert(prefix, hash(&mut read(path, false)?, path)?);
        }
        if entries.len() + directories.len() > 200_000 {
            return Err(invalid(root, "Owned data inventory exceeds its limit."));
        }
    }
    Ok((entries, directories))
}

/// The snapshot owns a flat payload directory before it owns any temporary
/// copy slots. An interrupted stream is replaced only under its recorded identity.
pub(crate) async fn prepare_payload(
    pool: &SqlitePool,
    archive: &Archive,
    snapshot: &mut Snapshot,
    root: &Path,
    lock: &InstanceSettingsLock,
) -> Result<(), StorageError> {
    let Some(plan) = &snapshot.external_program else {
        return Ok(());
    };
    let payload = root.join(PAYLOAD);
    let expected = plan.payload_identity.clone();
    // Retirement compensation also restores every target before discarding its
    // payload. Its persisted staging identity distinguishes that recovery from
    // an archive payload lost before restoration ever began.
    let recovery = (archive.state == "restoring"
        || (archive.state == "archiving" && plan.restore_identity.is_some()))
    .then(|| plan.clone());
    let parent = root.to_owned();
    let parent_identity = archive
        .identity
        .clone()
        .ok_or_else(|| invalid(root, "Instance directory has no committed identity."))?;
    let identity = lock
        .spawn_blocking(move || {
            let _parent = files::guard_identity(&parent, &parent_identity)?;
            if let Some(expected) = expected {
                if files::verify_identity(&payload, Some(&expected))? {
                    return Ok(expected);
                }
                let Some(plan) = recovery else {
                    return Err(invalid(&payload, "Archive program payload is missing."));
                };
                // Files were already restored and the copy removed before a failed
                // commit. Reconstruct this disposable copy only from identical bytes.
                for (relative, file) in &plan.files {
                    let path = plan.root.join(relative);
                    check(&mut read(&path, false)?, &path, file)?;
                }
            }
            admit_empty_directory(&payload)
        })
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "preparing external archive payload",
            message: error.to_string(),
        })??;
    if snapshot
        .external_program
        .as_ref()
        .and_then(|plan| plan.payload_identity.as_ref())
        != Some(&identity)
    {
        snapshot.external_program.as_mut().unwrap().payload_identity = Some(identity);
        let text = serde_json::to_string(snapshot)?;
        if text.len() > store::MAX_SNAPSHOT_BYTES {
            return Err(invalid(
                root,
                "External archive metadata exceeds its limit.",
            ));
        }
        sqlx::query("UPDATE instance_archives SET snapshot_json=?2,snapshot_sha256=?3 WHERE archive_id=?1 AND state IN ('archiving','restoring')")
            .bind(&archive.id).bind(&text).bind(store::digest(text.as_bytes())).execute(pool).await?;
    }
    Ok(())
}

pub(crate) fn finish_restore(root: &Path, plan: &ExternalProgramPlan) -> Result<(), StorageError> {
    // Once all recovered bytes are published, this duplicate is disposable.
    // A failed final commit can reconstruct it from those exact target bytes.
    for (relative, file) in &plan.files {
        let path = plan.root.join(relative);
        check(&mut read(&path, false)?, &path, file)?;
    }
    let payload = root.join(PAYLOAD);
    files::purge_tree(
        &payload,
        plan.payload_identity
            .as_deref()
            .ok_or_else(|| invalid(&payload, "Archive payload identity is missing."))?,
    )
}

pub(crate) async fn prepare_restore(
    pool: &SqlitePool,
    archive: &Archive,
    snapshot: &mut Snapshot,
    lock: &InstanceSettingsLock,
) -> Result<(), StorageError> {
    let Some(plan) = snapshot.external_program.clone() else {
        return Ok(());
    };
    let identity = lock
        .spawn_blocking(move || {
            let parent = plan
                .root
                .parent()
                .ok_or_else(|| invalid(&plan.root, "Program directory has no parent."))?;
            let _parent = files::guard_identity(parent, &plan.parent_identity)?;
            if !plan
                .root
                .try_exists()
                .map_err(|source| StorageError::ReadPath {
                    path: plan.root.clone(),
                    source,
                })?
            {
                if plan.requires_source() {
                    return Err(invalid(
                        &plan.root,
                        "Exact archived program source is missing.",
                    ));
                }
                fs::create_dir(&plan.root).map_err(|source| StorageError::CreatePath {
                    path: plan.root.clone(),
                    source,
                })?;
            }
            let _program = if plan.requires_source() {
                files::guard_identity(&plan.root, &plan.identity)?
            } else {
                native::open(&plan.root, true, false).map_err(|source| StorageError::ReadPath {
                    path: plan.root.clone(),
                    source,
                })?
            };
            let stage = plan
                .root
                .join(format!(".langame-archive-restore-{}", plan.restore_token));
            if let Some(expected) = plan.restore_identity.as_ref() {
                if files::verify_identity(&stage, Some(expected))? {
                    return Ok(expected.clone());
                }
                // A completed copy may have cleaned its staging before the final
                // database commit. Re-admit only when every target is intact.
                for (relative, file) in &plan.files {
                    let path = plan.root.join(relative);
                    check(&mut read(&path, false)?, &path, file)?;
                }
            }
            admit_empty_directory(&stage)
        })
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "preparing external program restoration",
            message: error.to_string(),
        })??;
    if snapshot
        .external_program
        .as_ref()
        .and_then(|plan| plan.restore_identity.as_ref())
        != Some(&identity)
    {
        snapshot.external_program.as_mut().unwrap().restore_identity = Some(identity);
        let text = serde_json::to_string(snapshot)?;
        sqlx::query("UPDATE instance_archives SET snapshot_json=?2,snapshot_sha256=?3 WHERE archive_id=?1 AND state IN ('archiving','restoring')")
            .bind(&archive.id).bind(&text).bind(store::digest(text.as_bytes())).execute(pool).await?;
    }
    Ok(())
}

fn payload_guard(
    root: &Path,
    plan: &ExternalProgramPlan,
) -> Result<native::OwnedNode, StorageError> {
    let payload = root.join(PAYLOAD);
    files::guard_identity(
        &payload,
        plan.payload_identity
            .as_deref()
            .ok_or_else(|| invalid(&payload, "Archive payload has no committed identity."))?,
    )
}

pub(crate) fn capture_files(root: &Path, plan: &ExternalProgramPlan) -> Result<(), StorageError> {
    let _payload = payload_guard(root, plan)?;
    let mut program_guard = if plan.requires_source() {
        Some(files::guard_identity(&plan.root, &plan.identity)?)
    } else {
        None
    };
    for (index, (relative, file)) in plan.files.iter().enumerate() {
        if !file.stored {
            continue;
        }
        let destination = root.join(PAYLOAD).join(format!("{index:06}"));
        if destination
            .try_exists()
            .map_err(|source| StorageError::ReadPath {
                path: destination.clone(),
                source,
            })?
        {
            check(&mut read(&destination, false)?, &destination, file)?;
            continue;
        }
        // A complete full archive is sufficient even when its installation was
        // uninstalled before an interrupted restore. Open the source only when
        // a missing payload slot actually needs those bytes to be rebuilt.
        if program_guard.is_none() {
            program_guard = Some(native::open(&plan.root, true, false).map_err(|source| {
                StorageError::ReadPath {
                    path: plan.root.clone(),
                    source,
                }
            })?);
        }
        let temporary = root.join(PAYLOAD).join(format!("{index:06}.partial"));
        if fs::symlink_metadata(&temporary).is_ok() {
            read(&temporary, true)?
                .remove()
                .map_err(|source| StorageError::DeletePath {
                    path: temporary.clone(),
                    source,
                })?;
        }
        let source = plan.root.join(relative);
        let mut source_guard = read(&source, false)?;
        // Keep the source pinned against writers/replacement through copying.
        // The copy stream checks its content; reject size changes before writing
        // staging bytes so an unexpectedly large source cannot exhaust storage.
        let source_bytes = source_guard
            .reader()
            .metadata()
            .map_err(|error| StorageError::ReadPath {
                path: source.clone(),
                source: error,
            })?
            .len();
        if source_bytes != file.bytes {
            return Err(invalid(
                &source,
                "Program or instance data changed after archive admission; the existing file was preserved.",
            ));
        }
        let copied = crate::instance_creation_io::copy_creation_file(&source, &temporary, None)?;
        if copied != file.sha256 {
            return Err(invalid(
                &temporary,
                "External archive copy checksum failed.",
            ));
        }
        let mut completed = read(&temporary, true)?;
        check(&mut completed, &temporary, file)?;
        completed
            .rename(&destination)
            .map_err(|source| StorageError::MovePath {
                from: temporary,
                to: destination,
                source,
            })?;
    }
    Ok(())
}

pub(crate) fn verify(root: &Path, plan: &ExternalProgramPlan) -> Result<(), StorageError> {
    inspect(root, plan)?;
    let _payload = payload_guard(root, plan)?;
    if plan.requires_source() {
        let _ = files::guard_identity(&plan.root, &plan.identity)?;
    }
    // Every saved byte is checked as well as every omitted source byte. A
    // version label or manifest never substitutes for a payload check.
    for (index, (relative, file)) in plan.files.iter().enumerate() {
        let path = if file.stored {
            root.join(PAYLOAD).join(format!("{index:06}"))
        } else {
            plan.root.join(relative)
        };
        check(&mut read(&path, false)?, &path, file)?;
    }
    Ok(())
}

pub(crate) fn inspect(root: &Path, plan: &ExternalProgramPlan) -> Result<(), StorageError> {
    let _payload = payload_guard(root, plan)?;
    if plan.requires_source() {
        let _ = files::guard_identity(&plan.root, &plan.identity)?;
    }
    let binding = root.join("runtime").join(if plan.exclusive {
        ".langame-exclusive-program.json"
    } else {
        ".langame-shared-program.json"
    });
    if digest_path(&binding)? != (plan.binding_sha256.clone(), plan.binding_bytes) {
        return Err(invalid(
            &binding,
            "Archived program binding was changed; restoration was refused.",
        ));
    }
    Ok(())
}

pub(crate) fn cleanup_owned(root: &Path, plan: &ExternalProgramPlan) -> Result<(), StorageError> {
    verify(root, plan)?;
    let _program = files::guard_identity(&plan.root, &plan.identity)?;
    // Resumed retirement can follow a partially or fully completed rollback.
    // Its staging is disposable only after the complete archive is verified;
    // removing it here lets the committed archive begin a fresh restore later.
    if let Some(expected) = &plan.restore_identity {
        let stage = plan
            .root
            .join(format!(".langame-archive-restore-{}", plan.restore_token));
        if files::plain_directory(&stage)? {
            files::purge_tree(&stage, expected)?;
        }
    }
    // Preflight the complete deletion allowlist before the first unlink.
    for (relative, file) in &plan.files {
        let path = plan.root.join(relative);
        if file.owned
            && path.try_exists().map_err(|source| StorageError::ReadPath {
                path: path.clone(),
                source,
            })?
        {
            check(&mut read(&path, false)?, &path, file)?;
        }
    }
    for (relative, file) in &plan.files {
        if !file.owned {
            continue;
        }
        let path = plan.root.join(relative);
        if !path.try_exists().map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })? {
            continue;
        }
        let mut owned = read(&path, true)?;
        check(&mut owned, &path, file)?;
        owned
            .remove()
            .map_err(|source| StorageError::DeletePath { path, source })?;
    }
    Ok(())
}

pub(crate) fn restore_files(root: &Path, plan: &ExternalProgramPlan) -> Result<(), StorageError> {
    verify(root, plan)?;
    let parent = plan
        .root
        .parent()
        .ok_or_else(|| invalid(&plan.root, "Program directory has no parent."))?;
    let _parent = files::guard_identity(parent, &plan.parent_identity)?;
    if !plan
        .root
        .try_exists()
        .map_err(|source| StorageError::ReadPath {
            path: plan.root.clone(),
            source,
        })?
    {
        if plan.requires_source() {
            return Err(invalid(
                &plan.root,
                "Exact archived program source is missing.",
            ));
        }
        fs::create_dir(&plan.root).map_err(|source| StorageError::CreatePath {
            path: plan.root.clone(),
            source,
        })?;
    }
    let _program =
        native::open(&plan.root, true, false).map_err(|source| StorageError::ReadPath {
            path: plan.root.clone(),
            source,
        })?;
    let stage_name = format!(".langame-archive-restore-{}", plan.restore_token);
    let stage = plan.root.join(&stage_name);
    let stage_identity = plan
        .restore_identity
        .as_deref()
        .ok_or_else(|| invalid(&stage, "Restoration staging has no committed identity."))?;
    let stage_guard = files::guard_identity(&stage, stage_identity)?;
    // Reject unknown changes before writing any recovered file.
    let (current, _) = inventory(&plan.root)?;
    for (relative, (sha, bytes)) in current {
        if relative.starts_with(&format!("{stage_name}/")) {
            continue;
        }
        if plan.complete_inventory
            && !plan
                .files
                .get(&relative)
                .is_some_and(|file| file.sha256 == sha && file.bytes == bytes)
        {
            return Err(invalid(
                &plan.root.join(relative),
                "Program restoration conflicts with an existing file; it was not overwritten.",
            ));
        }
        if let Some(file) = plan.files.get(&relative)
            && (file.sha256 != sha || file.bytes != bytes)
        {
            return Err(invalid(
                &plan.root.join(relative),
                "Restored data conflicts with an existing file; it was not overwritten.",
            ));
        }
    }
    for relative in &plan.directories {
        let directory = plan.root.join(relative);
        normalize_path(&directory)?;
        fs::create_dir_all(&directory).map_err(|source| StorageError::CreatePath {
            path: directory,
            source,
        })?;
    }
    let _payload = payload_guard(root, plan)?;
    for (index, (relative, file)) in plan.files.iter().enumerate() {
        if !file.stored {
            continue;
        }
        let target = plan.root.join(relative);
        if target
            .try_exists()
            .map_err(|source| StorageError::ReadPath {
                path: target.clone(),
                source,
            })?
        {
            continue;
        }
        let source = root.join(PAYLOAD).join(format!("{index:06}"));
        let temporary = stage.join(format!("{index:06}"));
        if fs::symlink_metadata(&temporary).is_ok() {
            read(&temporary, true)?
                .remove()
                .map_err(|source| StorageError::DeletePath {
                    path: temporary.clone(),
                    source,
                })?;
        }
        if crate::instance_creation_io::copy_creation_file(&source, &temporary, None)?
            != file.sha256
        {
            return Err(invalid(
                &temporary,
                "Restored external payload checksum failed.",
            ));
        }
        let mut completed = read(&temporary, true)?;
        check(&mut completed, &temporary, file)?;
        completed
            .rename(&target)
            .map_err(|source| StorageError::MovePath {
                from: temporary,
                to: target,
                source,
            })?;
    }
    drop(stage_guard);
    files::purge_tree(&stage, stage_identity)?;
    Ok(())
}
