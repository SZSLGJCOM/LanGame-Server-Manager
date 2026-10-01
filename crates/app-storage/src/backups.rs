use super::*;
use crate::atomic_file::write_file_atomically;
use crate::instance_isolation::ensure_instance_paths_available;
use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use crate::instances::effective_instance_install_root;
use crate::save_paths::{effective_instance_saves_dir, load_module_descriptor};
use crate::storage_db::{connect_pool, fetch_instance_record};

type InstanceTransactionLocks =
    std::sync::Mutex<HashMap<PathBuf, std::sync::Weak<std::sync::Mutex<()>>>>;

static INSTANCE_TRANSACTION_LOCKS: OnceLock<InstanceTransactionLocks> = OnceLock::new();

include!("backups_checked.rs");

#[path = "backups_publication.rs"]
pub(crate) mod publication;

#[cfg(test)]
#[path = "backups_concurrency_tests.rs"]
mod concurrency_tests;

pub async fn create_instance_backup(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceBackupResult, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
    let install_root = effective_instance_install_root(&record)?;
    let saves_dir = effective_instance_saves_dir(descriptor.as_ref(), &install_root, &record)?;
    pool.close().await;

    let instance_id = String::from(instance_id);
    let transaction_root = instance_root(&record);
    run_instance_transaction(transaction_root, move || {
        let backup = create_instance_backup_with_prefix(
            &record,
            &saves_dir,
            &instance_id,
            "saves",
            InstanceBackupKind::Manual,
        )?;
        prune_instance_backups(&record, &instance_id, &[backup.backup_id.as_str()])?;
        Ok(backup)
    })
    .await
}

/// Retain both the selected source and the new safeguard even at retention=1.
pub async fn create_instance_pre_restore_backup(
    paths: &StoragePaths,
    instance_id: &str,
    source_backup_id: &str,
) -> Result<InstanceBackupResult, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
    let install_root = effective_instance_install_root(&record)?;
    let saves_dir = effective_instance_saves_dir(descriptor.as_ref(), &install_root, &record)?;
    pool.close().await;
    let instance_id = instance_id.to_owned();
    let source_backup_id = source_backup_id.to_owned();
    run_instance_transaction(instance_root(&record), move || {
        let source = validated_backup_path(&record, &source_backup_id)?;
        load_instance_backup_for_instance(&source, &instance_id, &source_backup_id)?;
        let backup = create_instance_backup_with_prefix(
            &record,
            &saves_dir,
            &instance_id,
            "pre-restore",
            InstanceBackupKind::PreRestore,
        )?;
        prune_instance_backups(
            &record,
            &instance_id,
            &[source_backup_id.as_str(), backup.backup_id.as_str()],
        )?;
        Ok(backup)
    })
    .await
}

// Archiving must preserve earlier backups regardless of the retention setting.
// The caller holds the instance settings lock and has verified it is stopped.
pub(crate) async fn create_instance_archive_backup(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceBackupResult, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
    let install_root = effective_instance_install_root(&record)?;
    let saves_dir = effective_instance_saves_dir(descriptor.as_ref(), &install_root, &record)?;
    pool.close().await;

    let instance_id = instance_id.to_owned();
    run_instance_transaction(instance_root(&record), move || {
        create_instance_backup_with_prefix(
            &record,
            &saves_dir,
            &instance_id,
            "archive",
            InstanceBackupKind::Manual,
        )
    })
    .await
}

pub async fn create_instance_auto_stop_backup(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceBackupResult, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
    let install_root = effective_instance_install_root(&record)?;
    let saves_dir = effective_instance_saves_dir(descriptor.as_ref(), &install_root, &record)?;
    pool.close().await;

    let instance_id = String::from(instance_id);
    let transaction_root = instance_root(&record);
    run_instance_transaction(transaction_root, move || {
        let backup = create_instance_backup_with_prefix(
            &record,
            &saves_dir,
            &instance_id,
            "auto-stop",
            InstanceBackupKind::AutoStop,
        )?;
        prune_instance_backups(&record, &instance_id, &[backup.backup_id.as_str()])?;
        Ok(backup)
    })
    .await
}

pub async fn list_instance_backups(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<Vec<InstanceBackupResult>, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    pool.close().await;

    let instance_id = String::from(instance_id);
    let transaction_root = instance_root(&record);
    run_instance_transaction(transaction_root, move || {
        collect_instance_backups(&record, &instance_id)
    })
    .await
}

pub async fn rename_instance_backup(
    paths: &StoragePaths,
    instance_id: &str,
    backup_id: &str,
    display_name: Option<String>,
) -> Result<InstanceBackupResult, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    pool.close().await;

    let instance_id = String::from(instance_id);
    let backup_id = String::from(backup_id);
    let transaction_root = instance_root(&record);
    run_instance_transaction(transaction_root, move || {
        let backup_path = validated_backup_path(&record, &backup_id)?;
        let mut backup = load_instance_backup_for_instance(&backup_path, &instance_id, &backup_id)?;
        backup.display_name = normalize_backup_display_name(display_name);
        write_instance_backup_manifest(&backup_path, &backup)?;
        Ok(backup)
    })
    .await
}

