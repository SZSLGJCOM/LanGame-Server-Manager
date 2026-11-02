//! Verified relocation of managed roots. Copies remain separate from publication:
//! the source is retained until metadata, database references and settings agree.
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use app_core::{
    AppPathSettingsInput, AppSettings, StorageMigrationConflict, StorageMigrationPhase as Phase,
    StorageMigrationRoot, StorageMigrationStatus,
};
use serde::{Deserialize, Serialize};

use crate::instance_isolation::paths::{contains, normalize_path};
use crate::storage_migration_files::{self as files, FilePlan, FileProgress, RootMove};
use crate::{StorageError, StoragePaths};

#[path = "storage_migration_capacity.rs"]
mod capacity;
#[path = "storage_migration_journal.rs"]
mod journal;

pub type Progress = Arc<dyn Fn(StorageMigrationStatus) + Send + Sync>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PreparedMigration {
    current: AppSettings,
    next: AppSettings,
    files: FilePlan,
    metadata_roots: Vec<RootMove>,
    status: StorageMigrationStatus,
}

impl PreparedMigration {
    pub fn status(&self) -> StorageMigrationStatus {
        self.status.clone()
    }
}

pub fn initial_status(id: String, input: AppPathSettingsInput) -> StorageMigrationStatus {
    StorageMigrationStatus {
        operation_id: id,
        phase: Phase::Checking,
        input,
        roots: Vec::new(),
        total_files: 0,
        total_bytes: 0,
        completed_files: 0,
        completed_bytes: 0,
        reused_files: 0,
        preserved_destinations: Vec::new(),
        conflicts: Vec::new(),
        conflict_count: 0,
        can_cancel: true,
        recovery_required: false,
        error: None,
        cleanup_warnings: Vec::new(),
    }
}

pub fn path_moves(
    current: &StoragePaths,
    next: &AppSettings,
) -> Result<Vec<(PathBuf, PathBuf)>, StorageError> {
    let mut result = Vec::new();
    let mappings = path_mappings(current, next)?;
    let targets = files::normalize_destinations(
        &mappings
            .iter()
            .map(|(_, target)| target.clone())
            .collect::<Vec<_>>(),
    )?;
    for ((source, target), right) in mappings.into_iter().zip(targets) {
        let left = normalize_path(&source)?;
        if !contains(&left, &right) || !contains(&right, &left) {
            result.push((source, target));
        }
    }
    Ok(result)
}

pub fn path_mappings(
    current: &StoragePaths,
    next: &AppSettings,
) -> Result<Vec<(PathBuf, PathBuf)>, StorageError> {
    let next = current.with_app_settings(next);
    let targets = files::normalize_destinations(&[
        next.games_root.clone(),
        next.instances_root.clone(),
        next.archives_root.clone(),
        next.steamcmd_root.clone(),
    ])?;
    let mut result = Vec::<(PathBuf, PathBuf)>::new();
    let mut normalized = Vec::<(PathBuf, PathBuf)>::new();
    for ((source, target), normalized_target) in [
        (&current.games_root, &next.games_root),
        (&current.instances_root, &next.instances_root),
        (&current.archives_root, &next.archives_root),
        (&current.steamcmd_root, &next.steamcmd_root),
    ]
    .into_iter()
    .zip(targets)
    {
        let normalized_source = normalize_path(source)?;
        for (known_source, known_target) in &normalized {
            if contains(known_source, &normalized_source)
                && contains(&normalized_source, known_source)
                && (!contains(known_target, &normalized_target)
                    || !contains(&normalized_target, known_target))
            {
                return Err(invalid(
                    source,
                    "Several storage roles share this source directory but request different destinations. Separate this ambiguous layout before migration.",
                ));
            }
        }
        normalized.push((normalized_source, normalized_target));
        // Persist references with the configured spelling, including its
        // Windows namespace prefix. Canonical paths are only for comparison.
        result.push((source.clone(), target.clone()));
    }
    Ok(result)
}

pub fn map_path(path: &Path, moves: &[(PathBuf, PathBuf)]) -> Result<PathBuf, StorageError> {
    files::remap_path(
        path,
        &moves
            .iter()
            .map(|(source, target)| RootMove {
                source: source.clone(),
                target: target.clone(),
            })
            .collect::<Vec<_>>(),
    )
}

