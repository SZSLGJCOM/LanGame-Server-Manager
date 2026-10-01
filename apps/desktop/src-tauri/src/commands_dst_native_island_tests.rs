use super::*;
use crate::dst_mods::{DstModConfigurationSpec, DstModPrimitiveValue};

#[derive(serde::Serialize)]
pub(super) struct ModOptionProbe {
    mod_id: String,
    name: String,
    value: Value,
}

pub(super) fn configure(values: &mut serde_json::Map<String, Value>, ids: &[String]) {
    values.insert("shard_layout".into(), json!("island_adventures"));
    for shard in app_core::dst_shards::DST_SHARDS {
        values.insert(
            format!("{}_enabled_workshop_mod_ids", shard.process_key),
            json!(ids.join(",")),
        );
    }
    // Island Adventures release presets: the volcano resides in its own world,
    // so the Shipwrecked world must not also generate the integrated volcano.
    values.insert("islands_worldgenoverride_lua".into(), json!(
        "return { override_enabled = true, settings_preset = 'SURVIVAL_SHIPWRECKED_CLASSIC', worldgen_preset = 'SURVIVAL_SHIPWRECKED_CLASSIC', overrides = { volcanoisland = 'none' } }"
    ));
    values.insert("volcano_worldgenoverride_lua".into(), json!(
        "return { override_enabled = true, settings_preset = 'SURVIVAL_VOLCANO_CLASSIC', worldgen_preset = 'SURVIVAL_VOLCANO_CLASSIC', overrides = {} }"
    ));
}

pub(super) fn configure_mod_options(
    values: &mut serde_json::Map<String, Value>,
    specs: &[DstModConfigurationSpec],
) -> Vec<ModOptionProbe> {
    let mut selected_types = std::collections::HashSet::new();
    let mut probes = Vec::new();
    for spec in specs {
        for option in &spec.options {
            // Keep the author's world layout and experimental dev mode intact.
            // Explicit native defaults still verify serialization and runtime
            // scalar types on every shard without inventing a mod option.
            if option.name == "devmode" {
                continue;
            }
            let primitive = option
                .default_value
                .as_ref()
                .or_else(|| option.options.first().map(|choice| &choice.value));
            let (kind, value) = match primitive {
                Some(DstModPrimitiveValue::Boolean(value)) => ("boolean", json!(value)),
                Some(DstModPrimitiveValue::String(value)) => ("string", json!(value)),
                Some(DstModPrimitiveValue::Number(value)) if value.is_finite() => {
                    ("number", json!(value))
                }
                _ => continue,
            };
            if selected_types.insert(kind) {
                probes.push(ModOptionProbe {
                    mod_id: spec.mod_id.clone(),
                    name: option.name.clone(),
                    value,
                });
            }
        }
    }
    assert!(
        !probes.is_empty(),
        "IA modinfo must expose actual configurable options"
    );
    let mut by_mod = serde_json::Map::new();
    for probe in &probes {
        by_mod
            .entry(probe.mod_id.clone())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(probe.name.clone(), probe.value.clone());
    }
    for shard in app_core::dst_shards::DST_SHARDS {
        values.insert(
            format!("{}_mod_configuration_options", shard.process_key),
            Value::Object(by_mod.clone()),
        );
    }
    probes
}