pub async fn delete_instance_backup(
    paths: &StoragePaths,
    instance_id: &str,
    backup_id: &str,
) -> Result<InstanceBackupResult, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    pool.close().await;

    let instance_id = String::from(instance_id);
    let backup_id = String::from(backup_id);
    let transaction_root = instance_root(&record);
    run_instance_transaction(transaction_root, move || {
        let backup_path = validated_backup_path(&record, &backup_id)?;
        let backup = load_instance_backup_for_instance(&backup_path, &instance_id, &backup_id)?;
        remove_managed_directory(&backup_path, &instance_backup_root(&record))?;
        Ok(backup)
    })
    .await
}

pub async fn restore_instance_backup(
    paths: &StoragePaths,
    instance_id: &str,
    backup_id: &str,
) -> Result<InstanceBackupRestoreResult, StorageError> {
    restore_instance_backup_checked(paths, instance_id, backup_id, None).await
}

async fn restore_instance_backup_checked(
    paths: &StoragePaths,
    instance_id: &str,
    backup_id: &str,
    prepared: Option<PreparedInstanceBackupRestore>,
) -> Result<InstanceBackupRestoreResult, StorageError> {
    let lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let paths = paths.clone();
    let instance_id = String::from(instance_id);
    let backup_id = String::from(backup_id);
    lock.complete_mutation("restoring instance backup", async move {
        let pool = connect_pool(&paths).await?;
        let result = async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let record = fetch_instance_record(&mut *tx, &instance_id).await?;
            let descriptor = load_module_descriptor(&paths, &record.summary.module_id)?;
            let install_root = effective_instance_install_root(&record)?;
            let saves_dir =
                effective_instance_saves_dir(descriptor.as_ref(), &install_root, &record)?;
            ensure_instance_paths_available(
                &paths,
                &mut tx,
                &instance_id,
                &record.summary.module_id,
                &install_root,
                &record.config_dir,
                &saves_dir,
            )
            .await?;
            tx.rollback().await?;
            let transaction_lock = instance_transaction_lock(&instance_root(&record));
            let checked_record = record.clone();
            let checked_saves = saves_dir.clone();
            let checked_paths = paths.clone();
            publication::run(&pool, move |connection| {
                let record = checked_record.clone();
                let saves = checked_saves.clone();
                let paths = checked_paths.clone();
                Box::pin(async move {
                    let current = fetch_instance_record(&mut *connection, &record.summary.id).await?;
                    let descriptor = load_module_descriptor(&paths, &current.summary.module_id)?;
                    let install = effective_instance_install_root(&current)?;
                    let current_saves = effective_instance_saves_dir(descriptor.as_ref(), &install, &current)?;
                    if current.config_dir != record.config_dir || current_saves != saves
                        || serde_json::to_value(&current.summary)? != serde_json::to_value(&record.summary)?
                    {
                        return Err(backup_confirmation_error(&saves, "Instance ownership or runtime state changed while preparing its backup restore"));
                    }
                    ensure_instance_paths_available(&paths, connection, &current.summary.id,
                        &current.summary.module_id, &install, &current.config_dir, &current_saves).await
                })
            }, move |publication| {
                let _transaction = transaction_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                restore_instance_backup_with_publication(
                    &record,
                    &saves_dir,
                    &instance_id,
                    &backup_id,
                    prepared.as_ref(),
                    &publication,
                )
            })
            .await
        }
        .await;
        pool.close().await;
        result
    })
    .await
}

async fn run_instance_transaction<T, F>(
    instance_root: PathBuf,
    operation: F,
) -> Result<T, StorageError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, StorageError> + Send + 'static,
{
    let lock = instance_transaction_lock(&instance_root);
    let join_error_path = instance_root.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        operation()
    })
    .await
    .map_err(|source| StorageError::ReadPath {
        path: join_error_path,
        source: std::io::Error::other(format!("backup transaction task failed: {source}")),
    })?
}

fn instance_transaction_lock(instance_root: &Path) -> std::sync::Arc<std::sync::Mutex<()>> {
    let locks = INSTANCE_TRANSACTION_LOCKS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(instance_root).and_then(std::sync::Weak::upgrade) {
        return lock;
    }

    let lock = std::sync::Arc::new(std::sync::Mutex::new(()));
    locks.insert(
        instance_root.to_path_buf(),
        std::sync::Arc::downgrade(&lock),
    );
    lock
}

fn create_instance_backup_with_prefix(
    record: &StoredInstanceRecord,
    saves_dir: &Path,
    instance_id: &str,
    prefix: &str,
    backup_kind: InstanceBackupKind,
) -> Result<InstanceBackupResult, StorageError> {
    create_instance_backup_with_copy_hook(
        record,
        saves_dir,
        instance_id,
        prefix,
        backup_kind,
        &mut |_, _| Ok(()),
    )
}

