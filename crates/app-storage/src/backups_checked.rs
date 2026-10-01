/// A server-owned confirmation snapshot. It is never deserialized from a model
/// or client; both the selected backup and the saves it replaces are bound.
#[derive(Debug, Clone)]
pub struct PreparedInstanceBackupRestore {
    pub backup: InstanceBackupResult,
    source_sha256: String,
    target_sha256: String,
    target_path: PathBuf,
    dst_settings_sha256: Option<String>,
}

impl PreparedInstanceBackupRestore {
    pub fn validate_current_contents(&self) -> Result<(), StorageError> {
        self.validate_contents(&Path::new(&self.backup.backup_path).join("saves"), &self.target_path)
    }

    pub fn dst_settings_snapshot(&self) -> Result<Option<serde_json::Value>, StorageError> {
        let path = Path::new(&self.backup.backup_path).join("dst-instance.json");
        let Some(bytes) = read_dst_backup_settings(&path)? else { return Ok(None); };
        if Some(dst_settings_hash(&bytes)) != self.dst_settings_sha256 {
            return Err(backup_confirmation_error(&path, "DST configuration changed after preview"));
        }
        Ok(Some(serde_json::from_slice(&bytes)?))
    }
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    pub fn matches_current_saves(&self) -> bool {
        self.source_sha256 == self.target_sha256
    }

    fn validate(
        &self,
        current: &InstanceBackupResult,
        target: &Path,
        source: &Path,
    ) -> Result<(), StorageError> {
        if serde_json::to_value(current)? != serde_json::to_value(&self.backup)?
            || target != self.target_path
        {
            return Err(backup_confirmation_error(
                target,
                "Backup identity or save location changed after preview",
            ));
        }
        self.validate_contents(source, target)
    }

    fn validate_contents(&self, source: &Path, target: &Path) -> Result<(), StorageError> {
        let path = Path::new(&self.backup.backup_path).join("dst-instance.json");
        if read_dst_backup_settings(&path)?.as_deref().map(dst_settings_hash) != self.dst_settings_sha256 {
            return Err(backup_confirmation_error(&path, "DST configuration changed after preview"));
        }
        if backup_tree_sha256(source)? != self.source_sha256
            || backup_tree_sha256(target)? != self.target_sha256
        {
            return Err(backup_confirmation_error(
                target,
                "Backup or current saves changed after preview; request a new confirmation",
            ));
        }
        Ok(())
    }
}

pub async fn prepare_instance_backup_restore(
    paths: &StoragePaths,
    instance_id: &str,
    backup_id: &str,
) -> Result<PreparedInstanceBackupRestore, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
    let install_root = effective_instance_install_root(&record)?;
    let saves_dir = effective_instance_saves_dir(descriptor.as_ref(), &install_root, &record)?;
    pool.close().await;
    let instance_id = instance_id.to_owned();
    let backup_id = backup_id.to_owned();
    run_instance_transaction(instance_root(&record), move || {
        let path = validated_backup_path(&record, &backup_id)?;
        let backup = load_instance_backup_for_instance(&path, &instance_id, &backup_id)?;
        let saves_dir = backup_restore_scope(&record, &saves_dir, &backup);
        Ok(PreparedInstanceBackupRestore {
            backup,
            source_sha256: backup_tree_sha256(&path.join("saves"))?,
            target_sha256: backup_tree_sha256(&saves_dir)?,
            target_path: saves_dir,
            dst_settings_sha256: read_dst_backup_settings(&path.join("dst-instance.json"))?.as_deref().map(dst_settings_hash),
        })
    })
    .await
}

/// Reuse the server-owned path/identity and complete content binding when a
/// game-specific restore coordinates native saves with canonical settings.
pub async fn revalidate_prepared_instance_backup_restore(
    paths: &StoragePaths,
    instance_id: &str,
    prepared: &PreparedInstanceBackupRestore,
) -> Result<(), StorageError> {
    if prepared.backup.instance_id != instance_id {
        return Err(backup_confirmation_error(&prepared.target_path, "Backup confirmation belongs to another instance"));
    }
    let current = prepare_instance_backup_restore(paths, instance_id, &prepared.backup.backup_id).await?;
    if serde_json::to_value(&current.backup)? != serde_json::to_value(&prepared.backup)?
        || current.source_sha256 != prepared.source_sha256 || current.target_sha256 != prepared.target_sha256
        || current.target_path != prepared.target_path || current.dst_settings_sha256 != prepared.dst_settings_sha256
    {
        return Err(backup_confirmation_error(&prepared.target_path, "Backup or current saves changed after preview; request a new confirmation"));
    }
    Ok(())
}

