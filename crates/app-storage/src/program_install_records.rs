use std::path::{Path, PathBuf};

use app_core::InstallState;
use sqlx::{Row, Sqlite, SqliteConnection, Transaction, sqlite::SqliteRow};

use crate::instance_isolation::paths::{contains, normalize_path};
use crate::storage_db::{connect_pool, install_state_from_db_value, install_state_to_db_value};
use crate::{GameInstallSyncRecord, StorageError, StoragePaths};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramInstallScope {
    Library,
    Instance,
}

#[derive(Clone, Debug)]
pub struct ProgramInstallRecord {
    pub id: i64,
    pub module_id: String,
    pub install_root: PathBuf,
    pub install_state: InstallState,
    pub current_version: Option<String>,
    pub scope: ProgramInstallScope,
    pub owner_instance_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct InstanceProgramInstall {
    pub instance_id: String,
    pub instance_name: String,
    pub runtime_mode: String,
    pub install: ProgramInstallRecord,
}

pub async fn read_instance_program_install(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<Option<InstanceProgramInstall>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let row = sqlx::query(
            "SELECT g.*, i.id AS instance_id, i.name AS instance_name, i.runtime_mode,
                i.module_id AS instance_module_id,
                (SELECT COUNT(*) FROM instances peer WHERE peer.install_id=g.id) AS installation_reference_count
         FROM instances i JOIN game_installs g ON g.id = i.install_id WHERE i.id = ?1",
        )
        .bind(instance_id)
        .fetch_optional(&pool)
        .await?;
        row.as_ref().map(map_instance_install).transpose()
    }
    .await;
    pool.close().await;
    result
}

pub async fn read_module_instance_installs(
    paths: &StoragePaths,
    module_id: &str,
) -> Result<Vec<InstanceProgramInstall>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let rows = sqlx::query(
            "SELECT g.*, i.id AS instance_id, i.name AS instance_name, i.runtime_mode,
                i.module_id AS instance_module_id,
                (SELECT COUNT(*) FROM instances peer WHERE peer.install_id=g.id) AS installation_reference_count
         FROM instances i JOIN game_installs g ON g.id = i.install_id
         WHERE i.module_id = ?1 ORDER BY i.id LIMIT 4097",
        )
        .bind(module_id)
        .fetch_all(&pool)
        .await?;
        check_bound(rows.len(), Path::new(module_id))?;
        rows.iter().map(map_instance_install).collect()
    }
    .await;
    pool.close().await;
    result
}

pub async fn read_all_instance_program_installs(
    paths: &StoragePaths,
) -> Result<Vec<InstanceProgramInstall>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let rows = sqlx::query(
            "SELECT g.*, i.id AS instance_id, i.name AS instance_name, i.runtime_mode,
                i.module_id AS instance_module_id,
                (SELECT COUNT(*) FROM instances peer WHERE peer.install_id=g.id) AS installation_reference_count
             FROM instances i JOIN game_installs g ON g.id = i.install_id
             WHERE g.scope = 'instance' AND g.install_state = 'installed'
             ORDER BY i.id LIMIT 4097",
        )
        .fetch_all(&pool)
        .await?;
        check_bound(rows.len(), &paths.instances_root)?;
        rows.iter().map(map_instance_install).collect()
    }
    .await;
    pool.close().await;
    result
}

pub async fn read_library_program_install(
    paths: &StoragePaths,
    module_id: &str,
) -> Result<Option<ProgramInstallRecord>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let row = sqlx::query(
            "SELECT * FROM game_installs
         WHERE module_id = ?1 AND scope = 'library'
         ORDER BY (install_state = 'not_installed') ASC,
                  COALESCE(last_verified_at, updated_at) DESC, id DESC LIMIT 1",
        )
        .bind(module_id)
        .fetch_optional(&pool)
        .await?;
        row.as_ref().map(map_install).transpose()
    }
    .await;
    pool.close().await;
    result
}

/// Resolve a concrete directory without treating the newest instance as a library.
pub async fn read_program_install_owner(
    paths: &StoragePaths,
    install_root: &Path,
) -> Result<Option<ProgramInstallRecord>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let root = normalize_path(install_root)?;
        let rows = sqlx::query("SELECT * FROM game_installs ORDER BY id DESC LIMIT 4097")
            .fetch_all(&pool)
            .await?;
        check_bound(rows.len(), install_root)?;
        let mut found = None;
        for row in &rows {
            let record = map_install(row)?;
            if same_path(&normalize_path(&record.install_root)?, &root) {
                if found.is_some() {
                    return Err(invalid(
                        install_root,
                        "multiple installation records own this path",
                    ));
                }
                found = Some(record);
            }
        }
        Ok(found)
    }
    .await;
    pool.close().await;
    result
}