/// Resolve a future destination below roots whose old contents will be moved
/// aside. Installation revision paths need this before the copy has begun.
pub fn normalize_destination(path: &Path, settings: &AppSettings) -> Result<PathBuf, StorageError> {
    let mut paths = vec![
        PathBuf::from(&settings.servers_root),
        PathBuf::from(&settings.archives_root),
        PathBuf::from(&settings.games_root),
        PathBuf::from(&settings.steamcmd_root),
        path.to_owned(),
    ];
    paths = files::normalize_destinations(&paths)?;
    paths
        .pop()
        .ok_or_else(|| invalid(path, "Missing normalized destination."))
}

pub async fn program_roots(paths: &StoragePaths) -> Result<Vec<(String, PathBuf)>, StorageError> {
    crate::storage_migration_metadata::program_roots(paths).await
}

pub async fn preview(
    paths: &StoragePaths,
    next: AppSettings,
    id: String,
    additional_moves: Vec<(PathBuf, PathBuf)>,
    cancellation: Arc<AtomicBool>,
    progress: Progress,
) -> Result<PreparedMigration, StorageError> {
    let _lease = journal::acquire(paths)?;
    if let Some(previous) = journal::load(paths)?
        && matches!(
            previous.prepared.status.phase,
            Phase::Applying | Phase::Cleaning
        )
    {
        return Err(invalid(
            &paths.app_data_root,
            "An interrupted migration must finish recovery before another is prepared.",
        ));
    }
    let current =
        crate::settings::load_persisted_app_settings(paths)?.unwrap_or_else(|| paths.settings());
    let current_paths = paths.with_app_settings(&current);
    let next = current_paths.with_app_settings(&next).settings();
    crate::instance_archive_roots::validate_migration_archive_root(
        &current_paths.with_app_settings(&next),
    )?;
    let root_pairs = path_moves(&current_paths, &next)?;
    if root_pairs.is_empty() {
        return Err(invalid(
            &paths.settings_path,
            "Choose a different storage directory first.",
        ));
    }
    let main_targets = files::normalize_destinations(
        &root_pairs
            .iter()
            .map(|(_, target)| target.clone())
            .collect::<Vec<_>>(),
    )?;
    let all_pairs = root_pairs
        .iter()
        .chain(additional_moves.iter())
        .collect::<Vec<_>>();
    let normalized_targets = files::normalize_destinations(
        &all_pairs
            .iter()
            .map(|(_, target)| target.clone())
            .collect::<Vec<_>>(),
    )?;
    let mut roots: Vec<_> = all_pairs
        .iter()
        .zip(normalized_targets)
        .map(|((source, _), target)| {
            Ok(RootMove {
                source: normalize_path(source)?,
                target,
            })
        })
        .collect::<Result<_, StorageError>>()?;
    roots.sort_by(|a, b| a.source.cmp(&b.source));
    validate_protected_roots(&current_paths, &roots)?;
    // Moving a directory moves every configured child with it. Leaving a child
    // behind would make a successful migration retain the old source directory.
    let metadata_roots = path_mappings(&current_paths, &next)?
        .into_iter()
        .chain(additional_moves)
        .map(|(source, target)| RootMove { source, target })
        .collect::<Vec<_>>();
    let excluded = Vec::new();
    for root in [
        &current_paths.games_root,
        &current_paths.instances_root,
        &current_paths.archives_root,
        &current_paths.steamcmd_root,
    ] {
        let normalized = normalize_path(root)?;
        if !roots.iter().any(|moving| moving.source == normalized)
            && roots
                .iter()
                .any(|moving| contains(&moving.source, &normalized))
        {
            return Err(invalid(
                root,
                "此目录内还有未一起迁移的存储位置，请将其中的实例、归档或工具目录一起迁移。",
            ));
        }
        if !roots.iter().any(|moving| moving.source == normalized) {
            for target in &main_targets {
                if main_targets
                    .iter()
                    .any(|parent| parent != target && contains(parent, target))
                {
                    continue;
                }
                if files::paths_overlap(target, &normalized)? {
                    return Err(invalid(
                        root,
                        "目标目录与仍在使用的存储位置重叠，请选择独立的新目录，或将相关存储位置一起迁移。",
                    ));
                }
            }
        }
    }
    crate::storage_migration_metadata::preflight_metadata(&current_paths, &metadata_roots).await?;
    let mut status = initial_status(id, input(&next));
    status.roots = root_pairs
        .iter()
        .map(|(source, target)| StorageMigrationRoot {
            source: source.to_string_lossy().into_owned(),
            target: target.to_string_lossy().into_owned(),
        })
        .collect();
    let reported = status.clone();
    let callback = progress.clone();
    let checking_cancel = cancellation.clone();
    let plan = tokio::task::spawn_blocking(move || {
        files::plan(&roots, &excluded, Some(&checking_cancel), &mut |update| {
            report_files(&reported, update, &callback)
        })
    })
    .await
    .map_err(|error| invalid(&paths.app_data_root, error))??;
    if cancellation.load(Ordering::Acquire) {
        return Err(StorageError::StorageMigrationCancelled);
    }
    status.phase = Phase::Ready;
    status.can_cancel = false;
    status.total_files = plan.files.len() as u64;
    status.total_bytes = plan.total_bytes;
    status.reused_files = plan.files.iter().filter(|file| file.reused).count() as u64;
    status.conflict_count = plan.conflict_count;
    status.conflicts = plan
        .conflicts
        .iter()
        .map(|conflict| StorageMigrationConflict {
            source: conflict.source.to_string_lossy().into_owned(),
            target: conflict.target.to_string_lossy().into_owned(),
            message: conflict.message.clone(),
        })
        .collect();
    if plan.conflict_count == 0 {
        capacity::check(&plan)?;
    }
    let prepared = PreparedMigration {
        current,
        next,
        files: plan,
        metadata_roots,
        status,
    };
    journal::save(
        paths,
        &journal::Journal {
            version: 1,
            prepared: prepared.clone(),
            metadata: None,
        },
    )?;
    progress(prepared.status());
    Ok(prepared)
}