fn create_instance_backup_with_copy_hook<F>(
    record: &StoredInstanceRecord,
    saves_dir: &Path,
    instance_id: &str,
    prefix: &str,
    backup_kind: InstanceBackupKind,
    before_copy: &mut F,
) -> Result<InstanceBackupResult, StorageError>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    ensure_plain_directory(saves_dir)?;

    let backup_root = instance_backup_root(record);
    ensure_plain_directory(&backup_root)?;

    let created_at_unix_ms = backup_created_at_unix_ms();
    let random = Uuid::new_v4().as_u128() & 0x0000_ffff_ffff_ffff;
    let backup_id = format!("{prefix}-{created_at_unix_ms}-{random:012x}");
    let backup_path = backup_root.join(&backup_id);
    let staging_path = create_unique_directory(&backup_root, ".publish")?;
    let mut staging_cleanup = DirectoryCleanup::new(staging_path.clone(), backup_root.clone());
    let staging_saves_path = staging_path.join("saves");
    fs::create_dir(&staging_saves_path).map_err(|source| StorageError::CreatePath {
        path: staging_saves_path.clone(),
        source,
    })?;

    let dst_settings_path = record.config_dir.join("instance.json");
    let dst_settings = if record.summary.module_id == "dontstarve" {
        Some(
            read_dst_backup_settings(&dst_settings_path)?.ok_or_else(|| {
                backup_confirmation_error(
                    &dst_settings_path,
                    "DST canonical configuration is missing",
                )
            })?,
        )
    } else {
        None
    };
    let stats = copy_directory_contents_with_hook(saves_dir, &staging_saves_path, before_copy)?;
    if let Some(bytes) = dst_settings {
        if read_dst_backup_settings(&dst_settings_path)?.as_deref() != Some(bytes.as_slice()) {
            return Err(backup_confirmation_error(
                &dst_settings_path,
                "DST canonical configuration changed while its saves were backed up",
            ));
        }
        let path = staging_path.join("dst-instance.json");
        fs::write(&path, bytes).map_err(|source| StorageError::WriteConfig { path, source })?;
    }
    let result = InstanceBackupResult {
        backup_id,
        instance_id: String::from(instance_id),
        backup_kind,
        display_name: None,
        created_at_unix_ms,
        backup_path: backup_path.to_string_lossy().into_owned(),
        saves_path: saves_dir.to_string_lossy().into_owned(),
        file_count: stats.file_count,
        total_bytes: stats.total_bytes,
    };
    write_instance_backup_manifest(&staging_path, &result)?;
    publish_new_directory(&staging_path, &backup_path)?;
    staging_cleanup.disarm();

    Ok(result)
}

#[cfg(test)]
fn restore_instance_backup_transaction(
    record: &StoredInstanceRecord,
    saves_dir: &Path,
    instance_id: &str,
    backup_id: &str,
) -> Result<InstanceBackupRestoreResult, StorageError> {
    restore_instance_backup_transaction_checked(record, saves_dir, instance_id, backup_id, None)
}

#[cfg(test)]
fn restore_instance_backup_transaction_checked(
    record: &StoredInstanceRecord,
    saves_dir: &Path,
    instance_id: &str,
    backup_id: &str,
    prepared: Option<&PreparedInstanceBackupRestore>,
) -> Result<InstanceBackupRestoreResult, StorageError> {
    restore_instance_backup_with_publication(
        record,
        saves_dir,
        instance_id,
        backup_id,
        prepared,
        &publication::Publication::uncoordinated(),
    )
}

fn restore_instance_backup_with_publication(
    record: &StoredInstanceRecord,
    saves_dir: &Path,
    instance_id: &str,
    backup_id: &str,
    prepared: Option<&PreparedInstanceBackupRestore>,
    publication: &publication::Publication,
) -> Result<InstanceBackupRestoreResult, StorageError> {
    #[cfg(test)]
    crate::instance_archive::test_gate::pause(
        saves_dir,
        crate::instance_archive::test_gate::Point::BackupPreparing,
    );
    let backup_path = validated_backup_path(record, backup_id)?;
    let current = load_instance_backup_for_instance(&backup_path, instance_id, backup_id)?;
    let restore_scope = backup_restore_scope(record, saves_dir, &current);
    let saves_dir = restore_scope.as_path();
    let backup_saves_path = backup_path.join("saves");
    ensure_plain_directory(saves_dir)?;
    if let Some(prepared) = prepared {
        prepared.validate(&current, saves_dir, &backup_saves_path)?;
    }

    let safeguard_backup = create_instance_backup_with_prefix(
        record,
        saves_dir,
        instance_id,
        "pre-restore",
        InstanceBackupKind::PreRestore,
    )?;
    if let Some(prepared) = prepared
        && backup_tree_sha256(&Path::new(&safeguard_backup.backup_path).join("saves"))?
            != prepared.target_sha256
    {
        return Err(backup_confirmation_error(
            saves_dir,
            "The safeguard content changed during copying; current saves were not replaced",
        ));
    }
    let stats = replace_directory_from_source_with_publication(
        &backup_saves_path,
        saves_dir,
        &mut |_, _| Ok(()),
        publish_new_directory,
        |staging, target| {
            if let Some(prepared) = prepared {
                prepared.validate_contents(staging, target)?;
            }
            Ok(())
        },
        publication,
    )?;
    if let Some(prepared) = prepared
        && backup_tree_sha256(saves_dir)? != prepared.source_sha256
    {
        return Err(backup_confirmation_error(
            saves_dir,
            "Restored saves failed readback; the safeguard backup is retained",
        ));
    }

    let result = InstanceBackupRestoreResult {
        instance_id: String::from(instance_id),
        backup_id: String::from(backup_id),
        restored_at_unix_ms: backup_created_at_unix_ms(),
        saves_path: saves_dir.to_string_lossy().into_owned(),
        restored_file_count: stats.file_count,
        restored_total_bytes: stats.total_bytes,
        safeguard_backup_id: safeguard_backup.backup_id,
        safeguard_backup_path: safeguard_backup.backup_path,
    };

    prune_instance_backups(
        record,
        instance_id,
        &[backup_id, result.safeguard_backup_id.as_str()],
    )?;
    Ok(result)
}