fn read_dst_backup_settings(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    use std::io::Read;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(StorageError::ReadPath { path: path.to_owned(), source }),
    };
    let root = path.parent().ok_or_else(|| backup_confirmation_error(path, "DST configuration has no parent"))?;
    let canonical = canonical_plain_directory(root, root)?;
    require_plain_file(path, &canonical, &metadata)?;
    let limit = 1024 * 1024;
    if metadata.len() > limit { return Err(backup_confirmation_error(path, "DST configuration exceeds the 1 MiB verification limit")); }
    let mut bytes = Vec::new();
    fs::File::open(path).and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|source| StorageError::ReadPath { path: path.to_owned(), source })?;
    if bytes.len() as u64 > limit { return Err(backup_confirmation_error(path, "DST configuration changed during verification")); }
    Ok(Some(bytes))
}

fn dst_settings_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

// Historical ARK snapshots contain only the primary map. Restoring one must
// retain sibling worlds now owned by the aggregate Saved directory.
fn backup_restore_scope(
    record: &StoredInstanceRecord,
    saves_dir: &Path,
    backup: &InstanceBackupResult,
) -> PathBuf {
    let historical = Path::new(&backup.saves_path);
    let is_saved = |path: &Path| path.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("Saved"));
    if app_core::ark_maps::is_ark(&record.summary.module_id)
        && is_saved(saves_dir)
        && historical.file_name().is_some_and(|name| name == record.summary.id.as_str())
        && historical.parent().is_some_and(is_saved)
    {
        saves_dir.join(&record.summary.id)
    } else {
        saves_dir.to_owned()
    }
}

pub async fn restore_prepared_instance_backup(
    paths: &StoragePaths,
    instance_id: &str,
    prepared: PreparedInstanceBackupRestore,
) -> Result<InstanceBackupRestoreResult, StorageError> {
    if prepared.backup.instance_id != instance_id {
        return Err(backup_confirmation_error(
            &prepared.target_path,
            "Backup confirmation belongs to another instance",
        ));
    }
    let backup_id = prepared.backup.backup_id.clone();
    restore_instance_backup_checked(paths, instance_id, &backup_id, Some(prepared)).await
}

fn backup_confirmation_error(path: &Path, message: &str) -> StorageError {
    StorageError::ReadPath {
        path: path.to_owned(),
        source: std::io::Error::other(message.to_owned()),
    }
}

// Streaming content hashes include relative names and empty directories. The
// fixed limits bound memory/work, and unsafe entries fail instead of disappearing.
fn backup_tree_sha256(root: &Path) -> Result<String, StorageError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let canonical_root = canonical_plain_directory(root, root)?;
    let mut pending = vec![(root.to_owned(), 0_usize)];
    let mut entries = std::collections::BTreeMap::new();
    let mut total_bytes = 0_u64;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 64 {
            return Err(backup_confirmation_error(
                root,
                "Backup exceeds the 64-level verification limit",
            ));
        }
        for child in fs::read_dir(&directory).map_err(|source| StorageError::ReadDirectory {
            path: directory.clone(),
            source,
        })? {
            let path = child
                .map_err(|source| StorageError::ReadDirectory {
                    path: directory.clone(),
                    source,
                })?
                .path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|source| StorageError::ReadPath {
                    path: path.clone(),
                    source,
                })?;
            if entries.len() >= 200_000 {
                return Err(backup_confirmation_error(
                    root,
                    "Backup exceeds the 200000-entry verification limit",
                ));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| backup_confirmation_error(&path, "Backup entry escapes its root"))?
                .to_owned();
            if metadata.is_dir() {
                require_plain_directory(&path, &canonical_root, &metadata)?;
                entries.insert(relative, None);
                pending.push((path, depth + 1));
            } else {
                require_plain_file(&path, &canonical_root, &metadata)?;
                let mut file = fs::File::open(&path).map_err(|source| StorageError::ReadPath {
                    path: path.clone(),
                    source,
                })?;
                let mut hash = Sha256::new();
                let mut bytes = 0_u64;
                let mut buffer = [0_u8; 65536];
                loop {
                    let count =
                        file.read(&mut buffer)
                            .map_err(|source| StorageError::ReadPath {
                                path: path.clone(),
                                source,
                            })?;
                    if count == 0 {
                        break;
                    }
                    bytes += count as u64;
                    total_bytes = total_bytes
                        .checked_add(count as u64)
                        .filter(|bytes| *bytes <= 1024 * 1024 * 1024 * 1024)
                        .ok_or_else(|| {
                            backup_confirmation_error(
                                root,
                                "Backup exceeds the 1 TiB verification limit",
                            )
                        })?;
                    hash.update(&buffer[..count]);
                }
                if bytes != metadata.len() {
                    return Err(backup_confirmation_error(
                        &path,
                        "Backup file changed during verification",
                    ));
                }
                let digest = hash
                    .finalize()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                entries.insert(relative, Some((bytes, digest)));
            }
        }
    }
    let manifest = serde_json::to_vec(&entries)?;
    Ok(Sha256::digest(manifest)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