/// An instance update is explicit; a library scan can never change this record.
pub async fn sync_instance_game_install(
    paths: &StoragePaths,
    instance_id: &str,
    update: &GameInstallSyncRecord,
) -> Result<(), StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query(
            "SELECT g.* FROM game_installs g JOIN instances i ON i.install_id = g.id
         WHERE i.id = ?1 AND i.runtime_mode = 'independent'
           AND g.module_id = i.module_id
           AND ((g.scope = 'instance' AND g.owner_instance_id = i.id)
             OR (g.scope = 'library' AND (SELECT COUNT(*) FROM instances peer WHERE peer.install_id=g.id)=1))",
        )
        .bind(instance_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            invalid(
                Path::new(&update.install_root),
                "instance-owned installation is missing",
            )
        })?;
        let record = map_install(&row)?;
        if record.module_id != update.module_id
            || !same_path(
                &normalize_path(&record.install_root)?,
                &normalize_path(Path::new(&update.install_root))?,
            )
        {
            return Err(invalid(
                Path::new(&update.install_root),
                "installation update does not match its instance owner",
            ));
        }
        sqlx::query(
            "UPDATE game_installs SET install_state = ?2, current_version = ?3,
            last_verified_at = CASE WHEN ?4 THEN CURRENT_TIMESTAMP ELSE last_verified_at END,
            updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        )
        .bind(record.id)
        .bind(install_state_to_db_value(&update.install_state))
        .bind(&update.current_version)
        .bind(update.mark_verified)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    pool.close().await;
    result
}

/// The caller holds the module lifecycle lease and compensates its directory
/// rename if this transaction does not commit. This function never moves files.
pub async fn adopt_library_install_for_instance(
    tx: &mut Transaction<'_, Sqlite>,
    module_id: &str,
    library_install_id: i64,
    instance_id: &str,
    runtime_root: &Path,
) -> Result<i64, StorageError> {
    let target = validate_instance_target(tx, module_id, instance_id, runtime_root).await?;
    let source = require_library(tx, module_id, library_install_id).await?;
    let other_references: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM instances WHERE install_id = ?1 AND id != ?2")
            .bind(source.id)
            .bind(instance_id)
            .fetch_one(&mut **tx)
            .await?;
    if other_references != 0 {
        return Err(invalid(
            &source.install_root,
            "another instance still references this library",
        ));
    }
    ensure_instance_root_available(tx, instance_id, &target).await?;
    let owned: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM game_installs WHERE scope = 'instance' AND owner_instance_id = ?1",
    )
    .bind(instance_id)
    .fetch_optional(&mut **tx)
    .await?;
    if owned.is_some() {
        return Err(invalid(&target, "instance already owns an installation"));
    }
    sqlx::query(
        "UPDATE game_installs SET scope = 'instance', owner_instance_id = ?2,
            install_root = ?3, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
    )
    .bind(source.id)
    .bind(instance_id)
    .bind(runtime_root.to_string_lossy().as_ref())
    .execute(&mut **tx)
    .await?;
    bind_instance(tx, instance_id, source.id, "independent").await?;
    Ok(source.id)
}

pub async fn bind_shared_install_to_instance(
    tx: &mut Transaction<'_, Sqlite>,
    module_id: &str,
    library_install_id: i64,
    instance_id: &str,
) -> Result<(), StorageError> {
    require_instance(tx, module_id, instance_id).await?;
    let source = require_library(tx, module_id, library_install_id).await?;
    let exclusive: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM instances WHERE install_id=?1 AND runtime_mode='independent' AND id<>?2)",
    ).bind(source.id).bind(instance_id).fetch_one(&mut **tx).await?;
    if exclusive || crate::program_exclusive::library_was_exclusively_used(&source.install_root)? {
        return Err(invalid(
            &source.install_root,
            "installation is exclusively used by another instance",
        ));
    }
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM game_installs WHERE owner_instance_id = ?1)",
    )
    .bind(instance_id)
    .fetch_one(&mut **tx)
    .await?;
    if owned {
        return Err(invalid(
            &source.install_root,
            "independent installation must be retained explicitly before sharing",
        ));
    }
    bind_instance(tx, instance_id, source.id, "shared").await
}