#[cfg(test)]
fn replace_directory_from_source_with_publish<F, P>(
    source_dir: &Path,
    target_dir: &Path,
    before_copy: &mut F,
    publish: P,
) -> Result<DirectoryCopyStats, StorageError>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
    P: FnOnce(&Path, &Path) -> Result<(), StorageError>,
{
    replace_directory_from_source_checked(source_dir, target_dir, before_copy, publish, |_, _| {
        Ok(())
    })
}

#[cfg(test)]
fn replace_directory_from_source_checked<F, P, C>(
    source_dir: &Path,
    target_dir: &Path,
    before_copy: &mut F,
    publish: P,
    check: C,
) -> Result<DirectoryCopyStats, StorageError>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
    P: FnOnce(&Path, &Path) -> Result<(), StorageError>,
    C: FnOnce(&Path, &Path) -> Result<(), StorageError>,
{
    replace_directory_from_source_with_publication(
        source_dir,
        target_dir,
        before_copy,
        publish,
        check,
        &publication::Publication::uncoordinated(),
    )
}

fn replace_directory_from_source_with_publication<F, P, C>(
    source_dir: &Path,
    target_dir: &Path,
    before_copy: &mut F,
    publish: P,
    check: C,
    publication: &publication::Publication,
) -> Result<DirectoryCopyStats, StorageError>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
    P: FnOnce(&Path, &Path) -> Result<(), StorageError>,
    C: FnOnce(&Path, &Path) -> Result<(), StorageError>,
{
    ensure_plain_directory(target_dir)?;
    let parent = target_dir
        .parent()
        .ok_or_else(|| StorageError::UnsafeManagedPath {
            path: target_dir.to_path_buf(),
            root: target_dir.to_path_buf(),
        })?;
    ensure_plain_directory(parent)?;

    let staging_path = create_unique_directory(parent, ".restore-publish")?;
    let mut staging_cleanup = DirectoryCleanup::new(staging_path.clone(), parent.to_path_buf());
    let stats = copy_directory_contents_with_hook(source_dir, &staging_path, before_copy)?;
    // The instance lease protects ordinary backup contents; do not impose the
    // cluster inventory's per-tree budget on existing ordinary backup support.
    let stamp = publication::Stamp::directories(&[target_dir.to_path_buf(), staging_path.clone()])?;
    check(&staging_path, target_dir)?;
    let rollback_path = unique_available_path(parent, ".restore-rollback");

    publication.publish(&stamp, || {
    let mut rollback = RestoreRollback::new(target_dir.to_path_buf(), rollback_path.clone());
    move_directory(target_dir, &rollback_path)?;
    if let Err(publish_error) = publish(&staging_path, target_dir) {
        if let Err(rollback_error) = rollback.restore() {
            return Err(StorageError::MovePath {
                from: rollback_path.clone(),
                to: target_dir.to_path_buf(),
                source: std::io::Error::other(format!(
                    "restore publish failed ({publish_error}); restoring the original saves also failed: {rollback_error}"
                )),
            });
        }
        return Err(publish_error);
    }

    staging_cleanup.disarm();
    rollback.commit();
    Ok(())
    })?;
    #[cfg(test)]
    crate::instance_archive::test_gate::pause(
        target_dir,
        crate::instance_archive::test_gate::Point::BackupCleanup,
    );
    let _ = remove_managed_directory(&rollback_path, parent);
    Ok(stats)
}

