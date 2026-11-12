use super::*;
use crate::instance_creation_io::copy_creation_file;
use crate::instance_isolation::paths::contains;
use crate::private_runtime_refresh::{hash_file, relative_key};
use crate::program_library_cleanup::{
    MAX_TREE_ENTRIES, inspect_tree, library_cleanup_retained_paths,
};

/// Old manifests remain valid update inputs even without an archive digest.
/// The digest is evidence for skipping a later download, not an upgrade gate.
pub fn archive_program_has_baseline(
    root: &Path,
    descriptor: &ModuleDescriptor,
) -> Result<bool, StorageError> {
    if !descriptor.install.as_ref().is_some_and(|install| {
        install.download_url_windows.is_some() && install.download_integrity_windows.is_some()
    }) {
        return Ok(false);
    }
    let Some(manifest) = read_manifest(root)? else {
        return Ok(false);
    };
    // The next package can move its executable, as Terraria does with versioned
    // directories. The old inventory identifies old bytes, not the next layout.
    Ok(manifest.module_id == descriptor.summary.id)
}

/// Verify the current pinned official archive's program allowlist. User data
/// excluded by the module never becomes evidence for an original package.
pub fn archive_program_is_current(
    root: &Path,
    descriptor: &ModuleDescriptor,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    check_creation_cancelled(cancellation)?;
    let Some(expected) = descriptor
        .install
        .as_ref()
        .filter(|install| install.download_url_windows.is_some())
        .and_then(|install| install.download_integrity_windows.as_ref())
    else {
        return Ok(false);
    };
    let Some(manifest) = read_manifest(root)? else {
        return Ok(false);
    };
    if manifest.module_id != descriptor.summary.id
        || manifest.archive_sha256.as_deref() != Some(expected.sha256.as_str())
    {
        return Ok(false);
    }
    Ok(PackageVerifier::new(root, true, cancellation)
        .verify(Some(&descriptor.summary.id), Some(manifest))?
        .is_some())
}

/// Merge into an installer-owned staging tree, after its official inventory
/// has been recorded and before the existing directory publication transaction.
/// This operation never writes the source installation.
pub fn merge_archive_program_update(
    source: &Path,
    stage: &Path,
    descriptor: &ModuleDescriptor,
    protected: &[PathBuf],
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    check_creation_cancelled(cancellation)?;
    let source = normalize_path(source)?;
    let stage = normalize_path(stage)?;
    if contains(&source, &stage) || contains(&stage, &source) {
        return Err(invalid(
            &source,
            "archive update source and staging directories overlap",
        ));
    }
    if !archive_program_has_baseline(&source, descriptor)? {
        return Err(invalid(
            &source,
            "Archive update requires the original verified package inventory. Preserve this directory and install into a separate empty directory before migrating its saves and configuration.",
        ));
    }
    let original = read_clean_package_inventory(&source, descriptor)?.ok_or_else(|| {
        invalid(
            &source,
            "archive update has no verified original file inventory",
        )
    })?;
    let original_files = original
        .files
        .iter()
        .map(|(key, hash)| (key.to_ascii_lowercase(), hash))
        .collect::<BTreeMap<_, _>>();
    let identity = crate::program_removal_files::identity(&source, true)?;
    let package = require_clean_package_tree(&stage, &descriptor.summary.id, cancellation)?;
    let mandatory = package
        .files
        .iter()
        .map(|(key, hash)| (key.to_ascii_lowercase(), hash))
        .collect::<BTreeMap<_, _>>();
    let retained = retained_paths(&source, descriptor, protected)?;
    let protected = protected
        .iter()
        .map(|path| normalize_resource_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    let entries = inspect_tree(&source, MAX_TREE_ENTRIES)?;
    let original_entries = entries
        .iter()
        .map(|entry| (entry.key.clone(), entry.directory))
        .collect::<BTreeSet<_>>();
    inspect_tree(&stage, MAX_TREE_ENTRIES)?;
    let selected = entries
        .iter()
        .filter(|entry| retained.iter().any(|path| contains(path, &entry.path)))
        .collect::<Vec<_>>();
    let mut unchanged = BTreeMap::new();
    let mut repairs = BTreeMap::new();

    // Check the entire merge before changing even the disposable staging tree.
    for entry in &selected {
        check_creation_cancelled(cancellation)?;
        let key = relative_key(&entry.key)?.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            CLEAN_PACKAGE | INITIAL_PACKAGE | ".langame-program-acquisition.json"
        ) {
            return Err(invalid(
                &entry.path,
                "user data conflicts with the new package inventory",
            ));
        }
        if let Some(expected) = mandatory.get(&key) {
            if entry.directory {
                return Err(conflict(&entry.path));
            }
            let actual = hash_file(&entry.path, cancellation)?;
            if actual == expected.as_str() {
                unchanged.insert(entry.key.clone(), (**expected).clone());
            } else if let Some(original) = original_files.get(&key)
                && !protected.iter().any(|path| contains(path, &entry.path))
            {
                // The old filtered inventory proves this was program content.
                // Current exclusions and explicit instance data still take
                // precedence; unknown files never become repair candidates.
                repairs.insert(
                    entry.key.clone(),
                    ArchiveRepairFile {
                        original_sha256: (**original).clone(),
                        preserved_sha256: actual,
                        installed_sha256: (**expected).clone(),
                    },
                );
            } else {
                return Err(conflict(&entry.path));
            }
        } else if !entry.directory
            && mandatory
                .keys()
                .any(|path| path.starts_with(&(key.clone() + "/")))
        {
            return Err(conflict(&entry.path));
        }
    }

    let mut copied = preserve_repaired_files(&source, &stage, descriptor, &repairs, cancellation)?;
    for entry in selected {
        check_creation_cancelled(cancellation)?;
        normalize_resource_path(&entry.path)?;
        if repairs.contains_key(&entry.key) {
            // The verified new file is already staged. Its displaced bytes
            // travel with the same publication transaction in the recovery tree.
            continue;
        }
        let target = normalize_resource_path(&stage.join(&entry.key))?;
        if let Some(digest) = unchanged.get(&entry.key) {
            copied.push((entry.path.clone(), digest.clone()));
            continue;
        }
        if target.exists() {
            if entry.directory && target.is_dir() {
                continue;
            }
            if target.is_dir() {
                fs::remove_dir_all(&target).map_err(|source| StorageError::DeletePath {
                    path: target.clone(),
                    source,
                })?;
            } else {
                fs::remove_file(&target).map_err(|source| StorageError::DeletePath {
                    path: target.clone(),
                    source,
                })?;
            }
        }
        if entry.directory {
            fs::create_dir_all(&target).map_err(|source| StorageError::CreatePath {
                path: target,
                source,
            })?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
                    path: parent.to_owned(),
                    source,
                })?;
            }
            let digest = copy_creation_file(&entry.path, &target, cancellation)?;
            copied.push((entry.path.clone(), digest));
        }
    }
    for (path, digest) in copied {
        check_creation_cancelled(cancellation)?;
        normalize_resource_path(&path)?;
        if hash_file(&path, cancellation)? != digest {
            return Err(invalid(
                &path,
                "retained data changed during archive update; original files were preserved",
            ));
        }
    }
    let current_entries = inspect_tree(&source, MAX_TREE_ENTRIES)?
        .into_iter()
        .map(|entry| (entry.key, entry.directory))
        .collect::<BTreeSet<_>>();
    let current_original = read_clean_package_inventory(&source, descriptor)?;
    if original_entries != current_entries
        || identity.is_none()
        || crate::program_removal_files::identity(&source, true)? != identity
        || retained_paths(&source, descriptor, &protected)? != retained
        || current_original.is_none_or(|current| {
            current.files != original.files || current.directories != original.directories
        })
    {
        return Err(invalid(
            &source,
            "installation ownership or contents changed during archive update",
        ));
    }
    if descriptor.summary.id == "rimworld" {
        crate::templates::validate_rimworld_market_archive_update(&stage)?;
    }
    check_creation_cancelled(cancellation)
}