pub(crate) async fn bind_exclusive_install_to_instance(
    tx: &mut Transaction<'_, Sqlite>,
    module_id: &str,
    library_install_id: i64,
    instance_id: &str,
) -> Result<(), StorageError> {
    let config = require_instance(tx, module_id, instance_id).await?;
    let source = require_library(tx, module_id, library_install_id).await?;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM instances WHERE install_id=?1 AND id<>?2")
            .bind(source.id)
            .bind(instance_id)
            .fetch_one(&mut **tx)
            .await?;
    if count != 0 {
        return Err(invalid(
            &source.install_root,
            "installation already has an instance reference",
        ));
    }
    let source_root = normalize_path(&source.install_root)?;
    let other_roots: Vec<String> =
        sqlx::query_scalar("SELECT install_root FROM game_installs WHERE id<>?1 LIMIT 4097")
            .bind(source.id)
            .fetch_all(&mut **tx)
            .await?;
    check_bound(other_roots.len(), &source.install_root)?;
    for path in other_roots {
        let other = normalize_path(Path::new(&path))?;
        if contains(&source_root, &other) || contains(&other, &source_root) {
            return Err(invalid(
                &source.install_root,
                "exclusive installation overlaps another installation record",
            ));
        }
    }
    let root = config
        .parent()
        .ok_or_else(|| invalid(&config, "instance root is missing"))?;
    if !crate::instance_uses_exclusive_program(root)?
        || !same_path(
            &normalize_path(&crate::resolve_instance_runtime_root(root)?)?,
            &normalize_path(&source.install_root)?,
        )
    {
        return Err(invalid(
            &source.install_root,
            "exclusive installation binding does not match the registered library",
        ));
    }
    bind_instance(tx, instance_id, source.id, "independent").await
}

/// Register a newly prepared independent runtime or update its verified state.
/// A shared instance may detach to its own runtime under the caller's lease.
pub async fn register_instance_install(
    tx: &mut Transaction<'_, Sqlite>,
    module_id: &str,
    instance_id: &str,
    runtime_root: &Path,
    install_state: InstallState,
    current_version: Option<&str>,
) -> Result<i64, StorageError> {
    let target = validate_instance_target(tx, module_id, instance_id, runtime_root).await?;
    ensure_instance_root_available(tx, instance_id, &target).await?;
    let existing = sqlx::query(
        "SELECT * FROM game_installs WHERE scope = 'instance' AND owner_instance_id = ?1",
    )
    .bind(instance_id)
    .fetch_optional(&mut **tx)
    .await?;
    let state = install_state_to_db_value(&install_state);
    let id = if let Some(row) = existing {
        let record = map_install(&row)?;
        if record.module_id != module_id
            || !same_path(&normalize_path(&record.install_root)?, &target)
        {
            return Err(invalid(
                runtime_root,
                "instance installation identity cannot be replaced implicitly",
            ));
        }
        sqlx::query(
            "UPDATE game_installs SET install_state = ?2, current_version = ?3,
                last_verified_at = CASE WHEN ?2 = 'installed' THEN CURRENT_TIMESTAMP ELSE NULL END,
                updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        )
        .bind(record.id)
        .bind(state)
        .bind(current_version)
        .execute(&mut **tx)
        .await?;
        record.id
    } else {
        sqlx::query(
            "INSERT INTO game_installs
             (module_id, install_root, install_state, current_version, scope, owner_instance_id, last_verified_at)
             VALUES (?1, ?2, ?3, ?4, 'instance', ?5,
                CASE WHEN ?3 = 'installed' THEN CURRENT_TIMESTAMP ELSE NULL END)",
        )
        .bind(module_id).bind(runtime_root.to_string_lossy().as_ref()).bind(state)
        .bind(current_version).bind(instance_id).execute(&mut **tx).await?.last_insert_rowid()
    };
    bind_instance(tx, instance_id, id, "independent").await?;
    Ok(id)
}

/// Preflight before invoking an installer, while its lifecycle lease is held.
pub async fn ensure_library_program_target_available(
    paths: &StoragePaths,
    install_root: &Path,
) -> Result<(), StorageError> {
    ensure_library_program_path_isolated(paths, install_root).await?;
    let root = normalize_path(install_root)?;
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut connection = pool.acquire().await?;
        ensure_no_pending_library_removal(&mut connection, &root).await
    }
    .await;
    pool.close().await;
    result
}

