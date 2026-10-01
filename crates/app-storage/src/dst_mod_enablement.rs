use std::io::{self, Write};

use serde_json::{Map, Value, json};

use super::super::templates_render_dst::render_dst_modoverrides;
use super::lua;

const MAX_INPUT_BYTES: usize = 128 * 1024;
const MAX_OUTPUT_BYTES: usize = 8 * 1024;
const MAX_DECLARED_NAMES: usize = 100;
const MAX_DECLARED_NAME_BYTES: usize = 4096;

/// Released Island Adventures presets and shard RPCs require both server Mods
/// in every world. Validate before launch while allowing staged settings edits.
pub fn validate_dst_shard_mod_requirements(settings: &Value) -> Result<(), String> {
    if app_core::dst_shards::dst_shard_layout(settings)?
        != app_core::dst_shards::DstShardLayout::IslandAdventures
    {
        return Ok(());
    }
    let evidence = inspect_dst_mod_enablement(settings)?;
    for shard in evidence["shards"].as_array().into_iter().flatten() {
        let name = shard["shard"].as_str().unwrap_or("unknown");
        let Some(enabled) = shard["declaredEnabledModNames"].as_array() else {
            return Err(format!(
                "Island Adventures {name}: cannot confirm enabled Mods from modoverrides.lua. Use a static return table."
            ));
        };
        for id in ["3435352667", "1467214795"] {
            let required = format!("workshop-{id}");
            if !enabled
                .iter()
                .any(|value| value.as_str() == Some(required.as_str()))
            {
                return Err(format!(
                    "Island Adventures {name}: required Workshop Mod {id} is not enabled. Enable Core (3435352667) and Shipwrecked (1467214795) in all four shards."
                ));
            }
        }
        if enabled.iter().any(|value| {
            matches!(
                value.as_str(),
                Some("workshop-1505270912" | "workshop-2986194136")
            )
        }) {
            return Err(format!(
                "Island Adventures {name}: Tropical Experience / Tropical Adventures conflicts with this layout. Use separate instances for these map Mods."
            ));
        }
    }
    Ok(())
}

/// Describe only the enablement declarations rendered from current settings.
/// This performs no Lua execution, file discovery, or runtime inspection.
pub fn inspect_dst_mod_enablement(settings: &Value) -> Result<Value, String> {
    let settings_object = settings
        .as_object()
        .ok_or_else(|| String::from("DST settings must be an object."))?;
    let active = app_core::dst_shards::dst_shards(settings)?;
    let count = if app_core::dst_shards::dst_shard_layout(settings)?
        == app_core::dst_shards::DstShardLayout::IslandAdventures
    {
        4
    } else {
        2
    };
    let roles = app_core::dst_shards::DST_SHARDS[..count]
        .iter()
        .map(|spec| (spec.process_key, active.contains(&spec)))
        .collect::<Vec<_>>();
    if !fits_json_budget(settings, MAX_INPUT_BYTES) {
        return Ok(unknown_result(
            &roles,
            "Settings exceed the static analysis input limit.",
        ));
    }

    let mut name_count = 0;
    let mut name_bytes = 0;
    let shards = roles
        .iter()
        .map(|&(shard, active)| {
            let result = inspect_shard(settings_object, shard, active);
            for field in [
                "declaredEnabledModNames",
                "declaredDisabledModNames",
                "declaredUnspecifiedModNames",
            ] {
                if let Some(names) = result[field].as_array() {
                    name_count += names.len();
                    name_bytes += names
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::len)
                        .sum::<usize>();
                }
            }
            result
        })
        .collect::<Vec<_>>();
    if name_count > MAX_DECLARED_NAMES || name_bytes > MAX_DECLARED_NAME_BYTES {
        return Ok(unknown_result(
            &roles,
            "Mod declarations exceed the static analysis name limit; no partial name lists are supplied.",
        ));
    }
    let result = result_with_shards(shards);
    if !fits_json_budget(&result, MAX_OUTPUT_BYTES) {
        return Ok(unknown_result(
            &roles,
            "Mod declarations exceed the evidence byte limit; no partial name lists are supplied.",
        ));
    }
    Ok(result)
}

fn inspect_shard(settings: &Map<String, Value>, shard: &str, active: bool) -> Value {
    // Use the materializer's exact raw-script/structured-setting precedence.
    let rendered = render_dst_modoverrides(settings, shard);
    let Some(table) = lua::parse(&rendered, false) else {
        return unknown_shard(
            shard,
            active,
            "The rendered Lua is not a supported bounded static table; declarations are unknown.",
        );
    };
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();
    let mut unspecified = Vec::new();
    let mut client_mods_disabled = None;
    for entry in table.entries {
        let Some(name) = entry.key else {
            return unknown_shard(
                shard,
                active,
                "A Mod entry does not have a string key; declarations are unknown.",
            );
        };
        if name == "client_mods_disabled" {
            match entry.node.scalar() {
                Some(Value::Bool(value)) => client_mods_disabled = Some(*value),
                Some(Value::Null) => {}
                _ => {
                    return unknown_shard(
                        shard,
                        active,
                        "The client-mod policy is not a literal boolean; declarations are unknown.",
                    );
                }
            }
            continue;
        }
        let Some(options) = entry.node.table() else {
            return unknown_shard(
                shard,
                active,
                "A Mod entry is not a static options table; declarations are unknown.",
            );
        };
        match options.get("enabled") {
            None => unspecified.push(name),
            Some(node) => match node.scalar() {
                Some(Value::Bool(true)) => enabled.push(name),
                Some(Value::Bool(false)) => disabled.push(name),
                Some(Value::Null) => unspecified.push(name),
                _ => {
                    return unknown_shard(
                        shard,
                        active,
                        "A Mod enabled value is not a literal boolean or nil; declarations are unknown.",
                    );
                }
            },
        }
    }
    enabled.sort();
    disabled.sort();
    unspecified.sort();
    json!({
        "shard": shard,
        "active": active,
        "analysisStatus": "known",
        "declaredEnabledModNames": enabled,
        "declaredDisabledModNames": disabled,
        "declaredUnspecifiedModNames": unspecified,
        "clientModsDisabled": client_mods_disabled,
        "reason": "Static declarations from the configuration rendered from current settings. Missing or nil enabled leaves enablement unspecified; it does not declare disablement."
    })
}

fn unknown_shard(shard: &str, active: bool, reason: &str) -> Value {
    json!({
        "shard": shard,
        "active": active,
        "analysisStatus": "unknown",
        "declaredEnabledModNames": null,
        "declaredDisabledModNames": null,
        "declaredUnspecifiedModNames": null,
        "clientModsDisabled": null,
        "reason": reason
    })
}

fn unknown_result(roles: &[(&str, bool)], reason: &str) -> Value {
    result_with_shards(
        roles
            .iter()
            .map(|&(shard, active)| unknown_shard(shard, active, reason))
            .collect(),
    )
}

fn result_with_shards(shards: Vec<Value>) -> Value {
    json!({
        "shards": shards,
        "limitation": "Configured declarations are not proof of installation, actual loading, compatibility, or load order. Names are exact configuration keys; the engine may resolve aliases. clientModsDisabled is client-mod policy, not a Mod name. Inactive shards do not describe the running server."
    })
}

fn fits_json_budget(value: &Value, bytes: usize) -> bool {
    serde_json::to_writer(JsonByteBudget { remaining: bytes }, value).is_ok()
}

struct JsonByteBudget {
    remaining: usize,
}

impl Write for JsonByteBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("JSON byte budget exceeded"));
        }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "dst_mod_enablement_tests.rs"]
mod tests;
