use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::StorageError;
use crate::atomic_file::write_file_atomically;
use crate::instance_creation_io::check_creation_cancelled;
use crate::private_runtime::{PRIVATE_RUNTIME_MARKER, is_reparse_point};

const BASELINE_FILE: &str = ".langame-package-baseline.json";
const REFRESH_MARKER: &str = ".langame-refresh-staging";
const STAGING_NAME: &str = "runtime.refresh-staging";
const ROLLBACK_NAME: &str = "runtime.refresh-rollback";
const BASELINE_VERSION: u32 = 2;
const MAX_CONFLICT_PATHS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateRuntimeRefresh {
    Current,
    Refreshed,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PackageBaseline {
    pub(crate) version: u32,
    pub(crate) source_root: String,
    pub(crate) source_generation: Option<String>,
    pub(crate) excluded_paths: Vec<String>,
    pub(crate) exclude_dst_workshop_mods: bool,
    pub(crate) files: BTreeMap<String, String>,
    pub(crate) directories: BTreeSet<String>,
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PackageTree {
    pub(crate) files: BTreeMap<String, String>,
    pub(crate) directories: BTreeSet<String>,
}

impl PackageTree {
    pub(crate) fn copied_file(
        &mut self,
        relative: &Path,
        digest: String,
    ) -> Result<(), StorageError> {
        self.files.insert(relative_key(relative)?, digest);
        Ok(())
    }

    pub(crate) fn copied_directory(&mut self, relative: &Path) -> Result<(), StorageError> {
        self.directories.insert(relative_key(relative)?);
        Ok(())
    }

    /// Projection may create missing ancestors using configured path spelling.
    /// Reuse the spelling of existing package directories on Windows, while
    /// copied_directory still records real source collisions for validation.
    pub(crate) fn created_directory(&mut self, relative: &Path) -> Result<(), StorageError> {
        let key = relative_key(relative)?;
        #[cfg(windows)]
        {
            let mut prefix = String::new();
            for component in key.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(component);
                if let Some(existing) = self
                    .directories
                    .iter()
                    .find(|known| path_key_eq(known, &prefix))
                {
                    prefix = existing.clone();
                } else {
                    self.directories.insert(prefix.clone());
                }
            }
        }
        #[cfg(not(windows))]
        self.directories.insert(key);
        Ok(())
    }
}

/// Called while the shared install lifecycle lock is held, after the package
/// has been copied and before templates create instance-owned files.
#[cfg(test)]
pub(crate) fn record_package_baseline(
    shared_root: &Path,
    runtime_root: &Path,
    excluded_paths: &[PathBuf],
    source_generation: Option<&str>,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    record_package_baseline_with_rules(
        shared_root,
        runtime_root,
        excluded_paths,
        source_generation,
        false,
        cancellation,
    )
}

#[cfg(test)]
pub(crate) fn record_package_baseline_with_rules(
    shared_root: &Path,
    runtime_root: &Path,
    excluded_paths: &[PathBuf],
    source_generation: Option<&str>,
    exclude_dst_workshop_mods: bool,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    check_creation_cancelled(cancellation)?;
    validate_directory_root(shared_root)?;
    validate_directory_root(runtime_root)?;
    let excluded_paths = relative_exclusions(shared_root, excluded_paths)?;
    let package = scan_package(
        runtime_root,
        &excluded_paths,
        exclude_dst_workshop_mods,
        cancellation,
    )?;
    write_package_baseline(
        shared_root,
        runtime_root,
        excluded_paths,
        source_generation,
        exclude_dst_workshop_mods,
        package,
        cancellation,
    )
}

/// Creation already inspected and hashed each file while writing its private copy.
/// Preserve the same baseline format and exclusions without another payload read.
pub(crate) fn record_copied_package_baseline(
    shared_root: &Path,
    runtime_root: &Path,
    excluded_paths: &[PathBuf],
    source_generation: Option<&str>,
    exclude_dst_workshop_mods: bool,
    mut package: PackageTree,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    check_creation_cancelled(cancellation)?;
    validate_directory_root(shared_root)?;
    validate_directory_root(runtime_root)?;
    let excluded_paths = relative_exclusions(shared_root, excluded_paths)?;
    package
        .files
        .retain(|key, _| !excluded_package_path(key, &excluded_paths, exclude_dst_workshop_mods));
    package
        .directories
        .retain(|key| !excluded_package_path(key, &excluded_paths, exclude_dst_workshop_mods));
    #[cfg(windows)]
    validate_case_unique_path_keys(
        package.files.keys().chain(package.directories.iter()),
        runtime_root,
    )?;
    write_package_baseline(
        shared_root,
        runtime_root,
        excluded_paths,
        source_generation,
        exclude_dst_workshop_mods,
        package,
        cancellation,
    )
}

fn write_package_baseline(
    shared_root: &Path,
    runtime_root: &Path,
    excluded_paths: BTreeSet<String>,
    source_generation: Option<&str>,
    exclude_dst_workshop_mods: bool,
    package: PackageTree,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    let baseline = PackageBaseline {
        version: BASELINE_VERSION,
        source_root: source_root_identity(shared_root)?,
        source_generation: source_generation.map(str::to_owned),
        excluded_paths: excluded_paths.into_iter().collect(),
        exclude_dst_workshop_mods,
        files: package.files,
        directories: package.directories,
    };
    check_creation_cancelled(cancellation)?;
    write_baseline(runtime_root, &baseline)
}

/// Refresh a stopped, explicitly marked projection from its shared package.
/// Independent programs require their own installer; they are never refreshed
/// from another directory. The caller owns both mutation and install locks.
pub fn refresh_private_runtime(
    shared_root: &Path,
    instance_root: &Path,
    source_generation: Option<&str>,
) -> Result<PrivateRuntimeRefresh, StorageError> {
    let runtime_root = instance_root.join("runtime");
    let staging_root = instance_root.join(STAGING_NAME);
    let rollback_root = instance_root.join(ROLLBACK_NAME);
    crate::private_runtime::validate_projection_refresh_roots(shared_root, instance_root)?;
    recover_interrupted_refresh(&runtime_root, &staging_root, &rollback_root)?;
    crate::private_runtime::resolve_instance_private_runtime_root(instance_root)?;
    validate_directory_root(shared_root)?;
    let mut baseline = read_baseline(&runtime_root)?;
    if baseline.source_root != source_root_identity(shared_root)? {
        return Err(StorageError::PrivateRuntimeRefresh {
            path: runtime_root,
            message: String::from(
                "shared package root changed; this instance needs an explicit package reconciliation",
            ),
        });
    }
    if source_generation.is_some() && baseline.source_generation.as_deref() == source_generation {
        return Ok(PrivateRuntimeRefresh::Current);
    }

    validate_directory_root(shared_root)?;
    let excluded = validate_exclusions(&baseline.excluded_paths)?;
    let new_package = scan_package(
        shared_root,
        &excluded,
        baseline.exclude_dst_workshop_mods,
        None,
    )?;
    if new_package.files == baseline.files && new_package.directories == baseline.directories {
        baseline.source_generation = source_generation.map(str::to_owned);
        write_baseline(&runtime_root, &baseline)?;
        return Ok(PrivateRuntimeRefresh::Current);
    }

    let local_package = scan_package(&runtime_root, &BTreeSet::new(), false, None)?;
    let new_files = &new_package.files;
    let local_files = &local_package.files;
    let mut conflicts = Vec::new();
    let mut directory_additions = Vec::new();
    for relative in new_package.directories.difference(&baseline.directories) {
        if local_package.directories.contains(relative) || local_files.contains_key(relative) {
            conflicts.push(relative.clone());
        } else {
            directory_additions.push(relative.clone());
        }
    }
    let mut replacements = Vec::new();
    let mut removals = Vec::new();
    let all_paths = baseline
        .files
        .keys()
        .chain(new_files.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for relative in all_paths {
        let old = baseline.files.get(&relative);
        let new = new_files.get(&relative);
        let local = local_files.get(&relative);
        match (old, new) {
            (Some(old), Some(new)) if old != new => {
                if local == Some(old) {
                    replacements.push(relative);
                } else if local != Some(new) {
                    conflicts.push(relative);
                }
            }
            (Some(old), None) => {
                if local == Some(old) {
                    removals.push(relative);
                } else if local.is_some() {
                    conflicts.push(relative);
                }
            }
            (None, Some(_new)) => {
                if local_package.directories.contains(&relative) {
                    conflicts.push(relative);
                } else if local.is_none() {
                    replacements.push(relative);
                } else {
                    // Equal bytes do not transfer ownership of an instance file
                    // to the package: a later package removal must not delete it.
                    conflicts.push(relative);
                }
            }
            _ => {}
        }
    }
    if !conflicts.is_empty() {
        let count = conflicts.len();
        let examples = conflicts
            .into_iter()
            .take(MAX_CONFLICT_PATHS)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(StorageError::PrivateRuntimeRefresh {
            path: runtime_root,
            message: format!(
                "{count} package paths changed in both the shared install and this instance ({examples}); the existing runtime was preserved"
            ),
        });
    }

    let mut expected_files = local_package.files.clone();
    let mut expected_directories = local_package.directories.clone();
    expected_directories.extend(directory_additions.iter().cloned());
    for relative in &removals {
        expected_files.remove(relative);
    }
    for relative in &replacements {
        expected_files.insert(relative.clone(), new_files[relative].clone());
    }
    copy_directory(&runtime_root, &staging_root)?;
    let staged = (|| {
        for relative in directory_additions {
            let path = staging_root.join(validated_relative_path(&relative)?);
            fs::create_dir_all(&path)
                .map_err(|source| StorageError::CreatePath { path, source })?;
        }
        for relative in removals {
            let path = staging_root.join(validated_relative_path(&relative)?);
            fs::remove_file(&path).map_err(|source| StorageError::DeletePath { path, source })?;
        }
        for relative in replacements {
            let relative_path = validated_relative_path(&relative)?;
            let source = shared_root.join(&relative_path);
            let destination = staging_root.join(&relative_path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::copy(&source, &destination).map_err(|source_error| StorageError::CopyPath {
                from: source,
                to: destination,
                source: source_error,
            })?;
        }
        baseline.files = new_package.files;
        baseline.directories = new_package.directories;
        baseline.source_generation = source_generation.map(str::to_owned);
        write_baseline(&staging_root, &baseline)?;
        verify_staged_runtime(&staging_root, &expected_files, &expected_directories)?;
        Ok::<_, StorageError>(())
    })();
    if let Err(error) = staged {
        let _ = remove_owned_staging(&staging_root);
        return Err(error);
    }

    fs::rename(&runtime_root, &rollback_root).map_err(|source| StorageError::MovePath {
        from: runtime_root.clone(),
        to: rollback_root.clone(),
        source,
    })?;
    if let Err(source) = fs::rename(&staging_root, &runtime_root) {
        let restore = fs::rename(&rollback_root, &runtime_root);
        return Err(StorageError::PrivateRuntimeRefresh {
            path: runtime_root,
            message: match restore {
                Ok(()) => {
                    format!("failed to publish refreshed runtime: {source}; old runtime restored")
                }
                Err(restore_error) => format!(
                    "failed to publish refreshed runtime: {source}; recovery from {} also failed: {restore_error}",
                    rollback_root.display()
                ),
            },
        });
    }
    let _ = fs::remove_file(runtime_root.join(REFRESH_MARKER));
    // A failed cleanup leaves an owned rollback for the next startup to finish.
    let _ = fs::remove_dir_all(&rollback_root);
    Ok(PrivateRuntimeRefresh::Refreshed)
}

/// This recovery path preserves staging and never rebuilds instance-owned files.
pub(crate) fn restore_private_runtime_rollback(
    runtime_root: &Path,
    rollback_root: &Path,
) -> Result<(), StorageError> {
    ensure_private_runtime(rollback_root)?;
    read_baseline(rollback_root)?;
    match fs::symlink_metadata(runtime_root) {
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: runtime_root.to_path_buf(),
                source,
            });
        }
        Ok(_) => {
            return Err(StorageError::PrivateRuntimeRefresh {
                path: runtime_root.to_path_buf(),
                message: String::from(
                    "runtime appeared during recovery; existing directories were preserved",
                ),
            });
        }
    }
    fs::rename(rollback_root, runtime_root).map_err(|source| StorageError::MovePath {
        from: rollback_root.to_path_buf(),
        to: runtime_root.to_path_buf(),
        source,
    })
}

