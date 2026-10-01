use super::*;
use crate::migration_history::verify_existing_migration_history;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DatabaseFileIdentity {
    file_system_id: u64,
    file_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DatabaseFileObservation {
    exists: bool,
    identity: Option<DatabaseFileIdentity>,
}

enum StorageInitializationAttempt {
    Ready(SqlitePool),
    Retry,
}

const STORAGE_INITIALIZATION_MAX_ATTEMPTS: usize = 3;

static STORAGE_READY_DATABASES: OnceLock<
    tokio::sync::Mutex<HashMap<PathBuf, DatabaseFileIdentity>>,
> = OnceLock::new();

#[cfg(test)]
static STORAGE_FULL_READINESS_PASSES: OnceLock<std::sync::Mutex<HashMap<PathBuf, u64>>> =
    OnceLock::new();

#[cfg(test)]
type StorageBeforeInitOpenHook = Box<dyn FnOnce() + Send + 'static>;

#[cfg(test)]
static STORAGE_BEFORE_INIT_OPEN_HOOKS: OnceLock<
    std::sync::Mutex<HashMap<PathBuf, StorageBeforeInitOpenHook>>,
> = OnceLock::new();

pub async fn initialize_database(paths: &StoragePaths) -> Result<StorageStatus, StorageError> {
    let pool = connect_pool(paths).await?;

    let schema_version: i64 = sqlx::query_scalar("PRAGMA user_version;")
        .fetch_one(&pool)
        .await?;

    pool.close().await;

    Ok(StorageStatus {
        database_path: paths.database_path.to_string_lossy().into_owned(),
        migrations_path: paths.migrations_root.to_string_lossy().into_owned(),
        app_log_path: paths.app_log_path().to_string_lossy().into_owned(),
        database_exists: true,
        schema_version,
        migrations_applied: true,
    })
}

pub async fn sync_modules(
    paths: &StoragePaths,
    descriptors: &[ModuleDescriptor],
) -> Result<Vec<ModuleSummary>, StorageError> {
    let pool = connect_pool(paths).await?;

    let mut tx = pool.begin().await?;

    for descriptor in descriptors {
        let supported_platforms = descriptor.summary.supported_platforms.join(",");

        sqlx::query(
            r#"
            INSERT INTO modules (
                id, name, version, description, steam_app_id, manifest_toml, schema_json, supported_platforms, root_path
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                version = excluded.version,
                description = excluded.description,
                steam_app_id = excluded.steam_app_id,
                manifest_toml = excluded.manifest_toml,
                schema_json = excluded.schema_json,
                supported_platforms = excluded.supported_platforms,
                root_path = excluded.root_path,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(&descriptor.summary.id)
        .bind(&descriptor.summary.name)
        .bind(&descriptor.summary.version)
        .bind(&descriptor.summary.description)
        .bind(descriptor.summary.steam_app_id.map(i64::from))
        .bind(&descriptor.manifest_toml)
        .bind(&descriptor.schema_json)
        .bind(supported_platforms)
        .bind(descriptor.root.to_string_lossy().into_owned())
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let modules = load_modules(&pool).await?;
    pool.close().await;
    Ok(modules)
}

pub async fn sync_game_installs(
    paths: &StoragePaths,
    records: &[GameInstallSyncRecord],
) -> Result<(), StorageError> {
    if records.is_empty() {
        return Ok(());
    }

    let pool = connect_pool(paths).await?;
    let result = async {
        // Path identity is resolved before insert; serialize that read/write pair.
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        for record in records {
            upsert_game_install_record(&mut tx, record).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    .await;
    pool.close().await;
    result
}

pub(crate) async fn connect_pool(paths: &StoragePaths) -> Result<SqlitePool, StorageError> {
    let database_key = storage_ready_key(paths);
    if let Some(pool) = open_cached_pool(paths, &database_key).await? {
        return Ok(pool);
    }

    let _guard = storage_init_lock().lock().await;
    if let Some(pool) = open_cached_pool(paths, &database_key).await? {
        return Ok(pool);
    }
    storage_ready_databases().lock().await.remove(&database_key);

    for _ in 0..STORAGE_INITIALIZATION_MAX_ATTEMPTS {
        match initialize_storage_pool_once(paths, &database_key).await? {
            StorageInitializationAttempt::Ready(pool) => return Ok(pool),
            StorageInitializationAttempt::Retry => continue,
        }
    }

    Err(StorageError::DatabaseIdentityChanged {
        path: paths.database_path.clone(),
    })
}

async fn initialize_storage_pool_once(
    paths: &StoragePaths,
    database_key: &Path,
) -> Result<StorageInitializationAttempt, StorageError> {
    let observation_before_open = observe_database_file(&paths.database_path).await?;
    run_storage_before_init_open_hook(&paths.database_path);
    let pool = match open_pool(paths, !observation_before_open.exists).await {
        Ok(pool) => pool,
        Err(open_error) => {
            let observation_after_error = observe_database_file(&paths.database_path).await?;
            if observation_after_error != observation_before_open {
                return Ok(StorageInitializationAttempt::Retry);
            }
            return Err(open_error);
        }
    };
    let observation_after_open = match observe_database_file(&paths.database_path).await {
        Ok(observation) => observation,
        Err(error) => {
            pool.close().await;
            return Err(error);
        }
    };

    let opened_expected_file = if observation_before_open.exists {
        observations_refer_to_same_file(observation_before_open, observation_after_open)
    } else {
        observation_after_open.exists
    };
    if !opened_expected_file {
        pool.close().await;
        return Ok(StorageInitializationAttempt::Retry);
    }

    #[cfg(test)]
    record_storage_full_readiness_pass(database_key);

    if observation_before_open.exists
        && let Err(error) = verify_existing_migration_history(&pool).await
    {
        pool.close().await;
        return Err(error);
    }

    if let Err(error) = MIGRATOR.run(&pool).await {
        pool.close().await;
        return Err(error.into());
    }

    if let Err(error) = validate_storage_schema(&pool).await {
        pool.close().await;
        return Err(error);
    }

    let observation_after_validation = match observe_database_file(&paths.database_path).await {
        Ok(observation) => observation,
        Err(error) => {
            pool.close().await;
            return Err(error);
        }
    };
    if !observations_refer_to_same_file(observation_after_open, observation_after_validation) {
        pool.close().await;
        return Ok(StorageInitializationAttempt::Retry);
    }

    if let Some(identity) = observation_after_validation.identity {
        storage_ready_databases()
            .lock()
            .await
            .insert(database_key.to_path_buf(), identity);
    }

    Ok(StorageInitializationAttempt::Ready(pool))
}

async fn open_cached_pool(
    paths: &StoragePaths,
    database_key: &Path,
) -> Result<Option<SqlitePool>, StorageError> {
    let expected_identity = {
        let ready_databases = storage_ready_databases().lock().await;
        ready_databases.get(database_key).copied()
    };
    let Some(expected_identity) = expected_identity else {
        return Ok(None);
    };

    let observation_before_open = observe_database_file(&paths.database_path).await?;
    let Some(identity_before_open) = observation_before_open.identity else {
        return Ok(None);
    };
    if identity_before_open != expected_identity {
        return Ok(None);
    }

    let pool = match open_pool(paths, false).await {
        Ok(pool) => pool,
        Err(open_error) => {
            let observation_after_error = observe_database_file(&paths.database_path).await?;
            if observation_after_error.identity == Some(identity_before_open) {
                return Err(open_error);
            }
            return Ok(None);
        }
    };
    let observation_after_open = match observe_database_file(&paths.database_path).await {
        Ok(observation) => observation,
        Err(error) => {
            pool.close().await;
            return Err(error);
        }
    };
    if observation_after_open.identity != Some(identity_before_open) {
        pool.close().await;
        return Ok(None);
    }

    Ok(Some(pool))
}

fn observations_refer_to_same_file(
    expected: DatabaseFileObservation,
    actual: DatabaseFileObservation,
) -> bool {
    if !expected.exists || !actual.exists {
        return false;
    }
    match expected.identity {
        Some(expected_identity) => actual.identity == Some(expected_identity),
        None => true,
    }
}

async fn observe_database_file(
    database_path: &Path,
) -> Result<DatabaseFileObservation, StorageError> {
    let database_path = database_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        match read_database_file_identity_blocking(&database_path) {
            Ok(identity) => Ok(DatabaseFileObservation {
                exists: true,
                identity,
            }),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(DatabaseFileObservation {
                    exists: false,
                    identity: None,
                })
            }
            Err(source) => Err(StorageError::ReadPath {
                path: database_path,
                source,
            }),
        }
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "database file identity",
        message: error.to_string(),
    })?
}

#[cfg(test)]
fn run_storage_before_init_open_hook(database_path: &Path) {
    let hook = STORAGE_BEFORE_INIT_OPEN_HOOKS
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(database_path);
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(not(test))]
fn run_storage_before_init_open_hook(_database_path: &Path) {}

async fn validate_storage_schema(pool: &SqlitePool) -> Result<(), StorageError> {
    validate_current_schema(pool).await
}

async fn open_pool(
    paths: &StoragePaths,
    create_if_missing: bool,
) -> Result<SqlitePool, StorageError> {
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .create_if_missing(create_if_missing)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(30));

    Ok(SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?)
}

fn storage_init_lock() -> &'static tokio::sync::Mutex<()> {
    STORAGE_INIT_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn storage_ready_databases() -> &'static tokio::sync::Mutex<HashMap<PathBuf, DatabaseFileIdentity>>
{
    STORAGE_READY_DATABASES.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn storage_ready_key(paths: &StoragePaths) -> PathBuf {
    paths.database_path.clone()
}

#[cfg(windows)]
fn read_database_file_identity_blocking(
    database_path: &Path,
) -> std::io::Result<Option<DatabaseFileIdentity>> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = fs::File::open(database_path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // Identify the file by its volume and file index, independently of path and write time.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let file_index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Ok(Some(DatabaseFileIdentity {
        file_system_id: u64::from(information.dwVolumeSerialNumber),
        file_id: file_index,
    }))
}

