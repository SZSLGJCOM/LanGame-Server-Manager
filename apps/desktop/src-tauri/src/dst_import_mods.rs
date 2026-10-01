use std::collections::BTreeSet;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::commands_dst_import_validation::{is_link_or_reparse, require_plain_directory};

const MAX_CONFIGURATION_BYTES: usize = 120 * 1024;
const MAX_INDEX_BYTES: usize = 1024 * 1024;
const MAX_WORKSHOP_IDS: usize = 100;
const SHARD_KEYS: &[&str] = &["master", "caves", "islands", "volcano"];

#[derive(Debug)]
pub(crate) struct DstImportedShardMods {
    pub(crate) key: String,
    pub(crate) raw: String,
}

#[derive(Debug)]
struct SourceSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
    limit: usize,
}

/// Imported declarations own enablement and options. Cached downloads remain
/// instance-owned but are never interpreted as enabled in the imported world.
#[derive(Debug)]
pub(crate) struct DstImportedModPlan {
    pub(crate) sources: Vec<DstImportedShardMods>,
    pub(crate) workshop_ids: Vec<String>,
    snapshots: Vec<SourceSnapshot>,
}

impl DstImportedModPlan {
    pub(crate) fn apply(&self, settings: &mut Map<String, Value>) -> Result<(), String> {
        let mut next = settings.clone();
        let mut downloads = workshop_list(settings.get("shared_workshop_mod_ids"))?;
        downloads.extend(self.workshop_ids.iter().cloned());
        for key in SHARD_KEYS {
            next.insert(
                format!("{key}_modoverrides_lua"),
                Value::String(String::new()),
            );
            next.insert(
                format!("{key}_enabled_workshop_mod_ids"),
                Value::String(String::new()),
            );
            next.remove(&format!("{key}_mod_configuration_options"));
        }
        for source in &self.sources {
            next.insert(
                format!("{}_modoverrides_lua", source.key),
                Value::String(source.raw.clone()),
            );
        }
        next.insert(
            "shared_workshop_mod_ids".to_owned(),
            Value::String(downloads.into_iter().collect::<Vec<_>>().join("\n")),
        );
        if let Some(removed) = settings.get("dst_removed_workshop_mod_ids") {
            let mut removed = workshop_list(Some(removed))?;
            for id in &self.workshop_ids {
                removed.remove(id);
            }
            if removed.is_empty() {
                next.remove("dst_removed_workshop_mod_ids");
            } else {
                next.insert("dst_removed_workshop_mod_ids".to_owned(), json!(removed));
            }
        }
        *settings = next;
        Ok(())
    }

