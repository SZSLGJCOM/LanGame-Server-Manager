use super::*;
use crate::managed_console_log::owned_fs::FileIdentity;
use sha2::{Digest, Sha256};

pub(super) fn prepare(
    directory: &Path,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let path = directory.join("ownership.json");
    if !exists(&path)? {
        return Ok(());
    }
    let before = read_bytes(&path, 1024 * 1024)?;
    let manifest_identity: FileIdentity = serde_json::from_str(&identity(&path, false)?)?;
    let ledger = crate::managed_console_log::store::parse_ledger(&before, &manifest_identity)
        .map_err(|error| invalid(&path, error.to_string()))?;
    if ledger.has_pending_recovery() {
        return Err(invalid(
            &path,
            "Finish managed console retention recovery before moving storage.",
        ));
    }
    let mut value: Value = serde_json::from_slice(&before)?;
    for (run, parts) in &ledger.runs {
        for (index, part) in parts.iter().enumerate() {
            let source = directory.join(&part.file);
            let expected = serde_json::to_string(&part.identity)?;
            let next = rebind_identity(&source, &expected, false, moves, copied)?;
            if copied && next != expected {
                verify_copy(&source, &remap_path(&source, moves)?)?;
                let entry = value
                    .get_mut("runs")
                    .and_then(|runs| runs.get_mut(run))
                    .and_then(Value::as_array_mut)
                    .and_then(|parts| parts.get_mut(index))
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| {
                        invalid(
                            &path,
                            "Managed console part disappeared from its validated ledger.",
                        )
                    })?;
                entry.insert("identity".into(), serde_json::from_str(&next)?);
            }
        }
    }
    add_json_patch(&path, &before, &value, moves, copied, plan)
}

pub(super) fn verify_copy(source: &Path, target: &Path) -> Result<(), StorageError> {
    if same_path(source, target)? {
        return Ok(());
    }
    let mut source_node = native::open_verified_file(source, false).map_err(|source_error| {
        StorageError::ReadPath {
            path: source.to_owned(),
            source: source_error,
        }
    })?;
    let mut target_node = native::open_verified_file(target, false).map_err(|source_error| {
        StorageError::ReadPath {
            path: target.to_owned(),
            source: source_error,
        }
    })?;
    if hash(source_node.reader(), source)? != hash(target_node.reader(), target)? {
        return Err(invalid(
            target,
            "Copied identity-protected content differs from its source.",
        ));
    }
    Ok(())
}

fn hash(reader: &mut fs::File, path: &Path) -> Result<(u64, Vec<u8>), StorageError> {
    let mut digest = Sha256::new();
    let mut size = 0u64;
    let mut bytes = [0u8; 256 * 1024];
    loop {
        let count = reader
            .read(&mut bytes)
            .map_err(|source| StorageError::ReadPath {
                path: path.to_owned(),
                source,
            })?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or_else(|| invalid(path, "File size overflows migration hashing."))?;
        digest.update(&bytes[..count]);
    }
    Ok((size, digest.finalize().to_vec()))
}
