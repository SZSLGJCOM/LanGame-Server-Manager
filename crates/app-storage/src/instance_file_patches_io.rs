use super::*;
use crate::atomic_file::compare_and_swap_file_atomically;

fn validate_source(patch: &PreparedInstanceTextPatch) -> Result<(), StorageError> {
    validate_relative_file(&patch.preview.file)?;
    validate_runtime_marker(&patch.root, &patch.preview.file)?;
    let path = patch.root.join(&patch.preview.file);
    if io::read_bytes(&path)? != patch.original {
        return Err(invalid(
            &path,
            "A source changed after preview; the edit set was not applied.",
        ));
    }
    Ok(())
}

pub(super) fn apply_patches(
    prepared: &PreparedInstanceFilePatches,
) -> Result<InstanceFilePatchesResult, StorageError> {
    apply_with_hook(prepared, |_| Ok(()))
}

fn apply_with_hook(
    prepared: &PreparedInstanceFilePatches,
    mut before_write: impl FnMut(usize) -> Result<(), StorageError>,
) -> Result<InstanceFilePatchesResult, StorageError> {
    let mut guards = Vec::new();
    // Reject any stale source or protected path before writing any source file.
    for patch in &prepared.patches {
        let path = patch.root.join(&patch.preview.file);
        let parent = path
            .parent()
            .ok_or_else(|| invalid(&path, "File has no parent."))?;
        guards.extend(io::guard_directories(parent)?);
        validate_source(patch)?;
    }
    let mut result = InstanceFilePatchesResult {
        status: InstanceFilePatchesStatus::NotApplied,
        files: prepared
            .patches
            .iter()
            .map(|patch| InstanceFilePatchOutcome {
                file: patch.preview.file.clone(),
                source_sha256: patch.preview.source_sha256.clone(),
                result_sha256: patch.preview.result_sha256.clone(),
                backup_id: None,
                state: InstanceFilePatchState::NotApplied,
                read_back_verified: false,
                error: None,
            })
            .collect(),
        error: None,
    };
    // A write never starts before every original has a durable recovery copy.
    // Backup failures retain any earlier copies and report their IDs.
    for (index, patch) in prepared.patches.iter().enumerate() {
        match io::create_backup(patch) {
            Ok(id) => result.files[index].backup_id = Some(id),
            Err(error) => {
                result.error = Some(error.to_string());
                result.files[index].error = Some(error.to_string());
                return Ok(result);
            }
        }
    }
    for (index, patch) in prepared.patches.iter().enumerate() {
        if let Err(error) = validate_source(patch) {
            result.error = Some(error.to_string());
            result.files[index].error = Some(error.to_string());
            return Ok(result);
        }
    }
    // Each individual replacement is atomic. This set is not an OS transaction:
    // external writers or process termination can still require backup recovery.
    for (index, patch) in prepared.patches.iter().enumerate() {
        let path = patch.root.join(&patch.preview.file);
        let write = (|| {
            before_write(index)?;
            validate_source(patch)?;
            if !compare_and_swap_file_atomically(&path, &patch.original, &patch.replacement)
                .map_err(|error| invalid(&path, error.to_string()))?
            {
                return Err(invalid(&path, "The source changed at the write boundary."));
            }
            if io::read_bytes(&path)? != patch.replacement {
                return Err(invalid(
                    &path,
                    "Read-back did not match the prepared result.",
                ));
            }
            Ok(())
        })();
        if let Err(error) = write {
            result.error = Some(error.to_string());
            result.files[index].error = Some(error.to_string());
            // Include the failed write: its return alone cannot prove no mutation.
            rollback_attempt(prepared, &mut result, index);
            return Ok(result);
        }
        result.files[index].state = InstanceFilePatchState::Applied;
        result.files[index].read_back_verified = true;
    }
    // Recheck earlier files after the last write as well. A file edited by an
    // outside process during this batch must not produce a successful receipt.
    for (index, patch) in prepared.patches.iter().enumerate() {
        let path = patch.root.join(&patch.preview.file);
        let verified = io::read_bytes(&path).and_then(|bytes| {
            if bytes == patch.replacement {
                Ok(())
            } else {
                Err(invalid(
                    &path,
                    "A file changed before the edit set's final verification.",
                ))
            }
        });
        if let Err(error) = verified {
            result.error = Some(error.to_string());
            result.files[index].error = Some(error.to_string());
            rollback_attempt(prepared, &mut result, prepared.patches.len() - 1);
            return Ok(result);
        }
    }
    result.status = InstanceFilePatchesStatus::Applied;
    Ok(result)
}

fn rollback_attempt(
    prepared: &PreparedInstanceFilePatches,
    result: &mut InstanceFilePatchesResult,
    last: usize,
) {
    for index in (0..=last).rev() {
        rollback(&prepared.patches[index], &mut result.files[index]);
    }
    result.status = if result
        .files
        .iter()
        .any(|file| file.state == InstanceFilePatchState::RecoveryRequired)
    {
        InstanceFilePatchesStatus::Partial
    } else if result
        .files
        .iter()
        .any(|file| file.state == InstanceFilePatchState::RolledBack)
    {
        InstanceFilePatchesStatus::RolledBack
    } else {
        InstanceFilePatchesStatus::NotApplied
    };
}

fn rollback(patch: &PreparedInstanceTextPatch, outcome: &mut InstanceFilePatchOutcome) {
    let path = patch.root.join(&patch.preview.file);
    let restore = (|| {
        validate_relative_file(&patch.preview.file)?;
        validate_runtime_marker(&patch.root, &patch.preview.file)?;
        let current = io::read_bytes(&path)?;
        if current == patch.original {
            return Ok(false);
        }
        // Never overwrite unrelated edits made after this set began.
        if current != patch.replacement
            || !compare_and_swap_file_atomically(&path, &patch.replacement, &patch.original)
                .map_err(|error| invalid(&path, error.to_string()))?
        {
            return Err(invalid(
                &path,
                "Rollback encountered changed content; the external edit was preserved. Recover from the retained backup after inspection.",
            ));
        }
        if io::read_bytes(&path)? != patch.original {
            return Err(invalid(
                &path,
                "Rollback read-back failed; inspect the file and retained backup.",
            ));
        }
        Ok(true)
    })();
    match restore {
        Ok(restored) => {
            outcome.state = if restored || outcome.state == InstanceFilePatchState::Applied {
                InstanceFilePatchState::RolledBack
            } else {
                InstanceFilePatchState::NotApplied
            };
            outcome.read_back_verified = true;
        }
        Err(error) => {
            outcome.state = InstanceFilePatchState::RecoveryRequired;
            outcome.read_back_verified = false;
            outcome.error = Some(error.to_string());
        }
    }
}

#[cfg(test)]
pub(super) fn apply_with_test_hook(
    prepared: &PreparedInstanceFilePatches,
    before_write: impl FnMut(usize) -> Result<(), StorageError>,
) -> Result<InstanceFilePatchesResult, StorageError> {
    apply_with_hook(prepared, before_write)
}