fn collect_instance_backups(
    record: &StoredInstanceRecord,
    instance_id: &str,
) -> Result<Vec<InstanceBackupResult>, StorageError> {
    let backup_root = instance_backup_root(record);
    match fs::symlink_metadata(&backup_root) {
        Ok(metadata) => require_plain_directory(&backup_root, &backup_root, &metadata)?,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: backup_root,
                source,
            });
        }
    }

    let entries = fs::read_dir(&backup_root).map_err(|source| StorageError::ReadDirectory {
        path: backup_root.clone(),
        source,
    })?;
    let mut backups = Vec::new();

    for entry_result in entries {
        let entry = entry_result.map_err(|source| StorageError::ReadDirectory {
            path: backup_root.clone(),
            source,
        })?;
        let backup_path = entry.path();
        let metadata =
            fs::symlink_metadata(&backup_path).map_err(|source| StorageError::ReadPath {
                path: backup_path.clone(),
                source,
            })?;
        if is_link_or_reparse(&metadata) {
            return Err(StorageError::UnsafeManagedPath {
                path: backup_path,
                root: backup_root,
            });
        }
        if !metadata.is_dir() {
            continue;
        }
        validate_direct_child_directory(&backup_path, &backup_root)?;

        let manifest_path = backup_path.join("backup.json");
        match fs::symlink_metadata(&manifest_path) {
            Ok(metadata) if metadata.is_file() && !is_link_or_reparse(&metadata) => {}
            Ok(_) => {
                return Err(StorageError::UnsafeManagedPath {
                    path: manifest_path,
                    root: backup_path,
                });
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(StorageError::ReadPath {
                    path: manifest_path,
                    source,
                });
            }
        }

        let backup = read_instance_backup_manifest(&backup_path)?;
        let directory_name = backup_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if backup.backup_id != directory_name {
            return Err(StorageError::UnsafeManagedPath {
                path: backup_path,
                root: backup_root,
            });
        }
        if backup.instance_id != instance_id {
            continue;
        }

        backups.push(backup);
    }

    backups.sort_by_key(|backup| std::cmp::Reverse(backup.created_at_unix_ms));
    Ok(backups)
}

fn prune_instance_backups(
    record: &StoredInstanceRecord,
    instance_id: &str,
    preserve_backup_ids: &[&str],
) -> Result<(), StorageError> {
    let retention_count = record.backup_retention_count.max(1) as usize;
    let preserved_ids = preserve_backup_ids.iter().copied().collect::<HashSet<_>>();
    let backups = collect_instance_backups(record, instance_id)?;
    let preserved_count = backups
        .iter()
        .filter(|backup| preserved_ids.contains(backup.backup_id.as_str()))
        .count();
    // Protected snapshots count toward the limit, but a restore must keep both its
    // source and safeguard even when the configured limit cannot accommodate them.
    let unprotected_retention_count = retention_count.saturating_sub(preserved_count);
    let mut kept_non_preserved = 0usize;

    for backup in backups {
        if preserved_ids.contains(backup.backup_id.as_str()) {
            continue;
        }

        if kept_non_preserved < unprotected_retention_count {
            kept_non_preserved += 1;
            continue;
        }

        let backup_path = validated_backup_path(record, &backup.backup_id)?;
        remove_managed_directory(&backup_path, &instance_backup_root(record))?;
    }

    Ok(())
}

fn normalize_backup_display_name(display_name: Option<String>) -> Option<String> {
    let trimmed = display_name.unwrap_or_default().trim().to_owned();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn load_instance_backup_for_instance(
    backup_path: &Path,
    instance_id: &str,
    backup_id: &str,
) -> Result<InstanceBackupResult, StorageError> {
    match fs::symlink_metadata(backup_path) {
        Ok(metadata) => require_plain_directory(
            backup_path,
            backup_path.parent().unwrap_or(backup_path),
            &metadata,
        )?,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(StorageError::MissingBackup {
                instance_id: String::from(instance_id),
                backup_id: String::from(backup_id),
            });
        }
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: backup_path.to_path_buf(),
                source,
            });
        }
    }

    let backup = read_instance_backup_manifest(backup_path)?;
    if backup.instance_id != instance_id || backup.backup_id != backup_id {
        return Err(StorageError::MissingBackup {
            instance_id: String::from(instance_id),
            backup_id: String::from(backup_id),
        });
    }

    Ok(backup)
}

fn write_instance_backup_manifest(
    backup_path: &Path,
    backup: &InstanceBackupResult,
) -> Result<(), StorageError> {
    let manifest_path = backup_path.join("backup.json");
    let manifest = serde_json::to_string_pretty(backup)?;
    if let Ok(metadata) = fs::symlink_metadata(&manifest_path) {
        require_plain_file(&manifest_path, backup_path, &metadata)?;
    }
    write_file_atomically(&manifest_path, manifest.as_bytes()).map_err(|source| {
        StorageError::WriteConfig {
            path: manifest_path,
            source,
        }
    })?;
    Ok(())
}

fn instance_backup_root(record: &StoredInstanceRecord) -> PathBuf {
    instance_root(record).join("backups")
}

fn instance_root(record: &StoredInstanceRecord) -> PathBuf {
    record
        .config_dir
        .parent()
        .unwrap_or(record.config_dir.as_path())
        .to_path_buf()
}

fn validated_backup_path(
    record: &StoredInstanceRecord,
    backup_id: &str,
) -> Result<PathBuf, StorageError> {
    let path = Path::new(backup_id);
    let mut components = path.components();
    let is_single_normal = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    if backup_id.trim() != backup_id || backup_id.is_empty() || !is_single_normal {
        return Err(StorageError::InvalidBackupId {
            backup_id: String::from(backup_id),
        });
    }
    let root = instance_backup_root(record);
    let candidate = root.join(backup_id);
    match fs::symlink_metadata(&root) {
        Ok(metadata) => require_plain_directory(&root, &root, &metadata)?,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
        Err(source) => {
            return Err(StorageError::ReadPath { path: root, source });
        }
    }
    match fs::symlink_metadata(&candidate) {
        Ok(_) => validate_direct_child_directory(&candidate, &root)?,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: candidate.clone(),
                source,
            });
        }
    }
    Ok(candidate)
}