#[cfg(unix)]
fn read_database_file_identity_blocking(
    database_path: &Path,
) -> std::io::Result<Option<DatabaseFileIdentity>> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(database_path)?;
    Ok(Some(DatabaseFileIdentity {
        file_system_id: metadata.dev(),
        file_id: metadata.ino(),
    }))
}

#[cfg(not(any(windows, unix)))]
fn read_database_file_identity_blocking(
    database_path: &Path,
) -> std::io::Result<Option<DatabaseFileIdentity>> {
    let metadata = fs::metadata(database_path)?;
    let created = match metadata.created() {
        Ok(created) => created,
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => return Ok(None),
        Err(error) => return Err(error),
    };
    let elapsed = created
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| error.duration());
    Ok(Some(DatabaseFileIdentity {
        file_system_id: elapsed.as_secs(),
        file_id: u64::from(elapsed.subsec_nanos()),
    }))
}

#[cfg(test)]
fn record_storage_full_readiness_pass(database_path: &Path) {
    let passes =
        STORAGE_FULL_READINESS_PASSES.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut passes = passes.lock().unwrap_or_else(|error| error.into_inner());
    *passes.entry(database_path.to_path_buf()).or_default() += 1;
}

#[cfg(test)]
pub(crate) fn storage_full_readiness_passes_for_test(database_path: &Path) -> u64 {
    STORAGE_FULL_READINESS_PASSES
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(database_path)
        .copied()
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn install_storage_before_init_open_hook_for_test(
    database_path: &Path,
    hook: impl FnOnce() + Send + 'static,
) {
    STORAGE_BEFORE_INIT_OPEN_HOOKS
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(database_path.to_path_buf(), Box::new(hook));
}

pub(crate) async fn validate_current_schema(pool: &SqlitePool) -> Result<(), StorageError> {
    validate_table_columns(
        pool,
        "program_removals",
        &[
            "operation_id",
            "module_id",
            "install_id",
            "source_root",
            "phase",
            "journal_json",
            "created_at",
        ],
    )
    .await?;
    validate_table_columns(
        pool,
        "instance_archives",
        &[
            "archive_id",
            "instance_id",
            "instance_name",
            "module_id",
            "deleted_at_unix_ms",
            "original_root",
            "archive_leaf",
            "directory_identity_json",
            "snapshot_json",
            "snapshot_sha256",
            "preserved_external_saves_path",
            "state",
            "problem",
            "updated_at",
            "purpose",
            "instances_root_identity_json",
            "archive_parent_identity_json",
            "restore_staging_identity_json",
        ],
    )
    .await?;
    validate_table_columns(
        pool,
        "modules",
        &[
            "id",
            "name",
            "version",
            "description",
            "steam_app_id",
            "manifest_toml",
            "schema_json",
            "supported_platforms",
            "root_path",
            "created_at",
            "updated_at",
        ],
    )
    .await?;
    validate_table_columns(
        pool,
        "instances",
        &[
            "id",
            "name",
            "module_id",
            "bind_ip",
            "install_id",
            "runtime_mode",
            "status",
            "data_path",
            "config_path",
            "logs_path",
            "saves_path",
            "env_json",
            "args_json",
            "autostart",
            "crash_restart_limit",
            "auto_backup_on_stop",
            "backup_retention_count",
            "created_at",
            "updated_at",
        ],
    )
    .await?;
    validate_table_columns(
        pool,
        "instance_runs",
        &[
            "id",
            "instance_id",
            "pid",
            "status",
            "started_at",
            "stopped_at",
            "exit_code",
            "crash_flag",
            "log_path",
            "session_id",
            "process_key",
            "display_name",
            "is_primary",
            "process_creation_time",
            "process_image_path",
        ],
    )
    .await?;

    validate_table_columns(
        pool,
        "game_installs",
        &[
            "id",
            "module_id",
            "install_root",
            "install_state",
            "scope",
            "owner_instance_id",
        ],
    )
    .await?;

    Ok(())
}

async fn validate_table_columns(
    pool: &SqlitePool,
    table_name: &str,
    expected_columns: &[&str],
) -> Result<(), StorageError> {
    let present_columns = sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info(?1)")
        .bind(table_name)
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect::<HashSet<_>>();
    let missing_columns = expected_columns
        .iter()
        .filter(|column_name| !present_columns.contains(**column_name))
        .map(|column_name| (*column_name).to_string())
        .collect::<Vec<_>>();

    if missing_columns.is_empty() {
        return Ok(());
    }

    Err(StorageError::SchemaMismatch {
        table: table_name.to_string(),
        missing_columns,
    })
}

async fn upsert_game_install_record(
    tx: &mut Transaction<'_, Sqlite>,
    record: &GameInstallSyncRecord,
) -> Result<Option<i64>, StorageError> {
    let existing_id = crate::program_install_records::resolve_library_install_id(
        tx,
        &record.module_id,
        Path::new(&record.install_root),
    )
    .await?;
    let install_state = install_state_to_db_value(&record.install_state);

    if let Some(existing_id) = existing_id {
        // A probe may observe the removal's temporary scaffold or detached
        // payload. Only the durable removal transaction owns this row until
        // recovery finishes; startup scans must not publish those observations.
        if crate::program_install_records::library_install_removal_pending(tx, existing_id).await? {
            return Ok(Some(existing_id));
        }
        sqlx::query(
            r#"
            UPDATE game_installs
            SET install_state = ?2,
                current_version = COALESCE(?3, current_version),
                last_verified_at = CASE WHEN ?4 THEN CURRENT_TIMESTAMP ELSE last_verified_at END,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1
            "#,
        )
        .bind(existing_id)
        .bind(install_state)
        .bind(&record.current_version)
        .bind(record.mark_verified)
        .execute(&mut **tx)
        .await?;

        return Ok(Some(existing_id));
    }

    if matches!(record.install_state, InstallState::NotInstalled) {
        return Ok(None);
    }

    let query = if record.mark_verified {
        sqlx::query(
            r#"
            INSERT INTO game_installs (
                module_id, install_root, install_state, current_version, last_verified_at
            ) VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
            "#,
        )
    } else {
        sqlx::query(
            r#"
            INSERT INTO game_installs (
                module_id, install_root, install_state, current_version, last_verified_at
            ) VALUES (?1, ?2, ?3, ?4, NULL)
            "#,
        )
    };

    let result = query
        .bind(&record.module_id)
        .bind(&record.install_root)
        .bind(install_state)
        .bind(&record.current_version)
        .execute(&mut **tx)
        .await?;

    Ok(Some(result.last_insert_rowid()))
}

pub(crate) async fn resolve_installed_game_install_id_from_executor<'e, E>(
    executor: E,
    module_id: &str,
) -> Result<Option<i64>, StorageError>
where
    E: Executor<'e, Database = Sqlite>,
{
    Ok(sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM game_installs
        WHERE module_id = ?1 AND install_state = 'installed' AND scope = 'library'
        ORDER BY COALESCE(last_verified_at, updated_at) DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(module_id)
    .fetch_optional(executor)
    .await?)
}

