use std::fs;
use std::path::{Path, PathBuf};

use sqlx::{SqliteConnection, SqlitePool};

use crate::instance_archive::{config, external, program};
use crate::instance_archive_files as files;
use crate::instance_archive_models::InstanceArchiveRestoreResult;
use crate::instance_archive_store::{self as store, Archive, Snapshot, invalid};
use crate::instance_isolation::ensure_instance_paths_available;
use crate::instance_isolation::paths::{contains, normalize_path};
use crate::instance_settings_lock::InstanceSettingsLock;
use crate::instances::validate_managed_instance_root;
use crate::{StorageError, StoragePaths};

#[path = "instance_archive_conflicts.rs"]
mod conflicts;

#[path = "instance_archive_commit.rs"]
mod commit;

#[path = "instance_archive_admission.rs"]
mod admission;
pub(crate) use admission::archive_instance;

pub(super) async fn finish_archiving(
    pool: &SqlitePool,
    paths: &StoragePaths,
    archive: &Archive,
    lock: &InstanceSettingsLock,
) -> Result<(), StorageError> {
    let mut snapshot = store::snapshot(archive)?;
    let original = original_root(paths, archive, &snapshot)?;
    let target = files::archive_path(paths, &archive.leaf)?;
    let payload_root = if files::plain_directory(&original)? {
        &original
    } else {
        &target
    };
    external::prepare_payload(pool, archive, &mut snapshot, payload_root, lock).await?;
    let instance_id = store::string(store::instance(&snapshot), "id")?.to_owned();
    let mut tx = if archive.purpose == "delete" {
        pool.begin_with("BEGIN IMMEDIATE").await?
    } else {
        pool.begin().await?
    };
    commit::validate_archiving(&mut tx, paths, &instance_id, &original, &snapshot).await?;
    let library = if let Some(plan) = &snapshot.program {
        Some(program::library(&mut tx, &plan.module_id).await?.ok_or_else(|| invalid(&target, "Install or repair the archived program version before completing archival."))?.0)
    } else {
        None
    };
    // Permanent deletion retains its existing transaction boundary. Archive
    // copying uses its journal and instance/catalog/program leases instead of
    // holding SQLite's single writer while hashing or copying bytes.
    let deletion_transaction = if archive.purpose == "delete" {
        Some(tx)
    } else {
        tx.rollback().await?;
        None
    };
    let from = original.clone();
    let to = target.clone();
    let identity = archive.identity.clone();
    let hash = snapshot.config_sha256.clone();
    let checked_paths = paths.clone();
    let checked = archive.clone();
    let plan = snapshot.program.clone();
    let external_plan = snapshot.external_program.clone();
    let program_root = library.clone();
    lock.spawn_blocking(move || {
        #[cfg(test)]
        crate::instance_archive::test_gate::pause(&checked_paths.database_path, crate::instance_archive::test_gate::Point::Archiving);
        let _parents = files::guard_parents(&checked_paths, checked.instances_identity.as_deref(), checked.parent_identity.as_deref())?;
        if let (Some(plan), Some(library)) = (&plan, &program_root) { program::verify_library(library, plan)?; }
        match (files::plain_directory(&from)?, files::plain_directory(&to)?, identity.as_deref()) {
            (true, false, Some(expected)) => {
                let current_hash = if checked.purpose == "delete" { config::deletion_hash(&from)? } else { files::config_hash(&from)? };
                if current_hash != hash { return Err(invalid(&from, "Instance configuration changed after archive admission.")); }
                files::preflight_tree(&from, expected)?;
                files::move_directory(&from, &to, expected)?;
            }
            (false, true, Some(expected)) => {
                files::verify_identity(&to, Some(expected))?;
                let current_hash = if checked.purpose == "delete" { config::deletion_hash(&to)? } else { files::config_hash(&to)? };
                if current_hash != hash { return Err(invalid(&to, "Archived configuration changed before database deletion committed.")); }
            }
            (false, false, None) => {}
            _ => return Err(invalid(&to, "Archive transaction paths or ownership conflict; all available files were retained.")),
        }
        #[cfg(test)]
        crate::instance_archive::test_gate::pause(&checked_paths.database_path, crate::instance_archive::test_gate::Point::ArchiveMoved);
        if let (Some(plan), Some(library), Some(expected)) = (&plan, &program_root, identity.as_deref()) { program::omit_files(&to, expected, library, plan)?; }
        if let Some(plan) = &external_plan {
            external::capture_files(&to, plan)?;
            external::cleanup_owned(&to, plan)?;
        }
        Ok(())
    }).await.map_err(|error| worker_error("moving instance to archive", error))??;
    // cleanup_owned has retired any staging left by rollback. Publish that
    // lifecycle reset atomically with the new archived/deletion state: an old
    // restore identity must not imply that its now-removed data is restored.
    if let Some(plan) = snapshot.external_program.as_mut() {
        plan.restore_identity = None;
    }
    let snapshot_json = serde_json::to_string(&snapshot)?;
    let committed = async {
        let mut tx = if let Some(tx) = deletion_transaction { tx } else {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            commit::validate_archiving(&mut tx, paths, &instance_id, &original, &snapshot).await?;
            tx
        };
        sqlx::query("DELETE FROM instances WHERE id=?1").bind(&instance_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE instance_archives SET state=?2,snapshot_json=?3,snapshot_sha256=?4,problem=NULL,updated_at=CURRENT_TIMESTAMP WHERE archive_id=?1").bind(&archive.id).bind(if archive.purpose == "delete" { "purging" } else { "archived" }).bind(&snapshot_json).bind(store::digest(snapshot_json.as_bytes())).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok::<_, StorageError>(())
    }.await;
    if let Err(error) = committed {
        external::prepare_restore(pool, archive, &mut snapshot, lock).await?;
        if let Some(expected) = &archive.identity {
            let staging = if let Some(plan) = &snapshot.program {
                program::prepare_staging(pool, archive, &target, plan, lock).await?
            } else {
                None
            };
            let from = target.clone();
            let to = original.clone();
            let expected = expected.clone();
            let plan = snapshot.program.clone();
            let external_plan = snapshot.external_program.clone();
            let library = library.clone();
            let rollback = lock
                .spawn_blocking(move || {
                    if let Some(plan) = &external_plan {
                        external::restore_files(&from, plan)?;
                        external::finish_restore(&from, plan)?;
                    }
                    if let (Some(plan), Some(library)) = (&plan, &library) {
                        program::restore_files(
                            &from,
                            &expected,
                            library,
                            plan,
                            staging.as_deref(),
                        )?;
                    }
                    files::move_directory(&from, &to, &expected)
                })
                .await
                .map_err(|error| worker_error("rolling back instance archive", error))?;
            if let Err(rollback) = rollback {
                let message = format!("{error}; archive rollback remains pending: {rollback}");
                store::set_state(pool, &archive.id, "archiving", Some(&message)).await?;
                return Err(invalid(&target, message));
            }
        }
        sqlx::query("DELETE FROM instance_archives WHERE archive_id=?1")
            .bind(&archive.id)
            .execute(pool)
            .await?;
        return Err(error);
    }
    Ok(())
}

pub(super) fn original_root(
    paths: &StoragePaths,
    archive: &Archive,
    snapshot: &Snapshot,
) -> Result<PathBuf, StorageError> {
    let original = PathBuf::from(archive.original_root.as_deref().ok_or_else(|| {
        invalid(
            Path::new(&archive.leaf),
            "Archive original directory is missing.",
        )
    })?);
    validate_managed_instance_root(&original, &paths.instances_root)?;
    normalize_path(&original)?;
    let row = store::instance(snapshot);
    if store::string(row, "id")? != archive.instance_id.as_deref().unwrap_or_default()
        || store::string(row, "module_id")? != archive.module_id.as_deref().unwrap_or_default()
        || Path::new(store::string(row, "config_path")?).parent() != Some(original.as_path())
        || !matches!(store::string(row, "status")?, "stopped" | "error")
    {
        return Err(invalid(
            &original,
            "Archive metadata does not describe a stopped instance at its original managed directory.",
        ));
    }
    for field in ["data_path", "config_path", "logs_path"] {
        if !contains(
            &normalize_path(&original)?,
            &normalize_path(Path::new(store::string(row, field)?))?,
        ) {
            return Err(invalid(
                &original,
                "Archived instance paths escape their original directory.",
            ));
        }
    }
    Ok(original)
}

pub(crate) fn validate_archived_files(
    paths: &StoragePaths,
    archive: &Archive,
) -> Result<(), StorageError> {
    let snapshot = store::snapshot(archive)?;
    if archive.purpose != "archive" {
        return Err(invalid(
            Path::new(&archive.leaf),
            "A pending deletion cannot be restored as an archive.",
        ));
    }
    let _parents = files::guard_parents(
        paths,
        archive.instances_identity.as_deref(),
        archive.parent_identity.as_deref(),
    )?;
    let original = original_root(paths, archive, &snapshot)?;
    let archived = files::archive_path(paths, &archive.leaf)?;
    let root = if archive.state == "restoring" && !files::plain_directory(&archived)? {
        original
    } else {
        archived
    };
    if !files::verify_identity(&root, archive.identity.as_deref())? {
        return Err(invalid(&root, "Archive files are missing."));
    }
    let actual = files::config_hash(&root)?;
    if actual != snapshot.config_sha256
        && !(archive.state == "restoring"
            && snapshot.restored_config_sha256.as_deref() == Some(actual.as_str()))
    {
        return Err(invalid(
            &root,
            "Archived instance configuration differs from its recovery snapshot.",
        ));
    }
    let row = store::instance(&snapshot);
    let install = snapshot.tables["game_installs"]
        .first()
        .ok_or_else(|| invalid(&root, "Archive program ownership is missing."))?;
    if store::string(install, "module_id")? != store::string(row, "module_id")?
        || install["id"].as_i64().is_none()
        || install["id"] != row["install_id"]
    {
        return Err(invalid(
            &root,
            "Archived program module does not match the instance.",
        ));
    }
    if let Some(plan) = &snapshot.external_program {
        if store::string(install, "scope")? != "library"
            || !install["owner_instance_id"].is_null()
            || normalize_path(Path::new(store::string(install, "install_root")?))?
                != normalize_path(&plan.root)?
            || (plan.exclusive && store::string(row, "runtime_mode")? != "independent")
        {
            return Err(invalid(
                &root,
                "External archive program binding is inconsistent.",
            ));
        }
        return external::inspect(&root, plan);
    }
    let runtime = crate::program_runtime::resolve_instance_runtime_root(&root)?;
    match store::string(row, "runtime_mode")? {
        "shared"
            if store::string(install, "scope")? == "library"
                && install["owner_instance_id"].is_null() =>
        {
            if normalize_path(&runtime)?
                != normalize_path(Path::new(store::string(install, "install_root")?))?
            {
                return Err(invalid(
                    &root,
                    "Shared program no longer matches the archived binding.",
                ));
            }
        }
        "independent"
            if store::string(install, "scope")? == "instance"
                && install["owner_instance_id"] == row["id"] =>
        {
            if normalize_path(Path::new(store::string(install, "install_root")?))?
                != normalize_path(&original_root(paths, archive, &snapshot)?.join("runtime"))?
            {
                return Err(invalid(
                    &root,
                    "Archived independent program registration escapes its original instance.",
                ));
            }
            if normalize_path(&runtime)? != normalize_path(&root.join("runtime"))? {
                return Err(invalid(
                    &root,
                    "Archive independent runtime is not inside its owned directory.",
                ));
            }
        }
        _ => return Err(invalid(&root, "Archive program ownership is inconsistent.")),
    }
    Ok(())
}

async fn check_restore_metadata(
    connection: &mut SqliteConnection,
    paths: &StoragePaths,
    archive: &Archive,
    snapshot: &Snapshot,
) -> Result<(), StorageError> {
    let row = store::instance(snapshot);
    let original = original_root(paths, archive, snapshot)?;
    let instance_id = store::string(row, "id")?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM instances WHERE id=?1)")
        .bind(instance_id)
        .fetch_one(&mut *connection)
        .await?;
    if exists {
        return Err(invalid(
            &original,
            "The original instance ID is already registered; restoration will not overwrite it.",
        ));
    }
    let install = snapshot.tables["game_installs"]
        .first()
        .ok_or_else(|| invalid(&original, "Archive program ownership is missing."))?;
    let independent = store::string(install, "scope")? == "instance";
    if !independent {
        let current: Option<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT module_id,install_root,scope,owner_instance_id FROM game_installs WHERE id=?1",
        )
        .bind(install["id"].as_i64())
        .fetch_optional(&mut *connection)
        .await?;
        let Some((module, root, scope, owner)) = current else {
            return Err(invalid(
                &original,
                "The original shared program registration is missing.",
            ));
        };
        if module != store::string(row, "module_id")?
            || scope != "library"
            || owner.is_some()
            || normalize_path(Path::new(&root))?
                != normalize_path(Path::new(store::string(install, "install_root")?))?
        {
            return Err(invalid(
                &original,
                "The shared program registration has changed ownership.",
            ));
        }
    }
    ensure_instance_paths_available(
        paths,
        connection,
        instance_id,
        store::string(row, "module_id")?,
        Path::new(store::string(install, "install_root")?),
        Path::new(store::string(row, "config_path")?),
        Path::new(store::string(row, "saves_path")?),
    )
    .await?;
    conflicts::check(connection, snapshot, &original).await
}

