use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use app_modules::ModuleDescriptor;

use super::verification::{self, PackageVerifier};
use super::{
    CLEAN_PACKAGE, CleanPackage, INITIAL_PACKAGE, excluded, package_exclusions, validate_manifest,
    write_manifest, write_named_manifest,
};
use crate::StorageError;
use crate::instance_creation_io::check_creation_cancelled;
use crate::instance_isolation::paths::{normalize_path, normalize_resource_path};

/// Only the installer may assert this after acquiring into a new, empty directory,
/// or after official validation plus complete exact depot size/hash verification
/// establishes an equivalent tree with no non-package contents. Validation of an
/// old installation alone is not proof: it can leave unknown files behind.
pub fn record_library_program_baseline(
    root: &Path,
    descriptor: &ModuleDescriptor,
    source_is_clean: bool,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    if !source_is_clean {
        // An update has already changed this directory. Even a cancellation
        // arriving now must not leave the previous package's whitelist trusted.
        for name in [CLEAN_PACKAGE, INITIAL_PACKAGE] {
            let path = normalize_resource_path(&root.join(name))?;
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(StorageError::DeletePath { path, source }),
            }
        }
        return check_creation_cancelled(cancellation);
    }
    check_creation_cancelled(cancellation)?;
    let root = normalize_path(root)?;
    // Unknown roots have no marker; a present marker must belong to this exact
    // acquisition before the caller can finalize it as an official package.
    super::super::library_program_acquisition_is_trusted(&root, descriptor)?;
    let (package, verified_files) = verification::scan_verified_package(&root, cancellation)?;
    let initial = CleanPackage {
        version: 1,
        module_id: descriptor.summary.id.clone(),
        source: "official_clean_install".into(),
        files: package.files,
        directories: package.directories,
        verified_files,
    };
    publish_baseline(&root, descriptor, initial, cancellation)
}

/// Record only an allowlist established by exact official depot verification.
/// Unlike a fresh acquisition, an existing installation may contain operator
/// files: scanning that directory must never certify them as package contents.
pub fn record_verified_library_program_baseline(
    root: &Path,
    descriptor: &ModuleDescriptor,
    files: BTreeMap<String, String>,
    directories: BTreeSet<String>,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    check_creation_cancelled(cancellation)?;
    let root = normalize_path(root)?;
    super::super::library_program_acquisition_is_trusted(&root, descriptor)?;
    let initial = CleanPackage {
        version: 1,
        module_id: descriptor.summary.id.clone(),
        source: "official_clean_install".into(),
        files,
        directories,
        verified_files: BTreeMap::new(),
    };
    validate_manifest(&initial, &root)?;
    let initial = PackageVerifier::new(&root, false, cancellation)
        .verify(Some(&descriptor.summary.id), Some(initial))?
        .ok_or_else(|| StorageError::CleanLibraryProgramRequired { path: root.clone() })?;
    publish_baseline(&root, descriptor, initial, cancellation)
}

fn publish_baseline(
    root: &Path,
    descriptor: &ModuleDescriptor,
    initial: CleanPackage,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    let mut exclusions = package_exclusions(descriptor)?;
    // The runtime scanner intentionally recognizes only numeric Workshop
    // IDs. A clean seed must also omit manually named workshop-* directories.
    if descriptor.summary.id == "dontstarve" && root.join("mods").is_dir() {
        normalize_path(&root.join("mods"))?;
        for entry in
            fs::read_dir(root.join("mods")).map_err(|source| StorageError::ReadDirectory {
                path: root.join("mods"),
                source,
            })?
        {
            check_creation_cancelled(cancellation)?;
            let entry = entry.map_err(|source| StorageError::ReadDirectory {
                path: root.join("mods"),
                source,
            })?;
            if entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("workshop-")
            {
                exclusions.insert(format!("mods/{}", entry.file_name().to_string_lossy()));
            }
        }
    }
    // The full acquisition inventory proves that shipped default configuration
    // is original. The filtered manifest remains the only source for copies.
    validate_manifest(&initial, root)?;
    let manifest = CleanPackage {
        version: 1,
        module_id: descriptor.summary.id.clone(),
        source: "official_clean_install".into(),
        files: initial
            .files
            .iter()
            .filter(|(key, _)| !excluded(key, &exclusions, descriptor.summary.id == "dontstarve"))
            .map(|(key, hash)| (key.clone(), hash.clone()))
            .collect(),
        directories: initial
            .directories
            .iter()
            .filter(|key| !excluded(key, &exclusions, descriptor.summary.id == "dontstarve"))
            .cloned()
            .collect(),
        verified_files: initial
            .verified_files
            .iter()
            .filter(|(key, _)| !excluded(key, &exclusions, descriptor.summary.id == "dontstarve"))
            .map(|(key, cached)| (key.clone(), cached.clone()))
            .collect(),
    };
    validate_manifest(&manifest, root)?;
    check_creation_cancelled(cancellation)?;
    write_named_manifest(root, &initial, INITIAL_PACKAGE)?;
    write_manifest(root, &manifest)?;
    super::super::clear_completed_acquisition(root, descriptor)
}
