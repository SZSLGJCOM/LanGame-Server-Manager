use std::path::PathBuf;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

use crate::instance_archive::validate_archived_files;
use crate::instance_archive_files::archive_path;
use crate::instance_archive_store::{self as store, Archive, invalid};
use crate::instance_settings_lock::{InstanceSettingsLock, acquire_instance_settings_read_lock};
use crate::{StorageError, StoragePaths};

#[derive(Clone, Debug)]
pub struct ArchivedProgramSource {
    pub module_id: String,
    pub archive_id: String,
    pub install_root: PathBuf,
    pub current_version: Option<String>,
}

/// Returns independently owned programs in intact, committed archives. This
/// proves archive ownership, not official package contents; seed copying must
/// still verify the original allowlist. Invalid archives remain available for
/// inspection through the archive list and are never offered as program sources.
/// The returned paths carry no lease: a copier must revalidate under the specific
/// archive's source lease and retain it until its blocking worker finishes.
pub async fn read_archived_program_sources(
    paths: &StoragePaths,
) -> Result<Vec<ArchivedProgramSource>, StorageError> {
    let lock = acquire_instance_settings_read_lock(paths, "archive-inventory")?;
    read_archived_program_sources_locked(paths, &lock).await
}

/// The caller already holds the inventory lease for inspection.
pub(crate) async fn read_archived_program_sources_locked(
    paths: &StoragePaths,
    lock: &InstanceSettingsLock,
) -> Result<Vec<ArchivedProgramSource>, StorageError> {
    if !paths
        .database_path
        .try_exists()
        .map_err(|source| StorageError::ReadPath {
            path: paths.database_path.clone(),
            source,
        })?
    {
        return Ok(Vec::new());
    }
    // Catalog reads must not migrate an existing database or invent archive
    // registrations by scanning historical directories.
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .read_only(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let result = read_sources(&mut connection, paths, lock).await;
    connection.close().await?;
    result
}

/// Re-read one candidate after catalog enumeration. Restore and purge use the
/// matching exclusive lease before changing either its journal or its files.
/// Contention is reported to the caller; it must not silently select stale data.
pub(crate) async fn lease_archived_program_source(
    paths: &StoragePaths,
    archive_id: &str,
) -> Result<Option<(ArchivedProgramSource, InstanceSettingsLock)>, StorageError> {
    let lease = crate::instance_archive::program_source_read_lock(paths, archive_id)?;
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .read_only(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let row = sqlx::query("SELECT * FROM instance_archives WHERE archive_id=?1 AND state='archived' AND length(CAST(snapshot_json AS BLOB)) <= ?2")
        .bind(archive_id)
        .bind(store::MAX_SNAPSHOT_BYTES as i64)
        .fetch_optional(&mut connection)
        .await;
    connection.close().await?;
    let Some(row) = row? else {
        return Ok(None);
    };
    let archive = store::map_archive(&row)?;
    let worker_paths = paths.clone();
    let source = lease
        .spawn_blocking(move || {
            // Keep the catalog contract: damaged/unrecognized archives are not seed
            // candidates. Lease contention above remains an explicit busy error.
            checked_source(&worker_paths, &archive).ok().flatten()
        })
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "leasing archived program source",
            message: error.to_string(),
        })?;
    Ok(source.map(|source| (source, lease)))
}

/// Candidate identities are a bounded database snapshot, not a filesystem lease.
/// A copier revalidates each candidate under its own source lease; catalog writers
/// for unrelated archives therefore cannot prevent seed admission.
pub(crate) async fn archived_program_source_ids(
    paths: &StoragePaths,
    module_id: &str,
) -> Result<Vec<String>, StorageError> {
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .read_only(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT archive_id FROM instance_archives WHERE module_id=?1 AND state='archived' AND purpose='archive' ORDER BY archive_id LIMIT 4097",
    )
    .bind(module_id)
    .fetch_all(&mut connection)
    .await;
    connection.close().await?;
    let ids = ids?;
    if ids.len() > store::MAX_ARCHIVES {
        return Err(invalid(
            &paths.database_path,
            "Archived program inventory exceeds 4096 entries.",
        ));
    }
    Ok(ids)
}