fn backup_created_at_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn infer_backup_kind_from_backup_id(backup_id: &str) -> InstanceBackupKind {
    let normalized = backup_id.trim().to_ascii_lowercase();
    if normalized.starts_with("pre-restore-") {
        InstanceBackupKind::PreRestore
    } else if normalized.starts_with("auto-stop-") {
        InstanceBackupKind::AutoStop
    } else {
        InstanceBackupKind::Manual
    }
}

fn read_instance_backup_manifest(backup_path: &Path) -> Result<InstanceBackupResult, StorageError> {
    let manifest_path = backup_path.join("backup.json");
    let metadata =
        fs::symlink_metadata(&manifest_path).map_err(|source| StorageError::ReadPath {
            path: manifest_path.clone(),
            source,
        })?;
    require_plain_file(&manifest_path, backup_path, &metadata)?;
    let manifest_text =
        fs::read_to_string(&manifest_path).map_err(|source| StorageError::ReadConfig {
            path: manifest_path,
            source,
        })?;
    let manifest_value: Value = serde_json::from_str(&manifest_text)?;
    let mut backup: InstanceBackupResult = serde_json::from_value(manifest_value.clone())?;
    if manifest_value.get("backup_kind").is_none() {
        backup.backup_kind = infer_backup_kind_from_backup_id(&backup.backup_id);
    }
    Ok(backup)
}

#[cfg(test)]
fn copy_directory_contents(
    source_dir: &Path,
    target_dir: &Path,
) -> Result<DirectoryCopyStats, StorageError> {
    copy_directory_contents_with_hook(source_dir, target_dir, &mut |_, _| Ok(()))
}

fn copy_directory_contents_with_hook<F>(
    source_dir: &Path,
    target_dir: &Path,
    before_copy: &mut F,
) -> Result<DirectoryCopyStats, StorageError>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let canonical_source = canonical_plain_directory(source_dir, source_dir)?;
    let canonical_target = canonical_plain_directory(target_dir, target_dir)?;
    copy_directory_contents_recursive(
        source_dir,
        target_dir,
        &canonical_source,
        &canonical_target,
        before_copy,
    )
}

fn copy_directory_contents_recursive<F>(
    source_dir: &Path,
    target_dir: &Path,
    canonical_source_root: &Path,
    canonical_target_root: &Path,
    before_copy: &mut F,
) -> Result<DirectoryCopyStats, StorageError>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let canonical_target = canonical_plain_directory(target_dir, canonical_target_root)?;
    if !canonical_target.starts_with(canonical_target_root) {
        return Err(StorageError::UnsafeManagedPath {
            path: canonical_target,
            root: canonical_target_root.to_path_buf(),
        });
    }
    let mut stats = DirectoryCopyStats::default();
    let entries = fs::read_dir(source_dir).map_err(|source| StorageError::ReadDirectory {
        path: source_dir.to_path_buf(),
        source,
    })?;

    for entry_result in entries {
        let entry = entry_result.map_err(|source| StorageError::ReadDirectory {
            path: source_dir.to_path_buf(),
            source,
        })?;
        let source_path = entry.path();
        let target_path = target_dir.join(entry.file_name());
        let metadata =
            fs::symlink_metadata(&source_path).map_err(|source| StorageError::ReadPath {
                path: source_path.clone(),
                source,
            })?;
        if is_link_or_reparse(&metadata) {
            return Err(StorageError::UnsafeManagedPath {
                path: source_path,
                root: canonical_source_root.to_path_buf(),
            });
        }
        let canonical_source =
            fs::canonicalize(&source_path).map_err(|source| StorageError::ReadPath {
                path: source_path.clone(),
                source,
            })?;
        if !canonical_source.starts_with(canonical_source_root) {
            return Err(StorageError::UnsafeManagedPath {
                path: canonical_source,
                root: canonical_source_root.to_path_buf(),
            });
        }

        if metadata.is_dir() {
            fs::create_dir(&target_path).map_err(|source| StorageError::CreatePath {
                path: target_path.clone(),
                source,
            })?;
            let nested_stats = copy_directory_contents_recursive(
                &source_path,
                &target_path,
                canonical_source_root,
                canonical_target_root,
                before_copy,
            )?;
            stats.file_count += nested_stats.file_count;
            stats.total_bytes += nested_stats.total_bytes;
            continue;
        }
        if !metadata.is_file() {
            return Err(StorageError::UnsafeManagedPath {
                path: source_path,
                root: canonical_source_root.to_path_buf(),
            });
        }

        before_copy(&source_path, &target_path).map_err(|source| StorageError::CopyPath {
            from: source_path.clone(),
            to: target_path.clone(),
            source,
        })?;

        let copied_bytes =
            fs::copy(&source_path, &target_path).map_err(|source| StorageError::CopyPath {
                from: source_path,
                to: target_path,
                source,
            })?;
        stats.file_count += 1;
        stats.total_bytes += copied_bytes;
    }

    Ok(stats)
}