pub async fn execute(
    paths: &StoragePaths,
    mut prepared: PreparedMigration,
    cancellation: Arc<AtomicBool>,
    progress: Progress,
) -> Result<StorageMigrationStatus, StorageError> {
    let _lease = journal::acquire(paths)?;
    let saved = journal::load(paths)?
        .ok_or_else(|| invalid(&paths.app_data_root, "No migration preview is available."))?;
    if prepared.status.phase != Phase::Ready
        || prepared.status.conflict_count != 0
        || prepared.files.conflict_count != 0
        || saved.prepared.status.phase != Phase::Ready
        || serde_json::to_value(&saved.prepared)? != serde_json::to_value(&prepared)?
    {
        return Err(invalid(
            &paths.app_data_root,
            "This preview is no longer the current conflict-free migration. Check it again before moving files.",
        ));
    }
    let current =
        crate::settings::load_persisted_app_settings(paths)?.unwrap_or_else(|| paths.settings());
    if serde_json::to_value(&current)? != serde_json::to_value(&prepared.current)? {
        return Err(invalid(
            &paths.settings_path,
            "Storage settings changed after the preview. Check the migration again.",
        ));
    }
    let paths = paths.with_app_settings(&prepared.current);
    crate::storage_migration_metadata::preflight_metadata(&paths, &prepared.metadata_roots).await?;
    if cancellation.load(Ordering::Acquire) {
        return Err(StorageError::StorageMigrationCancelled);
    }
    prepared.status.phase = Phase::Verifying;
    prepared.status.can_cancel = true;
    progress(prepared.status());
    let original_plan = prepared.files.clone();
    let checked_cancel = cancellation.clone();
    let reported = prepared.status();
    let callback = progress.clone();
    tokio::task::spawn_blocking(move || {
        files::recheck(&original_plan, Some(&checked_cancel), &mut |update| {
            report_files(&reported, update, &callback)
        })
    })
    .await
    .map_err(|error| invalid(&paths.app_data_root, error))??;
    capacity::check(&prepared.files)?;
    prepared.status.phase = Phase::Copying;
    let mut record = journal::Journal {
        version: 1,
        prepared,
        metadata: None,
    };
    journal::save(&paths, &record)?;
    let plan = record.prepared.files.clone();
    let reported = record.prepared.status();
    let callback = progress.clone();
    let copying_cancel = cancellation.clone();
    let copied = tokio::task::spawn_blocking(move || {
        files::copy_verified(&plan, Some(&copying_cancel), &mut |update| {
            report_files(&reported, update, &callback)
        })
    })
    .await
    .map_err(|error| invalid(&paths.app_data_root, error))?;
    journal::refresh_preserved_destinations(&mut record)?;
    journal::save(&paths, &record)?;
    if let Err(error) = copied {
        journal::record_failure(&paths, &mut record, &error)?;
        return Err(error);
    }
    if cancellation.load(Ordering::Acquire) {
        journal::record_failure(
            &paths,
            &mut record,
            &StorageError::StorageMigrationCancelled,
        )?;
        return Err(StorageError::StorageMigrationCancelled);
    }
    record.prepared.status.phase = Phase::Verifying;
    progress(record.prepared.status());
    let metadata = crate::storage_migration_metadata::prepare_metadata(
        &paths,
        &record.prepared.metadata_roots,
    )
    .await;
    match metadata {
        Ok(metadata) => record.metadata = Some(metadata),
        Err(error) => {
            journal::record_failure(&paths, &mut record, &error)?;
            return Err(error);
        }
    }
    if cancellation.load(Ordering::Acquire) {
        journal::record_failure(
            &paths,
            &mut record,
            &StorageError::StorageMigrationCancelled,
        )?;
        return Err(StorageError::StorageMigrationCancelled);
    }
    // From this durable point cancellation cannot interrupt publication. A
    // restart replays the exact same patches before normal storage can open.
    record.prepared.status.phase = Phase::Applying;
    record.prepared.status.can_cancel = false;
    journal::save(&paths, &record)?;
    progress(record.prepared.status());
    // Final verification and cleanup hash entire trees. Keep those synchronous
    // filesystem operations off the async workers that serve status requests.
    let commit_paths = paths.clone();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        runtime.block_on(journal::finish(&commit_paths, &mut record, &progress))?;
        Ok(record.prepared.status())
    })
    .await
    .map_err(|error| invalid(&paths.app_data_root, error))?
}

