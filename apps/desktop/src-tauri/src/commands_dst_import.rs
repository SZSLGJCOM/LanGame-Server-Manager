use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::commands_dst_import_validation::{
    DST_SHARD_CONFIGURATION_FILES as PRESERVED_SHARD_CONFIGURATION_FILES, ValidatedDstImportSource,
    find_case_insensitive_child_dir, is_link_or_reparse, require_plain_directory,
    validate_dontstarve_import_paths, validate_dontstarve_import_policy,
    validate_dontstarve_shard_save, validate_plain_tree,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct DirectoryCopyStats {
    pub(super) file_count: usize,
    pub(super) total_bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct DstWorldImportTransactionResult {
    pub(super) imported_master: bool,
    pub(super) imported_caves: bool,
    pub(super) imported_shards: Vec<String>,
    pub(super) stats: DirectoryCopyStats,
}

/// The original cluster remains available until instance.json and its native
/// Mod configuration have committed. The instance mutation owner spans both.
#[derive(Debug)]
pub(super) struct PublishedDstWorldImport {
    target: PathBuf,
    rollback: PathBuf,
    discarded: PathBuf,
    result: Option<DstWorldImportTransactionResult>,
}

impl PublishedDstWorldImport {
    pub(super) fn commit(mut self) -> DstWorldImportTransactionResult {
        let result = self.result.take().expect("published DST import result");
        if let Err(error) = fs::remove_dir_all(&self.rollback) {
            eprintln!(
                "DST import committed; previous cluster retained at {}: {error}",
                self.rollback.display()
            );
        }
        result
    }

    pub(super) fn rollback(mut self) -> Result<(), String> {
        self.result.take();
        self.restore_original()?;
        if let Err(error) = fs::remove_dir_all(&self.discarded) {
            eprintln!(
                "DST import rolled back; unused imported cluster retained at {}: {error}",
                self.discarded.display()
            );
        }
        Ok(())
    }

    fn restore_original(&self) -> Result<(), String> {
        fs::rename(&self.target, &self.discarded).map_err(|error| format!("Cannot move failed imported world aside at {}: {error}. Original world is retained at {}.", self.target.display(), self.rollback.display()))?;
        if let Err(error) = fs::rename(&self.rollback, &self.target) {
            let restored_import = fs::rename(&self.discarded, &self.target);
            return Err(format!(
                "Cannot restore original DST world from {}: {error}; restoring imported directory: {restored_import:?}.",
                self.rollback.display()
            ));
        }
        Ok(())
    }
}

impl Drop for PublishedDstWorldImport {
    fn drop(&mut self) {
        if self.result.is_some() {
            // A panic must keep the old world recoverable. Only rename here;
            // potentially large cleanup is owned by the normal blocking path.
            if let Err(error) = self.restore_original() {
                eprintln!("DST import rollback failed: {error}");
            }
        }
    }
}

pub(super) fn prepare_dontstarve_target_cluster(target_cluster_root: &Path) -> Result<(), String> {
    if target_cluster_root.exists() {
        require_plain_directory(target_cluster_root, "DST target cluster")?;
        return Ok(());
    }
    fs::create_dir_all(target_cluster_root).map_err(|error| {
        format!(
            "Failed to prepare target cluster root {}: {}",
            target_cluster_root.display(),
            error
        )
    })?;
    require_plain_directory(target_cluster_root, "DST target cluster")
}

pub(super) fn import_dontstarve_world_transaction(
    source: &ValidatedDstImportSource,
    target_cluster_root: &Path,
    caves_enabled: bool,
) -> Result<PublishedDstWorldImport, String> {
    import_dontstarve_world_transaction_with_publish(
        source,
        target_cluster_root,
        caves_enabled,
        false,
        &mut |staging_path, target_path, rollback_path| {
            publish_staged_cluster(staging_path, target_path, rollback_path)
        },
    )
}

pub(super) fn restore_dontstarve_world_transaction(
    source: &ValidatedDstImportSource,
    target_cluster_root: &Path,
    prepared: &app_storage::PreparedInstanceBackupRestore,
) -> Result<PublishedDstWorldImport, String> {
    import_dontstarve_world_transaction_with_publish(
        source,
        target_cluster_root,
        false,
        true,
        &mut |staging, target, rollback| {
            prepared
                .validate_current_contents()
                .map_err(|error| error.to_string())?;
            publish_staged_cluster(staging, target, rollback)
        },
    )
}

fn import_dontstarve_world_transaction_with_publish<F>(
    source: &ValidatedDstImportSource,
    target_cluster_root: &Path,
    caves_enabled: bool,
    restore_generation: bool,
    publish: &mut F,
) -> Result<PublishedDstWorldImport, String>
where
    F: FnMut(&Path, &Path, &Path) -> Result<(), String>,
{
    validate_dontstarve_import_policy(source, caves_enabled)?;
    validate_dontstarve_import_paths(&source.cluster_root, target_cluster_root)?;
    prepare_dontstarve_target_cluster(target_cluster_root)?;

    let target_parent = target_cluster_root.parent().ok_or_else(|| {
        format!(
            "Target cluster folder {} has no parent directory.",
            target_cluster_root.display()
        )
    })?;
    let token = Uuid::new_v4().simple();
    let staging_path = target_parent.join(format!(".dst-import-staging-{token}"));
    let rollback_path = target_parent.join(format!(".dst-import-rollback-{token}"));
    fs::create_dir(&staging_path).map_err(|error| {
        format!(
            "Failed to create DST import staging folder {}: {}",
            staging_path.display(),
            error
        )
    })?;
    let mut staging_cleanup = ImportDirectoryCleanup::new(staging_path.clone());

    copy_directory_contents_recursive(target_cluster_root, &staging_path)?;
    let mut stats = DirectoryCopyStats::default();
    let source_shards = source.shards();
    let generation_preserved = PRESERVED_SHARD_CONFIGURATION_FILES
        .iter()
        .copied()
        .filter(|name| *name != "leveldataoverride.lua")
        .collect::<Vec<_>>();
    let mut imported_shards = Vec::new();
    for spec in app_core::dst_shards::DST_SHARDS {
        let staged_root = find_case_insensitive_child_dir(&staging_path, spec.directory)
            .unwrap_or_else(|| staging_path.join(spec.directory));
        if let Some((_, root)) = source_shards
            .iter()
            .find(|(key, _)| *key == spec.process_key)
        {
            import_dontstarve_shard_world_data(root, &staged_root, &mut stats, restore_generation)?;
            validate_dontstarve_shard_save(&staged_root, spec.directory)?;
            imported_shards.push(spec.directory.to_owned());
        } else if staged_root.exists()
            || (restore_generation
                && find_case_insensitive_child_dir(&source.cluster_root, spec.directory).is_some())
        {
            if !staged_root.exists() {
                fs::create_dir(&staged_root).map_err(|error| error.to_string())?;
            }
            clear_directory_contents_except(
                &staged_root,
                if restore_generation {
                    &generation_preserved
                } else {
                    PRESERVED_SHARD_CONFIGURATION_FILES
                },
            )?;
            if restore_generation
                && let Some(root) =
                    find_case_insensitive_child_dir(&source.cluster_root, spec.directory)
            {
                let level = root.join("leveldataoverride.lua");
                if level.exists() {
                    let bytes = fs::copy(&level, staged_root.join("leveldataoverride.lua"))
                        .map_err(|error| error.to_string())?;
                    stats.file_count += 1;
                    stats.total_bytes += bytes;
                }
            }
        }
    }
    validate_plain_tree(&staging_path, &staging_path)?;
    source.verify_unchanged()?;

    publish(&staging_path, target_cluster_root, &rollback_path)?;
    staging_cleanup.disarm();

    Ok(PublishedDstWorldImport {
        target: target_cluster_root.to_path_buf(),
        rollback: rollback_path,
        discarded: staging_path,
        result: Some(DstWorldImportTransactionResult {
            imported_master: source.master_saved,
            imported_caves: source.caves_root.is_some(),
            imported_shards,
            stats,
        }),
    })
}

fn import_dontstarve_shard_world_data(
    source_shard_root: &Path,
    target_shard_root: &Path,
    stats: &mut DirectoryCopyStats,
    restore_generation: bool,
) -> Result<(), String> {
    fs::create_dir_all(target_shard_root).map_err(|error| {
        format!(
            "Failed to prepare target shard directory {}: {}",
            target_shard_root.display(),
            error
        )
    })?;
    let generation_preserved = PRESERVED_SHARD_CONFIGURATION_FILES
        .iter()
        .copied()
        .filter(|name| *name != "leveldataoverride.lua")
        .collect::<Vec<_>>();
    let preserved = if restore_generation {
        generation_preserved.as_slice()
    } else {
        PRESERVED_SHARD_CONFIGURATION_FILES
    };
    // Native leveldataoverride is world data during a backup restore. The
    // managed worldgen/mod overrides are restored through canonical settings.
    clear_directory_contents_except(target_shard_root, preserved)?;
    let copied_stats = copy_directory_contents_skipping_root_files(
        source_shard_root,
        target_shard_root,
        preserved,
    )?;
    stats.file_count += copied_stats.file_count;
    stats.total_bytes += copied_stats.total_bytes;
    Ok(())
}

pub(super) fn clear_directory_contents_except(
    path: &Path,
    preserved_names: &[&str],
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    require_plain_directory(path, "directory to clear")?;
    let entries = fs::read_dir(path)
        .map_err(|error| format!("Failed to read directory {}: {}", path.display(), error))?;
    for entry_result in entries {
        let entry = entry_result.map_err(|error| {
            format!("Failed to inspect directory {}: {}", path.display(), error)
        })?;
        let entry_path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if preserved_names
            .iter()
            .any(|preserved| preserved.eq_ignore_ascii_case(file_name.as_ref()))
        {
            continue;
        }

        let metadata = fs::symlink_metadata(&entry_path)
            .map_err(|error| format!("Failed to inspect {}: {}", entry_path.display(), error))?;
        if is_link_or_reparse(&metadata) {
            return Err(format!(
                "Refusing to remove linked or reparse-point path {}.",
                entry_path.display()
            ));
        }
        if metadata.is_dir() {
            fs::remove_dir_all(&entry_path)
                .map_err(|error| format!("Failed to remove {}: {}", entry_path.display(), error))?;
        } else if metadata.is_file() {
            fs::remove_file(&entry_path)
                .map_err(|error| format!("Failed to remove {}: {}", entry_path.display(), error))?;
        } else {
            return Err(format!(
                "Refusing to remove unsupported filesystem entry {}.",
                entry_path.display()
            ));
        }
    }

    Ok(())
}

fn copy_directory_contents_recursive(
    source_dir: &Path,
    target_dir: &Path,
) -> Result<DirectoryCopyStats, String> {
    copy_directory_contents_internal(source_dir, target_dir, &[], true)
}

pub(super) fn copy_directory_contents_skipping_root_files(
    source_dir: &Path,
    target_dir: &Path,
    skipped_root_names: &[&str],
) -> Result<DirectoryCopyStats, String> {
    copy_directory_contents_internal(source_dir, target_dir, skipped_root_names, true)
}

fn copy_directory_contents_internal(
    source_dir: &Path,
    target_dir: &Path,
    skipped_root_names: &[&str],
    at_root: bool,
) -> Result<DirectoryCopyStats, String> {
    require_plain_directory(source_dir, "copy source")?;
    require_plain_directory(target_dir, "copy target")?;
    let mut stats = DirectoryCopyStats::default();
    let entries = fs::read_dir(source_dir).map_err(|error| {
        format!(
            "Failed to read directory {}: {}",
            source_dir.display(),
            error
        )
    })?;

    for entry_result in entries {
        let entry = entry_result.map_err(|error| {
            format!(
                "Failed to inspect directory {}: {}",
                source_dir.display(),
                error
            )
        })?;
        let source_path = entry.path();
        let file_name = entry.file_name();
        let file_name_text = file_name.to_string_lossy().into_owned();
        if at_root
            && skipped_root_names
                .iter()
                .any(|skipped| skipped.eq_ignore_ascii_case(file_name_text.as_str()))
        {
            continue;
        }

        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| format!("Failed to inspect {}: {}", source_path.display(), error))?;
        if is_link_or_reparse(&metadata) {
            return Err(format!(
                "Refusing to copy linked or reparse-point path {}.",
                source_path.display()
            ));
        }

        let target_path = target_dir.join(&file_name);
        if metadata.is_dir() {
            fs::create_dir(&target_path).map_err(|error| {
                format!(
                    "Failed to prepare directory {}: {}",
                    target_path.display(),
                    error
                )
            })?;
            let nested_stats = copy_directory_contents_internal(
                &source_path,
                &target_path,
                skipped_root_names,
                false,
            )?;
            stats.file_count += nested_stats.file_count;
            stats.total_bytes += nested_stats.total_bytes;
            continue;
        }
        if !metadata.is_file() {
            return Err(format!(
                "Refusing to copy unsupported filesystem entry {}.",
                source_path.display()
            ));
        }

        let copied_bytes = fs::copy(&source_path, &target_path).map_err(|error| {
            format!(
                "Failed to copy {} to {}: {}",
                source_path.display(),
                target_path.display(),
                error
            )
        })?;
        stats.file_count += 1;
        stats.total_bytes += copied_bytes;
    }

    Ok(stats)
}

fn publish_staged_cluster(
    staging_path: &Path,
    target_path: &Path,
    rollback_path: &Path,
) -> Result<(), String> {
    fs::rename(target_path, rollback_path).map_err(|error| {
        format!(
            "Failed to move the current DST world aside before import ({} -> {}): {}",
            target_path.display(),
            rollback_path.display(),
            error
        )
    })?;

    if let Err(publish_error) = fs::rename(staging_path, target_path) {
        return match fs::rename(rollback_path, target_path) {
            Ok(()) => Err(format!(
                "Failed to publish the staged DST world; the original world was restored: {}",
                publish_error
            )),
            Err(rollback_error) => Err(format!(
                "Failed to publish the staged DST world ({publish_error}) and failed to restore the original world from {} ({rollback_error}).",
                rollback_path.display()
            )),
        };
    }

    Ok(())
}

struct ImportDirectoryCleanup {
    path: PathBuf,
    armed: bool,
}

impl ImportDirectoryCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ImportDirectoryCleanup {
    fn drop(&mut self) {
        if self.armed
            && let Err(error) = fs::remove_dir_all(&self.path)
        {
            eprintln!(
                "DST import staging cleanup failed at {}: {error}",
                self.path.display()
            );
        }
    }
}

#[cfg(test)]
pub(super) fn import_dontstarve_world_transaction_with_forced_publish_failure(
    source: &ValidatedDstImportSource,
    target_cluster_root: &Path,
    caves_enabled: bool,
) -> Result<PublishedDstWorldImport, String> {
    import_dontstarve_world_transaction_with_publish(
        source,
        target_cluster_root,
        caves_enabled,
        false,
        &mut |staging_path, target_path, rollback_path| {
            fs::rename(target_path, rollback_path).map_err(|error| error.to_string())?;
            let forced_error = "forced publish failure";
            fs::rename(rollback_path, target_path).map_err(|rollback_error| {
                format!("{forced_error}; rollback failed: {rollback_error}")
            })?;
            let _ = staging_path;
            Err(String::from(forced_error))
        },
    )
}
