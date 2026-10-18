//! One-time document conversion; native launch code only accepts sparse overrides.
use super::*;
use std::collections::BTreeSet;

fn config<'a>(settings: &'a Value, id: &str) -> Result<Option<&'a Map<String, Value>>, String> {
    let Some(configurations) = settings.get("asa_mod_configurations") else {
        return Ok(None);
    };
    let configurations = configurations
        .as_object()
        .ok_or("ASA Mod configurations must be an object")?;
    let Some(configuration) = configurations.get(id) else {
        return Ok(None);
    };
    let configuration = configuration
        .as_object()
        .ok_or("ASA Mod configuration must be an object")?;
    for (file, value) in configuration {
        if !CONFIG_FILES.contains(&file.as_str()) || !value.is_string() {
            return Err(format!(
                "Mod {id}: invalid stored configuration field `{file}`"
            ));
        }
    }
    Ok(Some(configuration))
}

fn mode_ids(
    module_id: &str,
    settings: &Value,
    mode: &str,
    field: &str,
) -> Result<Vec<String>, String> {
    if mode == "active" || mode == "passive" {
        native_ids(module_id, settings.get(field))
    } else {
        settings.get(field).map_or(Ok(Vec::new()), validate_ids)
    }
}

fn convert_scope(module_id: &str, settings: &Value, snapshot: &Value) -> Result<Value, String> {
    let asa = ascended(module_id)?;
    let snapshot = snapshot
        .as_object()
        .ok_or("Whole-map Mod snapshot must be an object")?;
    let mut target = settings.clone();
    let fields = if asa {
        vec![
            "mod_ids_csv",
            "passive_mod_ids_csv",
            "asa_mod_configurations",
            "curseforge_disabled_mod_ids",
            "curseforge_disabled_passive_mod_ids",
            "curseforge_removed_mod_ids",
        ]
    } else {
        vec!["active_mod_ids"]
    };
    for (field, value) in snapshot {
        if !fields.contains(&field.as_str()) {
            return Err(format!("Unsupported whole-map Mod field `{field}`"));
        }
        let valid = if field == "asa_mod_configurations" {
            value.is_object()
        } else if field.starts_with("curseforge_") {
            value.as_array().is_some_and(|ids| ids.len() <= 8192)
        } else {
            value.as_str().is_some_and(|text| text.len() <= 128 * 1024)
        };
        if !valid {
            return Err(format!("Invalid whole-map Mod field `{field}`"));
        }
    }
    for field in fields {
        target
            .as_object_mut()
            .ok_or("ARK settings must be an object")?
            .remove(field);
        if let Some(value) = snapshot.get(field) {
            target[field] = value.clone();
        }
    }
    let mut ids = BTreeSet::new();
    let mut states = Vec::new();
    let mut converted = Map::new();
    for (mode, asa_field) in MODES {
        if !asa && mode != "active" {
            continue;
        }
        let field = if asa { asa_field } else { "active_mod_ids" };
        let shared = mode_ids(module_id, settings, mode, field)?;
        let actual = mode_ids(module_id, &target, mode, field)?;
        ids.extend(shared.iter().cloned());
        ids.extend(actual.iter().cloned());
        if mode == "active" || mode == "passive" {
            // Keep an existing isolated file boundary even when all values match
            // shared settings; unknown entries inside those files still belong to it.
            converted.insert(
                format!("{mode}_order"),
                Value::Array(actual.iter().cloned().map(Value::String).collect()),
            );
        }
        states.push((
            mode,
            shared.into_iter().collect::<HashSet<_>>(),
            actual.into_iter().collect::<HashSet<_>>(),
        ));
    }
    if asa {
        for source in [settings, &target] {
            if let Some(configurations) = source.get("asa_mod_configurations") {
                let configurations = configurations
                    .as_object()
                    .ok_or("ASA Mod configurations must be an object")?;
                ids.extend(configurations.keys().cloned());
            }
        }
    }
    let mut mods = Map::new();
    for id in ids {
        if canonical_id(&id).as_deref() != Some(id.as_str()) {
            return Err(format!("Invalid stored Mod ID `{id}`"));
        }
        let mut entry = Map::new();
        for (mode, shared, actual) in &states {
            if shared.contains(&id) != actual.contains(&id) {
                entry.insert((*mode).into(), Value::Bool(actual.contains(&id)));
            }
        }
        if asa {
            let shared = config(settings, &id)?;
            let actual = config(&target, &id)?;
            if shared.is_some() && actual.is_none() {
                entry.insert("configuration".into(), Value::Null);
            } else if let Some(actual) = actual {
                let mut files = Map::new();
                for file in CONFIG_FILES {
                    let shared_text = shared
                        .and_then(|configuration| configuration.get(file))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let actual_text = actual.get(file).and_then(Value::as_str).unwrap_or_default();
                    if shared_text != actual_text {
                        files.insert(file.into(), Value::String(actual_text.into()));
                    }
                }
                if !files.is_empty() {
                    entry.insert("configuration".into(), Value::Object(files));
                }
            }
        }
        if !entry.is_empty() {
            mods.insert(id, Value::Object(entry));
        }
    }
    converted.insert("mods".into(), Value::Object(mods));
    Ok(Value::Object(converted))
}

/// Converts a decoded document in memory. The caller's managed transaction owns
/// persistence; failures leave both the source value and original files untouched.
pub fn convert_map_snapshots(module_id: &str, settings: &mut Value) -> Result<bool, String> {
    let Some(snapshots) = settings.get(SNAPSHOTS_FIELD) else {
        return Ok(false);
    };
    ascended(module_id)?;
    let snapshots = snapshots
        .as_object()
        .ok_or("Whole-map Mod snapshots must be an object")?;
    if let Some(overrides) = settings.get(OVERRIDES_FIELD) {
        let overrides = overrides
            .as_object()
            .ok_or("ark_mod_map_overrides must be an object")?;
        if !snapshots.is_empty() && !overrides.is_empty() {
            return Err("Whole-map snapshots and per-Mod map overrides cannot coexist".into());
        }
    }
    let keys = configured_process_keys(settings)?;
    let mut converted = Map::new();
    for (key, snapshot) in snapshots {
        if !keys.contains(key) {
            return Err(format!(
                "Whole-map Mod snapshot targets unknown map `{key}`"
            ));
        }
        converted.insert(
            key.clone(),
            convert_scope(module_id, settings, snapshot)
                .map_err(|error| format!("Map `{key}`: {error}"))?,
        );
    }
    let mut result = settings.clone();
    result
        .as_object_mut()
        .ok_or("ARK settings must be an object")?
        .remove(SNAPSHOTS_FIELD);
    if !converted.is_empty() {
        result[OVERRIDES_FIELD] = Value::Object(converted);
    }
    validate_overrides(module_id, &result)?;
    *settings = result;
    Ok(true)
}
