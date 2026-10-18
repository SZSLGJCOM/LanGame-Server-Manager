//! Startup refresh follows this instance's native load/download intent, not cache ownership.
use std::collections::BTreeSet;

use serde_json::Value;

use super::{ManualModInventoryResult, extract_steam_workshop_item_id};

const MAX_ITEMS: usize = 8192;

pub(in crate::commands) fn collect_start_workshop_item_ids(
    module_id: &str,
    settings: &Value,
    inventory: Option<&ManualModInventoryResult>,
) -> Result<Vec<String>, String> {
    if !settings.is_object() {
        return Err("Workshop startup settings must be an object.".into());
    }
    let mut ids = BTreeSet::new();
    match module_id {
        "dontstarve" => collect_dst_ids(settings, &mut ids)?,
        "projectzomboid" => collect_setting_ids(settings, "workshop_items", &mut ids)?,
        "unturned" => collect_setting_ids(settings, "workshop_file_ids", &mut ids)?,
        "arksurvivalevolved" => collect_ark_ids(settings, &mut ids)?,
        "barotrauma" | "conanexiles" | "soulmask" => {
            collect_setting_ids(settings, "mod_workshop_ids", &mut ids)?;
        }
        "terraria" if settings["server_runtime"].as_str() == Some("tmodloader") => {
            collect_setting_ids(settings, "tmodloader_workshop_item_ids", &mut ids)?;
        }
        "palworld" | "squad" => {
            collect_inventory_ids(module_id, settings, inventory, &mut ids)?;
        }
        // Client-only modules and other providers have no server Workshop load intent.
        _ => {}
    }
    if ids.len() > MAX_ITEMS {
        return Err(format!(
            "Workshop startup refresh exceeds its {MAX_ITEMS}-item limit."
        ));
    }
    Ok(ids.into_iter().collect())
}

fn collect_ark_ids(settings: &Value, ids: &mut BTreeSet<String>) -> Result<(), String> {
    app_core::ark_mod_scopes::validate_overrides("arksurvivalevolved", settings)?;
    let keys = std::iter::once(String::from("main")).chain(
        app_core::ark_maps::parse_additional_maps(settings)?
            .into_iter()
            .filter(|map| map.enabled)
            .map(|map| app_core::ark_maps::process_key(&map.id)),
    );
    for key in keys {
        let effective =
            app_core::ark_mod_scopes::effective_settings("arksurvivalevolved", settings, &key)?;
        let raw = effective.get("active_mod_ids").map_or(Ok(""), |value| {
            value
                .as_str()
                .ok_or("Workshop startup setting active_mod_ids must be a string.")
        })?;
        ids.extend(
            app_core::ark_mods::parse_evolved_mod_ids(raw)
                .into_iter()
                .filter(|id| canonical_id(id)),
        );
    }
    Ok(())
}

fn collect_dst_ids(settings: &Value, ids: &mut BTreeSet<String>) -> Result<(), String> {
    let removed = read_markers(settings, "dst_removed_workshop_mod_ids")?;
    for shard in app_core::dst_shards::dst_shards(settings)? {
        let raw_key = format!("{}_modoverrides_lua", shard.process_key);
        let raw = settings[&raw_key].as_str().unwrap_or_default();
        let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
        // The generated empty raw sentinel lets the materializer use managed IDs.
        // Match render_dst_modoverrides' precedence before applying Lua budgets.
        if normalized.trim().is_empty() || normalized.trim() == "return {\n}" {
            collect_setting_ids(
                settings,
                &format!("{}_enabled_workshop_mod_ids", shard.process_key),
                ids,
            )?;
            continue;
        }
        // Static Lua budgets apply only to the overriding script. Unrelated world
        // settings and managed lists must not consume this analysis budget.
        let mut compact = serde_json::Map::new();
        for key in ["shard_layout", "enable_caves"] {
            if let Some(value) = settings.get(key) {
                compact.insert(key.into(), value.clone());
            }
        }
        compact.insert(raw_key, Value::String(raw.into()));
        let evidence = app_storage::inspect_dst_mod_enablement(&Value::Object(compact))?;
        let declaration = evidence["shards"]
            .as_array()
            .and_then(|shards| {
                shards
                    .iter()
                    .find(|declaration| declaration["shard"].as_str() == Some(shard.process_key))
            })
            .ok_or("DST Workshop startup evidence contains no matching shard.")?;
        let names = declaration["declaredEnabledModNames"]
            .as_array()
            .ok_or_else(|| {
                format!(
                    "Cannot determine enabled Workshop Mods for DST {}: {}",
                    shard.process_key,
                    declaration["reason"]
                        .as_str()
                        .unwrap_or("Use a supported static modoverrides.lua return table.")
                )
            })?;
        for name in names.iter().filter_map(Value::as_str) {
            if !name.starts_with("workshop-") && !canonical_id(name) {
                continue;
            }
            if let Some(id) = extract_steam_workshop_item_id(name)
                && canonical_id(&id)
            {
                ids.insert(id);
            }
        }
    }
    ids.retain(|id| !removed.contains(id));
    Ok(())
}

