//! Durable publication and recovery of one explicitly started migration.
//! A ready preview is inert. Applying/Cleaning records must finish before the
//! application can reconcile or consume the published storage references.
use super::*;
use crate::managed_console_log::owned_fs::reject_links;
use crate::storage_migration_metadata::MetadataPlan;
use std::fs::{File, OpenOptions};
use std::io::Read;

#[path = "storage_migration_publish.rs"]
mod publish;
#[path = "storage_migration_sql.rs"]
mod sql;
const MAX_JOURNAL_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
pub(super) struct Journal {
    pub version: u32,
    pub prepared: PreparedMigration,
    pub metadata: Option<MetadataPlan>,
}

fn location(paths: &StoragePaths) -> PathBuf {
    paths.app_data_root.join("storage-migration.json")
}

pub(super) fn acquire(paths: &StoragePaths) -> Result<File, StorageError> {
    let path = paths.app_data_root.join("storage-migration.lock");
    reject_links(&path).map_err(|source| StorageError::ReadPath {
        path: path.clone(),
        source,
    })?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
    file.try_lock().map_err(|error| {
        invalid(
            &path,
            format!("Another storage migration owns this application data: {error}"),
        )
    })?;
    Ok(file)
}

pub(super) fn load(paths: &StoragePaths) -> Result<Option<Journal>, StorageError> {
    let path = location(paths);
    reject_links(&path).map_err(|source| StorageError::ReadPath {
        path: path.clone(),
        source,
    })?;
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(StorageError::ReadPath { path, source }),
    };
    let mut bytes = Vec::new();
    file.take(MAX_JOURNAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(invalid(&path, "Migration journal exceeds its size limit."));
    }
    let record: Journal = serde_json::from_slice(&bytes)?;
    if record.version != 1 {
        return Err(invalid(&path, "Unsupported migration journal version."));
    }
    validate_protected_roots(paths, &record.prepared.files.roots)?;
    Ok(Some(record))
}

pub(super) fn save(paths: &StoragePaths, record: &Journal) -> Result<(), StorageError> {
    let path = location(paths);
    reject_links(&path).map_err(|source| StorageError::ReadPath {
        path: path.clone(),
        source,
    })?;
    let bytes = serde_json::to_vec(record)?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(invalid(&path, "Migration journal exceeds its size limit."));
    }
    crate::atomic_file::write_file_atomically(&path, &bytes)
        .map_err(|source| StorageError::WriteConfig { path, source })
}

pub(super) fn record_failure(
    paths: &StoragePaths,
    record: &mut Journal,
    error: &StorageError,
) -> Result<(), StorageError> {
    refresh_preserved_destinations(record)?;
    // An interrupted commit must remain a recovery barrier, never an ordinary
    // terminal failure that would allow unrelated writes to mixed metadata.
    if !matches!(
        record.prepared.status.phase,
        Phase::Applying | Phase::Cleaning
    ) {
        record.prepared.status.phase = if matches!(error, StorageError::StorageMigrationCancelled) {
            Phase::Cancelled
        } else {
            Phase::Failed
        };
    }
    record.prepared.status.can_cancel = false;
    record.prepared.status.error = Some(error.to_string());
    save(paths, record)
}

pub(super) fn refresh_preserved_destinations(record: &mut Journal) -> Result<(), StorageError> {
    record.prepared.status.preserved_destinations =
        files::preserved_destinations(&record.prepared.files)?
            .into_iter()
            .map(
                |(original_path, backup_path)| app_core::StorageMigrationPreservedDestination {
                    original_path: original_path.to_string_lossy().into_owned(),
                    backup_path: backup_path.to_string_lossy().into_owned(),
                },
            )
            .collect();
    Ok(())
}

fn overrides(record: &Journal) -> files::DestinationHashes {
    let mut hashes = files::DestinationHashes::new();
    if let Some(metadata) = &record.metadata {
        for patch in &metadata.files {
            if record
                .prepared
                .files
                .files
                .iter()
                .any(|file| file.target == patch.path)
            {
                hashes.insert(
                    patch.path.clone(),
                    files::FileDigest {
                        size: patch.after.len() as u64,
                        sha256: crate::instance_archive_store::digest(&patch.after),
                    },
                );
            }
        }
    }
    hashes
}

fn apply_files(paths: &StoragePaths, record: &Journal) -> Result<(), StorageError> {
    let metadata = record
        .metadata
        .as_ref()
        .ok_or_else(|| invalid(&location(paths), "Migration commit has no metadata plan."))?;
    for patch in &metadata.files {
        if crate::instance_archive_store::digest(&patch.before) != patch.before_sha256 {
            return Err(invalid(
                &patch.path,
                "Migration journal original metadata does not match its saved digest.",
            ));
        }
        publish::publish(&patch.path, &patch.before, &patch.after)?;
    }
    Ok(())
}

