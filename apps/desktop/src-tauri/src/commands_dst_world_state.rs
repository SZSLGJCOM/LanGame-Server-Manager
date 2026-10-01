use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use super::commands_dst_import_validation::{
    dontstarve_cluster_root_from_config_file_path, is_link_or_reparse,
};
use super::dst_save_validation::{DstShardSaveInspection, inspect_dst_shard_save};
use crate::state::{DesktopState, spawn_blocking_storage_context_task};
use app_core::dst_shards::{DST_SHARDS, dst_shards};

const MAX_SCAN_ENTRIES: usize = 8192;
const SHARD_LOG_CATEGORIES: &[&str] = &["server_log", "server_chat_log"];
const SHARD_CONFIGURATION_FILES: &[&str] = &[
    "server.ini",
    "worldgenoverride.lua",
    "leveldataoverride.lua",
    "modoverrides.lua",
];
const CLUSTER_CONFIGURATION_FILES: &[&str] = &[
    "cluster.ini",
    "cluster_token.txt",
    "adminlist.txt",
    "blocklist.txt",
    "whitelist.txt",
];

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DstWorldState {
    New,
    Existing,
    Unrecognized,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DstShardWorldState {
    pub shard: String,
    pub state: DstWorldState,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DstWorldStartPreview {
    pub instance_id: String,
    pub settings_json: String,
    pub shards: Vec<DstShardWorldState>,
}

pub(super) fn validate_dst_world_start_confirmation(
    expected: &DstWorldStartPreview,
    instance: &app_core::InstanceDetails,
) -> Result<(), String> {
    let current = build_dst_world_start_preview(instance)?;
    if expected != &current {
        return Err(serde_json::json!({
            "code": "dst_world_start_changed",
            "message": "The saved settings or world data changed during startup preparation. Try starting the server again."
        }).to_string());
    }
    Ok(())
}

fn build_dst_world_start_preview(
    instance: &app_core::InstanceDetails,
) -> Result<DstWorldStartPreview, String> {
    if instance.summary.module_id != "dontstarve" {
        return Err(format!(
            "instance `{}` is not a Don't Starve Together server",
            instance.summary.id
        ));
    }
    let cluster_root = dontstarve_cluster_root_from_config_file_path(&instance.config_file_path);
    let settings: serde_json::Value =
        serde_json::from_str(&instance.settings_json).map_err(|error| error.to_string())?;
    Ok(DstWorldStartPreview {
        instance_id: instance.summary.id.clone(),
        settings_json: instance.settings_json.clone(),
        shards: inspect_dst_world_shards(&cluster_root, &settings)?,
    })
}

pub(super) async fn acquire_dst_world_preview_permit()
-> Result<tokio::sync::OwnedSemaphorePermit, String> {
    static PREVIEW_WORKERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    PREVIEW_WORKERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| String::from("DST preview workers unavailable"))
}

#[tauri::command]
pub async fn preview_dontstarve_world_start(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<DstWorldStartPreview, String> {
    let operation = state.begin_storage_context_operation("DST world start preview")?;
    let settings = state
        .app_state
        .read()
        .map_err(|_| "desktop state lock poisoned")?
        .settings
        .clone();
    let permit = acquire_dst_world_preview_permit().await?;
    let instance_lock = state.acquire_instance_mutation(&instance_id).await;
    let runtime = tokio::runtime::Handle::current();
    spawn_blocking_storage_context_task(&operation, move || {
        // Keep both leases alive if the caller is cancelled while its worker runs.
        let (_permit, _instance_lock) = (permit, instance_lock);
        let paths = app_storage::StoragePaths::default().with_app_settings(&settings);
        let details = runtime
            .block_on(app_storage::read_instance_details(&paths, &instance_id))
            .map_err(|error| error.to_string())?;
        build_dst_world_start_preview(&details)
    })
    .await
    .map_err(|error| format!("DST world preview task failed: {error}"))?
}

fn inspect_dst_world_shards(
    cluster_root: &Path,
    settings: &serde_json::Value,
) -> Result<Vec<DstShardWorldState>, String> {
    let enabled_shards = dst_shards(settings)?;
    let mut budget = MAX_SCAN_ENTRIES;
    let root_metadata = inspect_path(cluster_root)?;
    let cluster_entries = match root_metadata {
        None => Some(Vec::new()),
        Some(metadata) if plain_directory(&metadata) => read_entries(cluster_root, &mut budget)?,
        Some(_) => None,
    };
    let mut cluster_unknown = cluster_entries.is_none();
    if let Some(entries) = &cluster_entries {
        for entry in entries {
            let name = file_name(entry);
            if DST_SHARDS
                .iter()
                .any(|shard| shard.directory.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            let metadata = inspect_path(entry)?;
            if !CLUSTER_CONFIGURATION_FILES.contains(&name.as_str())
                && metadata.as_ref().is_some_and(plain_directory)
            {
                let display_name = entry
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.clone());
                return Err(format!(
                    "Unsupported DST shard directory {display_name}; supported shards are Master, Caves, Islands and Volcano."
                ));
            }
            if !CLUSTER_CONFIGURATION_FILES.contains(&name.as_str())
                || !metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.is_file() && !is_link_or_reparse(metadata))
            {
                cluster_unknown = true;
            }
        }
    }
    DST_SHARDS
        .iter()
        .map(|spec| {
            let shard = spec.directory;
            let enabled = enabled_shards.contains(&spec);
            let shard_path = cluster_entries
                .as_ref()
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| file_name(entry).eq_ignore_ascii_case(shard))
                })
                .cloned()
                .unwrap_or_else(|| cluster_root.join(shard));
            let mut state = if cluster_entries.is_none() {
                DstWorldState::Unrecognized
            } else {
                inspect_shard(&shard_path, &mut budget)?
            };
            if cluster_unknown && state == DstWorldState::New {
                state = DstWorldState::Unrecognized;
            }
            Ok(DstShardWorldState {
                shard: shard.to_owned(),
                state,
                enabled,
            })
        })
        .collect()
}