pub(crate) fn ensure_plain_directory(path: &Path) -> Result<(), StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => require_plain_directory(path, path, &metadata),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| StorageError::UnsafeManagedPath {
                    path: path.to_path_buf(),
                    root: path.to_path_buf(),
                })?;
            if parent != path {
                ensure_plain_directory(parent)?;
            }
            fs::create_dir(path).map_err(|source| StorageError::CreatePath {
                path: path.to_path_buf(),
                source,
            })?;
            let metadata = fs::symlink_metadata(path).map_err(|source| StorageError::ReadPath {
                path: path.to_path_buf(),
                source,
            })?;
            require_plain_directory(path, path, &metadata)
        }
        Err(source) => Err(StorageError::ReadPath {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(crate) fn canonical_plain_directory(path: &Path, root: &Path) -> Result<PathBuf, StorageError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| StorageError::ReadPath {
        path: path.to_path_buf(),
        source,
    })?;
    require_plain_directory(path, root, &metadata)?;
    fs::canonicalize(path).map_err(|source| StorageError::ReadPath {
        path: path.to_path_buf(),
        source,
    })
}

fn require_plain_directory(
    path: &Path,
    root: &Path,
    metadata: &fs::Metadata,
) -> Result<(), StorageError> {
    if !metadata.is_dir() || is_link_or_reparse(metadata) {
        return Err(StorageError::UnsafeManagedPath {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        });
    }
    Ok(())
}

fn require_plain_file(
    path: &Path,
    root: &Path,
    metadata: &fs::Metadata,
) -> Result<(), StorageError> {
    if !metadata.is_file() || is_link_or_reparse(metadata) {
        return Err(StorageError::UnsafeManagedPath {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        });
    }
    Ok(())
}

fn validate_direct_child_directory(path: &Path, root: &Path) -> Result<(), StorageError> {
    let canonical_root = canonical_plain_directory(root, root)?;
    let canonical_path = canonical_plain_directory(path, &canonical_root)?;
    if canonical_path.parent() != Some(canonical_root.as_path()) {
        return Err(StorageError::UnsafeManagedPath {
            path: canonical_path,
            root: canonical_root,
        });
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

pub(crate) fn create_unique_directory(
    parent: &Path,
    prefix: &str,
) -> Result<PathBuf, StorageError> {
    for _ in 0..8 {
        let path = unique_available_path(parent, prefix);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(StorageError::CreatePath { path, source }),
        }
    }
    let path = unique_available_path(parent, prefix);
    Err(StorageError::CreatePath {
        path,
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a unique transaction directory",
        ),
    })
}

fn unique_available_path(parent: &Path, prefix: &str) -> PathBuf {
    let random = Uuid::new_v4().as_u128() & 0x0000_ffff_ffff_ffff;
    parent.join(format!("{prefix}-{random:012x}"))
}

fn publish_new_directory(from: &Path, to: &Path) -> Result<(), StorageError> {
    match fs::symlink_metadata(to) {
        Ok(_) => {
            return Err(StorageError::MovePath {
                from: from.to_path_buf(),
                to: to.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "refusing to replace an existing directory",
                ),
            });
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: to.to_path_buf(),
                source,
            });
        }
    }
    move_directory(from, to)
}

pub(crate) fn move_directory(from: &Path, to: &Path) -> Result<(), StorageError> {
    fs::rename(from, to).map_err(|source| StorageError::MovePath {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })
}

pub(crate) fn remove_managed_directory(path: &Path, root: &Path) -> Result<(), StorageError> {
    validate_direct_child_directory(path, root)?;
    fs::remove_dir_all(path).map_err(|source| StorageError::DeletePath {
        path: path.to_path_buf(),
        source,
    })
}

struct DirectoryCleanup {
    path: Option<PathBuf>,
    root: PathBuf,
}

impl DirectoryCleanup {
    fn new(path: PathBuf, root: PathBuf) -> Self {
        Self {
            path: Some(path),
            root,
        }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for DirectoryCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = remove_managed_directory(&path, &self.root);
        }
    }
}

struct RestoreRollback {
    target: PathBuf,
    rollback: Option<PathBuf>,
}

impl RestoreRollback {
    fn new(target: PathBuf, rollback: PathBuf) -> Self {
        Self {
            target,
            rollback: Some(rollback),
        }
    }

    fn restore(&mut self) -> Result<(), StorageError> {
        let Some(rollback) = self.rollback.as_ref() else {
            return Ok(());
        };
        move_directory(rollback, &self.target)?;
        self.rollback = None;
        Ok(())
    }

    fn commit(&mut self) {
        self.rollback = None;
    }
}