async fn restore_rows(
    connection: &mut SqliteConnection,
    paths: &StoragePaths,
    archive: &Archive,
    snapshot: &Snapshot,
) -> Result<(), StorageError> {
    check_restore_metadata(connection, paths, archive, snapshot).await?;
    let row = store::instance(snapshot);
    let original = original_root(paths, archive, snapshot)?;
    let instance_id = store::string(row, "id")?;
    let install = &snapshot.tables["game_installs"][0];
    let independent = store::string(install, "scope")? == "instance";
    let mut instance = row.clone();
    instance.insert("status".into(), serde_json::json!("stopped"));
    instance.insert("autostart".into(), serde_json::json!(0));
    if independent {
        instance.insert("install_id".into(), serde_json::Value::Null);
    }
    store::insert_row(connection, "instances", &instance).await?;
    if independent {
        store::insert_row(connection, "game_installs", install).await?;
        sqlx::query("UPDATE instances SET install_id=?2 WHERE id=?1")
            .bind(instance_id)
            .bind(install["id"].as_i64())
            .execute(&mut *connection)
            .await?;
    } else if snapshot.external_program.is_some() {
        // Full archives can rebuild a previously uninstalled library in place.
        // The existing registration has already passed ownership/path checks.
        sqlx::query("UPDATE game_installs SET install_state=?2,current_version=?3,last_verified_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE id=?1")
            .bind(install["id"].as_i64()).bind(store::string(install, "install_state")?)
            .bind(install["current_version"].as_str()).execute(&mut *connection).await?;
    }
    for table in [
        "instance_ports",
        "instance_runs",
        "instance_broadcast_policies",
        "instance_broadcast_events",
    ] {
        for child in &snapshot.tables[table] {
            if table == "instance_runs"
                && child.get("status").and_then(serde_json::Value::as_str) == Some("running")
            {
                return Err(invalid(
                    &original,
                    "Archive contains an active process record.",
                ));
            }
            store::insert_row(connection, table, child).await?;
        }
    }
    Ok(())
}

