//! Per-Mod map overrides inherit every unspecified Mod, mode and INI file.
use serde_json::{Map, Value};
use std::collections::HashSet;

#[path = "ark_mod_scope_conversion.rs"]
mod conversion;
pub use conversion::convert_map_snapshots;

pub const OVERRIDES_FIELD: &str = "ark_mod_map_overrides";
pub const SNAPSHOTS_FIELD: &str = "ark_map_mod_overrides";
const MAX_ITEMS: usize = 65_536;
const MODES: [(&str, &str); 5] = [
    ("active", "mod_ids_csv"),
    ("passive", "passive_mod_ids_csv"),
    ("disabled_active", "curseforge_disabled_mod_ids"),
    ("disabled_passive", "curseforge_disabled_passive_mod_ids"),
    ("removed", "curseforge_removed_mod_ids"),
];
const CONFIG_FILES: [&str; 2] = ["game_ini", "game_user_settings_ini"];

pub fn configured_process_keys(settings: &Value) -> Result<Vec<String>, String> {
    let mut keys = vec![String::from("main")];
    keys.extend(
        crate::ark_maps::parse_additional_maps(settings)?
            .iter()
            .map(|map| crate::ark_maps::process_key(&map.id)),
    );
    Ok(keys)
}

fn ascended(module_id: &str) -> Result<bool, String> {
    match module_id {
        "arksurvivalascended" => Ok(true),
        "arksurvivalevolved" => Ok(false),
        _ => Err("Map Mod overrides require an ARK instance".into()),
    }
}

fn canonical_id(id: &str) -> Option<String> {
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    id.parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
}

fn validate_ids(value: &Value) -> Result<Vec<String>, String> {
    let ids = value
        .as_array()
        .filter(|ids| ids.len() <= MAX_ITEMS)
        .ok_or("Mod order or membership must be a bounded list of project IDs")?;
    let mut seen = HashSet::new();
    ids.iter()
        .map(|value| {
            let id = value
                .as_str()
                .filter(|id| canonical_id(id).as_deref() == Some(*id))
                .ok_or("Mod IDs must be canonical nonzero u64 strings")?;
            if !seen.insert(id) {
                return Err("Mod IDs must be unique".into());
            }
            Ok(id.into())
        })
        .collect()
}

fn native_ids(module_id: &str, value: Option<&Value>) -> Result<Vec<String>, String> {
    let raw = value.map_or(Ok(""), |value| {
        value.as_str().ok_or("Mod loading lists must be strings")
    })?;
    let asa = ascended(module_id)?;
    let parsed = if asa {
        let mut parsed = Vec::new();
        for (index, token) in raw
            .split(|character: char| {
                character.is_whitespace()
                    || matches!(character, ',' | ';' | '\u{3001}' | '\u{ff0c}' | '\u{ff1b}')
            })
            .filter(|token| !token.is_empty())
            .enumerate()
        {
            let candidates = crate::ark_mods::parse_mod_ids(token);
            if candidates.is_empty() {
                return Err(format!(
                    "Unrecognized Mod reference at item {}; correct it before converting map Mod settings",
                    index + 1
                ));
            }
            parsed.extend(candidates);
        }
        parsed
    } else {
        // Preserve the existing native ASE renderer's accepted and ignored tokens.
        crate::ark_mods::parse_evolved_mod_ids(raw)
    };
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for id in parsed {
        if !asa && (id.len() < 6 || canonical_id(&id).as_deref() != Some(id.as_str())) {
            return Err("ASE Mod references require canonical project IDs with at least six digits; correct noncanonical references before applying map overrides".into());
        }
        let id =
            canonical_id(&id).ok_or("Native Mod references require nonzero u64 project IDs")?;
        if seen.insert(id.clone()) {
            ids.push(id);
        }
    }
    Ok(ids)
}

