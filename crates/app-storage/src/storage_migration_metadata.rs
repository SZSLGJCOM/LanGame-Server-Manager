//! Read-only preparation of typed storage migration metadata changes.
//!
//! The coordinator owns publication, rollback, database CAS, and source cleanup.
//! Preparation never treats an equal destination payload as new ownership proof:
//! source ownership is checked before its verified copy receives new identities.
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::instance_archive_files::{self as owned, native};
use crate::instance_archive_store::{self as archive_store, digest};
use crate::instance_isolation::paths::{contains, normalize_resource_path};
use crate::storage_migration_files::{RootMove, remap_path, remap_stored_path};
use crate::{StorageError, StoragePaths};

#[path = "storage_migration_metadata_archives.rs"]
mod archives;
#[path = "storage_migration_metadata_clusters.rs"]
mod clusters;
#[path = "storage_migration_metadata_console.rs"]
mod console;
#[path = "storage_migration_metadata_db.rs"]
mod db;
#[path = "storage_migration_metadata_documents.rs"]
mod documents;
#[path = "storage_migration_metadata_mods.rs"]
mod mods;
#[path = "storage_migration_metadata_preflight.rs"]
mod preflight;
#[path = "storage_migration_metadata_startup.rs"]
mod startup;

const MAX_JSON_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PLAN_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct FilePatch {
    pub path: PathBuf,
    pub before: Vec<u8>,
    pub before_sha256: String,
    pub after: Vec<u8>,
}

/// All identifiers originate in the fixed projections in `db`; the journal
/// executor must also validate its allowlist when reading an on-disk plan.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SqlPatch {
    pub table: String,
    pub key_column: String,
    pub key_value: Value,
    pub before: BTreeMap<String, Value>,
    pub after: BTreeMap<String, Value>,
}

#[derive(Default, Debug, Deserialize, Serialize)]
pub(crate) struct MetadataPlan {
    pub files: Vec<FilePatch>,
    pub rows: Vec<SqlPatch>,
}

pub(crate) async fn preflight_metadata(
    paths: &StoragePaths,
    moves: &[RootMove],
) -> Result<(), StorageError> {
    prepare(paths, moves, false).await.map(|_| ())
}

/// Call only after the file plan has copied and verified every source payload.
/// The source must remain intact until all returned patches are committed.
pub(crate) async fn prepare_metadata(
    paths: &StoragePaths,
    moves: &[RootMove],
) -> Result<MetadataPlan, StorageError> {
    prepare(paths, moves, true).await
}

/// Include archived installations: their program revision keys remain needed
/// even while the original runtime directory is absent until restoration.
pub(crate) async fn program_roots(
    paths: &StoragePaths,
) -> Result<Vec<(String, PathBuf)>, StorageError> {
    let database = db::read(paths).await?;
    let database_path = paths.database_path.clone();
    tokio::task::spawn_blocking(move || {
        let mut roots = BTreeMap::new();
        let mut insert = |module: &str, root: &Path| -> Result<(), StorageError> {
            if module.is_empty() {
                return Err(invalid(
                    &database_path,
                    "Program module identity is missing.",
                ));
            }
            let normalized = normalize_resource_path(root)?;
            roots
                .entry((module.to_owned(), normalized))
                .or_insert_with(|| root.to_owned());
            if roots.len() > 24_576 {
                return Err(invalid(
                    &database_path,
                    "Too many program roots to verify a migration.",
                ));
            }
            Ok(())
        };
        for row in &database.rows {
            if row.table == "game_installs" {
                insert(
                    row_text(&row.before, "module_id", &database_path)?,
                    Path::new(row_text(&row.before, "install_root", &database_path)?),
                )?;
            }
        }
        for archive in &database.archives {
            if matches!(archive.state.as_str(), "restored" | "purged") {
                continue;
            }
            if archive.state != "archived" || archive.purpose != "archive" {
                return Err(invalid(
                    &database_path,
                    "Finish pending archive recovery before moving program revisions.",
                ));
            }
            let snapshot = archive_store::snapshot(archive)?;
            for row in snapshot.tables.get("game_installs").into_iter().flatten() {
                insert(
                    row_text(row, "module_id", &database_path)?,
                    Path::new(row_text(row, "install_root", &database_path)?),
                )?;
            }
            if let Some(external) = snapshot.external_program {
                external.validate()?;
                let module = archive.module_id.as_deref().ok_or_else(|| {
                    invalid(
                        &database_path,
                        "Archived program module identity is missing.",
                    )
                })?;
                insert(module, &external.root)?;
            }
        }
        Ok(roots
            .into_iter()
            .map(|((module, _), root)| (module, root))
            .collect())
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "discovering program revision migration roots",
        message: error.to_string(),
    })?
}

async fn prepare(
    paths: &StoragePaths,
    moves: &[RootMove],
    copied: bool,
) -> Result<MetadataPlan, StorageError> {
    let database = db::read(paths).await?;
    let paths = paths.clone();
    let moves: Vec<_> = moves
        .iter()
        .map(|entry| RootMove {
            source: entry.source.clone(),
            target: entry.target.clone(),
        })
        .collect();
    tokio::task::spawn_blocking(move || {
        let mut plan = MetadataPlan::default();
        preflight::validate(&paths, &moves, &database, copied, &mut plan)?;
        documents::prepare(&paths, &moves, &database, copied, &mut plan)?;
        archives::prepare(&paths, &moves, &database, copied, &mut plan)?;
        clusters::prepare(&paths, &moves, copied, &mut plan)?;
        db::prepare_rows(&database, &moves, copied, &mut plan)?;
        plan.files.sort_by(|left, right| left.path.cmp(&right.path));
        if plan
            .files
            .windows(2)
            .any(|pair| pair[0].path == pair[1].path)
        {
            return Err(invalid(
                &paths.app_data_root,
                "Migration metadata contains duplicate file patches.",
            ));
        }
        Ok(plan)
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "preparing storage migration metadata",
        message: error.to_string(),
    })?
}

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::InvalidStorageMigration {
        path: path.to_owned(),
        message: message.into(),
    }
}

