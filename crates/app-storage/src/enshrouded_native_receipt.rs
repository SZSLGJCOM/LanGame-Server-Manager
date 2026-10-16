use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use app_core::{ActiveInstanceRun, InstanceDetails, PortBinding};
use serde_json::{Map, Value};

use crate::enshrouded_bans::parse_enshrouded_ban_records;
use crate::instance_file_patch::io::{guard_directories, is_link, read_bytes_with_limit};
use crate::instance_settings_lock::{
    InstanceSettingsLock, acquire_instance_settings_mutation_lock,
};
use crate::instances::{commit_instance_transaction, effective_instance_install_root};
use crate::player_access_normalization::parse_enshrouded_banned_account_ids;
use crate::runtime::load_active_instance_run;
use crate::save_paths::{
    InstanceSavePathContext, load_module_descriptor, module_declares_saves_path,
    plan_instance_saves_dir,
};
use crate::storage_db::{connect_pool, fetch_instance_record, load_instance_ports};
use crate::templates::{ManagedConfigMergePlan, ManagedConfigMutation};
use crate::{StorageError, StoragePaths, StoredInstanceRecord};

const MODULE: &str = "enshrouded";
const FIELD: &str = "banned_player_ids";
const FILE: &str = "enshrouded_server.json";
const MAX_BYTES: usize = crate::instance_file_patch::MAX_FILE_BYTES;
const OPERATION: &str = "record confirmed Enshrouded native moderation";

/// Mirror one verified native result without rendering or replacing the running
/// server's configuration. Native dispatch alone is not a receipt.
pub async fn record_enshrouded_ban_from_native(
    paths: &StoragePaths,
    instance_id: &str,
    expected_run_id: i64,
    account_id: u64,
    banned: bool,
) -> Result<InstanceDetails, StorageError> {
    if account_id == 0 || expected_run_id <= 0 {
        return Err(invalid("A native account and active run are required."));
    }
    let lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    let instance_id = instance_id.to_owned();
    lock.complete_mutation(OPERATION, async move {
        record_locked(
            &paths,
            &instance_id,
            expected_run_id,
            account_id,
            banned,
            &worker_lock,
        )
        .await
    })
    .await
}

async fn record_locked(
    paths: &StoragePaths,
    instance_id: &str,
    expected_run_id: i64,
    account_id: u64,
    banned: bool,
    lock: &InstanceSettingsLock,
) -> Result<InstanceDetails, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let record = fetch_instance_record(&pool, instance_id).await?;
        if record.summary.module_id != MODULE {
            return Err(invalid("Native moderation recording requires Enshrouded."));
        }
        let active_run = load_active_instance_run(&pool, instance_id).await?;
        ensure_run(active_run.as_ref(), expected_run_id)?;
        let ports = load_instance_ports(&pool, instance_id).await?;
        let worker_paths = paths.clone();
        let mut prepared = lock
            .spawn_blocking(move || {
                prepare(&worker_paths, record, ports, active_run, account_id, banned)
            })
            .await
            .map_err(join_error)??;
        #[cfg(test)]
        {
            let database = paths.database_path.clone();
            lock.spawn_blocking(move || {
                crate::instance_archive::test_gate::pause(
                    &database,
                    crate::instance_archive::test_gate::Point::NativeSettingsReady,
                );
            })
            .await
            .map_err(join_error)?;
        }
        // Retain SQLite's writer reservation only for the bounded publication.
        // A replacement run cannot become current between this check and CAS.
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = fetch_instance_record(&mut *tx, instance_id).await?;
        let active_run = load_active_instance_run(&mut *tx, instance_id).await?;
        ensure_run(active_run.as_ref(), expected_run_id)?;
        prepared.details.active_run = active_run;
        let AppliedReceipt {
            details,
            files,
            _native_guard,
            _directory_guards,
        } = lock
            .spawn_blocking(move || prepared.apply(current))
            .await
            .map_err(join_error)??;
        commit_instance_transaction(tx, files, lock).await?;
        Ok(details)
    }
    .await;
    pool.close().await;
    result
}

struct PreparedReceipt {
    config_path: PathBuf,
    config_bytes: Vec<u8>,
    native_path: PathBuf,
    native_bytes: Vec<u8>,
    managed_path: PathBuf,
    managed_bytes: Option<Vec<u8>>,
    plans: Vec<ManagedConfigMergePlan>,
    details: InstanceDetails,
}

struct AppliedReceipt {
    details: InstanceDetails,
    files: ManagedConfigMutation,
    _native_guard: File,
    _directory_guards: Vec<File>,
}

