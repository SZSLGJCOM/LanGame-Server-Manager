use std::collections::BTreeMap;

use app_modules::ModuleDescriptor;
use serde_json::{Map, Value};

use super::templates_render_dst_inventory::{
    DST_SHARED_OVERRIDE_ENTRIES, dst_shard_override_entries,
};
use crate::StorageError;

#[path = "dst_world_lua.rs"]
mod lua;

#[path = "dst_mod_enablement.rs"]
mod mod_enablement;
pub use mod_enablement::{inspect_dst_mod_enablement, validate_dst_shard_mod_requirements};

#[path = "dst_mod_preservation.rs"]
mod mod_preservation;
pub use mod_preservation::validate_dst_mod_preservation;

#[path = "dst_world_requirements.rs"]
mod requirements;
pub use requirements::dst_world_setting_evidence_known;

#[path = "dst_world_inheritance.rs"]
mod inheritance;
pub(super) use inheritance::inherited_master_settings;

type Settings = Map<String, Value>;

/// Extract a literal subtable without executing save or Mod Lua. The shared
/// parser enforces byte, nesting and entry budgets and preserves numeric keys.
pub fn extract_dst_static_lua_table(source: &str, field: &str) -> Result<Option<String>, String> {
    let table = lua::parse(source, false)
        .ok_or_else(|| String::from("DST Lua must be a bounded static return table."))?;
    let Some(node) = table.get(field) else {
        return Ok(None);
    };
    if node.table().is_none() {
        return Err(format!("DST saved {field} must be a literal table."));
    }
    Ok(Some(format!("return {}\n", &source[node.span.clone()])))
}

fn schema(descriptor: Option<&ModuleDescriptor>) -> Result<Option<Value>, StorageError> {
    let Some(descriptor) = descriptor.filter(|descriptor| descriptor.summary.id == "dontstarve")
    else {
        return Ok(None);
    };
    descriptor
        .schema_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(Into::into)
}

fn entries(
    shard: &str,
) -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)> {
    DST_SHARED_OVERRIDE_ENTRIES
        .iter()
        .filter(move |_| shard == "master")
        .chain(dst_shard_override_entries(shard))
}

fn text<'a>(settings: &'a Settings, key: &str) -> &'a str {
    settings
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn property<'a>(schema: &'a Value, key: &str) -> Option<&'a Value> {
    schema.get("properties")?.get(key)
}

fn default_value(schema: &Value, key: &str, fallback: &str) -> Value {
    property(schema, key)
        .and_then(|property| property.get("default"))
        .cloned()
        .unwrap_or_else(|| Value::String(fallback.to_string()))
}

fn valid_value(schema: &Value, key: &str, value: &Value) -> bool {
    let Some(value_text) = value.as_str() else {
        return false;
    };
    let Some(property) = property(schema, key) else {
        return true;
    };
    if let Some(allowed) = property.get("enum").and_then(Value::as_array) {
        return allowed.contains(value);
    }
    if key.ends_with("_preset") {
        return !value_text.is_empty()
            && value_text.len() <= 128
            && value_text.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphanumeric()
                    || byte == b'_'
                    || (index > 0 && matches!(byte, b'.' | b'-'))
            });
    }
    true
}

fn custom_raw<'a>(schema: &Value, settings: &'a Settings, shard: &str) -> Option<&'a str> {
    let key = format!("{shard}_worldgenoverride_lua");
    let raw = text(settings, &key);
    let default = property(schema, &key)
        .and_then(|property| property.get("default"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let normalize = |value: &str| {
        value
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim()
            .to_string()
    };
    (!raw.trim().is_empty() && normalize(raw) != normalize(default)).then_some(raw)
}

fn preset<'a>(table: &'a lua::Table, kind: &str) -> Option<&'a Value> {
    table
        .get(kind)
        .and_then(lua::Node::scalar)
        .filter(|value| !value.is_null())
        .or_else(|| table.get("preset").and_then(lua::Node::scalar))
}

/// Read only literal presets from an enabled data-table override. Dynamic Lua
/// remains authoritative and cannot establish a playstyle conflict here.
pub(crate) fn explicit_master_presets(raw: &str) -> Option<[Option<String>; 2]> {
    let table = lua::parse(raw, false)?;
    if table.get("override_enabled").and_then(lua::Node::scalar) != Some(&Value::Bool(true)) {
        return None;
    }
    let preset = |kind| {
        preset(&table, kind)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Some([preset("settings_preset"), preset("worldgen_preset")])
}

fn preset_node<'a>(table: &'a lua::Table, kind: &str) -> Option<&'a lua::Node> {
    table
        .get(kind)
        .filter(|node| node.scalar() != Some(&Value::Null))
        .or_else(|| {
            table
                .get("preset")
                .filter(|node| node.scalar() != Some(&Value::Null))
        })
}