/// Check only ownership and data boundaries. Removal recovery uses this check
/// while retaining its own durable intent; it does not authorize a new install.
pub async fn ensure_library_program_path_isolated(
    paths: &StoragePaths,
    install_root: &Path,
) -> Result<(), StorageError> {
    let root = normalize_path(install_root)?;
    let instances = normalize_path(&paths.instances_root)?;
    if contains(&root, &instances) || contains(&instances, &root) {
        return Err(invalid(
            install_root,
            "library path overlaps managed instance data",
        ));
    }
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut connection = pool.acquire().await?;
        ensure_library_install_root_available(&mut connection, &root).await
    }
    .await;
    pool.close().await;
    result
}

pub(crate) async fn library_install_removal_pending(
    connection: &mut SqliteConnection,
    install_id: i64,
) -> Result<bool, StorageError> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM program_removals WHERE install_id=?1)")
            .bind(install_id)
            .fetch_one(connection)
            .await?,
    )
}

async fn ensure_no_pending_library_removal(
    connection: &mut SqliteConnection,
    root: &Path,
) -> Result<(), StorageError> {
    let pending: Vec<String> =
        sqlx::query_scalar("SELECT source_root FROM program_removals LIMIT 4097")
            .fetch_all(connection)
            .await?;
    check_bound(pending.len(), root)?;
    for source in pending {
        let source = normalize_path(Path::new(&source))?;
        if contains(root, &source) || contains(&source, root) {
            return Err(pending_removal_error(root));
        }
    }
    Ok(())
}

fn pending_removal_error(root: &Path) -> StorageError {
    invalid(
        root,
        "此程序目录有未完成的卸载任务；请再次卸载以完成或恢复，再安装或创建实例。",
    )
}

pub(crate) async fn ensure_library_install_root_available(
    connection: &mut SqliteConnection,
    install_root: &Path,
) -> Result<(), StorageError> {
    let root = normalize_path(install_root)?;
    let rows =
        sqlx::query("SELECT install_root FROM game_installs WHERE scope = 'instance' LIMIT 4097")
            .fetch_all(connection)
            .await?;
    check_bound(rows.len(), install_root)?;
    for row in rows {
        let owned = normalize_path(Path::new(&row.get::<String, _>("install_root")))?;
        if contains(&root, &owned) || contains(&owned, &root) {
            return Err(invalid(
                install_root,
                "library path overlaps an instance-owned installation",
            ));
        }
    }
    Ok(())
}

pub(crate) async fn resolve_library_install_id(
    connection: &mut SqliteConnection,
    module_id: &str,
    install_root: &Path,
) -> Result<Option<i64>, StorageError> {
    ensure_library_install_root_available(connection, install_root).await?;
    let root = normalize_path(install_root)?;
    let rows = sqlx::query("SELECT * FROM game_installs WHERE scope = 'library' LIMIT 4097")
        .fetch_all(connection)
        .await?;
    check_bound(rows.len(), install_root)?;
    let mut found = None;
    for row in &rows {
        let record = map_install(row)?;
        if !same_path(&normalize_path(&record.install_root)?, &root) {
            continue;
        }
        if record.module_id != module_id {
            return Err(invalid(
                install_root,
                "library path belongs to another module",
            ));
        }
        if found.replace(record.id).is_some() {
            return Err(invalid(
                install_root,
                "multiple installation records own this path",
            ));
        }
    }
    Ok(found)
}

async fn ensure_instance_root_available(
    connection: &mut SqliteConnection,
    instance_id: &str,
    root: &Path,
) -> Result<(), StorageError> {
    let rows = sqlx::query(
        "SELECT install_root FROM game_installs
         WHERE owner_instance_id IS NULL OR owner_instance_id != ?1 LIMIT 4097",
    )
    .bind(instance_id)
    .fetch_all(connection)
    .await?;
    check_bound(rows.len(), root)?;
    for row in rows {
        let other = normalize_path(Path::new(&row.get::<String, _>("install_root")))?;
        if contains(root, &other) || contains(&other, root) {
            return Err(invalid(root, "runtime path overlaps another installation"));
        }
    }
    Ok(())
}

async fn require_instance(
    connection: &mut SqliteConnection,
    module_id: &str,
    instance_id: &str,
) -> Result<PathBuf, StorageError> {
    let row = sqlx::query("SELECT module_id, config_path FROM instances WHERE id = ?1")
        .bind(instance_id)
        .fetch_optional(connection)
        .await?
        .ok_or_else(|| StorageError::MissingInstance {
            id: instance_id.to_owned(),
        })?;
    let config = PathBuf::from(row.get::<String, _>("config_path"));
    if row.get::<String, _>("module_id") != module_id {
        return Err(invalid(&config, "instance and installation modules differ"));
    }
    Ok(config)
}

