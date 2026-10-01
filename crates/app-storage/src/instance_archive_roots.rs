use std::fs;
use std::path::{Component, Path, PathBuf};

use app_core::AppSettings;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

use crate::instance_archive::inventory_lock;
use crate::instance_archive_files::{native, plain_directory};
use crate::instance_archive_store::invalid;
use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};
use crate::instance_settings_lock::InstanceSettingsLock;
use crate::managed_console_log::owned_fs::FileIdentity;
use crate::{StorageError, StoragePaths};

pub struct ArchiveRootSettingsGuard {
    _inventory: InstanceSettingsLock,
}

/// Keep the returned lock until settings have been persisted. Existing files and
/// recovery journals are never moved or detached by a settings change.
pub async fn guard_archive_root_settings_update(
    current: &StoragePaths,
    settings: &AppSettings,
) -> Result<ArchiveRootSettingsGuard, StorageError> {
    let lock = inventory_lock(current)?;
    let next = current.with_app_settings(settings);
    let checked_current = current.clone();
    let checked_next = next.clone();
    let changed = lock.spawn_blocking(move || {
        validate_archive_root(&checked_next)?;
        let archives_changed = !same_path(&checked_current.archives_root, &checked_next.archives_root)?;
        let instances_changed = !same_path(&checked_current.instances_root, &checked_next.instances_root)?;
        if archives_changed || instances_changed {
            if directory_has_entries(&checked_current.archives_root)? {
                return Err(retained_archives(&checked_current.archives_root));
            }
            if archives_changed && directory_has_entries(&checked_next.archives_root)? {
                return Err(invalid(&checked_next.archives_root,
                    "The new instance archive directory must be empty. Existing files will not be moved or imported by changing this setting."));
            }
        }
        Ok::<_, StorageError>(archives_changed || instances_changed)
    }).await.map_err(|error| StorageError::BlockingTaskFailed {
        operation: "validating instance archive roots", message: error.to_string(),
    })??;

    if changed
        && current
            .database_path
            .try_exists()
            .map_err(|source| StorageError::ReadPath {
                path: current.database_path.clone(),
                source,
            })?
    {
        let options = SqliteConnectOptions::new()
            .filename(&current.database_path)
            .read_only(true);
        let mut connection = SqliteConnection::connect_with(&options).await?;
        let result = validate_registered_roots(&mut connection, current, &next).await;
        connection.close().await?;
        result?;
    }
    Ok(ArchiveRootSettingsGuard { _inventory: lock })
}

async fn validate_registered_roots(
    connection: &mut SqliteConnection,
    current: &StoragePaths,
    next: &StoragePaths,
) -> Result<(), StorageError> {
    let has_archives: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='instance_archives')",
    ).fetch_one(&mut *connection).await?;
    if has_archives {
        let retained: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM instance_archives WHERE state NOT IN ('restored','purged'))",
        ).fetch_one(&mut *connection).await?;
        if retained {
            return Err(retained_archives(&current.archives_root));
        }
    }
    let root = normalize_path(&next.archives_root)?;
    let references: Vec<String> = sqlx::query_scalar(
        "SELECT config_path FROM instances UNION SELECT logs_path FROM instances UNION SELECT data_path FROM instances UNION SELECT saves_path FROM instances UNION SELECT install_root FROM game_installs LIMIT 24577",
    ).fetch_all(&mut *connection).await?;
    if references.len() > 24576 {
        return Err(invalid(
            &root,
            "Too many registered paths to establish safe archive ownership.",
        ));
    }
    for reference in references
        .into_iter()
        .filter(|value| !value.trim().is_empty())
    {
        let reference = normalize_path(Path::new(&reference))?;
        if overlaps(&root, &reference) {
            return Err(invalid(
                &root,
                "The instance archive directory overlaps a registered instance, save, or program directory. Choose a separate empty directory.",
            ));
        }
    }
    Ok(())
}

fn retained_archives(path: &Path) -> StorageError {
    invalid(
        path,
        "The instance archive directory or recovery journal is not empty. Restore or permanently delete existing archives and finish failed deletions before changing the instance or archive root. No files were moved.",
    )
}

fn directory_has_entries(path: &Path) -> Result<bool, StorageError> {
    if !plain_directory(path)? {
        return Ok(false);
    }
    let mut entries = fs::read_dir(path).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    entries
        .next()
        .transpose()
        .map(|entry| entry.is_some())
        .map_err(|source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        })
}

fn overlaps(left: &Path, right: &Path) -> bool {
    contains(left, right) || contains(right, left)
}

fn same_path(left: &Path, right: &Path) -> Result<bool, StorageError> {
    let left = normalize_path(left)?;
    let right = normalize_path(right)?;
    Ok(contains(&left, &right) && contains(&right, &left))
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
    let is_historical = contains(&root, &historical) && contains(&historical, &root);
    if overlaps(&root, &instances) && !is_historical {
        return Err(invalid(
            &root,
            "The instance archive directory must be separate from the instance root; only its existing .trash archive directory is supported inside it.",
        ));
    }
    let games = normalize_path(&paths.games_root)?;
    // Existing short layouts can put libraries and instances in one common
    // container. Preserve its default .trash leaf; database ownership checks
    // still reject overlap with every actual installation and save directory.
    let historical_in_common_container =
        is_historical && contains(&games, &root) && !contains(&root, &games);
    if overlaps(&root, &games) && !historical_in_common_container {
        return Err(invalid(
            &root,
            "The instance archive directory overlaps the program directory. Choose a separate empty directory.",
        ));
    }
    for protected in [
        normalize_path(&paths.steamcmd_root)?,
        normalize_path(&paths.logs_root)?,
        normalize_bundled_directory(&paths.modules_root)?,
        normalize_bundled_directory(&paths.migrations_root)?,
    ] {
        if overlaps(&root, &protected) {
            return Err(invalid(
                &root,
                "The instance archive directory overlaps a program, SteamCMD, module, log, or migration directory. Choose a separate empty directory.",
            ));
        }
    }
    for protected in [&paths.database_path, &paths.settings_path] {
        if overlaps(&root, &normalize_resource_path(protected)?) {
            return Err(invalid(
                &root,
                "The instance archive directory overlaps the application database or settings. Choose a separate empty directory.",
            ));
        }
    }
    ensure_same_volume(
        &root,
        &nearest_directory_identity(&instances)?,
        &nearest_directory_identity(&root)?,
    )
}

/// Module and migration locations are application-owned and may be assembled
/// from CARGO_MANIFEST_DIR/../../. Inspect the original chain before collapsing
/// its parent components, so a link cannot disappear during normalization.
fn normalize_bundled_directory(path: &Path) -> Result<PathBuf, StorageError> {
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
    native::open(&ancestor, true, false)
        .and_then(|node| node.identity())
        .map_err(|source| StorageError::ReadPath {
            path: ancestor,
            source,
        })
}

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
