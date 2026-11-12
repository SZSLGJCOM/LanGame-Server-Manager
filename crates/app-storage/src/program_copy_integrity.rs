use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use app_modules::ModuleDescriptor;

use super::verification::PackageVerifier;
use super::{CleanPackage, checked_relative, excluded, invalid, package_exclusions, read_manifest};
use crate::StorageError;
use crate::instance_creation_io::check_creation_cancelled;
use crate::instance_isolation::paths::normalize_resource_path;

/// Check the entire original copy allowlist before repairing a shared source.
/// Some(true) permits repair of missing files/directories only when every
/// surviving program file still matches. A changed registered Steam ACF also
/// needs provider verification, but is not an intentional program patch.
/// Existing changed program bytes are a conflict, never permission to replace
/// an external loader. None keeps legacy installations on their existing path.
/// This read-only check does not certify a copy or update either inventory.
pub fn program_copy_requires_repair(
    root: &Path,
    descriptor: &ModuleDescriptor,
    cancellation: Option<&AtomicBool>,
) -> Result<Option<bool>, StorageError> {
    check_creation_cancelled(cancellation)?;
    let Some(mut manifest) = read_manifest(root)? else {
        return Ok(None);
    };
    if manifest.module_id != descriptor.summary.id {
        return Err(invalid(root, "program inventory belongs to another module"));
    }
    let exclusions = package_exclusions(descriptor)?;
    let omit = |key: &str| excluded(key, &exclusions, descriptor.summary.id == "dontstarve");
    manifest.files.retain(|key, _| !omit(key));
    manifest.directories.retain(|key| !omit(key));
    let mut requires_repair = false;
    for key in &manifest.directories {
        check_creation_cancelled(cancellation)?;
        let path = root.join(checked_relative(key)?);
        normalize_resource_path(&path)?;
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(invalid(&path, "shared program directory has been replaced")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => requires_repair = true,
            Err(source) => return Err(StorageError::ReadPath { path, source }),
        }
    }
    let steam_record = descriptor
        .summary
        .steam_app_id
        .filter(|id| *id > 0)
        .map(|id| format!("steamapps/appmanifest_{id}.acf"));
    let mut verifier = PackageVerifier::new(root, true, cancellation);
    for (key, expected) in manifest.files {
        check_creation_cancelled(cancellation)?;
        let path = root.join(checked_relative(&key)?);
        normalize_resource_path(&path)?;
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Err(invalid(&path, "shared program file has been replaced")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                requires_repair = true;
                continue;
            }
            Err(source) => return Err(StorageError::ReadPath { path, source }),
        }
        let verified_files = manifest
            .verified_files
            .remove(&key)
            .map(|cached| BTreeMap::from([(key.clone(), cached)]))
            .unwrap_or_default();
        let sample = CleanPackage {
            version: manifest.version,
            module_id: manifest.module_id.clone(),
            source: manifest.source.clone(),
            archive_sha256: manifest.archive_sha256.clone(),
            files: BTreeMap::from([(key.clone(), expected)]),
            directories: BTreeSet::new(),
            verified_files,
        };
        if verifier
            .verify(Some(&descriptor.summary.id), Some(sample))?
            .is_none()
        {
            if steam_record
                .as_ref()
                .is_some_and(|record| record.eq_ignore_ascii_case(&key))
            {
                requires_repair = true;
            } else {
                return Err(invalid(
                    &path,
                    "shared program file differs from its verified original; modified source was preserved",
                ));
            }
        }
    }
    check_creation_cancelled(cancellation)?;
    Ok(Some(requires_repair))
}

#[cfg(test)]
#[path = "program_copy_integrity_tests.rs"]
mod tests;