pub(super) fn verify_rendered_configuration(
    cluster: &Path,
    ids: &[String],
    probes: &[ModOptionProbe],
) -> Result<(), Box<dyn std::error::Error>> {
    for shard in app_core::dst_shards::DST_SHARDS {
        let lua = mlua::Lua::new();
        let config = fs::read_to_string(cluster.join(shard.directory).join("modoverrides.lua"))?;
        let mods = lua.load(&config).eval::<mlua::Table>()?;
        for id in ids {
            let entry = mods.get::<mlua::Table>(format!("workshop-{id}"))?;
            assert!(
                entry.get::<bool>("enabled")?,
                "{} mod {id}",
                shard.directory
            );
        }
        for probe in probes {
            let entry = mods.get::<mlua::Table>(format!("workshop-{}", probe.mod_id))?;
            let options = entry.get::<mlua::Table>("configuration_options")?;
            let actual = options.get::<mlua::Value>(probe.name.as_str())?;
            let matches = match (actual, &probe.value) {
                (mlua::Value::Boolean(actual), Value::Bool(expected)) => actual == *expected,
                (mlua::Value::Integer(actual), Value::Number(expected)) => {
                    Some(actual as f64) == expected.as_f64()
                }
                (mlua::Value::Number(actual), Value::Number(expected)) => {
                    Some(actual) == expected.as_f64()
                }
                (mlua::Value::String(actual), Value::String(expected)) => {
                    actual.to_str()?.as_ref() == expected.as_str()
                }
                _ => false,
            };
            assert!(
                matches,
                "{} explicit option {}",
                shard.directory, probe.name
            );
        }
        if matches!(shard.process_key, "islands" | "volcano") {
            let config =
                fs::read_to_string(cluster.join(shard.directory).join("worldgenoverride.lua"))?;
            let world = lua.load(&config).eval::<mlua::Table>()?;
            let preset = if shard.process_key == "islands" {
                "SURVIVAL_SHIPWRECKED_CLASSIC"
            } else {
                "SURVIVAL_VOLCANO_CLASSIC"
            };
            assert_eq!(world.get::<String>("settings_preset")?, preset);
            assert_eq!(world.get::<String>("worldgen_preset")?, preset);
            if shard.process_key == "islands" {
                assert_eq!(
                    world
                        .get::<mlua::Table>("overrides")?
                        .get::<String>("volcanoisland")?,
                    "none"
                );
            }
        }
    }
    Ok(())
}

pub(super) async fn verify_world(
    state: &DesktopState,
    id: &str,
    process: &StartedProcess,
    run_id: i64,
    probes: &[ModOptionProbe],
) -> Result<Value, Box<dyn std::error::Error>> {
    // These public functions and role values come from the author's published
    // Island Adventures release, rather than assuming fixed native shard IDs.
    let (dimension, prefab, task_set) = match process.process_key.as_str() {
        "master" => (1, "forest", "default"),
        "caves" => (2, "cave", "cave_default"),
        "islands" => (3, "shipwrecked", "shipwrecked"),
        "volcano" => (4, "volcanoworld", "volcano"),
        key => return Err(format!("Unexpected IA native shard {key}").into()),
    };
    let identity = native_probe(
        state, id, process, run_id, "",
        "tostring(GetWorldType())..' '..tostring(TheWorld.prefab)..' '..tostring(TheWorld.topology.overrides.task_set)",
    ).await?;
    assert_eq!(identity, format!("{dimension} {prefab} {task_set}"));
    let connected = native_probe(
        state, id, process, run_id,
        "local r={};for dimension,shardid in pairs(Shard_GetConnectedDimensions()) do assert(type(shardid)=='string' and shardid~='');r[#r+1]=tonumber(dimension) end;table.sort(r);",
        "table.concat(r,',')",
    ).await?;
    let expected = (1..=4)
        .filter(|candidate| *candidate != dimension)
        .map(|candidate| candidate.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        connected, expected,
        "{} native dimension connections",
        process.process_key
    );
    let mut checks = Vec::new();
    for probe in probes {
        let kind = match &probe.value {
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            _ => unreachable!("selected native scalar option"),
        };
        checks.push(format!(
            "do local v=GetModConfigData({},{});r[#r+1]=tostring(type(v)=='{kind}' and v=={}) end;",
            serde_json::to_string(&probe.name)?,
            serde_json::to_string(&format!("workshop-{}", probe.mod_id))?,
            serde_json::to_string(&probe.value)?,
        ));
    }
    let configuration = native_probe(
        state,
        id,
        process,
        run_id,
        &format!("local r={{}};{}", checks.join("")),
        "table.concat(r,',')",
    )
    .await?;
    assert_eq!(configuration, vec!["true"; probes.len()].join(","));
    Ok(json!({
        "shard": process.process_key, "world_identity": identity,
        "connected_dimensions": connected, "configured_options": probes,
        "configuration_verified": configuration,
    }))
}