pub(crate) async fn resolve_installed_game_install_id_from_pool(
    pool: &SqlitePool,
    module_id: &str,
) -> Result<Option<i64>, StorageError> {
    resolve_installed_game_install_id_from_executor(pool, module_id).await
}

pub(crate) async fn resolve_module_install_root_from_executor<'e, E>(
    executor: E,
    module_id: &str,
) -> Result<Option<String>, StorageError>
where
    E: Executor<'e, Database = Sqlite>,
{
    Ok(sqlx::query_scalar::<_, String>(
        r#"
        SELECT install_root
        FROM game_installs
        WHERE module_id = ?1 AND scope = 'library' AND install_state = 'installed'
        ORDER BY COALESCE(last_verified_at, updated_at) DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(module_id)
    .fetch_optional(executor)
    .await?)
}

pub(crate) async fn resolve_module_install_root_from_pool(
    pool: &SqlitePool,
    module_id: &str,
) -> Result<Option<String>, StorageError> {
    resolve_module_install_root_from_executor(pool, module_id).await
}

pub async fn resolve_module_install_root(
    paths: &StoragePaths,
    module_id: &str,
) -> Result<Option<String>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = resolve_module_install_root_from_pool(&pool, module_id).await;
    pool.close().await;
    result
}

async fn read_game_install_last_verified_unix_ms_from_connection(
    connection: &mut sqlx::SqliteConnection,
    module_id: &str,
    install_root: &str,
) -> Result<Option<u128>, StorageError> {
    let Some(install_id) = crate::program_install_records::resolve_library_install_id(
        connection,
        module_id,
        Path::new(install_root),
    )
    .await?
    else {
        return Ok(None);
    };
    let timestamp_ms = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT CAST(strftime('%s', last_verified_at) AS INTEGER) * 1000
        FROM game_installs
        WHERE id = ?1
          AND install_state = 'installed'
          AND scope = 'library'
          AND last_verified_at IS NOT NULL
        "#,
    )
    .bind(install_id)
    .fetch_optional(connection)
    .await?;

    Ok(timestamp_ms.and_then(|value| u128::try_from(value).ok()))
}