    /// Downloading can outlive the initial read. Never publish settings derived
    /// from source configuration that changed while the required Mods downloaded.
    pub(crate) fn verify_unchanged(&self) -> Result<(), String> {
        for snapshot in &self.snapshots {
            if read_optional_bytes(&snapshot.path, snapshot.limit)? != snapshot.content {
                return Err(format!(
                    "DST import source Mod configuration changed at {}. Select the source again before importing.",
                    snapshot.path.display()
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn read_import_mods(shards: &[(&str, &Path)]) -> Result<DstImportedModPlan, String> {
    if shards.is_empty() || shards.len() > SHARD_KEYS.len() {
        return Err("Read Mod configuration from one to four supported DST shards.".to_owned());
    }
    let mut keys = BTreeSet::new();
    let mut plan = DstImportedModPlan {
        sources: Vec::new(),
        workshop_ids: Vec::new(),
        snapshots: Vec::new(),
    };
    let mut ids = BTreeSet::new();
    for (key, root) in shards {
        if !SHARD_KEYS.contains(key) || !keys.insert(*key) {
            return Err(format!("Unsupported or duplicate DST import shard {key}."));
        }
        require_plain_directory(root, "DST source shard")?;
        let overrides_path = root.join("modoverrides.lua");
        let overrides = read_optional_bytes(&overrides_path, MAX_CONFIGURATION_BYTES)?;
        plan.snapshots.push(SourceSnapshot {
            path: overrides_path.clone(),
            content: overrides.clone(),
            limit: MAX_CONFIGURATION_BYTES,
        });
        let raw = if let Some(bytes) = overrides {
            decode_source(&overrides_path, bytes)?
        } else {
            let save_root = root.join("save");
            require_plain_directory(&save_root, "DST source save")?;
            let index_path = save_root.join("shardindex");
            let bytes = read_optional_bytes(&index_path, MAX_INDEX_BYTES)?
                .ok_or_else(|| missing_configuration(key))?;
            plan.snapshots.push(SourceSnapshot {
                path: index_path.clone(),
                content: Some(bytes.clone()),
                limit: MAX_INDEX_BYTES,
            });
            let index = decode_source(&index_path, bytes)?;
            // Klei's uncompressed text persistence envelope is data, not Lua.
            // Other envelopes still fail the static parser instead of executing.
            let index_data = index
                .strip_prefix("KLEI     1")
                .unwrap_or(&index)
                .trim_start();
            app_storage::extract_dst_static_lua_table(index_data, "enabled_mods")
                .map_err(|error| {
                    format!(
                        "Cannot recover {key} Mod configuration from {}: {error}",
                        index_path.display()
                    )
                })?
                .ok_or_else(|| missing_configuration(key))?
        };
        ids.extend(enabled_workshop_ids(key, &raw)?);
        if ids.len() > MAX_WORKSHOP_IDS {
            return Err("Imported DST world exceeds the 100 Workshop Mod limit; no partial Mod recovery was applied.".to_owned());
        }
        plan.sources.push(DstImportedShardMods {
            key: (*key).to_owned(),
            raw,
        });
    }
    plan.workshop_ids = ids.into_iter().collect();
    Ok(plan)
}

fn missing_configuration(shard: &str) -> String {
    format!(
        "Cannot determine the saved {shard} Mods: modoverrides.lua is missing and save/shardindex has no enabled_mods table. Select a complete cluster with its Mod configuration; no imported Mods were assumed to be disabled."
    )
}

fn enabled_workshop_ids(shard: &str, raw: &str) -> Result<BTreeSet<String>, String> {
    let evidence =
        app_storage::inspect_dst_mod_enablement(&json!({ "master_modoverrides_lua": raw }))?;
    let declaration = &evidence["shards"][0];
    if declaration["analysisStatus"] != "known" {
        return Err(format!(
            "Cannot automatically recover {shard} Mods: modoverrides.lua must be a bounded static Lua data table. Executable or unreadable configuration was not run or partially imported."
        ));
    }
    let unspecified = declaration["declaredUnspecifiedModNames"]
        .as_array()
        .ok_or("DST static Mod analysis did not return complete declarations.")?;
    if !unspecified.is_empty() {
        return Err(format!(
            "Cannot automatically recover {shard} Mods: every Mod must declare enabled as true or false."
        ));
    }
    let mut ids = BTreeSet::new();
    for name in declaration["declaredEnabledModNames"]
        .as_array()
        .ok_or("DST static Mod analysis did not return enabled declarations.")?
    {
        let name = name.as_str().ok_or("DST enabled Mod name must be text.")?;
        let Some(id) = name.strip_prefix("workshop-") else {
            return Err(format!(
                "Cannot automatically download local Mod {name} required by {shard}. Restore that Mod's files and configuration before importing this world."
            ));
        };
        ids.insert(canonical_workshop_id(id)?);
    }
    Ok(ids)
}

fn workshop_list(value: Option<&Value>) -> Result<BTreeSet<String>, String> {
    let entries = match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(text)) => text
            .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
            .filter(|entry| !entry.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "DST Workshop list must contain text IDs.".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("DST Workshop list must contain text IDs.".to_owned()),
    };
    entries
        .into_iter()
        .map(|id| canonical_workshop_id(&id))
        .collect()
}

fn canonical_workshop_id(id: &str) -> Result<String, String> {
    let canonical = id
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .map(|id| id.to_string());
    if canonical.as_deref() != Some(id) {
        return Err(format!(
            "Cannot automatically download invalid Workshop Mod ID {id}."
        ));
    }
    Ok(id.to_owned())
}

fn decode_source(path: &Path, bytes: Vec<u8>) -> Result<String, String> {
    let source = String::from_utf8(bytes).map_err(|_| {
        format!(
            "DST Mod configuration {} must contain UTF-8 Lua data.",
            path.display()
        )
    })?;
    Ok(source.trim_start_matches('\u{feff}').to_owned())
}

fn read_optional_bytes(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Cannot inspect DST Mod configuration {}: {error}",
                path.display()
            ));
        }
    };
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(format!(
            "DST Mod configuration {} must be a plain file.",
            path.display()
        ));
    }
    if metadata.len() > limit as u64 {
        return Err(format!(
            "DST Mod configuration {} exceeds the {limit} byte recovery limit.",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| {
            format!(
                "Cannot open DST Mod configuration {}: {error}",
                path.display()
            )
        })?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            format!(
                "Cannot read DST Mod configuration {}: {error}",
                path.display()
            )
        })?;
    if bytes.len() > limit {
        return Err(format!(
            "DST Mod configuration {} changed beyond its recovery limit.",
            path.display()
        ));
    }
    Ok(Some(bytes))
}

#[cfg(test)]
#[path = "dst_import_mods_tests.rs"]
mod tests;