pub(super) async fn finish(
    paths: &StoragePaths,
    record: &mut Journal,
    progress: &Progress,
) -> Result<(), StorageError> {
    let result = finish_inner(paths, record, progress).await;
    if let Err(error) = &result {
        record_failure(paths, record, error)?;
    }
    result
}

async fn finish_inner(
    paths: &StoragePaths,
    record: &mut Journal,
    progress: &Progress,
) -> Result<(), StorageError> {
    refresh_preserved_destinations(record)?;
    if record.prepared.status.phase == Phase::Applying {
        apply_files(paths, record)?;
        let hashes = overrides(record);
        files::verify_source_and_destination(&record.prepared.files, &hashes)?;
        let metadata = record
            .metadata
            .as_ref()
            .ok_or_else(|| invalid(&location(paths), "Migration metadata is missing."))?;
        sql::apply(paths, &metadata.rows).await?;
        let current = crate::settings::load_persisted_app_settings(paths)?
            .unwrap_or_else(|| paths.settings());
        if serde_json::to_value(&current)? != serde_json::to_value(&record.prepared.current)?
            && serde_json::to_value(&current)? != serde_json::to_value(&record.prepared.next)?
        {
            return Err(invalid(
                &paths.settings_path,
                "Settings changed while a migration commit was pending.",
            ));
        }
        crate::save_app_settings_with_paths(record.prepared.next.clone(), paths)?;
        record.prepared.status.phase = Phase::Cleaning;
        record.prepared.status.error = None;
        save(paths, record)?;
        progress(record.prepared.status());
    }
    if record.prepared.status.phase == Phase::Cleaning {
        // Recovery may begin after some source files were removed. Reconfirm
        // every reference (including metadata outside copied roots) without
        // requiring already-cleaned source payloads to reappear.
        apply_files(paths, record)?;
        let metadata = record
            .metadata
            .as_ref()
            .ok_or_else(|| invalid(&location(paths), "Migration metadata is missing."))?;
        sql::apply(paths, &metadata.rows).await?;
        let current = crate::settings::load_persisted_app_settings(paths)?
            .unwrap_or_else(|| paths.settings());
        if serde_json::to_value(&current)? != serde_json::to_value(&record.prepared.next)? {
            return Err(invalid(
                &paths.settings_path,
                "Source cleanup requires the published destination settings.",
            ));
        }
        let hashes = overrides(record);
        record.prepared.status.cleanup_warnings =
            files::cleanup_sources(&record.prepared.files, &hashes);
        // A clean completion means the original directories have disappeared,
        // including the application's idle lock files. Never hide a retained root.
        for root in &record.prepared.files.roots {
            match std::fs::symlink_metadata(&root.source) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Ok(_) => record.prepared.status.cleanup_warnings.push(format!(
                    "旧目录未完全移除，仍保留在 {}。",
                    root.source.display()
                )),
                Err(error) => record.prepared.status.cleanup_warnings.push(format!(
                    "无法确认旧目录 {} 已清理：{error}",
                    root.source.display()
                )),
            }
        }
        record.prepared.status.phase = Phase::Completed;
        record.prepared.status.completed_bytes = record.prepared.status.total_bytes;
        record.prepared.status.completed_files = record.prepared.status.total_files;
        record.prepared.status.can_cancel = false;
        record.prepared.status.error = None;
        save(paths, record)?;
        progress(record.prepared.status());
    }
    Ok(())
}

pub(super) fn recover_pending(paths: &StoragePaths) -> Result<(), StorageError> {
    if load(paths)?.is_none_or(|record| !record.prepared.status.is_active()) {
        return Ok(());
    }
    let _lease = acquire(paths)?;
    // The unlocked observation is only a fast path for inactive journals.
    // Never replay it: another process may have completed while we acquired.
    let Some(mut record) = load(paths)? else {
        return Ok(());
    };
    if !record.prepared.status.is_active() {
        return Ok(());
    }
    if matches!(
        record.prepared.status.phase,
        Phase::Applying | Phase::Cleaning
    ) {
        let recovery_paths = paths.clone();
        std::thread::spawn(move || {
            let paths = recovery_paths;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| invalid(&paths.app_data_root, error))?;
            let progress: Progress = Arc::new(|_| {});
            runtime.block_on(finish(&paths, &mut record, &progress))
        })
        .join()
        .map_err(|_| invalid(&paths.app_data_root, "Migration recovery task panicked."))?
    } else {
        let error = invalid(
            &paths.app_data_root,
            "Migration was interrupted before publication. Original paths and files were preserved; check a new preview before retrying.",
        );
        record_failure(paths, &mut record, &error)
    }
}
