use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use app_modules::ModuleDescriptor;
use serde::{Deserialize, Serialize};

use super::invalid;
use crate::StorageError;
use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};

pub(super) const ACQUISITION: &str = ".langame-program-acquisition.json";
pub(super) const MAX_ACQUISITION_BYTES: u64 = 4_096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramAcquisition {
    version: u32,
    module_id: String,
    target: PathBuf,
    id: String,
}

/// Captured only from a validated manager-owned target before installation.
/// Its private fields prevent creating authority for an arbitrary existing root.
#[derive(Debug)]
pub struct LibraryProgramAcquisition {
    root: PathBuf,
    bytes: Vec<u8>,
}

/// Start a first installation only in a genuinely empty directory. The caller
/// holds the module and target lifecycle lease through installation; existing
/// files, subdirectories and acquisition metadata are never adopted or replaced.
pub fn begin_empty_library_program_acquisition(
    root: &Path,
    descriptor: &ModuleDescriptor,
) -> Result<(), StorageError> {
    // Resolve and reject links before creating a missing tail, then check the
    // resulting directory again before recording its normalized ownership.
    let root = normalize_path(root)?;
    fs::create_dir_all(&root).map_err(|source| StorageError::CreatePath {
        path: root.clone(),
        source,
    })?;
    let root = normalize_path(&root)?;
    let mut entries = fs::read_dir(&root).map_err(|source| StorageError::ReadDirectory {
        path: root.clone(),
        source,
    })?;
    if let Some(entry) = entries.next() {
        entry.map_err(|source| StorageError::ReadDirectory {
            path: root.clone(),
            source,
        })?;
        return Err(invalid(
            &root,
            "a new program acquisition requires an empty installation directory",
        ));
    }
    write_acquisition(&root, &root, &descriptor.summary.id)
}

/// This proves only that the manager allocated an empty or allowlist-filtered
/// installation target. A retry must still complete official validation.
pub fn library_program_acquisition_is_trusted(
    root: &Path,
    descriptor: &ModuleDescriptor,
) -> Result<bool, StorageError> {
    Ok(read_library_program_acquisition(root, descriptor)?.is_some())
}

pub fn read_library_program_acquisition(
    root: &Path,
    descriptor: &ModuleDescriptor,
) -> Result<Option<LibraryProgramAcquisition>, StorageError> {
    let canonical = normalize_path(root)?;
    let path = normalize_resource_path(&root.join(ACQUISITION))?;
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(StorageError::ReadPath { path, source }),
    };
    let mut bytes = Vec::new();
    file.take(MAX_ACQUISITION_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
    if bytes.len() as u64 > MAX_ACQUISITION_BYTES {
        return Err(invalid(&path, "program acquisition record is too large"));
    }
    let pending: ProgramAcquisition = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(
            &path,
            format!("invalid program acquisition record: {error}"),
        )
    })?;
    let target = normalize_path(&pending.target)?;
    if pending.version != 1
        || pending.module_id != descriptor.summary.id
        || uuid::Uuid::parse_str(&pending.id).is_err()
        || !contains(&canonical, &target)
        || !contains(&target, &canonical)
    {
        return Err(invalid(
            &path,
            "program acquisition does not belong to this module and directory",
        ));
    }
    Ok(Some(LibraryProgramAcquisition {
        root: canonical,
        bytes,
    }))
}

/// A whole-directory installer may replace the target and discard its metadata.
/// Restore only evidence captured under the same installation lifecycle lease.
pub fn restore_library_program_acquisition(
    pending: &LibraryProgramAcquisition,
) -> Result<(), StorageError> {
    let root = normalize_path(&pending.root)?;
    if !root.is_dir() || !contains(&root, &pending.root) || !contains(&pending.root, &root) {
        return Err(invalid(
            &pending.root,
            "acquisition target changed or is missing after installation",
        ));
    }
    let path = normalize_resource_path(&root.join(ACQUISITION))?;
    match fs::File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_ACQUISITION_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|source| StorageError::ReadPath {
                    path: path.clone(),
                    source,
                })?;
            if bytes != pending.bytes {
                return Err(invalid(
                    &path,
                    "acquisition ownership changed during installation",
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::File::create_new(&path)
            .and_then(|mut file| file.write_all(&pending.bytes))
            .map_err(|source| StorageError::WriteConfig { path, source }),
        Err(source) => Err(StorageError::ReadPath { path, source }),
    }
}

pub(super) fn write_acquisition(
    stage: &Path,
    target: &Path,
    module_id: &str,
) -> Result<(), StorageError> {
    let path = stage.join(ACQUISITION);
    let bytes = serde_json::to_vec(&ProgramAcquisition {
        version: 1,
        module_id: module_id.to_owned(),
        target: normalize_path(target)?,
        id: uuid::Uuid::new_v4().to_string(),
    })?;
    fs::File::create_new(&path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|source| StorageError::WriteConfig { path, source })
}

pub(super) fn clear_completed_acquisition(
    root: &Path,
    descriptor: &ModuleDescriptor,
) -> Result<(), StorageError> {
    if library_program_acquisition_is_trusted(root, descriptor)? {
        let path = root.join(ACQUISITION);
        fs::remove_file(&path).map_err(|source| StorageError::DeletePath { path, source })?;
    }
    Ok(())
}