fn collect_setting_ids(
    settings: &Value,
    key: &str,
    ids: &mut BTreeSet<String>,
) -> Result<(), String> {
    let Some(value) = settings.get(key) else {
        return Ok(());
    };
    let raw = value
        .as_str()
        .ok_or_else(|| format!("Workshop startup setting {key} must be a string."))?;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with("--") {
            continue;
        }
        for entry in line.split([',', ';']).map(str::trim) {
            if entry.is_empty() || entry.starts_with('#') || entry.starts_with("--") {
                continue;
            }
            // Use the same URL / workshop-name / numeric reference parser as manual installs.
            // Native list materialization ignores entries without a usable ID.
            // Preserve that behavior when refreshing the already accepted list.
            let Some(id) = extract_steam_workshop_item_id(entry).filter(|id| canonical_id(id))
            else {
                continue;
            };
            ids.insert(id);
            if ids.len() > MAX_ITEMS {
                return Err(format!(
                    "Workshop startup setting {key} exceeds its {MAX_ITEMS}-item limit."
                ));
            }
        }
    }
    Ok(())
}

fn collect_inventory_ids(
    module_id: &str,
    settings: &Value,
    inventory: Option<&ManualModInventoryResult>,
    ids: &mut BTreeSet<String>,
) -> Result<(), String> {
    let Some(inventory) = inventory else {
        return Err(format!(
            "Workshop startup refresh requires the {module_id} instance Mod inventory."
        ));
    };
    if inventory.module_id != module_id {
        return Err("Workshop startup Mod inventory belongs to a different module.".into());
    }
    let removed = if module_id == "palworld" {
        read_markers(settings, "steam_workshop_removed_mod_ids")?
    } else {
        BTreeSet::new()
    };
    let enabled_names = if module_id == "palworld" {
        enabled_palworld_package_names(settings)?
    } else {
        BTreeSet::new()
    };
    if !inventory.target_exists {
        return Ok(());
    }
    let root = inventory.target_path.replace('\\', "/");
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let lower_prefix = prefix.to_lowercase();
    for item in &inventory.items {
        if item.file_count == 0 || item.management.as_ref().is_some_and(|state| !state.enabled) {
            continue;
        }
        if module_id == "palworld"
            && !item
                .inferred_id
                .as_deref()
                .is_some_and(|name| enabled_names.contains(&name.trim().to_lowercase()))
        {
            continue;
        }
        let path = item.path.replace('\\', "/");
        if !path.to_lowercase().starts_with(&lower_prefix) {
            continue;
        }
        let relative = path
            .get(prefix.len()..)
            .ok_or("Workshop inventory path is not a valid relative path.")?;
        if relative
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
        {
            return Err("Workshop inventory path contains invalid relative components.".into());
        }
        // Workshop staging preserves the numeric source root. Nested names are
        // package metadata and cannot authorize refreshing another Workshop item.
        let root_name = relative.split('/').next().unwrap_or_default();
        if canonical_id(root_name) && !removed.contains(root_name) {
            ids.insert(root_name.into());
        }
    }
    Ok(())
}

pub(in crate::commands) fn enabled_palworld_package_names(
    settings: &Value,
) -> Result<BTreeSet<String>, String> {
    let raw = settings.get("mod_package_names").map_or(Ok(""), |value| {
        value
            .as_str()
            .ok_or("Palworld enabled Mod package names must be a string.")
    })?;
    let names = raw
        .split(['\n', '\r', ',', ';'])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_lowercase)
        .collect::<BTreeSet<_>>();
    if names.len() > MAX_ITEMS {
        return Err("Palworld enabled Mod package names exceed their inspection limit.".into());
    }
    Ok(names)
}

pub(in crate::commands) fn read_markers(
    settings: &Value,
    key: &str,
) -> Result<BTreeSet<String>, String> {
    let Some(value) = settings.get(key) else {
        return Ok(BTreeSet::new());
    };
    let markers = value
        .as_array()
        .filter(|markers| markers.len() <= MAX_ITEMS)
        .ok_or_else(|| format!("Workshop startup setting {key} has invalid membership markers."))?;
    markers
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|id| canonical_id(id))
                .map(String::from)
                .ok_or_else(|| format!("Workshop startup setting {key} has an invalid item ID."))
        })
        .collect()
}

fn canonical_id(id: &str) -> bool {
    (6..=20).contains(&id.len())
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && !id.starts_with('0')
        && id.parse::<u64>().is_ok()
}

#[cfg(test)]
#[path = "commands_workshop_start_plan_tests.rs"]
mod tests;
