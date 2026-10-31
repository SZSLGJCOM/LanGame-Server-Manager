use super::{InstanceDetails, ProbeResult, fs, inventory, settings_readback};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SHARDS: [(&str, &str); 4] = [
    ("master", "Master"),
    ("caves", "Caves"),
    ("islands", "Islands"),
    ("volcano", "Volcano"),
];

pub(super) fn capture_managed_ini(root: &Path) -> ProbeResult<BTreeMap<PathBuf, Vec<u8>>> {
    SHARDS
        .iter()
        .map(|(_, shard)| {
            let relative = Path::new(shard).join("server.ini");
            let bytes = fs::read(root.join(&relative))?;
            stopped_native_ini(&bytes)?;
            Ok((relative, bytes))
        })
        .collect()
}

fn stopped_native_ini(managed: &[u8]) -> ProbeResult<Vec<u8>> {
    let text = std::str::from_utf8(managed)?;
    if text.len() > 16 * 1024
        || text.contains('\r')
        || !text.starts_with("[NETWORK]\n")
        || !text.ends_with('\n')
        || text.matches("\n\n[").count() != 2
        || !text.contains("\n\n[SHARD]\n")
        || !text.contains("\n\n[STEAM]\n")
    {
        return Err(
            "DST managed INI baseline no longer matches the reviewed template shape".into(),
        );
    }
    // Build 26335402 captures retain every field byte: native stop uses CRLF
    // and adds one blank line at each of the two section boundaries. The live
    // ACCOUNT/encode_user_path section is deliberately not an accepted backup.
    Ok(text
        .replace("\n\n[", "\n\n\n[")
        .replace('\n', "\r\n")
        .into_bytes())
}

pub(super) fn is_native_save_file(relative: &Path) -> bool {
    (relative.starts_with("Master/save/session") || relative.starts_with("Caves/save/session"))
        && relative
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()))
}

/// This fixture has two saved worlds. Cluster-level files and the three
/// manager-owned shard configuration files are retained/rematerialized, while
/// native leveldataoverride.lua belongs to the copied world even when inactive.
pub(super) fn copied_world_data(files: &BTreeMap<PathBuf, (u64, u64)>) -> (usize, u64) {
    files
        .iter()
        .filter(|(path, _)| {
            let mut parts = path.iter();
            let shard = parts.next().and_then(|part| part.to_str());
            let name = parts.next().and_then(|part| part.to_str());
            let root_file = parts.next().is_none();
            match shard {
                Some("Master" | "Caves") => {
                    !(root_file
                        && matches!(
                            name,
                            Some("server.ini" | "worldgenoverride.lua" | "modoverrides.lua")
                        ))
                }
                Some("Islands" | "Volcano") => root_file && name == Some("leveldataoverride.lua"),
                _ => false,
            }
        })
        .fold((0, 0), |(count, bytes), (_, (length, _))| {
            (count + 1, bytes + length)
        })
}

