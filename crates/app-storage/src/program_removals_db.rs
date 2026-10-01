//! Database commit authority for explicitly requested library program removals.
use std::path::{Path, PathBuf};

use sqlx::{Row, SqliteConnection};

use crate::instance_isolation::paths::{contains, normalize_path};
use crate::program_runtime::invalid;
use crate::storage_db::connect_pool;
use crate::{StorageError, StoragePaths};

const MAX_REMOVALS: usize = 4_096;
const MAX_JOURNAL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ProgramRemovalRecord {
    pub operation_id: String,
    pub module_id: String,
    pub install_id: i64,
    pub source_root: PathBuf,
    pub phase: String,
    pub journal_json: String,
}

pub async fn begin(
    paths: &StoragePaths,
    record: &ProgramRemovalRecord,
) -> Result<(), StorageError> {
    if record.phase != "prepared"
        || record.journal_json.len() > MAX_JOURNAL_BYTES
        || uuid::Uuid::parse_str(&record.operation_id)
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(&record.operation_id)
    {
        return Err(invalid(
            &record.source_root,
            "invalid program removal intent",
        ));
    }
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_owner(&mut tx, record).await?;
        sqlx::query("INSERT INTO program_removals(operation_id,module_id,install_id,source_root,phase,journal_json) VALUES(?1,?2,?3,?4,'prepared',?5)")
            .bind(&record.operation_id).bind(&record.module_id).bind(record.install_id)
            .bind(record.source_root.to_string_lossy().as_ref()).bind(&record.journal_json)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }.await;
    pool.close().await;
    result
}

pub async fn list(
    paths: &StoragePaths,
    module_id: &str,
) -> Result<Vec<ProgramRemovalRecord>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let rows = sqlx::query("SELECT operation_id,module_id,install_id,source_root,phase,journal_json FROM program_removals WHERE module_id=?1 ORDER BY created_at,operation_id LIMIT 4097")
            .bind(module_id).fetch_all(&pool).await?;
        if rows.len() > MAX_REMOVALS {
            return Err(invalid(Path::new(module_id), "program removal recovery inventory exceeds its limit"));
        }
        rows.into_iter().map(|row| {
            let record = ProgramRemovalRecord {
                operation_id: row.try_get("operation_id")?,
                module_id: row.try_get("module_id")?,
                install_id: row.try_get("install_id")?,
                source_root: PathBuf::from(row.try_get::<String, _>("source_root")?),
                phase: row.try_get("phase")?,
                journal_json: row.try_get("journal_json")?,
            };
            if record.journal_json.len() > MAX_JOURNAL_BYTES || !matches!(record.phase.as_str(), "prepared" | "committed") {
                return Err(invalid(&record.source_root, "invalid program removal recovery record"));
            }
            Ok(record)
        }).collect()
    }.await;
    pool.close().await;
    result
}

/// State and commit authority change in one SQLite transaction. A caller whose
/// commit returns an error must reload this phase before choosing compensation.
pub async fn commit(
    paths: &StoragePaths,
    record: &ProgramRemovalRecord,
) -> Result<(), StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_record(&mut tx, record, "prepared").await?;
        validate_owner(&mut tx, record).await?;
        mark_uninstalled(&mut tx, record.install_id).await?;
        sqlx::query("UPDATE program_removals SET phase='committed' WHERE operation_id=?1 AND phase='prepared'")
            .bind(&record.operation_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }.await;
    pool.close().await;
    result
}

pub async fn validate_recovery(
    paths: &StoragePaths,
    record: &ProgramRemovalRecord,
) -> Result<(), StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin().await?;
        validate_record(&mut tx, record, &record.phase).await?;
        validate_owner(&mut tx, record).await?;
        if record.phase == "committed" {
            let state: String =
                sqlx::query_scalar("SELECT install_state FROM game_installs WHERE id=?1")
                    .bind(record.install_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if state != "not_installed" {
                return Err(invalid(
                    &record.source_root,
                    "committed removal no longer owns the uninstalled registration",
                ));
            }
        }
        tx.rollback().await?;
        Ok(())
    }
    .await;
    pool.close().await;
    result
}