fn recover_interrupted_refresh(
    runtime_root: &Path,
    staging_root: &Path,
    rollback_root: &Path,
) -> Result<(), StorageError> {
    if rollback_root.exists() {
        ensure_private_runtime(rollback_root)?;
        if !runtime_root.exists() {
            fs::rename(rollback_root, runtime_root).map_err(|source| StorageError::MovePath {
                from: rollback_root.to_path_buf(),
                to: runtime_root.to_path_buf(),
                source,
            })?;
        } else {
            ensure_private_runtime(runtime_root)?;
            read_baseline(runtime_root)?;
            fs::remove_dir_all(rollback_root).map_err(|source| StorageError::DeletePath {
                path: rollback_root.to_path_buf(),
                source,
            })?;
        }
    }
    if staging_root.exists() {
        remove_owned_staging(staging_root)?;
    }
    Ok(())
}

fn remove_owned_staging(path: &Path) -> Result<(), StorageError> {
    if !path.join(REFRESH_MARKER).is_file() {
        return Err(StorageError::PrivateRuntimeRefresh {
            path: path.to_path_buf(),
            message: String::from("unrecognized refresh staging directory was preserved"),
        });
    }
    fs::remove_dir_all(path).map_err(|source| StorageError::DeletePath {
        path: path.to_path_buf(),
        source,
    })
}