impl PreparedReceipt {
    fn apply(self, current: StoredInstanceRecord) -> Result<AppliedReceipt, StorageError> {
        if current.summary.module_id != MODULE
            || current.config_dir.join("instance.json") != self.config_path
            || effective_instance_install_root(&current)?.join(FILE) != self.native_path
        {
            return Err(precondition(&current.summary.id));
        }
        let mut guards = guard_directories(parent(&self.config_path)?)?;
        guards.extend(guard_directories(parent(&self.native_path)?)?);
        // Keep the confirmed runtime file read-only shared until the database
        // reservation and all file publications have committed or compensated.
        let native_guard = native_read_guard(&self.native_path)?;
        if read_bytes_with_limit(&self.native_path, MAX_BYTES)? != self.native_bytes
            || read_optional(&self.managed_path)? != self.managed_bytes
            || read_bytes_with_limit(&self.config_path, MAX_BYTES)? != self.config_bytes
        {
            return Err(precondition(&current.summary.id));
        }
        let mut files = ManagedConfigMutation::new(MODULE);
        if let Err(error) = files.apply(self.plans) {
            return Err(files.rollback_after(error));
        }
        Ok(AppliedReceipt {
            details: self.details,
            files,
            _native_guard: native_guard,
            _directory_guards: guards,
        })
    }
}

fn prepare(
    paths: &StoragePaths,
    record: StoredInstanceRecord,
    ports: Vec<PortBinding>,
    active_run: Option<ActiveInstanceRun>,
    account_id: u64,
    banned: bool,
) -> Result<PreparedReceipt, StorageError> {
    let config_path = record.config_dir.join("instance.json");
    let config_bytes = read_bytes_with_limit(&config_path, MAX_BYTES)?;
    let mut document = parse_document(&config_bytes, &config_path)?;
    let settings = document
        .get_mut("settings")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("Instance configuration requires a settings object."))?;
    let install_root = effective_instance_install_root(&record)?;
    let native_path = install_root.join(FILE);
    let managed_path = record.config_dir.join(FILE);
    if crate::instance_isolation::paths::normalize_resource_path(&native_path)?
        == crate::instance_isolation::paths::normalize_resource_path(&managed_path)?
    {
        return Err(invalid(
            "Native and managed configurations must be distinct.",
        ));
    }
    let native_bytes = read_bytes_with_limit(&native_path, MAX_BYTES)?;
    confirm_native(
        &parse_document(&native_bytes, &native_path)?,
        &native_path,
        account_id,
        banned,
    )?;
    let managed_bytes = read_optional(&managed_path)?;
    let mut plans = Vec::new();
    if let Some(bytes) = &managed_bytes {
        let mut managed = parse_document(bytes, &managed_path)?;
        parse_enshrouded_ban_records(&managed, &managed_path)?;
        if !banned && remove_managed_account(&mut managed, account_id) {
            plans.push(ManagedConfigMergePlan {
                destination_path: managed_path.clone(),
                original: Some(bytes.clone()),
                replacement: serialize_document(&managed, bytes)?,
            });
        }
    }
    let mut ids: BTreeSet<_> = parse_enshrouded_banned_account_ids(settings)
        .map_err(|message| invalid(&message))?
        .into_iter()
        .collect();
    if banned {
        ids.insert(account_id);
    } else {
        ids.remove(&account_id);
    }
    let replacement_ids = Value::String(
        ids.iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let settings_changed = settings.get(FIELD) != Some(&replacement_ids);
    settings.insert(FIELD.to_owned(), replacement_ids);
    let settings_json = serde_json::to_string_pretty(settings)?;
    let descriptor = load_module_descriptor(paths, MODULE)?
        .ok_or_else(|| invalid("The Enshrouded module is unavailable."))?;
    let saves_path = plan_instance_saves_dir(
        Some(&descriptor),
        &InstanceSavePathContext {
            install_root: &install_root,
            instance_root: record.config_dir.parent().unwrap_or(&record.config_dir),
            config_dir: &record.config_dir,
            instance_id: &record.summary.id,
            instance_name: &record.summary.name,
            module_id: MODULE,
            settings: Some(settings),
        },
    )?;
    if settings_changed {
        plans.push(ManagedConfigMergePlan {
            destination_path: config_path.clone(),
            original: Some(config_bytes.clone()),
            replacement: serialize_document(&document, &config_bytes)?,
        });
    }
    Ok(PreparedReceipt {
        config_path: config_path.clone(),
        config_bytes,
        native_path,
        native_bytes,
        managed_path,
        managed_bytes,
        plans,
        details: InstanceDetails {
            summary: record.summary,
            config_file_path: config_path.to_string_lossy().into_owned(),
            saves_path: saves_path.to_string_lossy().into_owned(),
            backup_uses_declared_saves_path: module_declares_saves_path(Some(&descriptor)),
            auto_backup_on_stop: record.auto_backup_on_stop,
            backup_retention_count: record.backup_retention_count,
            settings_json,
            ports,
            active_run,
        },
    })
}

