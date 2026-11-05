use std::path::Path;

use app_core::{
    InstanceDetails, UpdateInstanceInput,
    terraria::{TML_MAX_WORLD_VERSION, TmodLoaderInstallation, installation, uses_tmodloader},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::commands_managed_save_control::io as native_file;
use super::commands_program_storage::instance_root;
use crate::state::{DesktopState, spawn_blocking_storage_context_task};

#[path = "terraria_world_files.rs"]
mod files;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TerrariaWorldConflict {
    pub instance_id: String,
    pub world_file: String,
    pub world_version: i32,
    pub max_world_version: i32,
    pub suggested_world_file: String,
    pub settings_sha256: String,
}

fn custom_world_limit(runtime: &Path) -> Result<Option<i32>, String> {
    // The runtime's own dependency manifest identifies its Terraria base
    // ("libraries": {"tModLoader/1.4.4.9": ...}). Do not apply the managed
    // release's limit to a classic or unrecognized operator-supplied build.
    let path = runtime.join("tModLoader.deps.json");
    let Some(bytes) = native_file::read_optional(&path, 4 * 1024 * 1024)
        .map_err(|err| format!("无法读取自选 tModLoader 的版本信息：{err}"))?
    else {
        return Ok(None);
    };
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|err| format!("自选 tModLoader 的版本清单无效：{err}"))?;
    let libraries = manifest
        .get("libraries")
        .and_then(Value::as_object)
        .ok_or("自选 tModLoader 的版本清单缺少 libraries。")?;
    let mut versions = libraries
        .keys()
        .filter_map(|key| key.strip_prefix("tModLoader/"));
    let base = versions.next();
    if versions.next().is_some() {
        return Err("自选 tModLoader 的版本清单包含冲突的 Terraria 基版。".into());
    }
    // Desktop version ↔ file format, for the known stable, LTS and 1.4.5 lines.
    // https://github.com/TEdit/Terraria-Map-Editor/blob/main/src/TEdit.Terraria/Data/versions.json
    Ok(match base {
        Some("1.4.4.9") => Some(279),
        Some("1.4.3.6") => Some(248),
        Some("1.3.5.3") => Some(194),
        Some("1.4.5.0" | "1.4.5.1" | "1.4.5.2") => Some(315),
        Some("1.4.5.3") => Some(316),
        Some("1.4.5.4") => Some(317),
        Some("1.4.5.5") => Some(318),
        Some("1.4.5.6") => Some(319),
        Some("1.4.5.7") => Some(325),
        Some("1.4.5.8") => Some(326),
        _ => None,
    })
}

fn selected_world(settings: &Value) -> Result<(&str, &str), String> {
    // Pre-start template materialization fills missing legacy fields from the
    // Terraria module schema. Invalid explicit values must not use its default.
    let name = match settings.get("world_file") {
        None => "world.wld",
        Some(value) => value.as_str().ok_or("实例的世界存档文件名无效。")?,
    };
    Ok((name, files::world_stem(name)?))
}

fn conflict(instance: &InstanceDetails) -> Result<Option<TerrariaWorldConflict>, String> {
    if instance.summary.module_id != "terraria" {
        return Ok(None);
    }
    let settings: Value =
        serde_json::from_str(&instance.settings_json).map_err(|err| err.to_string())?;
    let (name, stem) = selected_world(&settings)?;
    let root = instance_root(instance)?;
    let saves = Path::new(&instance.saves_path);
    files::check_saves_directory(root, saves)?;
    let world = saves.join(name);
    if !uses_tmodloader(&settings) {
        if files::metadata(&world.with_extension("twld"))?.is_some()
            || files::metadata(&world.with_extension("twld.bak"))?.is_some()
        {
            return Err(format!(
                "世界「{name}」包含 tModLoader 模组数据。请重新启用 Mod，或在「世界存档文件」中选回原版世界，再启动原版服务器。"
            ));
        }
        if files::metadata(&world)?.is_none() {
            files::ensure_slot_empty(saves, stem, None)?;
        }
        return Ok(None);
    }
    let Some(world_version) = files::read_world_version(&world)? else {
        // A missing primary can be a damaged save with surviving .twld/.bak.
        // Never let the game's autocreate reuse that slot as an empty world.
        files::ensure_slot_empty(saves, stem, Some(root))?;
        return Ok(None);
    };
    let max_world_version = match installation(root, &settings) {
        TmodLoaderInstallation::Managed(_) => Some(TML_MAX_WORLD_VERSION),
        TmodLoaderInstallation::Custom(runtime) => custom_world_limit(&runtime)?,
    };
    let Some(limit) = max_world_version.filter(|limit| world_version > *limit) else {
        return Ok(None);
    };
    Ok(Some(TerrariaWorldConflict {
        instance_id: instance.summary.id.clone(),
        world_file: name.to_owned(),
        world_version,
        max_world_version: limit,
        suggested_world_file: files::suggest_world(saves, stem, root)?,
        settings_sha256: native_file::digest(instance.settings_json.as_bytes()),
    }))
}

