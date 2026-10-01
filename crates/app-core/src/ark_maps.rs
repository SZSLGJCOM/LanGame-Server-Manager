use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{InstanceDetails, ModulePortGroupSpec, PortBinding};

pub const MAX_ADDITIONAL_MAPS: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArkMap {
    pub id: String,
    pub map_name: String,
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArkMapProcess {
    pub process_key: String,
    pub display_name: String,
    pub map_name: String,
    pub save_directory: String,
    pub native_log_path: String,
}

pub fn is_ark(module_id: &str) -> bool {
    matches!(module_id, "arksurvivalevolved" | "arksurvivalascended")
}

/// World backups own the full Saved tree; the primary world's native path and
/// the default transfer root retain their existing instance ID boundary.
pub fn primary_saves_dir(instance_id: &str, saves_dir: &Path) -> PathBuf {
    if saves_dir
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("Saved"))
    {
        saves_dir.join(instance_id)
    } else {
        saves_dir.to_owned()
    }
}

pub fn parse_additional_maps(settings: &Value) -> Result<Vec<ArkMap>, String> {
    let Some(value) = settings.get("additional_maps") else {
        return Ok(Vec::new());
    };
    let values = value.as_array().ok_or("additional_maps must be an array")?;
    if values.len() > MAX_ADDITIONAL_MAPS {
        return Err("An ARK instance supports at most 15 additional maps".into());
    }
    let mut ids = HashSet::new();
    let mut maps = Vec::with_capacity(values.len());
    for value in values {
        let map: ArkMap = serde_json::from_value(value.clone())
            .map_err(|error| format!("Invalid additional map: {error}"))?;
        if map.id.is_empty()
            || map.id.len() > 32
            || !map
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || !map.id.as_bytes()[0].is_ascii_alphanumeric()
            || !ids.insert(map.id.clone())
        {
            return Err(
                "Map IDs must be unique lowercase letters, digits or hyphens (1–32 characters)"
                    .into(),
            );
        }
        if !valid_map_name(&map.map_name) {
            return Err("Map names must be native package identifiers with letters, digits or underscores (1–128 characters)".into());
        }
        if map.name.trim().is_empty()
            || map.name.trim() != map.name
            || map.name.chars().count() > 80
            || map.name.chars().any(|character| {
                character.is_control() || matches!(character, '"' | '?' | '\u{2028}' | '\u{2029}')
            })
        {
            return Err("Map display names must be 1–80 characters without quotes, URL delimiters or control characters".into());
        }
        maps.push(map);
    }
    Ok(maps)
}

fn valid_map_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

pub fn process_key(map_id: &str) -> String {
    format!("map-{map_id}")
}

/// Native files belong to stable map identities, not display names or run IDs.
pub fn native_log_path(module_id: &str, logs: &Path, process_key: &str) -> Result<PathBuf, String> {
    let edition = match module_id {
        "arksurvivalascended" => "ascended",
        "arksurvivalevolved" => "evolved",
        _ => return Err("Map logs require an ARK instance".into()),
    };
    let suffix = if process_key == "main" {
        "server"
    } else {
        let id = process_key
            .strip_prefix("map-")
            .ok_or("Invalid ARK map process key")?;
        if id.is_empty()
            || id.len() > 32
            || !id.as_bytes()[0].is_ascii_alphanumeric()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("Invalid ARK map process key".into());
        }
        process_key
    };
    Ok(logs.join(format!("ark-{edition}-{suffix}.log")))
}

pub fn port_name(map_id: &str, base_name: &str) -> String {
    format!("{}-{base_name}", process_key(map_id))
}

/// Map identities remain attached to their retained worlds, even when paused.
pub fn validate_map_changes(
    previous: &Value,
    incoming: &Value,
    running: bool,
) -> Result<(), String> {
    let previous_maps = parse_additional_maps(previous)?;
    let incoming_maps = parse_additional_maps(incoming)?;
    if running && previous_maps != incoming_maps {
        return Err("Stop the ARK instance before changing its map servers".into());
    }
    for map in &incoming_maps {
        if previous_maps
            .iter()
            .any(|old| old.id == map.id && old.map_name != map.map_name)
        {
            return Err("An existing map's package cannot change. Add a new map to retain the previous world's identity".into());
        }
    }
    Ok(())
}

