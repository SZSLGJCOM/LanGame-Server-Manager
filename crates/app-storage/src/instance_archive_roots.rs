use std::path::{Component, Path, PathBuf};

use crate::instance_archive_files::{native, plain_directory};
use crate::instance_archive_store::invalid;
use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};
use crate::managed_console_log::owned_fs::FileIdentity;
use crate::{StorageError, StoragePaths};

fn overlaps(left: &Path, right: &Path) -> bool {
    contains(left, right) || contains(right, left)
}

/// Validate without creating directories, copying files, or following links.
/// Archive admission also applies this check before taking any save backup.
pub(crate) fn validate_archive_root(paths: &StoragePaths) -> Result<(), StorageError> {
    // Preserve the established unsafe-managed-path error for an archive
    // container that was replaced by a file, symlink, or junction.
    plain_directory(&paths.archives_root)?;
    let root = normalize_path(&paths.archives_root)?;
    let instances = normalize_path(&paths.instances_root)?;
    let historical = normalize_path(&paths.instances_root.join(".trash"))?;
    let games = normalize_path(&paths.games_root)?;
    validate_archive_layout(
        &root,
        &instances,
        &historical,
        &games,
        &[
            normalize_path(&paths.steamcmd_root)?,
            normalize_path(&paths.logs_root)?,
            normalize_bundled_directory(&paths.modules_root)?,
            normalize_bundled_directory(&paths.migrations_root)?,
        ],
        &[
            normalize_resource_path(&paths.database_path)?,
            normalize_resource_path(&paths.settings_path)?,
        ],
    )?;
    ensure_same_volume(
        &root,
        &nearest_directory_identity(&instances)?,
        &nearest_directory_identity(&root)?,
    )
}

/// A whole-target takeover rebuilds its descendants. Inspect only the outer
/// target and external ancestors; old inner files and links are retained in
/// the target backup, never interpreted as the future archive layout.
pub(crate) fn validate_migration_archive_root(paths: &StoragePaths) -> Result<(), StorageError> {
    let targets = crate::storage_migration_files::normalize_destinations(&[
        paths.instances_root.clone(),
        paths.archives_root.clone(),
        paths.games_root.clone(),
        paths.steamcmd_root.clone(),
    ])?;
    let [instances, root, games, steamcmd] = targets.as_slice() else {
        return Err(invalid(
            &paths.archives_root,
            "Migration destination normalization returned an invalid layout.",
        ));
    };
    validate_archive_layout(
        root,
        instances,
        &instances.join(".trash"),
        games,
        &[
            steamcmd.clone(),
            normalize_path(&paths.logs_root)?,
            normalize_bundled_directory(&paths.modules_root)?,
            normalize_bundled_directory(&paths.migrations_root)?,
        ],
        &[
            normalize_resource_path(&paths.database_path)?,
            normalize_resource_path(&paths.settings_path)?,
        ],
    )?;
    ensure_same_volume(
        root,
        &nearest_directory_identity(migration_target_parent(instances, &targets)?)?,
        &nearest_directory_identity(migration_target_parent(root, &targets)?)?,
    )
}

fn migration_target_parent<'a>(
    path: &Path,
    targets: &'a [PathBuf],
) -> Result<&'a Path, StorageError> {
    targets
        .iter()
        .filter(|target| contains(target, path))
        .min_by_key(|target| target.components().count())
        .and_then(|outer| outer.parent())
        .ok_or_else(|| invalid(path, "Migration target has no external parent."))
}

