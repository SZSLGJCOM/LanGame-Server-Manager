use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use app_core::{InstanceArchiveResult, InstanceStatus};
use sqlx::SqlitePool;

use super::{finish_archiving, worker_error};
use crate::instance_archive::{config, external, program};
use crate::instance_archive_files as files;
use crate::instance_archive_store::{self as store, Archive, invalid};
use crate::instance_isolation::paths::{contains, normalize_path};
use crate::instance_settings_lock::InstanceSettingsLock;
use crate::instances::{effective_instance_install_root, validate_managed_instance_root};
use crate::save_paths::{effective_instance_saves_dir, load_module_descriptor};
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{StorageError, StoragePaths};

pub(crate) async fn archive_instance(
    paths: &StoragePaths,
    instance_id: &str,
    lock: &InstanceSettingsLock,
    purpose: &str,
) -> Result<InstanceArchiveResult, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = admit(&pool, paths, instance_id, lock, purpose).await;
    pool.close().await;
    result
}

async fn admit(
    pool: &SqlitePool,
    paths: &StoragePaths,
    instance_id: &str,
    lock: &InstanceSettingsLock,
    purpose: &str,
) -> Result<InstanceArchiveResult, StorageError> {
    let pending: Option<String> = sqlx::query_scalar("SELECT archive_id FROM instance_archives WHERE instance_id=?1 AND purpose=?2 AND state='archiving'")
        .bind(instance_id).bind(purpose).fetch_optional(pool).await?;
    if let Some(id) = pending {
        let archive = store::load(pool, &id).await?;
        finish_archiving(pool, paths, &archive, lock).await?;
        return result(paths, &archive);
    }
    let mut connection = pool.acquire().await?;
    let record = fetch_instance_record(&mut *connection, instance_id).await?;
    if record.summary.active_process_count > 0
        || matches!(
            record.summary.status,
            InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
        )
        || crate::runtime::load_active_instance_run(&mut *connection, instance_id)
            .await?
            .is_some()
    {
        return Err(StorageError::ActiveInstanceDeletion {
            id: instance_id.to_owned(),
        });
    }
    let original = record
        .config_dir
        .parent()
        .unwrap_or(&record.config_dir)
        .to_owned();
    validate_managed_instance_root(&original, &paths.instances_root)?;
    store::ensure_instance_root_unshared(&mut connection, paths, &original, instance_id).await?;
    let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
    let (install, saves, library_reference, exclusive) = if purpose == "delete" {
        let retirement = crate::instance_retirement_paths::resolve(paths, &record).await?;
        (
            retirement.program,
            retirement.saves,
            retirement.library,
            retirement.exclusive,
        )
    } else {
        let install = effective_instance_install_root(&record)?;
        let saves = effective_instance_saves_dir(descriptor.as_ref(), &install, &record)?;
        (
            install,
            saves,
            crate::program_runtime::instance_uses_library_program(&original)?,
            crate::program_runtime::instance_uses_exclusive_program(&original)?,
        )
    };
    let root_paths = paths.clone();
    let claims = [
        original.clone(),
        record.config_dir.clone(),
        install.clone(),
        saves.clone(),
    ];
    lock.spawn_blocking(move || {
        crate::instance_archive_roots::validate_archive_root(&root_paths)?;
        crate::instance_archive_roots::ensure_paths_outside_archive_root(
            &root_paths,
            claims.iter().map(PathBuf::as_path),
        )
    })
    .await
    .map_err(|error| worker_error("validating archive root", error))??;
    store::ensure_unreferenced(&mut connection, paths, &paths.archives_root).await?;
    let within = contains(&normalize_path(&original)?, &normalize_path(&saves)?)
        || (exclusive
            && normalize_path(&saves)? != normalize_path(&install)?
            && contains(&normalize_path(&install)?, &normalize_path(&saves)?));
    let mut library =
        if purpose == "archive" && record.runtime_mode == "independent" && !library_reference {
            program::library(&mut connection, &record.summary.module_id).await?
        } else {
            None
        };
    if let Some((source, _)) = &library
        && !program::source_is_idle(&mut connection, source).await?
    {
        library = None;
    }
    let source_idle = if library_reference {
        program::source_is_idle(&mut connection, &install).await?
    } else {
        true
    };
    drop(connection);
    let external_backup = if purpose == "archive" && !within {
        if !files::plain_directory(&saves)? {
            return Err(invalid(
                &saves,
                "External saves directory is missing; no empty archive backup was created.",
            ));
        }
        Some(
            crate::backups::create_instance_archive_backup(paths, instance_id)
                .await?
                .backup_id,
        )
    } else {
        None
    };
    let id = uuid::Uuid::new_v4().to_string();
    let source = original.clone();
    let storage = paths.clone();
    let leaf = id.clone();
    let saves_copy = saves.clone();
    let independent = record.runtime_mode == "independent";
    let is_archive = purpose == "archive";
    let program_root = install.clone();
    let module = record.summary.module_id.clone();
    let instance = instance_id.to_owned();
    let (identity, config_hash, restored_hash, instances_identity, parent_identity, program, reason, external_program) = lock.spawn_blocking(move || {
        let target = files::archive_path(&storage, &leaf)?;
        let parent = target.parent().ok_or_else(|| invalid(&target, "Archive directory has no parent."))?;
        if !files::plain_directory(parent)? { fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath { path: parent.to_owned(), source })?; }
        let (instances_identity, parent_identity) = files::parent_identities(&storage)?;
        let _parents = files::guard_parents(&storage, Some(&instances_identity), Some(&parent_identity))?;
        let identity = files::identity(&source)?.ok_or_else(|| invalid(&source, "Instance directory is missing."))?;
        files::preflight_tree(&source, &identity)?;
        let (config_hash, restored_hash) = if is_archive { config::hashes(&source)? } else {
            let hash = config::deletion_hash(&source)?;
            (hash.clone(), hash)
        };
        let external_program = if library_reference && (is_archive || exclusive) {
            let mode = if is_archive { external::CaptureMode::Archive { source_idle } } else { external::CaptureMode::Delete };
            Some(external::plan(&source, &program_root, &saves_copy, &module, &instance, exclusive, mode)?)
        } else { None };
        let (plan, mut reason) = if is_archive && independent && !library_reference { program::plan(&source, &saves_copy, descriptor.as_ref(), library)? }
            else if is_archive { (None, Some("Shared program files remain in the library; the archive contains the instance-owned files.".into())) }
            else { (None, None) };
        if is_archive && let Some(external) = &external_program {
            reason = Some(if external.requires_source() {
                "Unchanged verified program files remain in the exact installation; native data, Mods and custom files are captured in this data archive."
            } else {
                "All external program and data bytes were captured because no verified unchanged program files could safely be omitted."
            }.into());
        }
        Ok::<_, StorageError>((identity, config_hash, restored_hash, instances_identity, parent_identity, plan, reason, external_program))
    }).await.map_err(|error| worker_error("planning instance archive or deletion", error))??;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    store::ensure_instance_root_unshared(&mut tx, paths, &original, instance_id).await?;
    let mut snapshot = store::capture(&mut tx, instance_id, config_hash).await?;
    snapshot.version = if external_program.is_some() { 3 } else { 2 };
    snapshot.restored_config_sha256 = Some(restored_hash);
    snapshot.program = program;
    snapshot.external_program = external_program;
    snapshot.program_retention_reason = reason;
    snapshot.external_saves_backup_id = external_backup;
    snapshot.effective_saves_path = Some(saves.to_string_lossy().into_owned());
    let snapshot_json = serde_json::to_string(&snapshot)?;
    if snapshot_json.len() > store::MAX_SNAPSHOT_BYTES {
        return Err(invalid(
            &original,
            "Archive exceeds the recovery metadata limit; no instance files were removed.",
        ));
    }
    let deleted_at = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .map_err(|_| invalid(&original, "Archive timestamp exceeds its supported range."))?;
    let external = (!within).then(|| saves.to_string_lossy().into_owned());
    sqlx::query("INSERT INTO instance_archives (archive_id,instance_id,instance_name,module_id,deleted_at_unix_ms,original_root,archive_leaf,directory_identity_json,snapshot_json,snapshot_sha256,preserved_external_saves_path,purpose,instances_root_identity_json,archive_parent_identity_json,state) VALUES (?1,?2,?3,?4,?5,?6,?1,?7,?8,?9,?10,?11,?12,?13,'archiving')")
        .bind(&id).bind(instance_id).bind(&record.summary.name).bind(&record.summary.module_id).bind(deleted_at)
        .bind(original.to_string_lossy().as_ref()).bind(identity).bind(&snapshot_json).bind(store::digest(snapshot_json.as_bytes())).bind(external).bind(purpose).bind(instances_identity).bind(parent_identity)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    let archive = store::load(pool, &id).await?;
    if let Err(error) = finish_archiving(pool, paths, &archive, lock).await {
        // Commit compensation may have removed the journal; updating no row is safe.
        store::set_state(pool, &id, "archiving", Some(&error.to_string())).await?;
        return Err(error);
    }
    result(paths, &archive)
}