#[derive(Serialize)]
struct ArchiveRepairFile {
    original_sha256: String,
    preserved_sha256: String,
    installed_sha256: String,
}

fn preserve_repaired_files(
    source: &Path,
    stage: &Path,
    descriptor: &ModuleDescriptor,
    repairs: &BTreeMap<PathBuf, ArchiveRepairFile>,
    cancellation: Option<&AtomicBool>,
) -> Result<Vec<(PathBuf, String)>, StorageError> {
    if repairs.is_empty() {
        return Ok(Vec::new());
    }
    check_creation_cancelled(cancellation)?;
    // This tree is deliberately outside the clean inventory. Future updates
    // and removal retain it as non-package data, including its recovery receipt.
    let recovery = stage.join(format!(
        ".langame-archive-recovery-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&recovery).map_err(|source| StorageError::CreatePath {
        path: recovery.clone(),
        source,
    })?;
    let mut copied = Vec::new();
    let mut files = BTreeMap::new();
    for (relative, repair) in repairs {
        check_creation_cancelled(cancellation)?;
        let original = normalize_resource_path(&source.join(relative))?;
        let target = normalize_resource_path(&recovery.join("files").join(relative))?;
        let parent = target
            .parent()
            .ok_or_else(|| invalid(&target, "missing recovery parent"))?;
        fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
            path: parent.to_owned(),
            source,
        })?;
        let digest = copy_creation_file(&original, &target, cancellation)?;
        if digest != repair.preserved_sha256 || hash_file(&target, cancellation)? != digest {
            return Err(invalid(
                &original,
                "program file changed while its recovery copy was prepared; original files were preserved",
            ));
        }
        copied.push((original, digest));
        files.insert(relative_key(relative)?, repair);
    }
    let receipt = serde_json::json!({
        "version": 1,
        "module_id": descriptor.summary.id,
        "source_root": source,
        "archive_sha256": descriptor.install.as_ref()
            .and_then(|install| install.download_integrity_windows.as_ref())
            .map(|integrity| &integrity.sha256),
        "files": files,
    });
    let path = recovery.join("recovery.json");
    let bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| invalid(&path, format!("invalid archive recovery receipt: {error}")))?;
    crate::atomic_file::create_file_atomically(&path, &bytes)
        .map_err(|source| StorageError::WriteConfig { path, source })?;
    Ok(copied)
}

fn retained_paths(
    source: &Path,
    descriptor: &ModuleDescriptor,
    protected: &[PathBuf],
) -> Result<BTreeSet<PathBuf>, StorageError> {
    let mut retained = library_cleanup_retained_paths(source, descriptor)?.ok_or_else(|| {
        invalid(
            source,
            "archive update has no verified original file inventory",
        )
    })?;
    retained.extend(crate::program_runtime::program_update_metadata_paths(
        source,
        &descriptor.summary.id,
    )?);
    for path in protected {
        let path = normalize_resource_path(path)?;
        if !contains(source, &path) || path == source {
            return Err(invalid(
                &path,
                "archive update cannot distinguish protected data from the program root",
            ));
        }
        if path.exists() {
            retained.push(path);
        }
    }
    Ok(retained.into_iter().collect())
}

fn conflict(path: &Path) -> StorageError {
    invalid(
        path,
        "A user-modified file conflicts with a required file in the new server package. Original files were preserved; move the conflicting customization aside before updating.",
    )
}