pub(super) async fn check_restore_conflicts(
    pool: &SqlitePool,
    paths: &StoragePaths,
    archive: &Archive,
) -> Result<(), StorageError> {
    let snapshot = store::snapshot(archive)?;
    let original = original_root(paths, archive, &snapshot)?;
    if archive.state != "restoring" && fs::symlink_metadata(&original).is_ok() {
        return Err(invalid(
            &original,
            "The original instance directory already exists.",
        ));
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let result = check_restore_metadata(&mut tx, paths, archive, &snapshot).await;
    tx.rollback().await?;
    result
}

pub(super) async fn restore(
    pool: &SqlitePool,
    paths: &StoragePaths,
    archive: &Archive,
    _inventory_lock: &InstanceSettingsLock,
) -> Result<InstanceArchiveRestoreResult, StorageError> {
    // Catalog admission precedes this per-archive lease. Blocking workers keep
    // the source lease even if their async waiter is dropped.
    let source_lock = crate::instance_archive::program_source_mutation_lock(paths, &archive.id)?;
    let lock = &source_lock;
    if archive.purpose != "archive" || !matches!(archive.state.as_str(), "archived" | "restoring") {
        return Err(invalid(
            Path::new(&archive.leaf),
            "Archive is not in a recoverable state.",
        ));
    }
    let mut snapshot = store::snapshot(archive)?;
    let original = original_root(paths, archive, &snapshot)?;
    let archived = files::archive_path(paths, &archive.leaf)?;
    if archive.state == "restoring" && snapshot.external_program.is_some() {
        let root = if files::plain_directory(&archived)? {
            &archived
        } else {
            &original
        };
        external::prepare_payload(pool, archive, &mut snapshot, root, lock).await?;
        let plan = snapshot.external_program.clone().unwrap();
        let root = root.clone();
        lock.spawn_blocking(move || external::capture_files(&root, &plan))
            .await
            .map_err(|error| {
                worker_error("recovering restored external archive payload", error)
            })??;
    }
    let paths_copy = paths.clone();
    let archive_copy = store::load(pool, &archive.id).await?;
    lock.spawn_blocking(move || validate_archived_files(&paths_copy, &archive_copy))
        .await
        .map_err(|error| worker_error("checking archive recovery files", error))??;
    check_restore_conflicts(pool, paths, archive).await?;
    let mut admission = pool.begin_with("BEGIN IMMEDIATE").await?;
    check_restore_metadata(&mut admission, paths, archive, &snapshot).await?;
    // Publish exclusive path reservations atomically with the conflict check.
    // Configuration and instance creation read these restoring journal claims.
    sqlx::query("UPDATE instance_archives SET state='restoring',problem=NULL,updated_at=CURRENT_TIMESTAMP WHERE archive_id=?1")
        .bind(&archive.id).execute(&mut *admission).await?;
    admission.commit().await?;
    let working_root = if files::plain_directory(&archived)? {
        archived.clone()
    } else {
        original.clone()
    };
    let (library, staging) = if let Some(plan) = &snapshot.program {
        let mut connection = pool.acquire().await?;
        let library = program::library(&mut connection, &plan.module_id).await?.ok_or_else(|| invalid(&archived, "Install or repair this archive's exact program version first; no download was started."))?.0;
        drop(connection);
        let source = library.clone();
        let expected = plan.clone();
        lock.spawn_blocking(move || program::verify_library(&source, &expected))
            .await
            .map_err(|error| worker_error("verifying archive reconstruction library", error))??;
        let staging = program::prepare_staging(pool, archive, &working_root, plan, lock).await?;
        (Some(library), staging)
    } else {
        (None, None)
    };
    external::prepare_restore(pool, archive, &mut snapshot, lock).await?;
    let result = async {
        let from = archived.clone(); let to = original.clone(); let expected = archive.identity.clone().ok_or_else(|| invalid(&archived, "Archive identity is missing."))?;
        let plan = snapshot.program.clone(); let original_hash = snapshot.config_sha256.clone(); let restored_hash = snapshot.restored_config_sha256.clone();
        let external_plan = snapshot.external_program.clone();
        let checked_paths = paths.clone(); let checked = archive.clone();
        lock.spawn_blocking(move || {
            #[cfg(test)]
            crate::instance_archive::test_gate::pause(&checked_paths.database_path, crate::instance_archive::test_gate::Point::Restoring);
            let _parents = files::guard_parents(&checked_paths, checked.instances_identity.as_deref(), checked.parent_identity.as_deref())?;
            let root = if files::plain_directory(&from)? { &from } else { &to };
            if !files::verify_identity(root, Some(&expected))? { return Err(invalid(root, "Both archive and restoration files are missing.")); }
            if let (Some(plan), Some(library)) = (&plan, &library) { program::restore_files(root, &expected, library, plan, staging.as_deref())?; }
            if let Some(plan) = &external_plan { external::restore_files(root, plan)?; }
            if let Some(restored_hash) = restored_hash { config::restore(root, &expected, &original_hash, &restored_hash)?; }
            if let Some(plan) = &external_plan { external::finish_restore(root, plan)?; }
            Ok(())
        }).await.map_err(|error| worker_error("reconstructing archive files", error))??;
        // Recheck path/port/record conflicts after reconstruction. The durable
        // restoring journal makes an interrupted copy resumable; only row
        // publication and the same-volume rename need the SQLite write lock.
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        restore_rows(&mut tx, paths, archive, &snapshot).await?;
        let from = archived.clone(); let to = original.clone();
        let expected = archive.identity.clone().ok_or_else(|| invalid(&archived, "Archive identity is missing."))?;
        let checked_paths = paths.clone(); let checked = archive.clone();
        lock.spawn_blocking(move || {
            let _parents = files::guard_parents(&checked_paths, checked.instances_identity.as_deref(), checked.parent_identity.as_deref())?;
            if files::plain_directory(&from)? { files::move_directory(&from, &to, &expected)?; }
            else if !files::verify_identity(&to, Some(&expected))? { return Err(invalid(&to, "Both archive and restoration files are missing.")); }
            Ok(())
        }).await.map_err(|error| worker_error("publishing restored archive directory", error))??;
        sqlx::query("UPDATE instance_archives SET state='restored', problem=NULL, updated_at=CURRENT_TIMESTAMP WHERE archive_id=?1").bind(&archive.id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok::<_, StorageError>(())
    }.await;
    if let Err(error) = result {
        store::set_state(pool, &archive.id, "restoring", Some(&error.to_string())).await?;
        return Err(error);
    }
    Ok(InstanceArchiveRestoreResult {
        archive_id: archive.id.clone(),
        instance_id: store::string(store::instance(&snapshot), "id")?.to_owned(),
        instance_name: store::string(store::instance(&snapshot), "name")?.to_owned(),
        restored_instance_root: original.to_string_lossy().into_owned(),
        external_saves_restore_required: snapshot.external_saves_backup_id.is_some(),
        external_saves_backup_id: snapshot.external_saves_backup_id,
        preserved_external_saves_path: archive.external_saves.clone(),
    })
}

fn worker_error(operation: &'static str, error: tokio::task::JoinError) -> StorageError {
    StorageError::BlockingTaskFailed {
        operation,
        message: error.to_string(),
    }
}
