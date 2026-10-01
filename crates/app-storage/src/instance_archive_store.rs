use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection, SqlitePool};

use crate::{StorageError, StoragePaths};

pub(crate) const MAX_ARCHIVES: usize = 4096;
pub(crate) const MAX_SNAPSHOT_BYTES: usize = 32 * 1024 * 1024;
const MAX_SNAPSHOT_ROWS: i64 = 100_000;
pub(crate) type SavedRow = BTreeMap<String, Value>;

const TABLES: [(&str, &str); 6] = [
    (
        "instances",
        "id,name,module_id,bind_ip,install_id,status,data_path,config_path,logs_path,saves_path,env_json,args_json,autostart,crash_restart_limit,auto_backup_on_stop,backup_retention_count,created_at,updated_at,runtime_mode",
    ),
    (
        "game_installs",
        "id,module_id,install_root,install_state,current_version,last_verified_at,created_at,updated_at,scope,owner_instance_id",
    ),
    (
        "instance_ports",
        "id,instance_id,name,port,protocol,created_at",
    ),
    (
        "instance_runs",
        "id,instance_id,pid,status,started_at,stopped_at,exit_code,crash_flag,log_path,session_id,process_key,display_name,is_primary,process_creation_time,process_image_path",
    ),
    (
        "instance_broadcast_policies",
        "instance_id,enabled,rules_json,updated_at_unix_ms",
    ),
    (
        "instance_broadcast_events",
        "event_id,instance_id,module_id,source,rule_id,message,ai_provider,ai_model,action_id,transport,command_preview,status,response_text,error_message,created_at_unix_ms,initiator,policy_snapshot_json",
    ),
];

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Snapshot {
    pub version: u32,
    pub tables: BTreeMap<String, Vec<SavedRow>>,
    pub config_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored_config_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<crate::instance_archive::program::ProgramPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_retention_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_saves_backup_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_saves_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_program: Option<crate::instance_archive::external::ExternalProgramPlan>,
}

#[derive(Clone, Debug)]
pub(crate) struct Archive {
    pub id: String,
    pub instance_id: Option<String>,
    pub instance_name: Option<String>,
    pub module_id: Option<String>,
    pub deleted_at: Option<i64>,
    pub original_root: Option<String>,
    pub leaf: String,
    pub identity: Option<String>,
    pub snapshot: Option<String>,
    pub snapshot_hash: Option<String>,
    pub external_saves: Option<String>,
    pub state: String,
    pub problem: Option<String>,
    pub purpose: String,
    pub instances_identity: Option<String>,
    pub parent_identity: Option<String>,
    pub restore_staging_identity: Option<String>,
}

pub(crate) fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::InvalidInstancePath {
        path: path.to_owned(),
        message: message.into(),
    }
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) async fn archive_ids(pool: &SqlitePool) -> Result<Vec<String>, StorageError> {
    let rows = sqlx::query_scalar("SELECT archive_id FROM instance_archives WHERE state NOT IN ('restored','purged') ORDER BY deleted_at_unix_ms DESC, archive_id LIMIT 4097")
        .fetch_all(pool).await?;
    if rows.len() > MAX_ARCHIVES {
        return Err(invalid(
            Path::new(".trash"),
            "Archive inventory exceeds 4096 entries; no entries were discarded.",
        ));
    }
    Ok(rows)
}

