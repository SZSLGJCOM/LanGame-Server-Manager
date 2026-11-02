use super::*;
use crate::instance_archive_store::{self as store, Archive};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};

const MAX_ROWS: usize = 100_000;

#[derive(Default)]
pub(super) struct Database {
    pub rows: Vec<SqlPatch>,
    pub archives: Vec<Archive>,
    pub cursors: Vec<Cursor>,
}

pub(super) struct Cursor {
    pub patch: SqlPatch,
    pub source: PathBuf,
}

pub(super) async fn read(paths: &StoragePaths) -> Result<Database, StorageError> {
    if !exists(&paths.database_path)? {
        return Ok(Database::default());
    }
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .read_only(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let result = read_snapshot(&mut connection, paths).await;
    connection.close().await?;
    result
}

async fn read_snapshot(
    connection: &mut SqliteConnection,
    paths: &StoragePaths,
) -> Result<Database, StorageError> {
    let mut tx = connection.begin().await?;
    let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM program_removals)")
        .fetch_one(&mut *tx)
        .await?;
    if pending {
        return Err(invalid(
            &paths.database_path,
            "Finish the pending program removal before moving storage; its recovery journal retains old paths and filesystem identities.",
        ));
    }
    let mut database = Database::default();
    let instances: Vec<String> = sqlx::query_scalar(
        "SELECT json_object('id',id,'module_id',module_id,'status',status,'install_id',install_id,'data_path',data_path,'config_path',config_path,'logs_path',logs_path,'saves_path',saves_path,'env_json',env_json,'args_json',args_json,'runtime_mode',runtime_mode) FROM instances ORDER BY id LIMIT 100001",
    ).fetch_all(&mut *tx).await?;
    let installs: Vec<String> = sqlx::query_scalar(
        "SELECT json_object('id',id,'module_id',module_id,'install_root',install_root,'scope',scope,'owner_instance_id',owner_instance_id) FROM game_installs ORDER BY id LIMIT 100001",
    ).fetch_all(&mut *tx).await?;
    let runs: Vec<String> = sqlx::query_scalar(
        "SELECT json_object('id',id,'instance_id',instance_id,'status',status,'log_path',log_path,'process_image_path',process_image_path) FROM instance_runs ORDER BY id LIMIT 100001",
    ).fetch_all(&mut *tx).await?;
    for (table, key, values) in [
        ("instances", "id", instances),
        ("game_installs", "id", installs),
        ("instance_runs", "id", runs),
    ] {
        if values.len() > MAX_ROWS {
            return Err(invalid(
                &paths.database_path,
                format!("Too many {table} records to prepare a bounded migration."),
            ));
        }
        for value in values {
            let mut before: BTreeMap<String, Value> = serde_json::from_str(&value)?;
            let key_value = before
                .remove(key)
                .ok_or_else(|| invalid(&paths.database_path, "Migration row key is missing."))?;
            if matches!(table, "instances" | "instance_runs") {
                let status = row_text(&before, "status", &paths.database_path)?;
                if matches!(status, "starting" | "running" | "stopping") {
                    return Err(invalid(
                        &paths.database_path,
                        "Stop every instance and finish its recorded run before moving storage.",
                    ));
                }
            }
            database.rows.push(SqlPatch {
                table: table.to_owned(),
                key_column: key.to_owned(),
                key_value,
                after: before.clone(),
                before,
            });
        }
    }
    let records = sqlx::query("SELECT * FROM instance_archives ORDER BY archive_id LIMIT 4097")
        .fetch_all(&mut *tx)
        .await?;
    if records.len() > store::MAX_ARCHIVES {
        return Err(invalid(
            &paths.database_path,
            "Too many instance archives to migrate safely.",
        ));
    }
    database.archives = records
        .iter()
        .map(store::map_archive)
        .collect::<Result<_, _>>()?;
    let cursors = sqlx::query("SELECT c.run_id,c.byte_offset,c.generation,c.source_identity,c.completed,r.log_path FROM player_history_cursors c JOIN instance_runs r ON r.id=c.run_id WHERE c.source_identity IS NOT NULL ORDER BY c.run_id LIMIT 100001")
        .fetch_all(&mut *tx).await?;
    if cursors.len() > MAX_ROWS {
        return Err(invalid(
            &paths.database_path,
            "Too many player history cursors to migrate safely.",
        ));
    }
    for row in cursors {
        let before = BTreeMap::from([
            (
                "byte_offset".to_owned(),
                Value::from(row.try_get::<i64, _>("byte_offset")?),
            ),
            (
                "generation".to_owned(),
                Value::from(row.try_get::<i64, _>("generation")?),
            ),
            (
                "completed".to_owned(),
                Value::from(row.try_get::<i64, _>("completed")?),
            ),
            (
                "source_identity".to_owned(),
                Value::from(row.try_get::<String, _>("source_identity")?),
            ),
        ]);
        if let Some(source) = row.try_get::<Option<String>, _>("log_path")? {
            database.cursors.push(Cursor {
                source: PathBuf::from(source),
                patch: SqlPatch {
                    table: "player_history_cursors".into(),
                    key_column: "run_id".into(),
                    key_value: Value::from(row.try_get::<i64, _>("run_id")?),
                    after: before.clone(),
                    before,
                },
            });
        }
    }
    tx.rollback().await?;
    Ok(database)
}

