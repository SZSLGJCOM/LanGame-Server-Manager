use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::verification::PackageVerifier;
use super::{CLEAN_PACKAGE, INITIAL_PACKAGE, read_manifest, read_named_manifest};
use crate::StorageError;
use crate::instance_creation_io::check_creation_cancelled;
use crate::instance_isolation::paths::normalize_resource_path;
use crate::private_runtime_refresh::validated_relative_path;
use crate::program_runtime::invalid;

/// A retired library can keep empty directories and omit deleted defaults, but
/// must not carry any old settings, saves, Mods or unrecognized metadata forward.
/// Previews verify only surviving defaults; program bytes are checked at creation.
pub(crate) fn retired_library_is_clean(
    root: &Path,
    module_id: &str,
    verify_program: bool,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    check_creation_cancelled(cancellation)?;
    let Some(clean) = read_manifest(root)?.filter(|manifest| manifest.module_id == module_id)
    else {
        return Ok(false);
    };
    let initial = read_named_manifest(root, INITIAL_PACKAGE)?;
    if initial.as_ref().is_some_and(|initial| {
        initial.module_id != module_id
            || clean
                .files
                .iter()
                .any(|(key, hash)| initial.files.get(key) != Some(hash))
    }) {
        return Err(invalid(
            root,
            "initial and clean program inventories disagree",
        ));
    }
    let files = initial
        .as_ref()
        .unwrap_or(&clean)
        .files
        .keys()
        .map(|key| fold(key))
        .collect::<BTreeSet<_>>();
    if !only_package_files(root, &files, cancellation)? {
        return Ok(false);
    }
    let mut verifier = PackageVerifier::new(root, true, cancellation);
    if let Some(mut defaults) = initial {
        // Native configuration cleanup can remove shipped defaults. Only the
        // filtered clean manifest defines mandatory executable/runtime files.
        defaults
            .files
            .retain(|key, _| !clean.files.contains_key(key));
        let mut present = BTreeSet::new();
        for key in defaults.files.keys() {
            check_creation_cancelled(cancellation)?;
            let path = root.join(key);
            normalize_resource_path(&path)?;
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    present.insert(key.clone());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(StorageError::ReadPath { path, source }),
            }
        }
        defaults.files.retain(|key, _| present.contains(key));
        // All existing directory entries were checked without following links.
        // Missing directories outside the mandatory program tree are harmless.
        defaults.directories.clear();
        if verifier.verify(Some(module_id), Some(defaults))?.is_none() {
            return Ok(false);
        }
    }
    if verify_program {
        let Some(clean) = verifier.verify(Some(module_id), Some(clean))? else {
            return Ok(false);
        };
        verifier.persist_verification(&clean, CLEAN_PACKAGE)?;
    }
    Ok(true)
}

fn fold(value: &str) -> String {
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value.to_owned()
    }
}

fn only_package_files(
    root: &Path,
    files: &BTreeSet<String>,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    let mut pending = vec![PathBuf::new()];
    let mut visited = 0_usize;
    while let Some(relative) = pending.pop() {
        check_creation_cancelled(cancellation)?;
        let directory = root.join(&relative);
        for entry in fs::read_dir(&directory).map_err(|source| StorageError::ReadDirectory {
            path: directory.clone(),
            source,
        })? {
            check_creation_cancelled(cancellation)?;
            visited += 1;
            if visited > 1_000_000 {
                return Err(invalid(
                    root,
                    "program inventory exceeds its safe entry limit",
                ));
            }
            let entry = entry.map_err(|source| StorageError::ReadDirectory {
                path: directory.clone(),
                source,
            })?;
            let relative = relative.join(entry.file_name());
            let key = relative
                .to_str()
                .ok_or_else(|| invalid(&relative, "program path is not UTF-8"))?
                .replace('\\', "/");
            validated_relative_path(&key)?;
            normalize_resource_path(&entry.path())?;
            let metadata = entry.metadata().map_err(|source| StorageError::ReadPath {
                path: entry.path(),
                source,
            })?;
            let key = fold(&key);
            let managed = matches!(
                key.as_str(),
                ".langame-clean-package.json"
                    | ".langame-initial-package.json"
                    | ".langame-program-identity.json"
                    | ".langame-program-usage.json"
            );
            if metadata.is_dir() && !key.split('/').any(|part| part.starts_with(".langame-")) {
                // Deletion owns files, not arbitrary directories. Traversing
                // empty ordinary directories does not import any instance data.
                pending.push(relative);
            } else if !metadata.is_file() || (!managed && !files.contains(&key)) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