fn verify_staged_runtime(
    staged_root: &Path,
    expected_files: &BTreeMap<String, String>,
    expected_directories: &BTreeSet<String>,
) -> Result<(), StorageError> {
    let staged = scan_package(staged_root, &BTreeSet::new(), false, None)?;
    if staged.files == *expected_files && staged.directories == *expected_directories {
        return Ok(());
    }
    let differing = expected_files
        .keys()
        .chain(staged.files.keys())
        .find(|relative| expected_files.get(*relative) != staged.files.get(*relative))
        .or_else(|| {
            expected_directories
                .symmetric_difference(&staged.directories)
                .next()
        })
        .cloned()
        .unwrap_or_else(|| String::from("unknown"));
    Err(StorageError::PrivateRuntimeRefresh {
        path: staged_root.join(validated_relative_path(&differing)?),
        message: String::from("staged runtime failed package readback verification"),
    })
}
pub(crate) fn read_baseline(runtime_root: &Path) -> Result<PackageBaseline, StorageError> {
    let path = runtime_root.join(BASELINE_FILE);
    let bytes = fs::read(&path).map_err(|source| StorageError::PrivateRuntimeRefresh {
        path: path.clone(),
        message: format!(
            "package baseline is missing or unreadable: {source}; existing runtime was preserved"
        ),
    })?;
    let baseline: PackageBaseline =
        serde_json::from_slice(&bytes).map_err(|source| StorageError::PrivateRuntimeRefresh {
            path: path.clone(),
            message: format!(
                "package baseline is invalid: {source}; existing runtime was preserved"
            ),
        })?;
    if baseline.version != BASELINE_VERSION {
        return Err(StorageError::PrivateRuntimeRefresh {
            path,
            message: String::from(
                "package baseline version is unsupported; existing runtime was preserved",
            ),
        });
    }
    for relative in baseline.files.keys().chain(baseline.directories.iter()) {
        validated_relative_path(relative)?;
    }
    validate_exclusions(&baseline.excluded_paths)?;
    #[cfg(windows)]
    validate_case_unique_path_keys(
        baseline.files.keys().chain(baseline.directories.iter()),
        runtime_root,
    )?;
    Ok(baseline)
}

