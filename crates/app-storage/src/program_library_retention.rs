use std::fs;
use std::io::Read;
use std::path::Path;

use app_modules::ModuleDescriptor;
use serde::{Deserialize, Serialize};

use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};
use crate::program_runtime::invalid;
use crate::{StorageError, StoragePaths};

pub(crate) const RETAINED_LIBRARY: &str = ".langame-retained-library.json";
const MAX_MARKER_BYTES: u64 = 16_384;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedLibrary {
    version: u32,
    module_id: String,
    program_root: String,
}

/// Reserve a library as a reusable source instead of allowing first-instance
/// exclusive use. This records ownership intent, never payload integrity.
/// The caller holds the module lifecycle lease through installation and use.
pub fn retain_library_program_source(
    paths: &StoragePaths,
    root: &Path,
    descriptor: &ModuleDescriptor,
) -> Result<(), StorageError> {
    let root = validate_retained_library_target(paths, root)?;
    write_retained_library_source(&root, &root, &descriptor.summary.id)
}

pub(crate) fn validate_retained_library_target(
    paths: &StoragePaths,
    root: &Path,
) -> Result<std::path::PathBuf, StorageError> {
    let root = normalize_path(root)?;
    for reserved in [
        &paths.instances_root,
        &paths.archives_root,
        &paths.steamcmd_root,
    ] {
        let reserved = normalize_path(reserved)?;
        if contains(&reserved, &root) || contains(&root, &reserved) {
            return Err(invalid(
                &root,
                "retained library overlaps managed instance or tool storage",
            ));
        }
    }
    let games = normalize_path(&paths.games_root)?;
    if contains(&root, &games) {
        return Err(invalid(
            &root,
            "retained library cannot own the game library directory",
        ));
    }
    Ok(root)
}

// A seed writes the final root identity into its staging directory before the
// atomic publication, so cancellation or a crash cannot turn it into first use.
pub(crate) fn write_retained_library_source(
    directory: &Path,
    target: &Path,
    module_id: &str,
) -> Result<(), StorageError> {
    if module_id.is_empty()
        || module_id.len() > 256
        || !module_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid(
            target,
            "invalid retained library module identifier",
        ));
    }
    let target = normalize_path(target)?;
    let path = normalize_resource_path(&directory.join(RETAINED_LIBRARY))?;
    let bytes = serde_json::to_vec(&RetainedLibrary {
        version: 1,
        module_id: module_id.to_owned(),
        program_root: target.to_string_lossy().into_owned(),
    })?;
    if bytes.len() as u64 > MAX_MARKER_BYTES {
        return Err(invalid(
            &path,
            "retained library metadata exceeds its size limit",
        ));
    }
    match crate::atomic_file::create_file_atomically(&path, &bytes) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if read_retained_library_source(directory, &target, module_id)? {
                Ok(())
            } else {
                Err(invalid(
                    &path,
                    "existing retained library metadata does not match this installation",
                ))
            }
        }
        Err(source) => Err(StorageError::WriteConfig { path, source }),
    }
}

pub(crate) fn retained_library_program_source(
    root: &Path,
    module_id: &str,
) -> Result<bool, StorageError> {
    let root = normalize_path(root)?;
    read_retained_library_source(&root, &root, module_id)
}

fn read_retained_library_source(
    directory: &Path,
    target: &Path,
    module_id: &str,
) -> Result<bool, StorageError> {
    let path = normalize_resource_path(&directory.join(RETAINED_LIBRARY))?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(StorageError::ReadPath { path, source }),
    };
    if !metadata.is_file() || metadata.len() > MAX_MARKER_BYTES {
        return Ok(false);
    }
    let file = fs::File::open(&path).map_err(|source| StorageError::ReadPath {
        path: path.clone(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath { path, source })?;
    if bytes.len() as u64 > MAX_MARKER_BYTES {
        return Ok(false);
    }
    let Ok(record) = serde_json::from_slice::<RetainedLibrary>(&bytes) else {
        return Ok(false);
    };
    Ok(record.version == 1
        && record.module_id == module_id
        && record.program_root == target.to_string_lossy())
}