pub fn processes(instance: &InstanceDetails) -> Result<Vec<ArkMapProcess>, String> {
    if !is_ark(&instance.summary.module_id) {
        return Err("Map processes require an ARK instance".into());
    }
    let settings: Value =
        serde_json::from_str(&instance.settings_json).map_err(|error| error.to_string())?;
    let maps = parse_additional_maps(&settings)?;
    let default_map = if instance.summary.module_id == "arksurvivalascended" {
        "TheIsland_WP"
    } else {
        "TheIsland"
    };
    let map_name = settings
        .get("map_name")
        .and_then(Value::as_str)
        .unwrap_or(default_map);
    if !valid_map_name(map_name) {
        return Err("The main map must be a native package identifier".into());
    }
    let logs = Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("The instance configuration has no owned root")?
        .join("logs");
    let mut processes = vec![ArkMapProcess {
        process_key: "main".into(),
        display_name: map_name.into(),
        map_name: map_name.into(),
        save_directory: instance.summary.id.clone(),
        native_log_path: native_log_path(&instance.summary.module_id, &logs, "main")?
            .to_string_lossy()
            .into_owned(),
    }];
    for map in maps.into_iter().filter(|map| map.enabled) {
        let key = process_key(&map.id);
        processes.push(ArkMapProcess {
            native_log_path: native_log_path(&instance.summary.module_id, &logs, &key)?
                .to_string_lossy()
                .into_owned(),
            save_directory: format!("{}-{key}", instance.summary.id),
            process_key: key,
            display_name: map.name,
            map_name: map.map_name,
        });
    }
    Ok(processes)
}

/// A projection changes native launch/transport inputs, never managed ownership.
pub fn project_process(
    instance: &InstanceDetails,
    requested_key: Option<&str>,
) -> Result<InstanceDetails, String> {
    if !is_ark(&instance.summary.module_id) {
        return Ok(instance.clone());
    }
    let key = requested_key
        .filter(|key| !key.is_empty())
        .unwrap_or("main");
    let process = processes(instance)?
        .into_iter()
        .find(|process| process.process_key == key)
        .ok_or_else(|| format!("ARK map process `{key}` is unknown or disabled"))?;
    let mut projected = instance.clone();
    let mut settings: Value =
        serde_json::from_str(&instance.settings_json).map_err(|error| error.to_string())?;
    let has_maps = !parse_additional_maps(&settings)?.is_empty();
    let base_name = settings
        .get("server_name")
        .and_then(Value::as_str)
        .unwrap_or(&instance.summary.name)
        .to_owned();
    settings["map_name"] = Value::String(process.map_name);
    settings["_managed_ark_save_directory"] = Value::String(process.save_directory);
    settings["_managed_ark_native_log"] = Value::String(process.native_log_path);
    if has_maps {
        settings["server_name"] = Value::String(format!("{base_name} | {}", process.display_name));
        if settings
            .get("cluster_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            settings["cluster_id"] = Value::String(format!("lgsm-{}", instance.summary.id));
        }
    }
    if !settings
        .get("cluster_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .is_empty()
        && settings
            .get("cluster_directory")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        let primary = primary_saves_dir(&instance.summary.id, Path::new(&instance.saves_path));
        settings["cluster_directory"] =
            Value::String(primary.join("cluster").to_string_lossy().into_owned());
    }
    if key != "main" {
        // IDs may share prefixes; only the exact native bindings belong to
        // this map. A prefix match would expose another map's RCON endpoint.
        projected.ports = ["game", "peer", "query", "rcon"]
            .into_iter()
            .filter_map(|name| {
                instance
                    .ports
                    .iter()
                    .find(|port| port.name == format!("{key}-{name}"))
                    .map(|port| PortBinding {
                        name: name.into(),
                        ..port.clone()
                    })
            })
            .collect();
        for required in ["game", "query", "rcon"] {
            if !projected
                .ports
                .iter()
                .any(|port| port.name == required && port.port != 0)
            {
                return Err(format!("ARK map `{key}` has no registered {required} port"));
            }
        }
    } else {
        projected
            .ports
            .retain(|port| !port.name.starts_with("map-"));
    }
    projected.settings_json =
        serde_json::to_string(&settings).map_err(|error| error.to_string())?;
    Ok(projected)
}

