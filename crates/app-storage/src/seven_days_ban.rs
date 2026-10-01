use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use app_core::InstanceDetails;
use serde_json::{Map, Value, json};

use crate::atomic_file::compare_and_swap_file_atomically;
use crate::instance_file_patch::io::{guard_directories, is_link, read_bytes_with_limit};
use crate::instance_settings_lock::{
    InstanceSettingsLock, acquire_instance_settings_mutation_lock,
};
use crate::instances::effective_instance_install_root;
use crate::player_access_normalization::normalize_module_player_access_settings_strict;
use crate::runtime::load_active_instance_run;
use crate::save_paths::{
    InstanceSavePathContext, load_module_descriptor, module_declares_saves_path,
    plan_instance_saves_dir,
};
use crate::settings_validation::{SettingsValidationPhase, collect_settings_schema_diagnostics};
use crate::storage_db::{connect_pool, fetch_instance_record, load_instance_ports};
use crate::{StorageError, StoragePaths, StoredInstanceRecord};

#[path = "seven_days_ban_xml.rs"]
mod xml;

const MODULE_ID: &str = "sevendaystodie";
const FIELD: &str = "blacklist_entries";
const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
const OPERATION: &str = "record confirmed Seven Days ban";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SevenDaysBanReceipt {
    pub canonical_target: String,
    pub unban_date: String,
    pub reason: String,
}

/// Persist only native entries explicitly confirmed by this run's ban command. Never
/// materialize configuration here: that would overwrite the game's new roster.
pub async fn record_seven_days_bans_from_native(
    paths: &StoragePaths,
    instance_id: &str,
    expected_run_id: i64,
    expected_root_target: &str,
    receipts: &[SevenDaysBanReceipt],
) -> Result<InstanceDetails, StorageError> {
    validate_receipts(expected_root_target, receipts)?;
    let lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let transaction_lock = lock.clone();
    let paths = paths.clone();
    let instance_id = instance_id.to_string();
    let receipts = receipts.to_vec();
    lock.complete_mutation(OPERATION, async move {
        record_locked(
            &paths,
            &instance_id,
            expected_run_id,
            &receipts,
            &transaction_lock,
        )
        .await
    })
    .await
}

async fn record_locked(
    paths: &StoragePaths,
    instance_id: &str,
    expected_run_id: i64,
    receipts: &[SevenDaysBanReceipt],
    lock: &InstanceSettingsLock,
) -> Result<InstanceDetails, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let record = fetch_instance_record(&pool, instance_id).await?;
        if record.summary.module_id != MODULE_ID {
            return Err(invalid(
                "Native ban recording is only available for Seven Days to Die.",
            ));
        }
        let active_run = load_active_instance_run(&pool, instance_id).await?;
        ensure_run(active_run.as_ref().map(|run| run.run_id), expected_run_id)?;
        let ports = load_instance_ports(&pool, instance_id).await?;
        let worker_paths = paths.clone();
        let worker_receipts = receipts.to_vec();
        let prepared = lock
            .spawn_blocking(move || {
                prepare(&worker_paths, record, ports, active_run, &worker_receipts)
            })
            .await
            .map_err(join_error)??;
        // The process can stop while its native file is being read. A stale
        // command must never be recorded as a result from a replacement run.
        let current_run = load_active_instance_run(&pool, instance_id).await?;
        ensure_run(current_run.as_ref().map(|run| run.run_id), expected_run_id)?;
        lock.spawn_blocking(move || prepared.commit())
            .await
            .map_err(join_error)?
    }
    .await;
    pool.close().await;
    result
}