fn raw_is_editable(schema: &Value, shard: &str, table: &lua::Table) -> bool {
    if table.get("override_enabled").and_then(lua::Node::scalar) != Some(&Value::Bool(true)) {
        return false;
    }
    if let Some(overrides) = table.get("overrides") {
        if let Some(overrides) = overrides.table() {
            if !overrides_are_editable(schema, shard, overrides) {
                return false;
            }
        } else if overrides.scalar() != Some(&Value::Null) {
            return false;
        }
    }
    ["settings_preset", "worldgen_preset"].iter().all(|kind| {
        let key = format!("{shard}_{kind}");
        let node = preset_node(table, kind);
        node.is_none_or(|node| {
            node.scalar()
                .is_some_and(|value| valid_value(schema, &key, value))
        })
    })
}

fn overrides_are_editable(schema: &Value, shard: &str, table: &lua::Table) -> bool {
    entries(shard).all(|(key, native, _)| {
        table.get(native).is_none_or(|node| {
            node.scalar()
                .is_some_and(|value| value.is_null() || valid_value(schema, key, value))
        })
    })
}

pub(crate) fn project(
    descriptor: Option<&ModuleDescriptor>,
    settings: &mut Settings,
) -> Result<(), StorageError> {
    if let Some(schema) = schema(descriptor)? {
        project_schema(&schema, settings);
    }
    Ok(())
}

fn project_schema(schema: &Value, settings: &mut Settings) {
    for shard in ["master", "caves"] {
        if let Some(raw) = custom_raw(schema, settings, shard) {
            let table = lua::parse(raw, false);
            for (key, native, fallback) in entries(shard) {
                let value = table
                    .as_ref()
                    .and_then(|table| table.get("overrides"))
                    .and_then(lua::Node::table)
                    .and_then(|table| table.get(native))
                    .and_then(lua::Node::scalar)
                    .filter(|value| valid_value(schema, key, value))
                    .cloned()
                    .unwrap_or_else(|| default_value(schema, key, fallback));
                settings.insert(key.to_string(), value);
            }
            for kind in ["settings_preset", "worldgen_preset"] {
                let key = format!("{shard}_{kind}");
                let fallback = if shard == "master" {
                    "SURVIVAL_TOGETHER"
                } else {
                    "DST_CAVE"
                };
                let value = table
                    .as_ref()
                    .and_then(|table| preset(table, kind))
                    .filter(|value| valid_value(schema, &key, value))
                    .cloned()
                    .unwrap_or_else(|| default_value(schema, &key, fallback));
                settings.insert(key, value);
            }
        } else {
            let extra = lua::parse(
                text(settings, &format!("{shard}_world_overrides_extra")),
                true,
            );
            if let Some(extra) = extra {
                for (key, native, fallback) in entries(shard) {
                    if let Some(node) = extra.get(native) {
                        if node.scalar() == Some(&Value::Null) {
                            continue;
                        }
                        let value = node
                            .scalar()
                            .filter(|value| valid_value(schema, key, value))
                            .cloned()
                            .unwrap_or_else(|| default_value(schema, key, fallback));
                        settings.insert(key.to_string(), value);
                    }
                }
            }
        }
    }
}

/// Reconcile at the locked storage boundary so direct callers and the form save the same file.
pub(crate) fn reconcile(
    descriptor: Option<&ModuleDescriptor>,
    previous: Settings,
    incoming: Settings,
) -> Result<Settings, StorageError> {
    let Some(schema) = schema(descriptor)? else {
        return Ok(incoming);
    };
    reconcile_schema(&schema, previous, incoming)
}