pub async fn read_game_install_last_verified_unix_ms(
    paths: &StoragePaths,
    module_id: &str,
    install_root: &str,
) -> Result<Option<u128>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut connection = pool.acquire().await?;
        read_game_install_last_verified_unix_ms_from_connection(
            &mut connection,
            module_id,
            install_root,
        )
        .await
    }
    .await;
    pool.close().await;
    result
}
pub(crate) fn install_state_to_db_value(install_state: &InstallState) -> &'static str {
    match install_state {
        InstallState::NotInstalled => "not_installed",
        InstallState::Installing => "installing",
        InstallState::Incomplete => "incomplete",
        InstallState::Installed => "installed",
        InstallState::Updating => "updating",
        InstallState::Uninstalling => "uninstalling",
        InstallState::Corrupted => "corrupted",
    }
}

pub(crate) fn install_state_from_db_value(value: Option<&str>) -> InstallState {
    match value
        .unwrap_or("not_installed")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "installing" => InstallState::Installing,
        "incomplete" | "partial" | "downloading" => InstallState::Incomplete,
        "installed" | "ready" | "ok" => InstallState::Installed,
        "updating" | "validating" => InstallState::Updating,
        "uninstalling" | "removing" => InstallState::Uninstalling,
        "corrupted" | "broken" | "missing_executable" => InstallState::Corrupted,
        _ => InstallState::NotInstalled,
    }
}