pub(super) fn prepare_rows(
    database: &Database,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    for original in &database.rows {
        let mut patch = original.clone();
        let columns: &[&str] = match patch.table.as_str() {
            "instances" => &["data_path", "config_path", "logs_path", "saves_path"],
            "game_installs" => &["install_root"],
            "instance_runs" => &["log_path", "process_image_path"],
            _ => {
                return Err(invalid(
                    Path::new(&patch.table),
                    "Unsupported migration table.",
                ));
            }
        };
        for column in columns {
            let value = patch.after.get_mut(*column).ok_or_else(|| {
                invalid(Path::new(&patch.table), "Missing migration path column.")
            })?;
            rewrite_path(value, moves, Path::new(&patch.table))?;
        }
        if copied && patch.before != patch.after {
            plan.rows.push(patch);
        }
    }
    for cursor in &database.cursors {
        if !moved(&cursor.source, moves)? {
            continue;
        }
        if !exists(&cursor.source)? {
            if cursor.patch.before.get("completed").and_then(Value::as_i64) == Some(1) {
                continue;
            }
            return Err(invalid(
                &cursor.source,
                "An unfinished player history cursor refers to a missing log; finish history collection before moving it.",
            ));
        }
        let expected = row_text(&cursor.patch.before, "source_identity", &cursor.source)?;
        let next = rebind_identity(&cursor.source, expected, false, moves, copied)?;
        if copied && next != expected {
            console::verify_copy(&cursor.source, &remap_path(&cursor.source, moves)?)?;
            let mut patch = cursor.patch.clone();
            patch
                .after
                .insert("source_identity".into(), Value::String(next));
            plan.rows.push(patch);
        }
    }
    Ok(())
}

pub(super) fn rewrite_saved_rows(
    rows: &mut BTreeMap<String, Vec<store::SavedRow>>,
    moves: &[RootMove],
    path: &Path,
) -> Result<(), StorageError> {
    for (table, columns) in [
        (
            "instances",
            &["data_path", "config_path", "logs_path", "saves_path"][..],
        ),
        ("game_installs", &["install_root"][..]),
        ("instance_runs", &["log_path", "process_image_path"][..]),
    ] {
        if let Some(records) = rows.get_mut(table) {
            for record in records {
                for column in columns {
                    let value = record.get_mut(*column).ok_or_else(|| {
                        invalid(path, "Archive migration path column is missing.")
                    })?;
                    rewrite_path(value, moves, path)?;
                }
            }
        }
    }
    Ok(())
}