fn write_baseline(runtime_root: &Path, baseline: &PackageBaseline) -> Result<(), StorageError> {
    let path = runtime_root.join(BASELINE_FILE);
    let content = serde_json::to_vec(baseline)?;
    write_file_atomically(&path, &content)
        .map_err(|source| StorageError::WriteConfig { path, source })
}

pub(crate) fn scan_package(
    root: &Path,
    excluded: &BTreeSet<String>,
    exclude_dst_workshop_mods: bool,
    cancellation: Option<&AtomicBool>,
) -> Result<PackageTree, StorageError> {
    scan_package_with(
        root,
        excluded,
        exclude_dst_workshop_mods,
        cancellation,
        |_, path| hash_file(path, cancellation),
    )
}

pub(crate) fn scan_package_with(
    root: &Path,
    excluded: &BTreeSet<String>,
    exclude_dst_workshop_mods: bool,
    cancellation: Option<&AtomicBool>,
    mut hash: impl FnMut(&str, &Path) -> Result<String, StorageError>,
) -> Result<PackageTree, StorageError> {
    let mut package = PackageTree::default();
    let mut visit_file = |key: String, path: &Path| {
        let digest = hash(&key, path)?;
        package.files.insert(key, digest);
        Ok(())
    };
    scan_directory(
        root,
        root,
        excluded,
        exclude_dst_workshop_mods,
        &mut package.directories,
        &mut visit_file,
        cancellation,
    )?;
    #[cfg(windows)]
    validate_case_unique_path_keys(package.files.keys().chain(package.directories.iter()), root)?;
    Ok(package)
}