async fn load_modules(pool: &SqlitePool) -> Result<Vec<ModuleSummary>, StorageError> {
    let rows = sqlx::query(
        r#"
        SELECT modules.id,
               modules.name,
               modules.version,
               modules.description,
               modules.steam_app_id,
               modules.supported_platforms,
               (
                   SELECT game_installs.install_state
                   FROM game_installs
                   WHERE game_installs.module_id = modules.id
                    ORDER BY (game_installs.install_state = 'installed') DESC,
                             COALESCE(game_installs.last_verified_at, game_installs.updated_at) DESC,
                            game_installs.id DESC
                   LIMIT 1
               ) AS install_state
        FROM modules
        ORDER BY name ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(map_module_summary).collect())
}

pub(crate) async fn load_instances(
    pool: &SqlitePool,
) -> Result<Vec<InstanceSummary>, StorageError> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, module_id, status, bind_ip, autostart,
               (SELECT COUNT(*) FROM instance_ports WHERE instance_id = instances.id) AS port_count,
               (SELECT COUNT(*) FROM instance_runs
                WHERE instance_id = instances.id AND status = 'running') AS active_process_count
        FROM instances
        ORDER BY name ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(map_instance_summary).collect())
}

pub(crate) async fn fetch_instance_record<'e, E>(
    executor: E,
    instance_id: &str,
) -> Result<StoredInstanceRecord, StorageError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(
        r#"
        SELECT id, name, module_id, status, bind_ip, autostart, config_path, saves_path,
               auto_backup_on_stop, backup_retention_count, runtime_mode,
               (SELECT g.install_root FROM game_installs g WHERE g.id = instances.install_id
                AND g.module_id = instances.module_id
                AND ((instances.runtime_mode = 'shared' AND g.scope = 'library')
                  OR (instances.runtime_mode = 'independent'
                      AND ((g.scope = 'instance' AND g.owner_instance_id = instances.id)
                        OR (g.scope = 'library' AND (SELECT COUNT(*) FROM instances peer WHERE peer.install_id=g.id)=1))))) AS program_install_root,
               (SELECT COUNT(*) FROM instance_ports WHERE instance_id = instances.id) AS port_count,
               (SELECT COUNT(*) FROM instance_runs
                WHERE instance_id = instances.id AND status = 'running') AS active_process_count
        FROM instances
        WHERE id = ?1
        "#,
    )
    .bind(instance_id)
    .fetch_optional(executor)
    .await?
    .ok_or_else(|| StorageError::MissingInstance {
        id: String::from(instance_id),
    })?;

    Ok(map_stored_instance(&row))
}