fn validate_archive_layout(
    root: &Path,
    instances: &Path,
    historical: &Path,
    games: &Path,
    protected_directories: &[PathBuf],
    protected_files: &[PathBuf],
) -> Result<(), StorageError> {
    let is_historical = contains(root, historical) && contains(historical, root);
    if overlaps(root, instances) && !is_historical {
        return Err(invalid(
            root,
            "The instance archive directory must be separate from the instance root; only its existing .trash archive directory is supported inside it.",
        ));
    }
    // Existing short layouts can put libraries and instances in one common
    // container. Preserve its default .trash leaf; database ownership checks
    // still reject overlap with every actual installation and save directory.
    let historical_in_common_container =
        is_historical && contains(games, root) && !contains(root, games);
    if overlaps(root, games) && !historical_in_common_container {
        return Err(invalid(
            root,
            "The instance archive directory overlaps the program directory. Choose a separate empty directory.",
        ));
    }
    for protected in protected_directories {
        if overlaps(root, protected) {
            return Err(invalid(
                root,
                "The instance archive directory overlaps a program, SteamCMD, module, log, or migration directory. Choose a separate empty directory.",
            ));
        }
    }
    for protected in protected_files {
        if overlaps(root, protected) {
            return Err(invalid(
                root,
                "The instance archive directory overlaps the application database or settings. Choose a separate empty directory.",
            ));
        }
    }
    Ok(())
}

/// Module and migration locations are application-owned and may be assembled
/// from CARGO_MANIFEST_DIR/../../. Inspect the original chain before collapsing
/// its parent components, so a link cannot disappear during normalization.
pub(crate) fn normalize_bundled_directory(path: &Path) -> Result<PathBuf, StorageError> {
    if !path.is_absolute() {
        return normalize_path(path);
    }
    crate::managed_console_log::owned_fs::reject_links(path).map_err(|source| {
        StorageError::ReadPath {
            path: path.to_owned(),
            source,
        }
    })?;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(invalid(
                        path,
                        "Bundled resource path escapes its filesystem root.",
                    ));
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalize_path(&normalized)
}

pub(crate) fn ensure_paths_outside_archive_root<'a>(
    paths: &StoragePaths,
    claims: impl IntoIterator<Item = &'a Path>,
) -> Result<(), StorageError> {
    let archives = normalize_path(&paths.archives_root)?;
    for claim in claims {
        if overlaps(&archives, &normalize_resource_path(claim)?) {
            return Err(invalid(
                claim,
                "Instance configuration, saves, and program paths must not overlap the instance archive directory or any of its parents. Choose a separate instance-owned path.",
            ));
        }
    }
    Ok(())
}

fn nearest_directory_identity(path: &Path) -> Result<FileIdentity, StorageError> {
    let mut ancestor: PathBuf = path.to_owned();
    while !plain_directory(&ancestor)? {
        if !ancestor.pop() {
            return Err(invalid(path, "No existing directory ancestor was found."));
        }
    }
    let identity = if ancestor.parent().is_none() {
        crate::storage_migration_files::volume_root_identity(&ancestor)
    } else {
        native::open(&ancestor, true, false).and_then(|node| node.identity())
    };
    identity.map_err(|source| StorageError::ReadPath {
        path: ancestor,
        source,
    })
}

#[cfg(test)]
#[path = "instance_archive_migration_roots_tests.rs"]
mod migration_tests;

fn ensure_same_volume(
    path: &Path,
    source: &FileIdentity,
    destination: &FileIdentity,
) -> Result<(), StorageError> {
    if !source.same_volume(destination) {
        return Err(invalid(
            path,
            "The instance archive directory must be on the same volume as the instance root. Cross-volume archival is not supported; no files were moved.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_volume_archive_roots_are_rejected() {
        let source = serde_json::from_str::<FileIdentity>(r#"{"volume":1,"index":2}"#).unwrap();
        let same = serde_json::from_str::<FileIdentity>(r#"{"volume":1,"index":3}"#).unwrap();
        let other = serde_json::from_str::<FileIdentity>(r#"{"volume":2,"index":2}"#).unwrap();
        ensure_same_volume(Path::new("archives"), &source, &same).unwrap();
        assert!(
            ensure_same_volume(Path::new("archives"), &source, &other)
                .unwrap_err()
                .to_string()
                .contains("same volume")
        );
    }
}