fn inspect_shard(path: &Path, budget: &mut usize) -> Result<DstWorldState, String> {
    let Some(metadata) = inspect_path(path)? else {
        return Ok(DstWorldState::New);
    };
    if !plain_directory(&metadata) {
        return Ok(DstWorldState::Unrecognized);
    }
    let Some(entries) = read_entries(path, budget)? else {
        return Ok(DstWorldState::Unrecognized);
    };
    let mut has_data = false;
    for entry in entries {
        let name = file_name(&entry);
        let metadata = inspect_path(&entry)?;
        if (SHARD_CONFIGURATION_FILES.contains(&name.as_str())
            || matches!(name.as_str(), "server_log.txt" | "server_chat_log.txt"))
            && metadata
                .as_ref()
                .is_some_and(|metadata| metadata.is_file() && !is_link_or_reparse(metadata))
        {
            continue;
        }
        if name == "backup"
            && metadata.as_ref().is_some_and(plain_directory)
            && contains_only_native_log_backups(&entry, budget)?
        {
            continue;
        }
        has_data = true;
        if name == "save"
            && metadata.as_ref().is_some_and(plain_directory)
            && has_native_snapshot(&entry, budget)?
        {
            return Ok(DstWorldState::Existing);
        }
    }
    Ok(if has_data {
        DstWorldState::Unrecognized
    } else {
        DstWorldState::New
    })
}

fn contains_only_native_log_backups(path: &Path, budget: &mut usize) -> Result<bool, String> {
    // Klei rotates each log into backup/<category>/<category>_<timestamp>.txt.
    // Validate every entry; a directory called "backup" may contain retained data.
    let Some(categories) = read_entries(path, budget)? else {
        return Ok(false);
    };
    for category_path in categories {
        let category = file_name(&category_path);
        if !SHARD_LOG_CATEGORIES.contains(&category.as_str())
            || !inspect_path(&category_path)?
                .as_ref()
                .is_some_and(plain_directory)
        {
            return Ok(false);
        }
        let Some(logs) = read_entries(&category_path, budget)? else {
            return Ok(false);
        };
        for log in logs {
            if !is_native_rotated_log_name(&file_name(&log), &category)
                || !inspect_path(&log)?
                    .as_ref()
                    .is_some_and(|metadata| metadata.is_file() && !is_link_or_reparse(metadata))
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn is_native_rotated_log_name(name: &str, category: &str) -> bool {
    let Some(timestamp) = name
        .strip_prefix(category)
        .and_then(|suffix| suffix.strip_prefix('_'))
        .and_then(|suffix| suffix.strip_suffix(".txt"))
    else {
        return false;
    };
    timestamp.len() == 19
        && timestamp.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7 | 10 | 13 | 16) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

fn has_native_snapshot(save: &Path, budget: &mut usize) -> Result<bool, String> {
    Ok(matches!(
        inspect_dst_shard_save(save, budget)?,
        DstShardSaveInspection::Valid
    ))
}

fn inspect_path(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            verify_missing_path_ancestor(path)?;
            Ok(None)
        }
        Err(error) => Err(format!(
            "Failed to inspect DST world path {}: {error}",
            path.display()
        )),
    }
}

fn verify_missing_path_ancestor(path: &Path) -> Result<(), String> {
    // Windows reports a non-directory ancestor as NotFound. Confirm the nearest
    // existing ancestor before using absence as evidence for a new world.
    for ancestor in path.ancestors().skip(1).take(64) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if plain_directory(&metadata) => return Ok(()),
            Ok(_) => {
                return Err(format!(
                    "Failed to inspect DST world path {}: ancestor {} is not a plain directory",
                    path.display(),
                    ancestor.display()
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "Failed to inspect DST world ancestor {}: {error}",
                    ancestor.display()
                ));
            }
        }
    }
    Err(format!(
        "Failed to inspect DST world path {}: no accessible parent directory",
        path.display()
    ))
}

fn plain_directory(metadata: &fs::Metadata) -> bool {
    metadata.is_dir() && !is_link_or_reparse(metadata)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

// Fixed nesting depth and one shared entry budget bound work even on malformed saves.
fn read_entries(path: &Path, budget: &mut usize) -> Result<Option<Vec<PathBuf>>, String> {
    let entries = fs::read_dir(path).map_err(|error| {
        format!(
            "Failed to read DST world directory {}: {error}",
            path.display()
        )
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to inspect DST world directory {}: {error}",
                path.display()
            )
        })?;
        if *budget == 0 {
            return Ok(None);
        }
        *budget -= 1;
        paths.push(entry.path());
    }
    Ok(Some(paths))
}

#[cfg(test)]
#[path = "commands_dst_world_state_tests.rs"]
mod tests;
