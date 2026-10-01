use std::fs;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::StorageError;
use crate::atomic_file::write_file_atomically;
use crate::instance_isolation::paths::normalize_path;
use crate::private_runtime::{PROJECTION_RUNTIME_MARKER, is_reparse_point};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionIdentity {
    version: u32,
    instance_root: String,
    source_root: String,
}

pub(crate) fn record_projection_identity(
    source: &Path,
    instance: &Path,
    runtime: &Path,
) -> Result<(), StorageError> {
    let identity = ProjectionIdentity {
        version: 1,
        instance_root: normalize_path(instance)?.to_string_lossy().into_owned(),
        source_root: normalize_path(source)?.to_string_lossy().into_owned(),
    };
    let path = runtime.join(PROJECTION_RUNTIME_MARKER);
    let bytes = serde_json::to_vec(&identity).map_err(|error| invalid(&path, error))?;
    write_file_atomically(&path, &bytes)
        .map_err(|source| StorageError::WriteConfig { path, source })
}

/// Refresh can resume after runtime was moved aside. Every existing participant
/// must belong to this same explicit projection before recovery may modify it.
pub(crate) fn validate_projection_refresh_roots(
    source: &Path,
    instance: &Path,
) -> Result<(), StorageError> {
    let runtime = instance.join("runtime");
    let rollback = instance.join("runtime.refresh-rollback");
    if !runtime
        .try_exists()
        .map_err(|error| invalid(&runtime, error))?
        && !rollback
            .try_exists()
            .map_err(|error| invalid(&rollback, error))?
    {
        return crate::private_runtime::resolve_instance_private_runtime_root(instance).map(|_| ());
    }
    for name in [
        "runtime",
        "runtime.refresh-rollback",
        "runtime.refresh-staging",
    ] {
        let root = instance.join(name);
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(invalid(&root, error)),
            Ok(metadata) => {
                if !metadata.is_dir()
                    || metadata.file_type().is_symlink()
                    || is_reparse_point(&root)?
                {
                    return Err(invalid(
                        &root,
                        "projection directory is not a plain owned directory",
                    ));
                }
            }
        }
        let path = root.join(PROJECTION_RUNTIME_MARKER);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            invalid(&path, format!("independent or unrecognized runtime cannot be refreshed from a shared package; projection identity unavailable: {error}"))
        })?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || is_reparse_point(&path)?
            || metadata.len() > 16 * 1024
        {
            return Err(invalid(&path, "invalid projection identity file"));
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .and_then(|file| file.take(16 * 1024 + 1).read_to_end(&mut bytes))
            .map_err(|error| invalid(&path, error))?;
        if bytes.len() > 16 * 1024 {
            return Err(invalid(
                &path,
                "projection identity exceeded its size limit",
            ));
        }
        let identity: ProjectionIdentity =
            serde_json::from_slice(&bytes).map_err(|error| invalid(&path, error))?;
        if identity.version != 1
            || identity.instance_root != normalize_path(instance)?.to_string_lossy()
            || identity.source_root != normalize_path(source)?.to_string_lossy()
        {
            return Err(invalid(
                &path,
                "projection identity does not match its instance and source",
            ));
        }
    }
    Ok(())
}

fn invalid(path: &Path, message: impl std::fmt::Display) -> StorageError {
    StorageError::PrivateRuntimeRefresh {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "projection_identity_tests.rs"]
mod tests;