fn inventory_difference(
    expected: &BTreeMap<PathBuf, (u64, u64)>,
    actual: &BTreeMap<PathBuf, (u64, u64)>,
) -> String {
    expected
        .keys()
        .chain(actual.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|path| expected.get(*path) != actual.get(*path))
        .take(8)
        .map(|path| {
            format!(
                "{path:?}: expected_bytes={:?}, actual_bytes={:?}",
                expected.get(path).map(|entry| entry.0),
                actual.get(path).map(|entry| entry.0)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn configuration_difference(expected: &Value, actual: &Value) -> String {
    let mut changed = Vec::new();
    let mut pending = vec![(String::new(), expected, actual)];
    while let Some((path, left, right)) = pending.pop() {
        if left == right {
            continue;
        }
        if let (Some(left), Some(right)) = (left.as_object(), right.as_object()) {
            let keys = left
                .keys()
                .chain(right.keys())
                .collect::<std::collections::BTreeSet<_>>();
            for key in keys.into_iter().rev() {
                pending.push((
                    format!("{path}/{key}"),
                    left.get(key).unwrap_or(&Value::Null),
                    right.get(key).unwrap_or(&Value::Null),
                ));
            }
        } else {
            changed.push(format!("{path:?}"));
            if changed.len() == 8 {
                break;
            }
        }
    }
    changed.join(", ")
}

/// The native probe uses a standard Master/Caves cluster without operator Mods.
/// DST restores canonical Lua from the backup and rematerializes configuration;
/// only those exact predictions may differ from stopped backup bytes.
pub(super) struct NoModsRestore {
    configuration: Value,
    world_options: BTreeMap<PathBuf, Vec<u8>>,
    managed_ini: BTreeMap<PathBuf, Vec<u8>>,
}

impl NoModsRestore {
    pub(super) fn capture(
        backup: &Path,
        configuration: &[u8],
        templates: &Path,
        managed_ini: &BTreeMap<PathBuf, Vec<u8>>,
    ) -> ProbeResult<Self> {
        if managed_ini.len() != SHARDS.len() {
            return Err("DST native restore requires exactly four managed INI baselines".into());
        }
        let mut configuration: Value = serde_json::from_slice(configuration)?;
        let settings = configuration["settings"]
            .as_object_mut()
            .ok_or("DST native restore requires canonical settings")?;
        if settings.get("shard_layout").and_then(Value::as_str) != Some("standard")
            || settings.get("enable_caves").and_then(Value::as_bool) != Some(true)
            || settings
                .get("shared_workshop_mod_ids")
                .and_then(Value::as_str)
                != Some("")
        {
            return Err(
                "DST native restore prediction requires the no-Mods Master/Caves fixture".into(),
            );
        }
        let mut world_options = BTreeMap::new();
        for (key, directory) in SHARDS {
            let ini_path = Path::new(directory).join("server.ini");
            let baseline = managed_ini
                .get(&ini_path)
                .ok_or("DST native restore is missing a managed shard INI baseline")?;
            let stopped = stopped_native_ini(baseline)?;
            let saved_ini = fs::read(backup.join(&ini_path))?;
            let native_form = matches!(directory, "Master" | "Caves") && saved_ini == stopped;
            if saved_ini != *baseline && !native_form {
                return Err(format!(
                    "DST backup INI differs from its managed baseline and exact stopped-native serialization at {ini_path:?}: managed_bytes={}, native_bytes={}, actual_bytes={}",
                    baseline.len(), stopped.len(), saved_ini.len()
                ).into());
            }
            let mods = fs::read_to_string(backup.join(directory).join("modoverrides.lua"))?;
            let raw_mod_key = format!("{key}_modoverrides_lua");
            let current_mods = settings
                .get(&raw_mod_key)
                .and_then(Value::as_str)
                .ok_or("DST native restore requires explicit raw Mod defaults")?;
            if mods.replace("\r\n", "\n").trim() != "return {\n}"
                || current_mods.replace("\r\n", "\n").trim() != "return {\n}"
                || settings
                    .get(&format!("{key}_enabled_workshop_mod_ids"))
                    .and_then(Value::as_str)
                    != Some("")
                || !settings
                    .get(&format!("{key}_mod_configuration_options"))
                    .and_then(Value::as_object)
                    .is_some_and(|options| options.is_empty())
            {
                return Err("DST native restore prediction does not accept operator Mods".into());
            }
            // The existing import plan stores the complete Mod file as raw
            // canonical text; empty structured defaults remain unchanged.
            settings.insert(raw_mod_key, Value::String(mods));
            let relative = Path::new(directory).join("worldgenoverride.lua");
            let raw = fs::read_to_string(backup.join(&relative))?;
            let raw = raw.trim_start_matches('\u{feff}');
            let body = raw.strip_prefix("KLEI     1 ").unwrap_or(raw);
            if !body.starts_with("return {") || body.len() > 120 * 1024 {
                return Err(
                    "DST native restore prediction requires a bounded native return table".into(),
                );
            }
            settings.insert(
                format!("{key}_worldgenoverride_lua"),
                Value::String(body.into()),
            );
            // Check the actual template contract before predicting its output.
            // The body is not replaced with the expanded pre-start defaults.
            let template =
                fs::read_to_string(templates.join(directory).join("worldgenoverride.lua.hbs"))?;
            if template != format!("{{{{dst.{key}_worldgenoverride}}}}\n") {
                return Err(
                    "DST native restore world template changed; review the byte prediction".into(),
                );
            }
            let mut rendered = body.replace("\r\n", "\n").replace('\r', "\n");
            if !rendered.ends_with('\n') {
                rendered.push('\n');
            }
            rendered.push('\n');
            world_options.insert(relative, rendered.into_bytes());
        }
        Ok(Self {
            configuration,
            world_options,
            managed_ini: managed_ini.clone(),
        })
    }

    pub(super) fn verify_saves(
        &self,
        root: &Path,
        original: &BTreeMap<PathBuf, (u64, u64)>,
    ) -> ProbeResult<()> {
        let mut expected = original.clone();
        let mut actual = inventory::save_inventory(root)?;
        for (relative, bytes) in self.world_options.iter().chain(&self.managed_ini) {
            if expected.remove(relative).is_none()
                || actual.remove(relative).is_none()
                || fs::read(root.join(relative))? != *bytes
            {
                return Err(format!(
                    "DST restore did not render the exact expected configuration at {relative:?}: expected_bytes={}, actual_bytes={:?}",
                    bytes.len(), fs::metadata(root.join(relative)).ok().map(|metadata| metadata.len())
                ).into());
            }
        }
        // Includes every native save byte, leveldataoverride.lua, Mod file
        // and other cluster data; new or missing files also fail.
        if actual != expected {
            return Err(format!(
                "DST restore changed native save bytes or unrelated cluster files: {}",
                inventory_difference(&expected, &actual)
            )
            .into());
        }
        Ok(())
    }

    pub(super) fn verify_configuration(
        &self,
        bytes: &[u8],
        started: SystemTime,
        finished: SystemTime,
    ) -> ProbeResult<()> {
        let mut actual: Value = serde_json::from_slice(bytes)?;
        let generated = actual["generated_at_unix_ms"]
            .as_u64()
            .ok_or("DST restored configuration lacks its generation timestamp")?;
        if u128::from(generated) < started.duration_since(UNIX_EPOCH)?.as_millis()
            || u128::from(generated) > finished.duration_since(UNIX_EPOCH)?.as_millis()
        {
            return Err("DST restored configuration timestamp is outside the restore call".into());
        }
        actual["generated_at_unix_ms"] = self.configuration["generated_at_unix_ms"].clone();
        if actual != self.configuration {
            return Err(format!(
                "DST restored canonical configuration differs from the exact backup prediction: {}",
                configuration_difference(&self.configuration, &actual)
            )
            .into());
        }
        Ok(())
    }

    pub(super) fn verify_stopped_details(
        &self,
        before: &InstanceDetails,
        after: &InstanceDetails,
        program_root: &Path,
    ) -> ProbeResult<()> {
        let mut expected = before.clone();
        // Predict from the backup, never from the post-restore readback. Storage
        // exposes pretty JSON, while the unchanged verifier compares it exactly.
        expected.settings_json = serde_json::to_string_pretty(&self.configuration["settings"])?;
        settings_readback::verify_stopped(&expected, after, program_root)
    }
}

#[cfg(test)]
#[path = "commands_native_dst_restore_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "commands_native_dst_restore_readback_tests.rs"]
mod readback_tests;