pub fn validate_overrides(module_id: &str, settings: &Value) -> Result<(), String> {
    if !settings.is_object() {
        return Err("ARK settings must be an object".into());
    }
    if settings.get(SNAPSHOTS_FIELD).is_some() {
        return Err("Convert whole-map Mod snapshots before using map Mod overrides".into());
    }
    let Some(value) = settings.get(OVERRIDES_FIELD) else {
        return Ok(());
    };
    let asa = ascended(module_id)?;
    let overrides = value
        .as_object()
        .ok_or("ark_mod_map_overrides must be an object")?;
    let keys = configured_process_keys(settings)?;
    for (key, scope) in overrides {
        if !keys.contains(key) {
            return Err(format!("Mod overrides target unknown ARK map `{key}`"));
        }
        let scope = scope
            .as_object()
            .ok_or_else(|| format!("Mod overrides for `{key}` must be an object"))?;
        for (field, value) in scope {
            if field == "active_order" || (asa && field == "passive_order") {
                let ids = validate_ids(value)
                    .map_err(|error| format!("Map `{key}` / {field}: {error}"))?;
                if !asa && ids.iter().any(|id| id.len() < 6) {
                    return Err(format!(
                        "Map `{key}` / {field}: ASE Mod IDs require at least six digits"
                    ));
                }
            } else if field != "mods" {
                return Err(format!(
                    "Map `{key}` contains unsupported Mod field `{field}`"
                ));
            }
        }
        let mods = scope
            .get("mods")
            .and_then(Value::as_object)
            .filter(|mods| mods.len() <= MAX_ITEMS)
            .ok_or_else(|| format!("Map `{key}` requires a bounded mods object"))?;
        for (id, entry) in mods {
            if canonical_id(id).as_deref() != Some(id.as_str()) || (!asa && id.len() < 6) {
                return Err(format!("Map `{key}` has an invalid Mod ID `{id}`"));
            }
            let entry = entry
                .as_object()
                .ok_or_else(|| format!("Map `{key}` / Mod {id} must be an object"))?;
            for (field, value) in entry {
                if field == "active" || (asa && MODES.iter().any(|(mode, _)| mode == field)) {
                    if !value.is_boolean() {
                        return Err(format!("Map `{key}` / Mod {id} / {field} must be boolean"));
                    }
                } else if asa && field == "configuration" {
                    if value.is_null() {
                        continue;
                    }
                    let configuration = value
                        .as_object()
                        .ok_or("Mod configuration must be an object or null")?;
                    for (file, text) in configuration {
                        if !CONFIG_FILES.contains(&file.as_str())
                            || !text.as_str().is_some_and(|text| text.len() <= 64 * 1024)
                        {
                            return Err(format!(
                                "Map `{key}` / Mod {id}: invalid INI file `{file}`"
                            ));
                        }
                    }
                } else {
                    return Err(format!(
                        "Map `{key}` / Mod {id} contains unsupported field `{field}`"
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn has_overrides(settings: &Value) -> bool {
    settings
        .get(OVERRIDES_FIELD)
        .and_then(Value::as_object)
        .is_some_and(|overrides| !overrides.is_empty())
}

fn apply_mode(
    mut ids: Vec<String>,
    mods: &Map<String, Value>,
    mode: &str,
    order: Option<&Value>,
) -> Result<Vec<String>, String> {
    ids.retain(|id| {
        mods.get(id)
            .and_then(|entry| entry.get(mode))
            .and_then(Value::as_bool)
            != Some(false)
    });
    let mut seen: HashSet<String> = ids.iter().cloned().collect();
    let mut entries = mods.iter().collect::<Vec<_>>();
    entries.sort_unstable_by_key(|(id, _)| *id);
    for (id, entry) in entries {
        if entry.get(mode).and_then(Value::as_bool) == Some(true) && seen.insert(id.clone()) {
            ids.push(id.clone());
        }
    }
    if let Some(order) = order {
        let mut preferred = validate_ids(order)?
            .into_iter()
            .filter(|id| seen.contains(id))
            .collect::<Vec<_>>();
        let specified: HashSet<_> = preferred.iter().cloned().collect();
        preferred.extend(ids.into_iter().filter(|id| !specified.contains(id)));
        ids = preferred;
    }
    Ok(ids)
}

pub fn effective_settings(
    module_id: &str,
    settings: &Value,
    process_key: &str,
) -> Result<Value, String> {
    let asa = ascended(module_id)?;
    validate_overrides(module_id, settings)?;
    if !configured_process_keys(settings)?
        .iter()
        .any(|key| key == process_key)
    {
        return Err(format!("Unknown ARK map `{process_key}`"));
    }
    let mut effective = settings.clone();
    let Some(scope) = settings
        .get(OVERRIDES_FIELD)
        .and_then(|overrides| overrides.get(process_key))
        .and_then(Value::as_object)
    else {
        return Ok(effective);
    };
    let mods = scope
        .get("mods")
        .and_then(Value::as_object)
        .ok_or("Map Mod overrides require mods")?;
    for (mode, asa_field) in MODES {
        if !asa && mode != "active" {
            continue;
        }
        let field = if asa { asa_field } else { "active_mod_ids" };
        let order = scope.get(&format!("{mode}_order"));
        if order.is_none() && !mods.values().any(|entry| entry.get(mode).is_some()) {
            continue;
        }
        let ids = if mode == "active" || mode == "passive" {
            native_ids(module_id, settings.get(field))?
        } else {
            settings.get(field).map_or(Ok(Vec::new()), validate_ids)?
        };
        let ids = apply_mode(ids, mods, mode, order)?;
        effective[field] = if mode == "active" || mode == "passive" {
            Value::String(ids.join(","))
        } else {
            Value::Array(ids.into_iter().map(Value::String).collect())
        };
    }
    if asa
        && mods.values().any(|entry| {
            entry.get("configuration").is_some_and(|configuration| {
                configuration.is_null()
                    || configuration
                        .as_object()
                        .is_some_and(|files| !files.is_empty())
            })
        })
    {
        let mut configurations =
            settings
                .get("asa_mod_configurations")
                .map_or(Ok(Map::new()), |value| {
                    value
                        .as_object()
                        .cloned()
                        .ok_or("ASA Mod configurations must be an object")
                })?;
        for (id, entry) in mods {
            let Some(configuration) = entry.get("configuration") else {
                continue;
            };
            if configuration.is_null() {
                configurations.remove(id);
            } else {
                if configuration.as_object().is_some_and(Map::is_empty) {
                    continue;
                }
                let mut merged = configurations.get(id).map_or(Ok(Map::new()), |value| {
                    value
                        .as_object()
                        .cloned()
                        .ok_or("ASA Mod configuration must be an object")
                })?;
                for (file, text) in configuration
                    .as_object()
                    .ok_or("Invalid Mod configuration")?
                {
                    merged.insert(file.clone(), text.clone());
                }
                configurations.insert(id.clone(), Value::Object(merged));
            }
        }
        effective["asa_mod_configurations"] = Value::Object(configurations);
    }
    Ok(effective)
}

#[cfg(test)]
#[path = "ark_mod_scopes_tests.rs"]
mod tests;