/// Local ownership transfer needs path safety, not a content baseline for the
/// separate projection refresh workflow. Reuse the same traversal and checks.
pub(crate) fn validate_package_paths(
    root: &Path,
    excluded: &BTreeSet<String>,
    exclude_dst_workshop_mods: bool,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::new();
    scan_directory(
        root,
        root,
        excluded,
        exclude_dst_workshop_mods,
        &mut directories,
        &mut |key, _| {
            files.insert(key);
            Ok(())
        },
        cancellation,
    )?;
    #[cfg(windows)]
    validate_case_unique_path_keys(files.iter().chain(directories.iter()), root)?;
    Ok(())
}

fn scan_directory(
    root: &Path,
    directory: &Path,
    excluded: &BTreeSet<String>,
    exclude_dst_workshop_mods: bool,
    directories: &mut BTreeSet<String>,
    visit_file: &mut impl FnMut(String, &Path) -> Result<(), StorageError>,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    check_creation_cancelled(cancellation)?;
    for entry in fs::read_dir(directory).map_err(|source| StorageError::ReadDirectory {
        path: directory.to_path_buf(),
        source,
    })? {
        check_creation_cancelled(cancellation)?;
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| StorageError::UnsafeManagedPath {
                path: path.clone(),
                root: root.to_path_buf(),
            })?;
        let key = relative_key(relative)?;
        if excluded_package_path(&key, excluded, exclude_dst_workshop_mods) {
            continue;
        }
        let kind = entry.file_type().map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
        if kind.is_symlink() || is_reparse_point(&path)? {
            return Err(StorageError::UnsafeManagedPath {
                path,
                root: root.to_path_buf(),
            });
        }
        if kind.is_dir() {
            directories.insert(key);
            scan_directory(
                root,
                &path,
                excluded,
                exclude_dst_workshop_mods,
                directories,
                visit_file,
                cancellation,
            )?;
        } else if kind.is_file() {
            visit_file(key, &path)?;
        }
        #[cfg(test)]
        if let Some(cancellation) = cancellation {
            crate::instance_creation_io::test_gate::pause_if_registered(
                cancellation,
                crate::instance_creation_io::test_gate::PausePoint::Inspect,
            );
        }
    }
    Ok(())
}

pub(crate) fn excluded_package_path(
    key: &str,
    excluded: &BTreeSet<String>,
    exclude_dst_workshop_mods: bool,
) -> bool {
    key.split('/')
        .next()
        .is_some_and(|name| name.to_ascii_lowercase().starts_with(".langame-"))
        || [PRIVATE_RUNTIME_MARKER, BASELINE_FILE, REFRESH_MARKER]
            .iter()
            .any(|name| path_matches_or_descends(key, name))
        || excluded
            .iter()
            .any(|excluded| path_matches_or_descends(key, excluded))
        || (exclude_dst_workshop_mods && is_dst_workshop_mod_path(key))
}