pub fn port_groups(
    module_id: &str,
    settings: &Value,
    base: &[ModulePortGroupSpec],
) -> Result<Vec<ModulePortGroupSpec>, String> {
    let mut groups = base.to_vec();
    if !is_ark(module_id) {
        return Ok(groups);
    }
    for map in parse_additional_maps(settings)? {
        for group in base {
            groups.push(ModulePortGroupSpec {
                id: format!("{}-{}", process_key(&map.id), group.id),
                members: group
                    .members
                    .iter()
                    .map(|name| port_name(&map.id, name))
                    .collect(),
                member_offsets: group.member_offsets.as_ref().map(|offsets| {
                    offsets
                        .iter()
                        .map(|(name, offset)| (port_name(&map.id, name), *offset))
                        .collect()
                }),
            });
        }
    }
    Ok(groups)
}

/// Persist every map's ports, including paused maps, so resume keeps its endpoints.
pub fn requested_ports(
    module_id: &str,
    settings: &Value,
    defaults: &[PortBinding],
    incoming: &[PortBinding],
    current: &[PortBinding],
) -> Result<Vec<PortBinding>, String> {
    if !is_ark(module_id) {
        return Ok(incoming.to_vec());
    }
    let maps = parse_additional_maps(settings)?;
    let mut ports: Vec<_> = incoming
        .iter()
        .filter(|port| !port.name.starts_with("map-"))
        .cloned()
        .collect();
    let mut used: HashSet<_> = ports
        .iter()
        .map(|port| (port.protocol.clone(), port.port))
        .collect();
    let mut retained_maps = Vec::with_capacity(maps.len());
    for map in maps {
        let mut additions = Vec::new();
        for default in defaults {
            let name = port_name(&map.id, &default.name);
            if let Some(port) = incoming
                .iter()
                .find(|port| port.name == name)
                .or_else(|| current.iter().find(|port| port.name == name))
            {
                if port.protocol != default.protocol || port.port == 0 {
                    return Err(format!(
                        "ARK map binding `{name}` must have a nonzero {} port",
                        default.protocol
                    ));
                }
                used.insert((port.protocol.clone(), port.port));
                additions.push(port.clone());
            }
        }
        retained_maps.push((map, additions));
    }
    // Reserve every retained map before allocating a new map. Configuration
    // order must not let an earlier new map take a later paused map's endpoints.
    for (map, mut additions) in retained_maps {
        for default in defaults {
            let name = port_name(&map.id, &default.name);
            if additions.iter().any(|port| port.name == name) {
                continue;
            }
            let mut candidate = default.port;
            let needs_peer = module_id == "arksurvivalevolved" && default.name == "game";
            while used.contains(&(default.protocol.clone(), candidate))
                || (needs_peer
                    && candidate
                        .checked_add(1)
                        .is_none_or(|peer| used.contains(&(default.protocol.clone(), peer))))
            {
                candidate = candidate
                    .checked_add(10)
                    .ok_or("No port remains for the additional map")?;
            }
            // ASE's peer is derived from the game listener, not an independent socket choice.
            if module_id == "arksurvivalevolved" && default.name == "peer" {
                let game_name = port_name(&map.id, "game");
                if let Some(game) = additions.iter().find(|port| port.name == game_name) {
                    candidate = game
                        .port
                        .checked_add(1)
                        .ok_or("ARK peer port exceeds 65535")?;
                }
            }
            used.insert((default.protocol.clone(), candidate));
            additions.push(PortBinding {
                name,
                port: candidate,
                protocol: default.protocol.clone(),
            });
        }
        ports.extend(additions);
    }
    Ok(ports)
}

#[cfg(test)]
#[path = "ark_maps_tests.rs"]
mod tests;