pub(super) fn ensure_compatible_for_start(instance: &InstanceDetails) -> Result<(), String> {
    if let Some(issue) = conflict(instance)? {
        return Err(format!(
            "世界「{}」的存档格式为 {}，当前 tModLoader 最高支持 {}。请在管理器中启动并确认创建新世界；原存档将保留。",
            issue.world_file, issue.world_version, issue.max_world_version,
        ));
    }
    Ok(())
}

pub(super) fn ensure_fresh_for_start(
    instance: &InstanceDetails,
    expected_world_file: &str,
) -> Result<(), String> {
    let settings: Value =
        serde_json::from_str(&instance.settings_json).map_err(|err| err.to_string())?;
    let (name, stem) = selected_world(&settings)?;
    if instance.summary.module_id != "terraria"
        || name != expected_world_file
        || !uses_tmodloader(&settings)
    {
        return Err("实例或世界选择已改变，请重新启动并确认。".into());
    }
    let saves = Path::new(&instance.saves_path);
    let root = instance_root(instance)?;
    files::check_saves_directory(root, saves)?;
    files::ensure_slot_empty(saves, stem, Some(root))
}

#[tauri::command]
pub async fn preview_terraria_mod_world(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<Option<TerrariaWorldConflict>, String> {
    let operation = state.begin_storage_context_operation("Terraria world start preview")?;
    let storage = state.bootstrap_storage()?;
    let guard = state.acquire_instance_mutation(&instance_id).await;
    let instance = app_storage::read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|err| err.to_string())?;
    spawn_blocking_storage_context_task(&operation, move || {
        let _guard = guard;
        conflict(&instance)
    })
    .await
    .map_err(|err| format!("无法检查泰拉瑞亚世界：{err}"))?
}

/// Called by the reserved startup while it owns the instance mutation lock.
/// Only switch configuration. The native server creates the world on first
/// start; the previous .wld, .twld and rollback files are never written here.
pub(super) async fn select_new_for_start(
    paths: &app_storage::StoragePaths,
    details: &InstanceDetails,
    expected: &TerrariaWorldConflict,
) -> Result<InstanceDetails, String> {
    let current = details.clone();
    let fresh = tokio::task::spawn_blocking(move || conflict(&current))
        .await
        .map_err(|err| format!("无法复核泰拉瑞亚世界：{err}"))??;
    if fresh.as_ref() != Some(expected) {
        return Err("实例设置、原世界或新文件名已改变。请重新启动以确认新的世界选择。".into());
    }
    let mut settings: serde_json::Map<String, Value> =
        serde_json::from_str(&details.settings_json).map_err(|err| err.to_string())?;
    settings.insert(
        "world_file".into(),
        Value::String(expected.suggested_world_file.clone()),
    );
    let input = UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: details.summary.bind_ip.clone(),
        ports: details.ports.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string(&settings).map_err(|err| err.to_string())?,
    };
    app_storage::update_instance_if_current(paths, input, &details.settings_json)
        .await
        .map_err(|err| err.to_string())
}

#[cfg(test)]
#[path = "commands_terraria_world_tests.rs"]
mod tests;