async fn validate_instance_target(
    connection: &mut SqliteConnection,
    module_id: &str,
    instance_id: &str,
    runtime_root: &Path,
) -> Result<PathBuf, StorageError> {
    let config = require_instance(connection, module_id, instance_id).await?;
    if !config
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("config"))
    {
        return Err(invalid(
            &config,
            "instance config directory is not recognized",
        ));
    }
    let expected = normalize_path(
        &config
            .parent()
            .ok_or_else(|| invalid(&config, "instance root is missing"))?
            .join("runtime"),
    )?;
    let actual = normalize_path(runtime_root)?;
    if !same_path(&expected, &actual) {
        return Err(invalid(
            runtime_root,
            "independent installation must be the instance runtime directory",
        ));
    }
    Ok(actual)
}

async fn require_library(
    connection: &mut SqliteConnection,
    module_id: &str,
    install_id: i64,
) -> Result<ProgramInstallRecord, StorageError> {
    let row = sqlx::query("SELECT * FROM game_installs WHERE id = ?1")
        .bind(install_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or_else(|| invalid(Path::new(module_id), "library installation is missing"))?;
    let record = map_install(&row)?;
    if library_install_removal_pending(connection, install_id).await? {
        return Err(pending_removal_error(&record.install_root));
    }
    if record.module_id != module_id
        || record.scope != ProgramInstallScope::Library
        || record.install_state != InstallState::Installed
    {
        return Err(invalid(
            &record.install_root,
            "an installed library for the same module is required",
        ));
    }
    Ok(record)
}

async fn bind_instance(
    connection: &mut SqliteConnection,
    instance_id: &str,
    install_id: i64,
    mode: &str,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE instances SET install_id = ?2, runtime_mode = ?3, updated_at = CURRENT_TIMESTAMP WHERE id = ?1")
        .bind(instance_id).bind(install_id).bind(mode).execute(connection).await?;
    Ok(())
}

fn map_install(row: &SqliteRow) -> Result<ProgramInstallRecord, StorageError> {
    let install_root = PathBuf::from(row.try_get::<String, _>("install_root")?);
    let owner_instance_id: Option<String> = row.try_get("owner_instance_id")?;
    let scope = match (
        row.try_get::<String, _>("scope")?.as_str(),
        &owner_instance_id,
    ) {
        ("library", None) => ProgramInstallScope::Library,
        ("instance", Some(_)) => ProgramInstallScope::Instance,
        _ => {
            return Err(invalid(
                &install_root,
                "invalid installation ownership record",
            ));
        }
    };
    Ok(ProgramInstallRecord {
        id: row.try_get("id")?,
        module_id: row.try_get("module_id")?,
        install_root,
        install_state: install_state_from_db_value(Some(
            &row.try_get::<String, _>("install_state")?,
        )),
        current_version: row.try_get("current_version")?,
        scope,
        owner_instance_id,
    })
}

fn map_instance_install(row: &SqliteRow) -> Result<InstanceProgramInstall, StorageError> {
    let install = map_install(row)?;
    let instance_id: String = row.try_get("instance_id")?;
    let runtime_mode: String = row.try_get("runtime_mode")?;
    let valid = match runtime_mode.as_str() {
        "shared" => install.scope == ProgramInstallScope::Library,
        "independent" => {
            (install.scope == ProgramInstallScope::Library
                && row.try_get::<i64, _>("installation_reference_count")? == 1)
                || (install.scope == ProgramInstallScope::Instance
                    && install.owner_instance_id.as_deref() == Some(instance_id.as_str()))
        }
        _ => false,
    };
    if !valid || row.try_get::<String, _>("instance_module_id")? != install.module_id {
        return Err(invalid(
            &install.install_root,
            "instance program reference does not match its ownership",
        ));
    }
    Ok(InstanceProgramInstall {
        instance_id,
        instance_name: row.try_get("instance_name")?,
        runtime_mode,
        install,
    })
}

fn same_path(left: &Path, right: &Path) -> bool {
    contains(left, right) && contains(right, left)
}

fn check_bound(count: usize, path: &Path) -> Result<(), StorageError> {
    if count > 4096 {
        return Err(invalid(
            path,
            "installation ownership inspection exceeds 4096 records",
        ));
    }
    Ok(())
}

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::InvalidInstancePath {
        path: path.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "program_install_records_tests.rs"]
mod tests;