fn confirm_native(
    document: &Map<String, Value>,
    path: &Path,
    account_id: u64,
    banned: bool,
) -> Result<(), StorageError> {
    let canonical = document
        .get("bannedAccounts")
        .filter(|value| value.is_array())
        .ok_or_else(|| {
            invalid("The runtime must contain a complete canonical bannedAccounts array.")
        })?;
    let records = parse_enshrouded_ban_records(document, path)?;
    let canonical_records = parse_enshrouded_ban_records(
        &Map::from_iter([("bannedAccounts".to_owned(), canonical.clone())]),
        path,
    )?;
    if canonical_records.contains_key(&account_id) != banned {
        return Err(invalid(
            "The runtime configuration does not confirm this native moderation result.",
        ));
    }
    if !banned && records.contains_key(&account_id) {
        return Err(invalid(
            "A retired runtime ban alias still contains this account; refusing an incomplete unban.",
        ));
    }
    Ok(())
}

fn remove_managed_account(document: &mut Map<String, Value>, account_id: u64) -> bool {
    let mut changed = false;
    for key in ["bannedAccounts", "bans"] {
        if let Some(Value::Array(entries)) = document.get_mut(key) {
            let previous = entries.len();
            entries
                .retain(|entry| entry.get("accountId").and_then(Value::as_u64) != Some(account_id));
            changed |= entries.len() != previous;
        }
    }
    changed
}

fn parse_document(bytes: &[u8], path: &Path) -> Result<Map<String, Value>, StorageError> {
    serde_json::from_slice::<Value>(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
        .map_err(|source| StorageError::InvalidConfigJson {
            path: path.to_owned(),
            source,
        })?
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("Configuration must contain a JSON object."))
}

fn serialize_document(
    document: &Map<String, Value>,
    original: &[u8],
) -> Result<Vec<u8>, StorageError> {
    let mut text = serde_json::to_string_pretty(document)?;
    if original.windows(2).any(|bytes| bytes == b"\r\n") {
        text = text.replace('\n', "\r\n");
    }
    if original.ends_with(b"\n") {
        text.push_str(if original.ends_with(b"\r\n") {
            "\r\n"
        } else {
            "\n"
        });
    }
    let mut bytes = if original.starts_with(&[0xef, 0xbb, 0xbf]) {
        vec![0xef, 0xbb, 0xbf]
    } else {
        Vec::new()
    };
    bytes.extend_from_slice(text.as_bytes());
    if bytes.len() > MAX_BYTES {
        return Err(invalid(
            "Updated configuration exceeds the safe 256 KiB limit.",
        ));
    }
    Ok(bytes)
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => read_bytes_with_limit(path, MAX_BYTES).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(StorageError::ReadPath {
            path: path.to_owned(),
            source,
        }),
    }
}

fn native_read_guard(path: &Path) -> Result<File, StorageError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ);
    }
    let file = options
        .open(path)
        .map_err(|source| StorageError::ReadConfig {
            path: path.to_owned(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| StorageError::ReadConfig {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_BYTES as u64 {
        return Err(invalid(
            "The confirmed runtime configuration must remain a bounded ordinary file.",
        ));
    }
    Ok(file)
}

fn ensure_run(actual: Option<&ActiveInstanceRun>, expected: i64) -> Result<(), StorageError> {
    if actual.map(|run| run.run_id) != Some(expected) {
        return Err(invalid(
            "The active run changed before native moderation could be recorded.",
        ));
    }
    Ok(())
}

fn parent(path: &Path) -> Result<&Path, StorageError> {
    path.parent()
        .ok_or_else(|| invalid("Configuration has no parent directory."))
}

fn precondition(instance_id: &str) -> StorageError {
    StorageError::InstanceSettingsPreconditionFailed {
        id: instance_id.to_owned(),
    }
}

fn invalid(message: &str) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: MODULE.to_owned(),
        field: FIELD.to_owned(),
        message: message.to_owned(),
    }
}

fn join_error(error: tokio::task::JoinError) -> StorageError {
    StorageError::BlockingTaskFailed {
        operation: OPERATION,
        message: error.to_string(),
    }
}
