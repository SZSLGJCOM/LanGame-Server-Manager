use std::fs;
use std::io::Read;
use std::path::Path;

use crate::StorageError;
use crate::instance_archive_files::{self as files, native};
use crate::instance_archive_store::{digest, invalid};

fn read(root: &Path) -> Result<Vec<u8>, StorageError> {
    let path = root.join("config/instance.json");
    crate::instance_isolation::paths::normalize_resource_path(&path)?;
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .and_then(|file| file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(invalid(&path, "Instance configuration exceeds 4 MiB."));
    }
    Ok(bytes)
}

fn stopped_config(bytes: &[u8], root: &Path) -> Result<Vec<u8>, StorageError> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| invalid(root, "Instance configuration must be an object."))?;
    match object.get("autostart") {
        Some(serde_json::Value::Bool(true)) => {
            object.insert("autostart".into(), false.into());
            Ok(serde_json::to_vec_pretty(&value)?)
        }
        Some(serde_json::Value::Bool(false)) | None => Ok(bytes.to_vec()),
        _ => Err(invalid(
            root,
            "Instance autostart configuration is not a boolean.",
        )),
    }
}

pub(crate) fn hashes(root: &Path) -> Result<(String, String), StorageError> {
    let bytes = read(root)?;
    Ok((digest(&bytes), digest(&stopped_config(&bytes, root)?)))
}

/// A deletion journal protects the exact remaining bytes, including malformed
/// JSON. Its absent-file token cannot collide with a SHA-256 hex fingerprint.
pub(crate) fn deletion_hash(root: &Path) -> Result<String, StorageError> {
    match read(root) {
        Ok(bytes) => Ok(digest(&bytes)),
        Err(StorageError::ReadPath { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok("missing-instance-config".to_owned())
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn restore(
    root: &Path,
    identity: &str,
    original_hash: &str,
    restored_hash: &str,
) -> Result<(), StorageError> {
    let config = root.join("config");
    let _guard = native::open(&config, true, false).map_err(|source| StorageError::ReadPath {
        path: config,
        source,
    })?;
    if !files::verify_identity(root, Some(identity))? {
        return Err(invalid(root, "Restore directory is missing."));
    }
    let bytes = read(root)?;
    let actual = digest(&bytes);
    if actual == restored_hash {
        return Ok(());
    }
    if actual != original_hash {
        return Err(invalid(
            root,
            "Instance configuration was edited outside archive restoration; the edit was preserved.",
        ));
    }
    let replacement = stopped_config(&bytes, root)?;
    if digest(&replacement) != restored_hash {
        return Err(invalid(
            root,
            "Restored configuration does not match the journal.",
        ));
    }
    let path = root.join("config/instance.json");
    if !crate::atomic_file::compare_and_swap_file_atomically(&path, &bytes, &replacement)
        .map_err(|source| StorageError::WriteConfig { path, source })?
    {
        return Err(invalid(
            root,
            "Instance configuration changed before publication; restoration remains pending.",
        ));
    }
    Ok(())
}
