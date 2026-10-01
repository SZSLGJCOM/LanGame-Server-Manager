use serde_json::{Map, Value};

use crate::StorageError;

pub(super) fn validate_dontstarve_mod_settings(
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    let shards =
        app_core::dst_shards::dst_shards(&Value::Object(settings.clone())).map_err(|message| {
            StorageError::InvalidModuleSetting {
                module_id: String::from("dontstarve"),
                field: String::from("shard_layout"),
                message,
            }
        })?;
    for spec in shards {
        let shard = spec.process_key;
        let raw_field = format!("{shard}_modoverrides_lua");
        let Some(raw) = settings.get(&raw_field).and_then(Value::as_str) else {
            continue;
        };
        let raw = raw.replace("\r\n", "\n").replace('\r', "\n");
        if raw.trim().is_empty() || raw.trim() == "return {\n}" {
            continue;
        }
        let enabled = settings.get(&format!("{shard}_enabled_workshop_mod_ids"));
        let has_enabled = enabled.is_some_and(|value| match value {
            Value::String(text) => text
                .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
                .any(|entry| !entry.is_empty()),
            Value::Array(entries) => !entries.is_empty(),
            _ => false,
        });
        let has_configuration = settings
            .get(&format!("{shard}_mod_configuration_options"))
            .and_then(Value::as_object)
            .is_some_and(|options| !options.is_empty());
        if has_enabled || has_configuration {
            return Err(StorageError::InvalidModuleSetting {
                module_id: String::from("dontstarve"),
                field: raw_field,
                message: String::from(
                    "raw modoverrides.lua cannot be combined with the enabled Mod list or structured Mod options",
                ),
            });
        }
    }
    Ok(())
}

pub(super) fn validate_dontstarve_playstyle(
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    let legacy_preset = match settings.get("game_mode").and_then(Value::as_str) {
        Some("endless") => "ENDLESS",
        Some("wilderness") => "WILDERNESS",
        _ => return Ok(()),
    };
    let raw = settings
        .get("master_worldgenoverride_lua")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let presets = if !raw.trim().is_empty()
        && raw.replace("\r\n", "\n").replace('\r', "\n").trim()
            != crate::templates::DEFAULT_DST_MASTER_WORLDGENOVERRIDE_LUA.trim()
    {
        crate::templates::dst_world_settings::explicit_master_presets(raw).unwrap_or([None, None])
    } else {
        ["master_settings_preset", "master_worldgen_preset"]
            .map(|key| settings.get(key).and_then(Value::as_str).map(str::to_owned))
    };
    for preset in presets.into_iter().flatten() {
        if preset != "SURVIVAL_TOGETHER" && preset != legacy_preset {
            return Err(StorageError::InvalidModuleSetting {
                module_id: String::from("dontstarve"),
                field: String::from("game_mode"),
                message: String::from(
                    "Set the cluster game mode to Survival before choosing a different Master playstyle preset.",
                ),
            });
        }
    }
    Ok(())
}

/// Keep Klei's data-collection opt-out and offline mode as one persisted state.
/// The UI applies the same rule at edit time; this is the storage-boundary guard
/// for API and imported settings.
pub(crate) fn normalize_dontstarve_operational_settings(settings: &mut Map<String, Value>) {
    if settings.get("shard_layout").and_then(Value::as_str) == Some("island_adventures") {
        settings.insert(String::from("enable_caves"), Value::Bool(true));
    }
    if settings
        .get("disable_data_collection")
        .and_then(Value::as_bool)
        == Some(true)
    {
        settings.insert(String::from("offline_cluster"), Value::Bool(true));
    }
}