fn validate_receipts(
    expected_root_target: &str,
    receipts: &[SevenDaysBanReceipt],
) -> Result<(), StorageError> {
    xml::account_identity(expected_root_target)?;
    if !(1..=2).contains(&receipts.len()) {
        return Err(invalid(
            "A ban must confirm its target and at most one family-sharing owner.",
        ));
    }
    let primary = &receipts[0];
    if primary.canonical_target != expected_root_target {
        return Err(invalid(
            "The primary ban receipt does not match the selected account.",
        ));
    }
    for receipt in receipts {
        xml::account_identity(&receipt.canonical_target)?;
        if receipt.reason != "LanGame"
            || receipt.unban_date.len() != 19
            || !crate::settings_value_formats::is_date_or_local_datetime(&receipt.unban_date)
            || receipt.unban_date != primary.unban_date
            || receipt.reason != primary.reason
        {
            return Err(invalid(
                "The native ban receipt must contain this command's exact local expiry and reason.",
            ));
        }
    }
    if let Some(owner) = receipts.get(1)
        && (!owner.canonical_target.starts_with("Steam_")
            || owner
                .canonical_target
                .eq_ignore_ascii_case(expected_root_target))
    {
        return Err(invalid(
            "The additional receipt must identify a distinct Steam family-sharing owner.",
        ));
    }
    Ok(())
}

fn ensure_run(actual: Option<i64>, expected: i64) -> Result<(), StorageError> {
    if actual != Some(expected) {
        return Err(invalid(
            "The instance run changed before its ban could be recorded.",
        ));
    }
    Ok(())
}

struct PreparedBan {
    config_path: PathBuf,
    native_path: PathBuf,
    original_config: Vec<u8>,
    original_native: Vec<u8>,
    replacement: Vec<u8>,
    details: InstanceDetails,
}