fn path_key_eq(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn path_matches_or_descends(key: &str, excluded: &str) -> bool {
    path_key_eq(key, excluded)
        || (key
            .get(..excluded.len())
            .is_some_and(|prefix| path_key_eq(prefix, excluded))
            && key.as_bytes().get(excluded.len()) == Some(&b'/'))
}

fn is_dst_workshop_mod_path(key: &str) -> bool {
    let mut components = key.split('/');
    let (Some(mods), Some(workshop_dir)) = (components.next(), components.next()) else {
        return false;
    };
    let prefix = "workshop-";
    let (Some(actual_prefix), Some(id)) = (
        workshop_dir.get(..prefix.len()),
        workshop_dir.get(prefix.len()..),
    ) else {
        return false;
    };
    path_key_eq(mods, "mods")
        && path_key_eq(actual_prefix, prefix)
        && !id.is_empty()
        && id.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(windows)]
pub(crate) fn validate_case_unique_path_keys<'a>(
    keys: impl Iterator<Item = &'a String>,
    root: &Path,
) -> Result<(), StorageError> {
    let mut folded = BTreeMap::new();
    for path in keys {
        let mut prefix = String::new();
        for component in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if let Some(previous) = folded.insert(prefix.to_ascii_lowercase(), prefix.clone())
                && previous != prefix
            {
                return Err(StorageError::PrivateRuntimeRefresh {
                    path: root.to_path_buf(),
                    message: format!(
                        "package paths differ only by ASCII case: {previous}, {prefix}"
                    ),
                });
            }
        }
    }
    Ok(())
}
pub(crate) fn hash_file(
    path: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<String, StorageError> {
    check_creation_cancelled(cancellation)?;
    let mut file = fs::File::open(path).map_err(|source| StorageError::ReadPath {
        path: path.to_path_buf(),
        source,
    })?;
    hash_open_file(&mut file, path, cancellation)
}

pub(crate) fn hash_open_file(
    file: &mut fs::File,
    path: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<String, StorageError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 256 * 1024];
    loop {
        check_creation_cancelled(cancellation)?;
        let count = file
            .read(&mut buffer)
            .map_err(|source| StorageError::ReadPath {
                path: path.to_path_buf(),
                source,
            })?;
        if count == 0 {
            break;
        }
        #[cfg(test)]
        crate::instance_archive::read_probe::record(path, count as u64);
        hasher.update(&buffer[..count]);
        #[cfg(test)]
        if let Some(cancellation) = cancellation {
            crate::instance_creation_io::test_gate::pause_if_registered(
                cancellation,
                crate::instance_creation_io::test_gate::PausePoint::Hash,
            );
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), StorageError> {
    if destination.exists() {
        return Err(StorageError::PrivateRuntimeRefresh {
            path: destination.to_path_buf(),
            message: String::from("refresh staging directory already exists"),
        });
    }
    fs::create_dir(destination).map_err(|source| StorageError::CreatePath {
        path: destination.to_path_buf(),
        source,
    })?;
    let identity = crate::private_runtime::PROJECTION_RUNTIME_MARKER;
    fs::copy(source.join(identity), destination.join(identity)).map_err(|source_error| {
        StorageError::CopyPath {
            from: source.join(identity),
            to: destination.join(identity),
            source: source_error,
        }
    })?;
    fs::write(destination.join(REFRESH_MARKER), b"managed\n").map_err(|source| {
        StorageError::WriteConfig {
            path: destination.join(REFRESH_MARKER),
            source,
        }
    })?;
    let result = copy_directory_contents(source, destination);
    if result.is_err() {
        let _ = remove_owned_staging(destination);
    }
    result
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<(), StorageError> {
    for entry in fs::read_dir(source).map_err(|source_error| StorageError::ReadDirectory {
        path: source.to_path_buf(),
        source: source_error,
    })? {
        let entry = entry.map_err(|source_error| StorageError::ReadDirectory {
            path: source.to_path_buf(),
            source: source_error,
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|source_error| StorageError::ReadPath {
                path: source_path.clone(),
                source: source_error,
            })?;
        if kind.is_symlink() || is_reparse_point(&source_path)? {
            return Err(StorageError::UnsafeManagedPath {
                path: source_path,
                root: source.to_path_buf(),
            });
        }
        if kind.is_dir() {
            fs::create_dir(&destination_path).map_err(|source| StorageError::CreatePath {
                path: destination_path.clone(),
                source,
            })?;
            copy_directory_contents(&source_path, &destination_path)?;
        } else if kind.is_file() {
            fs::copy(&source_path, &destination_path).map_err(|source_error| {
                StorageError::CopyPath {
                    from: source_path,
                    to: destination_path,
                    source: source_error,
                }
            })?;
        }
    }
    Ok(())
}

pub(crate) fn relative_exclusions(
    shared_root: &Path,
    paths: &[PathBuf],
) -> Result<BTreeSet<String>, StorageError> {
    paths
        .iter()
        .map(|path| {
            let relative =
                path.strip_prefix(shared_root)
                    .map_err(|_| StorageError::UnsafeManagedPath {
                        path: path.clone(),
                        root: shared_root.to_path_buf(),
                    })?;
            relative_key(relative)
        })
        .collect()
}

fn validate_exclusions(paths: &[String]) -> Result<BTreeSet<String>, StorageError> {
    paths
        .iter()
        .map(|path| {
            validated_relative_path(path)?;
            Ok(path.clone())
        })
        .collect()
}

pub(crate) fn relative_key(path: &Path) -> Result<String, StorageError> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(StorageError::PrivateRuntimeRefresh {
            path: path.to_path_buf(),
            message: String::from("package path must be relative with normal components"),
        });
    }
    let mut parts = Vec::new();
    for part in path.components() {
        let text =
            part.as_os_str()
                .to_str()
                .ok_or_else(|| StorageError::PrivateRuntimeRefresh {
                    path: path.to_path_buf(),
                    message: String::from("package path is not UTF-8"),
                })?;
        parts.push(text.to_owned());
    }
    Ok(parts.join("/"))
}

pub(crate) fn validated_relative_path(value: &str) -> Result<PathBuf, StorageError> {
    if value.is_empty() || value.contains('\\') || value.contains(':') {
        return Err(StorageError::PrivateRuntimeRefresh {
            path: PathBuf::from(value),
            message: String::from("package baseline contains an unsafe path"),
        });
    }
    let path = PathBuf::from(value);
    if !path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(StorageError::PrivateRuntimeRefresh {
            path,
            message: String::from("package baseline contains an unsafe path"),
        });
    }
    Ok(path)
}

fn source_root_identity(path: &Path) -> Result<String, StorageError> {
    let canonical = fs::canonicalize(path).map_err(|source| StorageError::ReadPath {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(canonical.to_string_lossy().into_owned())
}

fn validate_directory_root(path: &Path) -> Result<(), StorageError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| StorageError::ReadPath {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(path)? {
        return Err(StorageError::UnsafeManagedPath {
            path: path.to_path_buf(),
            root: path.to_path_buf(),
        });
    }
    Ok(())
}

fn ensure_private_runtime(root: &Path) -> Result<(), StorageError> {
    validate_directory_root(root)?;
    let marker = root.join(PRIVATE_RUNTIME_MARKER);
    let valid = fs::symlink_metadata(&marker)
        .ok()
        .is_some_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        && !is_reparse_point(&marker)?
        && fs::read(&marker).map_err(|source| StorageError::ReadPath {
            path: marker.clone(),
            source,
        })? == b"managed\n";
    if !valid {
        return Err(StorageError::PrivateRuntimeRefresh {
            path: root.to_path_buf(),
            message: String::from(
                "private runtime marker is invalid; existing files were preserved",
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "private_runtime_refresh/tests.rs"]
mod tests;