async fn read_sources(
    connection: &mut SqliteConnection,
    paths: &StoragePaths,
    lock: &InstanceSettingsLock,
) -> Result<Vec<ArchivedProgramSource>, StorageError> {
    let has_archives: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='instance_archives')",
    )
    .fetch_one(&mut *connection)
    .await?;
    if !has_archives {
        return Ok(Vec::new());
    }
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM instance_archives WHERE state='archived'")
            .fetch_one(&mut *connection)
            .await?;
    if count > store::MAX_ARCHIVES as i64 {
        return Err(invalid(
            &paths.database_path,
            "Archived program inventory exceeds 4096 entries.",
        ));
    }

    // Recovery snapshots can be large. Bounded pages avoid loading every
    // snapshot at once while also avoiding one database query per archive.
    const PAGE_SIZE: i64 = 8;
    let mut offset = 0_i64;
    let mut sources = Vec::new();
    loop {
        let rows = sqlx::query(
            "SELECT * FROM instance_archives WHERE state='archived' \
             AND length(CAST(snapshot_json AS BLOB)) <= ?1 \
             ORDER BY archive_id LIMIT ?2 OFFSET ?3",
        )
        .bind(store::MAX_SNAPSHOT_BYTES as i64)
        .bind(PAGE_SIZE)
        .bind(offset)
        .fetch_all(&mut *connection)
        .await?;
        if rows.is_empty() {
            break;
        }
        offset += rows.len() as i64;
        let archives = rows
            .iter()
            .map(store::map_archive)
            .collect::<Result<Vec<_>, _>>()?;
        let worker_paths = paths.clone();
        let page = lock
            .spawn_blocking(move || {
                archives
                    .into_iter()
                    .filter_map(|archive| {
                        // Damaged or unrecognized archives are not reusable candidates.
                        // Their diagnostic and recovery state remain in the archive UI.
                        checked_source(&worker_paths, &archive).ok().flatten()
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .map_err(|error| StorageError::BlockingTaskFailed {
                operation: "reading archived program sources",
                message: error.to_string(),
            })?;
        sources.extend(page);
    }
    Ok(sources)
}

fn checked_source(
    paths: &StoragePaths,
    archive: &Archive,
) -> Result<Option<ArchivedProgramSource>, StorageError> {
    if archive.state != "archived" || archive.purpose != "archive" {
        return Ok(None);
    }
    store::validate_id(&archive.id)?;
    let snapshot = store::snapshot(archive)?;
    if snapshot.program.is_some() || snapshot.external_program.is_some() {
        return Ok(None);
    }
    let instance = store::instance(&snapshot);
    if store::string(instance, "runtime_mode")? != "independent" {
        return Ok(None);
    }
    validate_archived_files(paths, archive)?;
    let root = archive_path(paths, &archive.leaf)?;
    let install = snapshot.tables["game_installs"]
        .first()
        .ok_or_else(|| invalid(&root, "Archived program registration is missing."))?;
    if store::string(install, "install_state")? != "installed" {
        return Ok(None);
    }
    let current_version = match &install["current_version"] {
        serde_json::Value::Null => None,
        serde_json::Value::String(version) => Some(version.clone()),
        _ => return Err(invalid(&root, "Archived program version is invalid.")),
    };
    Ok(Some(ArchivedProgramSource {
        module_id: store::string(instance, "module_id")?.to_owned(),
        archive_id: archive.id.clone(),
        install_root: root.join("runtime"),
        current_version,
    }))
}

#[cfg(test)]
#[path = "archived_programs_tests.rs"]
mod tests;