impl Drop for RestoreRollback {
    fn drop(&mut self) {
        if let Some(rollback) = self.rollback.take() {
            let _ = move_directory(&rollback, &self.target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, Mutex};

    include!("backups_retention_tests.rs");
    include!("backups_restore_isolation_tests.rs");
    include!("backups_checked_tests.rs");

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "langame-backup-{label}-{}",
            Uuid::new_v4().as_simple()
        ))
    }

    fn test_record(root: &Path) -> StoredInstanceRecord {
        let instance_root = root.join("instance");
        StoredInstanceRecord {
            summary: InstanceSummary {
                id: String::from("instance-1"),
                name: String::from("Backup Test"),
                module_id: String::from("test"),
                status: InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: String::from("127.0.0.1"),
                port_count: 0,
                autostart: false,
            },
            config_dir: instance_root.join("config"),
            saves_dir: instance_root.join("saves"),
            runtime_mode: String::from("independent"),
            program_install_root: Some(instance_root.join("runtime")),
            auto_backup_on_stop: false,
            backup_retention_count: 3,
        }
    }

    #[test]
    fn failed_backup_copy_is_cleaned_without_publishing() {
        let root = test_root("copy-failure");
        let record = test_record(&root);
        fs::create_dir_all(&record.saves_dir).unwrap();
        fs::write(record.saves_dir.join("world.db"), b"world").unwrap();

        let error = create_instance_backup_with_copy_hook(
            &record,
            &record.saves_dir,
            &record.summary.id,
            "saves",
            InstanceBackupKind::Manual,
            &mut |_, _| Err(std::io::Error::other("injected copy failure")),
        )
        .expect_err("copy failure must abort backup publication");

        assert!(matches!(error, StorageError::CopyPath { .. }));
        assert_eq!(
            fs::read_dir(instance_backup_root(&record)).unwrap().count(),
            0
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publish_refuses_an_existing_destination_without_mutation() {
        let root = test_root("publish-conflict");
        let staging = root.join(".publish-source");
        let destination = root.join("saves-existing");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(staging.join("new.db"), b"new").unwrap();
        fs::write(destination.join("world.db"), b"original").unwrap();

        let error = publish_new_directory(&staging, &destination)
            .expect_err("an existing destination must never be replaced");

        assert!(matches!(error, StorageError::MovePath { .. }));
        assert_eq!(fs::read(destination.join("world.db")).unwrap(), b"original");
        assert_eq!(fs::read(staging.join("new.db")).unwrap(), b"new");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_restore_publish_rolls_back_original_saves() {
        let root = test_root("restore-rollback");
        let source = root.join("source");
        let target = root.join("saves");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("world.db"), b"replacement").unwrap();
        fs::write(target.join("world.db"), b"original").unwrap();

        let error = replace_directory_from_source_with_publish(
            &source,
            &target,
            &mut |_, _| Ok(()),
            |from, to| {
                Err(StorageError::MovePath {
                    from: from.to_path_buf(),
                    to: to.to_path_buf(),
                    source: std::io::Error::other("injected publish failure"),
                })
            },
        )
        .expect_err("publish failure must abort the restore");

        assert!(matches!(error, StorageError::MovePath { .. }));
        assert_eq!(fs::read(target.join("world.db")).unwrap(), b"original");
        let transaction_directories = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".restore-"))
            .count();
        assert_eq!(transaction_directories, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recursive_copy_rejects_directory_links() {
        let root = test_root("linked-source");
        let source = root.join("source");
        let target = root.join("target");
        let outside = root.join("outside");
        let linked = source.join("linked");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.db"), b"outside").unwrap();
        create_directory_link(&linked, &outside);

        let error = copy_directory_contents(&source, &target)
            .expect_err("links and Windows reparse points must be rejected");

        assert!(matches!(error, StorageError::UnsafeManagedPath { .. }));
        assert!(!target.join("linked").exists());
        fs::remove_dir(&linked).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    fn create_directory_link(link: &Path, target: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn create_directory_link(link: &Path, target: &Path) {
        let output = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "failed to create test junction: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelling_a_waiter_does_not_release_the_running_transaction_lock() {
        let transaction_root = test_root("cancel-lock");
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let first_release = Arc::clone(&release);
        let (first_started_tx, first_started_rx) = tokio::sync::oneshot::channel();
        let first = tokio::spawn(run_instance_transaction(
            transaction_root.clone(),
            move || {
                let _ = first_started_tx.send(());
                let (released, wake) = &*first_release;
                let mut released = released
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*released {
                    released = wake
                        .wait(released)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                Ok::<(), StorageError>(())
            },
        ));
        first_started_rx.await.unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());

        let (second_started_tx, mut second_started_rx) = tokio::sync::oneshot::channel();
        let second = tokio::spawn(run_instance_transaction(transaction_root, move || {
            let _ = second_started_tx.send(());
            Ok::<(), StorageError>(())
        }));
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut second_started_rx)
                .await
                .is_err(),
            "a cancelled caller released the lock while its blocking transaction was running"
        );

        let (released, wake) = &*release;
        *released
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        wake.notify_all();
        tokio::time::timeout(Duration::from_secs(2), &mut second_started_rx)
            .await
            .unwrap()
            .unwrap();
        second.await.unwrap().unwrap();
    }
}