impl PreparedBan {
    fn commit(self) -> Result<InstanceDetails, StorageError> {
        let _guards = guard_directories(
            self.config_path
                .parent()
                .ok_or_else(|| invalid("Configuration has no parent directory."))?,
        )?;
        let _native_directories = guard_directories(
            self.native_path
                .parent()
                .ok_or_else(|| invalid("The native roster has no parent directory."))?,
        )?;
        // On Windows, retain a read-only sharing handle through the config CAS,
        // so the game cannot change or replace the confirmed native snapshot.
        let _native_guard = native_read_guard(&self.native_path)?;
        if read_bytes_with_limit(&self.native_path, MAX_DOCUMENT_BYTES)? != self.original_native {
            return Err(invalid(
                "The native roster changed while confirming the ban; no stored settings were changed.",
            ));
        }
        if self.replacement.len() > MAX_DOCUMENT_BYTES {
            return Err(invalid(
                "The updated instance configuration exceeds the read budget.",
            ));
        }
        let changed = compare_and_swap_file_atomically(
            &self.config_path,
            &self.original_config,
            &self.replacement,
        )
        .map_err(|source| StorageError::WriteConfig {
            path: self.config_path.clone(),
            source,
        })?;
        if !changed {
            return Err(invalid(
                "Instance settings changed while confirming the ban; no stored settings were changed.",
            ));
        }
        Ok(self.details)
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
            path: path.to_path_buf(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| StorageError::ReadConfig {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_DOCUMENT_BYTES as u64 {
        return Err(invalid(
            "The native roster must remain an ordinary bounded file.",
        ));
    }
    Ok(file)
}

fn prepare(
    paths: &StoragePaths,
    record: StoredInstanceRecord,
    ports: Vec<app_core::PortBinding>,
    active_run: Option<app_core::ActiveInstanceRun>,
    receipts: &[SevenDaysBanReceipt],
) -> Result<PreparedBan, StorageError> {
    let config_path = record.config_dir.join("instance.json");
    let original_config = read_bytes_with_limit(&config_path, MAX_DOCUMENT_BYTES)?;
    let mut document: Value = serde_json::from_slice(
        original_config
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(&original_config),
    )
    .map_err(|source| StorageError::InvalidConfigJson {
        path: config_path.clone(),
        source,
    })?;
    let settings = document
        .get_mut("settings")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("Instance configuration must contain a settings object."))?;
    let descriptor = load_module_descriptor(paths, MODULE_ID)?
        .ok_or_else(|| invalid("The Seven Days module is unavailable."))?;
    let install_root = effective_instance_install_root(&record)?;
    let saves_path = if module_declares_saves_path(Some(&descriptor)) {
        plan_instance_saves_dir(
            Some(&descriptor),
            &InstanceSavePathContext {
                install_root: &install_root,
                instance_root: record.config_dir.parent().unwrap_or(&record.config_dir),
                config_dir: &record.config_dir,
                instance_id: &record.summary.id,
                instance_name: &record.summary.name,
                module_id: MODULE_ID,
                settings: Some(settings),
            },
        )?
    } else {
        record.saves_dir.clone()
    };
    let native_path = saves_path.join("serveradmin.xml");
    let original_native = read_bytes_with_limit(&native_path, MAX_DOCUMENT_BYTES)?;
    let native_text = std::str::from_utf8(&original_native)
        .map_err(|_| invalid("The native serveradmin.xml must be UTF-8 text."))?;
    let schema: Value = serde_json::from_str(
        descriptor
            .schema_json
            .as_deref()
            .ok_or_else(|| invalid("The Seven Days roster schema is unavailable."))?,
    )?;
    for receipt in receipts {
        let confirmed = xml::read_confirmed_ban(native_text, &receipt.canonical_target)?;
        if confirmed.entry.get("unbandate").and_then(Value::as_str)
            != Some(receipt.unban_date.as_str())
            || confirmed.entry.get("reason").and_then(Value::as_str)
                != Some(receipt.reason.as_str())
        {
            return Err(invalid(
                "The native ban does not match this command's expiry and reason; stored settings were not changed.",
            ));
        }
        merge_confirmed(settings, &schema, confirmed, &receipt.canonical_target)?;
    }
    let settings_json = serde_json::to_string_pretty(settings)?;
    let replacement = serde_json::to_vec_pretty(&document)?;
    Ok(PreparedBan {
        config_path: config_path.clone(),
        native_path,
        original_config,
        original_native,
        replacement,
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

fn merge_confirmed(
    settings: &mut Map<String, Value>,
    schema: &Value,
    confirmed: xml::ConfirmedBan,
    target: &str,
) -> Result<(), StorageError> {
    let mut entry_settings = Map::from_iter([(FIELD.to_string(), json!([confirmed.entry]))]);
    normalize_module_player_access_settings_strict(MODULE_ID, &mut entry_settings)?;
    let field_schema = schema
        .get("properties")
        .and_then(|properties| properties.get(FIELD))
        .ok_or_else(|| invalid("The blacklist schema is unavailable."))?;
    let scoped_schema = json!({"type":"object", "properties":{"blacklist_entries":field_schema}});
    if let Some(diagnostic) = collect_settings_schema_diagnostics(
        &scoped_schema,
        &entry_settings,
        SettingsValidationPhase::Complete,
    )
    .into_iter()
    .next()
    {
        return Err(invalid(&format!(
            "Native ban {}: {}",
            diagnostic.field, diagnostic.message
        )));
    }
    let entry = entry_settings[FIELD][0].clone();
    let entries = settings
        .entry(FIELD.to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| invalid("The stored blacklist must be an array."))?;
    let mut matches = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| xml::matches_account(entry, target))
        .map(|(index, _)| index);
    let existing = matches.next();
    if matches.next().is_some() {
        return Err(invalid(
            "Duplicate stored entries for the banned account are ambiguous.",
        ));
    }
    if let Some(index) = existing {
        let mut updated = entries[index]
            .as_object()
            .cloned()
            .ok_or_else(|| invalid("The stored ban must be an object."))?;
        updated.extend(
            entry
                .as_object()
                .ok_or_else(|| invalid("The confirmed ban must be an object."))?
                .clone(),
        );
        entries[index] = Value::Object(updated);
    } else {
        entries.push(entry);
    }
    // Native AddBan removes this account's administrator entry. Only mirror that
    // deletion after a complete users section proves it is absent.
    if !confirmed.target_is_admin
        && let Some(admins) = settings.get_mut("admin_users")
    {
        let admins = admins
            .as_array_mut()
            .ok_or_else(|| invalid("Stored administrators must be an array."))?;
        admins.retain(|entry| !xml::matches_account(entry, target));
    }
    Ok(())
}

fn invalid(message: &str) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: MODULE_ID.to_string(),
        field: FIELD.to_string(),
        message: message.to_string(),
    }
}

fn join_error(error: tokio::task::JoinError) -> StorageError {
    StorageError::BlockingTaskFailed {
        operation: OPERATION,
        message: error.to_string(),
    }
}

#[cfg(test)]
#[path = "seven_days_ban_tests.rs"]
mod tests;