fn same_path(left: &Path, right: &Path) -> Result<bool, StorageError> {
    let left = normalize_resource_path(left)?;
    let right = normalize_resource_path(right)?;
    Ok(contains(&left, &right) && contains(&right, &left))
}

fn moved(path: &Path, moves: &[RootMove]) -> Result<bool, StorageError> {
    Ok(!same_path(path, &remap_path(path, moves)?)?)
}

fn read_bytes(path: &Path, limit: u64) -> Result<Vec<u8>, StorageError> {
    let mut node =
        native::open_verified_file(path, false).map_err(|source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        })?;
    let mut bytes = Vec::new();
    node.reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > limit {
        return Err(invalid(path, "Migration metadata exceeds its size limit."));
    }
    Ok(bytes)
}

fn read_json(path: &Path) -> Result<(Vec<u8>, Value), StorageError> {
    let bytes = read_bytes(path, MAX_JSON_BYTES)?;
    let value = serde_json::from_slice(&bytes)?;
    Ok((bytes, value))
}

fn exists(path: &Path) -> Result<bool, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StorageError::ReadPath {
            path: path.to_owned(),
            source,
        }),
    }
}

fn text<'a>(value: &'a Value, key: &str, path: &Path) -> Result<&'a str, StorageError> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| {
        invalid(
            path,
            format!("Migration metadata field {key} must be a string."),
        )
    })
}

fn row_text<'a>(
    row: &'a BTreeMap<String, Value>,
    key: &str,
    path: &Path,
) -> Result<&'a str, StorageError> {
    row.get(key).and_then(Value::as_str).ok_or_else(|| {
        invalid(
            path,
            format!("Stored migration field {key} must be a string."),
        )
    })
}

fn rewrite_path(value: &mut Value, moves: &[RootMove], path: &Path) -> Result<(), StorageError> {
    if value.is_null() {
        return Ok(());
    }
    let old = value
        .as_str()
        .ok_or_else(|| invalid(path, "Stored path is not a string."))?;
    if old.is_empty() {
        return Ok(());
    }
    let old_path = Path::new(old);
    if !old_path.is_absolute() {
        return Err(invalid(
            path,
            format!("Stored managed path must be absolute: {old}"),
        ));
    }
    // Ordinary persisted paths must use the configured root's spelling: archive
    // and instance admission compare their components directly with settings.
    // Canonical-only fields (package baselines and SteamCMD ownership) are
    // handled explicitly by their document preparers instead.
    let next = remap_stored_path(old_path, moves)?;
    if !same_path(old_path, &next)? {
        *value = Value::String(next.to_string_lossy().into_owned());
    }
    Ok(())
}

fn rewrite_field(
    value: &mut Value,
    key: &str,
    moves: &[RootMove],
    path: &Path,
) -> Result<(), StorageError> {
    let field = value
        .get_mut(key)
        .ok_or_else(|| invalid(path, format!("Missing managed path field {key}.")))?;
    rewrite_path(field, moves, path)
}

fn add_json_patch(
    source: &Path,
    before: &[u8],
    value: &Value,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let original: Value = serde_json::from_slice(before)?;
    if !copied || original == *value {
        return Ok(());
    }
    let target = remap_path(source, moves)?;
    if read_bytes(&target, MAX_JSON_BYTES)? != before {
        return Err(invalid(
            &target,
            "Copied metadata differs from its validated source; no ownership was rebound.",
        ));
    }
    let after = serde_json::to_vec_pretty(value)?;
    let total: usize = plan
        .files
        .iter()
        .map(|patch| patch.before.len().saturating_add(patch.after.len()))
        .sum();
    if total
        .saturating_add(before.len())
        .saturating_add(after.len())
        > MAX_PLAN_BYTES
    {
        return Err(invalid(
            &target,
            "Migration metadata plan exceeds its byte limit.",
        ));
    }
    plan.files.push(FilePatch {
        path: target,
        before: before.to_owned(),
        before_sha256: digest(before),
        after,
    });
    Ok(())
}

fn identity(path: &Path, directory: bool) -> Result<String, StorageError> {
    let node = native::open(path, directory, false).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    let value = node.identity().map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    Ok(serde_json::to_string(&value)?)
}

fn rebind_identity(
    source: &Path,
    expected: &str,
    directory: bool,
    moves: &[RootMove],
    copied: bool,
) -> Result<String, StorageError> {
    if identity(source, directory)? != expected {
        return Err(invalid(
            source,
            "Source filesystem identity differs from its ownership metadata.",
        ));
    }
    if copied {
        identity(&remap_path(source, moves)?, directory)
    } else {
        Ok(expected.to_owned())
    }
}

#[cfg(test)]
#[path = "storage_migration_metadata_tests.rs"]
mod tests;