pub fn read_status(paths: &StoragePaths) -> Result<Option<StorageMigrationStatus>, StorageError> {
    let Some(mut record) = journal::load(paths)? else {
        return Ok(None);
    };
    journal::refresh_preserved_destinations(&mut record)?;
    Ok(Some(record.prepared.status()))
}

pub fn read_prepared(paths: &StoragePaths, id: &str) -> Result<PreparedMigration, StorageError> {
    let record = journal::load(paths)?
        .ok_or_else(|| invalid(&paths.app_data_root, "No migration preview is available."))?;
    if record.prepared.status.operation_id != id
        || record.prepared.status.phase != Phase::Ready
        || record.prepared.status.conflict_count != 0
    {
        return Err(invalid(
            &paths.app_data_root,
            "This migration is not ready to execute.",
        ));
    }
    Ok(record.prepared)
}

pub(crate) fn recover_pending(paths: &StoragePaths) -> Result<(), StorageError> {
    journal::recover_pending(paths)
}

fn input(settings: &AppSettings) -> AppPathSettingsInput {
    AppPathSettingsInput {
        servers_root: settings.servers_root.clone(),
        archives_root: settings.archives_root.clone(),
        games_root: settings.games_root.clone(),
        steamcmd_root: settings.steamcmd_root.clone(),
    }
}

fn report_files(base: &StorageMigrationStatus, update: FileProgress, progress: &Progress) {
    let mut status = base.clone();
    if update.phase == "copying" {
        status.phase = Phase::Copying;
    }
    status.total_bytes = update.total_bytes;
    status.total_files = update.total_files;
    status.completed_bytes = update.completed_bytes;
    status.completed_files = update.completed_files;
    progress(status);
}

fn validate_protected_roots(paths: &StoragePaths, roots: &[RootMove]) -> Result<(), StorageError> {
    for root in roots {
        for protected in [
            &paths.app_data_root,
            &paths.modules_root,
            &paths.migrations_root,
        ] {
            let protected = crate::instance_archive_roots::normalize_bundled_directory(protected)?;
            for candidate in [&root.source, &root.target] {
                if contains(candidate, &protected) || contains(&protected, candidate) {
                    return Err(invalid(
                        candidate,
                        "Storage migration must stay separate from application data and bundled resources.",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn invalid(path: &Path, message: impl std::fmt::Display) -> StorageError {
    StorageError::InvalidStorageMigration {
        path: path.to_owned(),
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "storage_migration_tests.rs"]
mod tests;