pub(crate) async fn load_module_instance_records(
    pool: &SqlitePool,
    module_id: &str,
) -> Result<Vec<StoredInstanceRecord>, StorageError> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, module_id, status, bind_ip, autostart, config_path, saves_path,
               auto_backup_on_stop, backup_retention_count, runtime_mode,
               (SELECT g.install_root FROM game_installs g WHERE g.id = instances.install_id
                AND g.module_id = instances.module_id
                AND ((instances.runtime_mode = 'shared' AND g.scope = 'library')
                  OR (instances.runtime_mode = 'independent'
                      AND ((g.scope = 'instance' AND g.owner_instance_id = instances.id)
                        OR (g.scope = 'library' AND (SELECT COUNT(*) FROM instances peer WHERE peer.install_id=g.id)=1))))) AS program_install_root,
               (SELECT COUNT(*) FROM instance_ports WHERE instance_id = instances.id) AS port_count,
               (SELECT COUNT(*) FROM instance_runs
                WHERE instance_id = instances.id AND status = 'running') AS active_process_count
        FROM instances
        WHERE module_id = ?1
        ORDER BY id
        "#,
    )
    .bind(module_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(map_stored_instance).collect())
}

/// One database snapshot supplies all registered instance ownership.
pub(crate) async fn load_instance_isolation_records(
    connection: &mut sqlx::SqliteConnection,
) -> Result<Vec<StoredInstanceRecord>, StorageError> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, module_id, status, bind_ip, autostart, config_path, saves_path,
               auto_backup_on_stop, backup_retention_count, runtime_mode,
               (SELECT g.install_root FROM game_installs g WHERE g.id = instances.install_id
                AND g.module_id = instances.module_id
                AND ((instances.runtime_mode = 'shared' AND g.scope = 'library')
                  OR (instances.runtime_mode = 'independent'
                      AND ((g.scope = 'instance' AND g.owner_instance_id = instances.id)
                        OR (g.scope = 'library' AND (SELECT COUNT(*) FROM instances peer WHERE peer.install_id=g.id)=1))))) AS program_install_root,
               (SELECT COUNT(*) FROM instance_ports WHERE instance_id = instances.id) AS port_count,
               (SELECT COUNT(*) FROM instance_runs
                WHERE instance_id = instances.id AND status = 'running') AS active_process_count
        FROM instances ORDER BY id LIMIT 4097
        "#,
    )
    .fetch_all(connection)
    .await?;
    if rows.len() > 4096 {
        return Err(StorageError::InvalidInstancePath {
            path: PathBuf::new(),
            message: String::from(
                "instance path inspection is limited to 4096 registered instances",
            ),
        });
    }
    Ok(rows.iter().map(map_stored_instance).collect())
}