fn reconcile_schema(
    schema: &Value,
    mut previous: Settings,
    mut incoming: Settings,
) -> Result<Settings, StorageError> {
    project_schema(schema, &mut previous);
    for shard in ["master", "caves"] {
        let raw_key = format!("{shard}_worldgenoverride_lua");
        let extra_key = format!("{shard}_world_overrides_extra");
        let inactive =
            shard == "caves" && incoming.get("enable_caves").and_then(Value::as_bool) != Some(true);
        // A direct script edit is authoritative, including replacing or clearing the file.
        if incoming.contains_key(&raw_key) && incoming.get(&raw_key) != previous.get(&raw_key) {
            continue;
        }
        let raw = custom_raw(schema, &incoming, shard).map(str::to_string);
        if raw.is_none()
            && incoming.contains_key(&extra_key)
            && incoming.get(&extra_key) != previous.get(&extra_key)
        {
            continue;
        }
        let mut overrides = BTreeMap::new();
        for (key, native, fallback) in entries(shard) {
            if let Some(value) = incoming.get(*key) {
                let old = previous
                    .get(*key)
                    .cloned()
                    .unwrap_or_else(|| default_value(schema, key, fallback));
                if *value != old {
                    if !valid_value(schema, key, value) {
                        if inactive {
                            continue;
                        }
                        return Err(invalid_guided_value(key));
                    }
                    if let Some(value) = value.as_str() {
                        overrides.insert(native.to_string(), lua::quote(value));
                    }
                }
            }
        }
        let mut presets = BTreeMap::new();
        for kind in ["settings_preset", "worldgen_preset"] {
            let key = format!("{shard}_{kind}");
            let fallback = if shard == "master" {
                "SURVIVAL_TOGETHER"
            } else {
                "DST_CAVE"
            };
            if let Some(value) = incoming.get(&key) {
                let old = previous
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| default_value(schema, &key, fallback));
                if *value != old {
                    if !valid_value(schema, &key, value) {
                        if inactive {
                            continue;
                        }
                        return Err(invalid_guided_value(&key));
                    }
                    if let Some(value) = value.as_str() {
                        presets.insert(kind.to_string(), lua::quote(value));
                    }
                }
            }
        }
        if overrides.is_empty() && presets.is_empty() {
            continue;
        }
        if let Some(raw) = raw {
            let table = lua::parse(&raw, false)
                .filter(|table| raw_is_editable(schema, shard, table))
                .ok_or_else(|| script_edit_error(&raw_key))?;
            let raw = if let Some(table) = table.get("overrides").and_then(lua::Node::table) {
                lua::set_fields(&raw, table, &overrides)
            } else if overrides.is_empty() {
                raw
            } else {
                let body = overrides
                    .iter()
                    .map(|(key, value)| format!("{key} = {value},"))
                    .collect::<Vec<_>>()
                    .join(" ");
                lua::set_fields(
                    &raw,
                    &table,
                    &BTreeMap::from([(String::from("overrides"), format!("{{ {body} }}"))]),
                )
            };
            let raw = if presets.is_empty() {
                raw
            } else {
                let table = lua::parse(&raw, false).ok_or_else(|| script_edit_error(&raw_key))?;
                lua::set_fields(&raw, &table, &presets)
            };
            if lua::parse(&raw, false).is_none() {
                return Err(script_edit_error(&raw_key));
            }
            incoming.insert(raw_key.clone(), Value::String(raw));
        }
        let extra = text(&incoming, &extra_key);
        if let Some(table) =
            lua::parse(extra, true).filter(|table| overrides_are_editable(schema, shard, table))
        {
            let mut updates = BTreeMap::new();
            for (key, native, fallback) in entries(shard) {
                if let Some(value) = overrides.get(*native) {
                    let force_default = custom_raw(schema, &incoming, shard).is_none()
                        && has_custom_preset(schema, &incoming, shard)
                        && incoming.get(*key) == Some(&default_value(schema, key, fallback));
                    if table.get(native).is_some() || force_default {
                        updates.insert(native.to_string(), value.clone());
                    }
                }
            }
            if !updates.is_empty() {
                let patched = lua::set_fields(extra, &table, &updates);
                if lua::parse(&patched, true).is_none() {
                    return Err(script_edit_error(&extra_key));
                }
                incoming.insert(extra_key, Value::String(patched));
            }
        } else if custom_raw(schema, &incoming, shard).is_none() {
            return Err(script_edit_error(&extra_key));
        }
    }
    project_schema(schema, &mut incoming);
    Ok(incoming)
}

fn has_custom_preset(schema: &Value, settings: &Settings, shard: &str) -> bool {
    ["settings_preset", "worldgen_preset"].iter().any(|kind| {
        let key = format!("{shard}_{kind}");
        settings.get(&key).is_some_and(|value| {
            Some(value) != property(schema, &key).and_then(|property| property.get("default"))
        })
    })
}

fn script_edit_error(field: &str) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: String::from("dontstarve"),
        field: field.to_string(),
        message: String::from(
            "This Lua source cannot be safely edited as a data table. Edit the corresponding setting in the original Lua source; other server settings can still be saved.",
        ),
    }
}

fn invalid_guided_value(field: &str) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: String::from("dontstarve"),
        field: field.to_string(),
        message: String::from("must be a supported value from the module schema"),
    }
}

#[cfg(test)]
#[path = "dst_world_settings_tests.rs"]
mod tests;