pub(crate) async fn load(pool: &SqlitePool, id: &str) -> Result<Archive, StorageError> {
    validate_id(id)?;
    let row = sqlx::query("SELECT * FROM instance_archives WHERE archive_id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| invalid(Path::new(".trash"), "Archive ID is not registered."))?;
    map_archive(&row)
}

pub(crate) fn validate_id(id: &str) -> Result<(), StorageError> {
    if uuid::Uuid::parse_str(id).is_err() || id.len() != 36 {
        return Err(invalid(
            Path::new(".trash"),
            "An application-issued archive ID is required.",
        ));
    }
    Ok(())
}

pub(crate) fn map_archive(row: &sqlx::sqlite::SqliteRow) -> Result<Archive, StorageError> {
    Ok(Archive {
        id: row.try_get("archive_id")?,
        instance_id: row.try_get("instance_id")?,
        instance_name: row.try_get("instance_name")?,
        module_id: row.try_get("module_id")?,
        deleted_at: row.try_get("deleted_at_unix_ms")?,
        original_root: row.try_get("original_root")?,
        leaf: row.try_get("archive_leaf")?,
        identity: row.try_get("directory_identity_json")?,
        snapshot: row.try_get("snapshot_json")?,
        snapshot_hash: row.try_get("snapshot_sha256")?,
        external_saves: row.try_get("preserved_external_saves_path")?,
        state: row.try_get("state")?,
        problem: row.try_get("problem")?,
        purpose: migration_four_text(row, "purpose")?.unwrap_or_else(|| "archive".into()),
        instances_identity: migration_four_text(row, "instances_root_identity_json")?,
        parent_identity: migration_four_text(row, "archive_parent_identity_json")?,
        restore_staging_identity: migration_four_text(row, "restore_staging_identity_json")?,
    })
}

// The read-only catalog may run before startup applies migration 0004.
fn migration_four_text(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<String>, sqlx::Error> {
    match row.try_get(column) {
        Err(sqlx::Error::ColumnNotFound(_)) => Ok(None),
        result => result,
    }
}

pub(crate) async fn set_state(
    pool: &SqlitePool,
    id: &str,
    state: &str,
    problem: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE instance_archives SET state = ?2, problem = ?3, updated_at = CURRENT_TIMESTAMP WHERE archive_id = ?1")
        .bind(id).bind(state).bind(problem).execute(pool).await?;
    Ok(())
}

pub(crate) async fn capture(
    connection: &mut SqliteConnection,
    instance_id: &str,
    config_sha256: String,
) -> Result<Snapshot, StorageError> {
    let mut tables = BTreeMap::new();
    let mut total = 0usize;
    for (table, columns) in TABLES {
        let object = format!(
            "json_object({})",
            columns
                .split(',')
                .map(|name| format!("'{name}',{name}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut query = QueryBuilder::<Sqlite>::new(format!(
            "SELECT COUNT(*) AS n, COALESCE(SUM(length({object})),0) AS bytes FROM {table} WHERE "
        ));
        filter_instance_rows(&mut query, table, instance_id);
        let limits = query.build().fetch_one(&mut *connection).await?;
        let count: i64 = limits.try_get("n")?;
        let bytes: i64 = limits.try_get("bytes")?;
        total = total.saturating_add(usize::try_from(bytes).unwrap_or(usize::MAX));
        if count > MAX_SNAPSHOT_ROWS || total > MAX_SNAPSHOT_BYTES {
            return Err(invalid(
                Path::new(instance_id),
                "Instance archive snapshot exceeds its row or byte limit; deletion was refused without discarding history.",
            ));
        }
        let mut query = QueryBuilder::<Sqlite>::new(format!("SELECT {object} FROM {table} WHERE "));
        filter_instance_rows(&mut query, table, instance_id);
        query.push(" ORDER BY rowid");
        let rows: Vec<String> = query
            .build_query_scalar()
            .fetch_all(&mut *connection)
            .await?;
        tables.insert(
            table.to_owned(),
            rows.iter()
                .map(|row| serde_json::from_str(row))
                .collect::<Result<_, _>>()?,
        );
    }
    Ok(Snapshot {
        version: 1,
        tables,
        config_sha256,
        restored_config_sha256: None,
        program: None,
        program_retention_reason: None,
        external_saves_backup_id: None,
        effective_saves_path: None,
        external_program: None,
    })
}

fn filter_instance_rows(query: &mut QueryBuilder<Sqlite>, table: &str, instance_id: &str) {
    query
        .push(match table {
            "instances" => "id = ",
            "game_installs" => "id = (SELECT install_id FROM instances WHERE id = ",
            _ => "instance_id = ",
        })
        .push_bind(instance_id.to_owned());
    if table == "game_installs" {
        query.push(")");
    }
}

pub(crate) fn snapshot(archive: &Archive) -> Result<Snapshot, StorageError> {
    let text = archive.snapshot.as_deref().ok_or_else(|| {
        invalid(
            Path::new(&archive.leaf),
            "This archive has no complete recovery metadata.",
        )
    })?;
    if text.len() > MAX_SNAPSHOT_BYTES
        || archive.snapshot_hash.as_deref() != Some(digest(text.as_bytes()).as_str())
    {
        return Err(invalid(
            Path::new(&archive.leaf),
            "Archive recovery metadata checksum or size is invalid.",
        ));
    }
    let snapshot: Snapshot = serde_json::from_str(text)?;
    if !matches!(snapshot.version, 1..=3)
        || snapshot.tables.len() != TABLES.len()
        || (snapshot.version == 1
            && (snapshot.program.is_some()
                || snapshot.restored_config_sha256.is_some()
                || snapshot.external_saves_backup_id.is_some()))
    {
        return Err(invalid(
            Path::new(&archive.leaf),
            "Archive recovery metadata version or table set is unsupported.",
        ));
    }
    for (table, columns) in TABLES {
        let rows = snapshot.tables.get(table).ok_or_else(|| {
            invalid(
                Path::new(&archive.leaf),
                "Archive recovery table is missing.",
            )
        })?;
        if rows.len() as i64 > MAX_SNAPSHOT_ROWS
            || (table == "instances" && rows.len() != 1)
            || (table == "game_installs" && rows.len() > 1)
        {
            return Err(invalid(
                Path::new(&archive.leaf),
                "Archive recovery row counts are invalid.",
            ));
        }
        for row in rows {
            if row.len() != columns.split(',').count()
                || columns.split(',').any(|column| !row.contains_key(column))
            {
                return Err(invalid(
                    Path::new(&archive.leaf),
                    "Archive recovery columns do not match this schema.",
                ));
            }
            let owner = if table == "instances" {
                row.get("id")
            } else if table == "game_installs" {
                None
            } else {
                row.get("instance_id")
            };
            if owner.is_some_and(|value| value.as_str() != archive.instance_id.as_deref()) {
                return Err(invalid(
                    Path::new(&archive.leaf),
                    "Archive recovery row belongs to another instance.",
                ));
            }
        }
    }
    if let Some(program) = &snapshot.program {
        program.validate(archive.module_id.as_deref().unwrap_or_default())?;
    }
    if let Some(program) = &snapshot.external_program {
        if snapshot.version < 3 || snapshot.program.is_some() {
            return Err(invalid(
                Path::new(&archive.leaf),
                "External archive metadata is inconsistent.",
            ));
        }
        program.validate()?;
    }
    Ok(snapshot)
}

pub(crate) fn instance(snapshot: &Snapshot) -> &SavedRow {
    &snapshot.tables["instances"][0]
}

pub(crate) fn string<'a>(row: &'a SavedRow, key: &str) -> Result<&'a str, StorageError> {
    row.get(key).and_then(Value::as_str).ok_or_else(|| {
        invalid(
            Path::new(".trash"),
            format!("Archive field {key} is not text."),
        )
    })
}

pub(crate) async fn insert_row(
    connection: &mut SqliteConnection,
    table: &str,
    row: &SavedRow,
) -> Result<(), StorageError> {
    let (_, columns) = TABLES
        .iter()
        .find(|(name, _)| *name == table)
        .ok_or_else(|| invalid(Path::new(".trash"), "Unrecognized archive table."))?;
    let names: Vec<_> = columns.split(',').collect();
    // Both identifiers come from TABLES; all archived values remain bound parameters.
    let mut query =
        QueryBuilder::<Sqlite>::new(format!("INSERT INTO {table} ({columns}) VALUES ("));
    for (index, name) in names.into_iter().enumerate() {
        if index > 0 {
            query.push(",");
        }
        match &row[name] {
            Value::Null => {
                query.push_bind(Option::<String>::None);
            }
            Value::String(value) => {
                query.push_bind(value.clone());
            }
            Value::Number(value) if value.as_i64().is_some() => {
                query.push_bind(value.as_i64().unwrap());
            }
            _ => {
                return Err(invalid(
                    Path::new(".trash"),
                    format!("Unsupported value in archived {table}.{name}."),
                ));
            }
        }
    }
    query.push(")").build().execute(connection).await?;
    Ok(())
}

pub(crate) async fn ensure_unreferenced(
    connection: &mut SqliteConnection,
    paths: &StoragePaths,
    archive_root: &Path,
) -> Result<(), StorageError> {
    let rows: Vec<String> = sqlx::query_scalar("SELECT data_path FROM instances UNION SELECT config_path FROM instances UNION SELECT logs_path FROM instances UNION SELECT saves_path FROM instances UNION SELECT install_root FROM game_installs UNION SELECT preserved_external_saves_path FROM instance_archives WHERE state NOT IN ('restored','purged') AND preserved_external_saves_path IS NOT NULL LIMIT 24577")
        .fetch_all(connection).await?;
    check_references(paths, archive_root, rows)
}

pub(crate) async fn ensure_instance_root_unshared(
    connection: &mut SqliteConnection,
    paths: &StoragePaths,
    root: &Path,
    instance_id: &str,
) -> Result<(), StorageError> {
    let rows: Vec<String> = sqlx::query_scalar("SELECT data_path FROM instances WHERE id<>?1 UNION SELECT config_path FROM instances WHERE id<>?1 UNION SELECT logs_path FROM instances WHERE id<>?1 UNION SELECT saves_path FROM instances WHERE id<>?1 UNION SELECT install_root FROM game_installs WHERE scope<>'instance' OR owner_instance_id IS NULL OR owner_instance_id<>?1 UNION SELECT preserved_external_saves_path FROM instance_archives WHERE state NOT IN ('restored','purged') AND preserved_external_saves_path IS NOT NULL LIMIT 24577")
        .bind(instance_id).fetch_all(connection).await?;
    check_references(paths, root, rows)
}

fn check_references(
    paths: &StoragePaths,
    archive_root: &Path,
    rows: Vec<String>,
) -> Result<(), StorageError> {
    use crate::instance_isolation::paths::{contains, normalize_path};
    if rows.len() > 24576 {
        return Err(invalid(
            archive_root,
            "Too many registered paths to establish safe archive ownership.",
        ));
    }
    let root = normalize_path(archive_root)?;
    for path in rows {
        let candidate = normalize_path(Path::new(&path))?;
        if contains(&root, &candidate) || contains(&candidate, &root) {
            return Err(invalid(
                archive_root,
                format!(
                    "Archive overlaps a registered instance or program path: {}",
                    candidate.display()
                ),
            ));
        }
    }
    for protected in [
        paths.games_root.as_path(),
        paths.modules_root.as_path(),
        paths.app_data_root.as_path(),
        paths.steamcmd_root.as_path(),
        paths.logs_root.as_path(),
        paths.database_path.parent().unwrap_or(&paths.database_path),
        paths.settings_path.parent().unwrap_or(&paths.settings_path),
    ] {
        let protected = normalize_path(protected)?;
        if contains(&root, &protected) {
            return Err(invalid(
                archive_root,
                "Archive contains an application storage root.",
            ));
        }
    }
    Ok(())
}