pub(super) fn result(
    paths: &StoragePaths,
    archive: &Archive,
) -> Result<InstanceArchiveResult, StorageError> {
    let snapshot = store::snapshot(archive)?;
    let row = store::instance(&snapshot);
    let original = PathBuf::from(archive.original_root.as_deref().ok_or_else(|| {
        invalid(
            Path::new(&archive.leaf),
            "Operation original directory is missing.",
        )
    })?);
    Ok(InstanceArchiveResult {
        archive_id: archive.id.clone(),
        instance_id: store::string(row, "id")?.into(),
        instance_name: store::string(row, "name")?.into(),
        module_id: store::string(row, "module_id")?.into(),
        deleted_at_unix_ms: archive.deleted_at.unwrap_or_default().max(0) as u128,
        previous_instance_root: original.to_string_lossy().into_owned(),
        archived_instance_root: archive.identity.as_ref().map(|_| {
            paths
                .archives_root
                .join(&archive.leaf)
                .to_string_lossy()
                .into_owned()
        }),
        effective_saves_path: snapshot
            .effective_saves_path
            .clone()
            .unwrap_or(store::string(row, "saves_path")?.to_owned()),
        saves_archived_with_instance_root: archive.external_saves.is_none(),
        preserved_external_saves_path: archive.external_saves.clone(),
        external_saves_backup_id: snapshot.external_saves_backup_id,
    })
}