pub(crate) async fn load_instance_ports<'e, E>(
    executor: E,
    instance_id: &str,
) -> Result<Vec<PortBinding>, StorageError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let rows = sqlx::query(
        r#"
        SELECT name, protocol, port
        FROM instance_ports
        WHERE instance_id = ?1
        ORDER BY id ASC
        "#,
    )
    .bind(instance_id)
    .fetch_all(executor)
    .await?;

    Ok(rows.iter().map(map_port_binding).collect())
}
fn map_stored_instance(row: &SqliteRow) -> StoredInstanceRecord {
    StoredInstanceRecord {
        summary: map_instance_summary(row),
        config_dir: PathBuf::from(row.get::<String, _>("config_path")),
        saves_dir: PathBuf::from(row.get::<String, _>("saves_path")),
        runtime_mode: row.get("runtime_mode"),
        program_install_root: row
            .get::<Option<String>, _>("program_install_root")
            .map(PathBuf::from),
        auto_backup_on_stop: row.try_get::<i64, _>("auto_backup_on_stop").unwrap_or(0) != 0,
        backup_retention_count: row
            .try_get::<i64, _>("backup_retention_count")
            .unwrap_or(10)
            .clamp(1, i64::from(u16::MAX)) as u32,
    }
}

fn map_port_binding(row: &SqliteRow) -> PortBinding {
    PortBinding {
        name: row.try_get("name").unwrap_or_else(|_| String::from("port")),
        protocol: row.get("protocol"),
        port: row
            .try_get::<i64, _>("port")
            .unwrap_or(0)
            .clamp(0, i64::from(u16::MAX)) as u16,
    }
}

fn map_module_summary(row: &SqliteRow) -> ModuleSummary {
    let supported_platforms = row
        .try_get::<String, _>("supported_platforms")
        .unwrap_or_else(|_| String::from("windows"));

    ModuleSummary {
        id: row.get("id"),
        name: row.get("name"),
        version: row.get("version"),
        description: row.try_get("description").ok(),
        steam_app_id: row
            .try_get::<Option<i64>, _>("steam_app_id")
            .ok()
            .flatten()
            .map(|value| value as u32),
        install_state: install_state_from_db_value(
            row.try_get::<Option<String>, _>("install_state")
                .ok()
                .flatten()
                .as_deref(),
        ),
        instance_program_count: 0,
        archived_program_count: 0,
        supported_platforms: supported_platforms
            .split(',')
            .filter(|item| !item.is_empty())
            .map(String::from)
            .collect(),
    }
}

pub(crate) fn map_instance_summary(row: &SqliteRow) -> InstanceSummary {
    let status = match row
        .try_get::<String, _>("status")
        .unwrap_or_else(|_| String::from("stopped"))
        .as_str()
    {
        "starting" => InstanceStatus::Starting,
        "running" => InstanceStatus::Running,
        "stopping" => InstanceStatus::Stopping,
        "error" => InstanceStatus::Error,
        _ => InstanceStatus::Stopped,
    };

    InstanceSummary {
        id: row.get("id"),
        name: row.get("name"),
        module_id: row.get("module_id"),
        status,
        active_process_count: row
            .try_get::<i64, _>("active_process_count")
            .unwrap_or(0)
            .max(0) as usize,
        bind_ip: row.get("bind_ip"),
        port_count: row.try_get::<i64, _>("port_count").unwrap_or(0).max(0) as usize,
        autostart: row.try_get::<i64, _>("autostart").unwrap_or(0) > 0,
    }
}