pub async fn finish(
    paths: &StoragePaths,
    record: &ProgramRemovalRecord,
) -> Result<(), StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_record(&mut tx, record, &record.phase).await?;
        sqlx::query("DELETE FROM program_removals WHERE operation_id=?1 AND phase=?2")
            .bind(&record.operation_id)
            .bind(&record.phase)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    pool.close().await;
    result
}

pub async fn mark_missing_removed(
    paths: &StoragePaths,
    module_id: &str,
    install_id: i64,
    source_root: &Path,
) -> Result<(), StorageError> {
    let pool = connect_pool(paths).await?;
    let record = ProgramRemovalRecord {
        operation_id: String::new(),
        module_id: module_id.to_owned(),
        install_id,
        source_root: source_root.to_owned(),
        phase: String::new(),
        journal_json: String::new(),
    };
    let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_owner(&mut tx, &record).await?;
        let pending: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM program_removals WHERE install_id=?1)")
                .bind(install_id)
                .fetch_one(&mut *tx)
                .await?;
        if pending {
            return Err(invalid(
                source_root,
                "recover the pending program removal first",
            ));
        }
        if source_root
            .try_exists()
            .map_err(|source| StorageError::ReadPath {
                path: source_root.to_owned(),
                source,
            })?
        {
            return Err(invalid(
                source_root,
                "program removal source is no longer absent",
            ));
        }
        mark_uninstalled(&mut tx, install_id).await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    pool.close().await;
    result
}

async fn mark_uninstalled(
    connection: &mut SqliteConnection,
    install_id: i64,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE game_installs SET install_state='not_installed',current_version=NULL,last_verified_at=NULL,updated_at=CURRENT_TIMESTAMP WHERE id=?1")
        .bind(install_id).execute(connection).await?;
    Ok(())
}

async fn validate_record(
    connection: &mut SqliteConnection,
    record: &ProgramRemovalRecord,
    phase: &str,
) -> Result<(), StorageError> {
    let matches: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM program_removals WHERE operation_id=?1 AND module_id=?2 AND install_id=?3 AND source_root=?4 AND phase=?5 AND journal_json=?6)")
        .bind(&record.operation_id).bind(&record.module_id).bind(record.install_id)
        .bind(record.source_root.to_string_lossy().as_ref()).bind(phase).bind(&record.journal_json)
        .fetch_one(connection).await?;
    if !matches {
        return Err(invalid(
            &record.source_root,
            "program removal journal changed; existing files were preserved",
        ));
    }
    Ok(())
}

async fn validate_owner(
    connection: &mut SqliteConnection,
    record: &ProgramRemovalRecord,
) -> Result<(), StorageError> {
    let row = sqlx::query(
        "SELECT module_id,install_root,scope,owner_instance_id FROM game_installs WHERE id=?1",
    )
    .bind(record.install_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| {
        invalid(
            &record.source_root,
            "program removal registration is missing",
        )
    })?;
    let root = normalize_path(Path::new(&row.try_get::<String, _>("install_root")?))?;
    let expected = normalize_path(&record.source_root)?;
    if row.try_get::<String, _>("module_id")? != record.module_id
        || row.try_get::<String, _>("scope")? != "library"
        || row
            .try_get::<Option<String>, _>("owner_instance_id")?
            .is_some()
        || !contains(&root, &expected)
        || !contains(&expected, &root)
    {
        return Err(invalid(
            &record.source_root,
            "program removal registration no longer owns this library",
        ));
    }
    let referenced: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM instances WHERE install_id=?1)")
            .bind(record.install_id)
            .fetch_one(connection)
            .await?;
    if referenced {
        return Err(invalid(
            &record.source_root,
            "an instance still references this program removal source",
        ));
    }
    Ok(())
}
