use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;

use serde_json::{Map, Value};

use super::{ManagedConfigMutation, ModuleSupportMaterializationContext, StorageError};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

fn failure(path: &Path, message: impl ToString) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: String::from("necesse"),
        path: path.to_owned(),
        message: message.to_string(),
    }
}

pub(super) fn materialize_server_settings(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let root = super::instance_root_from_config_dir(context.config_dir);
    let data = root.join("data");
    let cfg = data.join("cfg");
    let path = cfg.join("server.cfg");
    for candidate in [&data, &cfg, &path] {
        match fs::symlink_metadata(candidate) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || crate::private_runtime::is_reparse_point(candidate)?
                {
                    return Err(failure(
                        candidate,
                        "Necesse settings must stay in regular instance-owned files and directories",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failure(candidate, error)),
        }
    }
    let existing = match fs::File::open(&path) {
        Ok(file) => {
            let mut content = String::new();
            file.take(MAX_CONFIG_BYTES + 1)
                .read_to_string(&mut content)
                .map_err(|error| failure(&path, error))?;
            if content.len() as u64 > MAX_CONFIG_BYTES {
                return Err(failure(
                    &path,
                    "Necesse server.cfg exceeds the 1 MiB configuration limit",
                ));
            }
            content
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(failure(&path, error)),
    };
    let rendered = render_necesse_server_settings(&existing, context.settings)
        .map_err(|message| failure(&path, message))?;
    if rendered != existing {
        files.write(&path, rendered.as_bytes())?;
    }
    Ok(())
}

fn render_necesse_server_settings(
    existing: &str,
    settings: &Map<String, Value>,
) -> Result<String, String> {
    let mappings = [
        ("max_client_latency_seconds", "maxClientLatencySeconds"),
        ("unload_levels_cooldown", "unloadLevelsCooldown"),
        ("dropped_items_life_minutes", "droppedItemsLifeMinutes"),
        ("unload_settlements", "unloadSettlements"),
        ("max_settlements_per_player", "maxSettlementsPerPlayer"),
        ("max_settlers_per_settlement", "maxSettlersPerSettlement"),
        ("world_border_size", "worldBorderSize"),
    ];
    let mut overrides = BTreeMap::new();
    for (key, native) in mappings {
        let Some(value) = settings.get(key).filter(|value| !value.is_null()) else {
            continue;
        };
        let rendered = if key == "unload_settlements" {
            value.as_bool().map(|value| value.to_string())
        } else {
            value
                .as_i64()
                .filter(|value| i32::try_from(*value).is_ok())
                .map(|value| value.to_string())
        }
        .ok_or_else(|| format!("Invalid Necesse setting type: {key}"))?;
        overrides.insert(native, rendered);
    }
    // An absent override never changes existing native settings, including values
    // written by the game. A new empty SERVER section lets the game use defaults.
    if overrides.is_empty() && !existing.trim().is_empty() {
        return Ok(existing.to_owned());
    }
    let content = if existing.trim().is_empty() {
        "SERVER = {\n}\n"
    } else {
        existing
    };
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut depth = 0_i32;
    let mut server_found = false;
    let mut server_open = false;
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if depth == 0
            && trimmed.split_once('=').is_some_and(|(key, value)| {
                key.trim() == "SERVER" && value.trim_start().starts_with('{')
            })
        {
            if server_found {
                return Err(String::from(
                    "Multiple SERVER sections in Necesse server.cfg",
                ));
            }
            server_found = true;
            server_open = true;
        }
        if server_open && depth == 1 && trimmed.starts_with('}') {
            for (key, value) in &overrides {
                if !seen.contains(key) {
                    output.push(format!("\t{key} = {value},"));
                }
            }
            server_open = false;
        }
        let mut replacement = None;
        if server_open
            && depth == 1
            && let Some((key, _)) = trimmed.split_once('=')
        {
            let key = key.trim();
            if let Some(value) = overrides.get(key) {
                if !seen.insert(key) {
                    return Err(format!("Duplicate Necesse setting: {key}"));
                }
                let indent = &line[..line.len() - line.trim_start().len()];
                let comment = line
                    .split_once("//")
                    .map(|(_, value)| format!(" //{value}"))
                    .unwrap_or_default();
                replacement = Some(format!("{indent}{key} = {value},{comment}"));
            }
        }
        depth += brace_delta(line)?;
        if depth < 0 {
            return Err(String::from("Invalid braces in Necesse server.cfg"));
        }
        output.push(replacement.unwrap_or_else(|| line.to_owned()));
    }
    if !server_found || server_open || depth != 0 {
        return Err(String::from(
            "Necesse server.cfg requires a complete SERVER section",
        ));
    }
    Ok(output.join(newline) + newline)
}

fn brace_delta(line: &str) -> Result<i32, String> {
    let mut quoted = false;
    let mut escaped = false;
    let mut delta = 0;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else {
            match ch {
                '/' if chars.peek() == Some(&'/') => break,
                '"' => quoted = true,
                '{' => delta += 1,
                '}' => delta -= 1,
                _ => {}
            }
        }
    }
    if quoted {
        Err(String::from("Unterminated string in Necesse server.cfg"))
    } else {
        Ok(delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_overrides_preserve_existing_native_values_byte_for_byte() {
        let source = "SERVER = {\r\n\tmaxClientLatencySeconds = 61, // custom\r\n}\r\n";
        assert_eq!(
            render_necesse_server_settings(source, &Map::new()).unwrap(),
            source
        );
    }

    #[test]
    fn explicit_overrides_preserve_secrets_comments_unknown_and_nested_keys() {
        let source = "SERVER = {\n password = \"fixture-password\",\n motd = \"keep{braces}//text\",\n maxClientLatencySeconds = 61, // keep comment\n EXTRA = {\n maxClientLatencySeconds = 900,\n }\n future = 7,\n}\n";
        let settings = json!({"max_client_latency_seconds": 45, "unload_settlements": true, "world_border_size": 600}).as_object().unwrap().clone();
        let actual = render_necesse_server_settings(source, &settings).unwrap();
        assert!(actual.contains("maxClientLatencySeconds = 45, // keep comment"));
        assert!(actual.contains("maxClientLatencySeconds = 900,"));
        assert!(actual.contains("password = \"fixture-password\","));
        assert!(actual.contains("motd = \"keep{braces}//text\","));
        assert!(actual.contains("future = 7,"));
        assert!(actual.contains("unloadSettlements = true,"));
        assert!(actual.contains("worldBorderSize = 600,"));
    }

    #[test]
    fn malformed_or_duplicate_managed_options_are_rejected_before_writing() {
        let settings = json!({"max_client_latency_seconds": 45})
            .as_object()
            .unwrap()
            .clone();
        for source in [
            "SERVER = {\n",
            "SERVER = {\n maxClientLatencySeconds=1,\n maxClientLatencySeconds=2,\n}\n",
            "OTHER = {\n}\n",
        ] {
            assert!(render_necesse_server_settings(source, &settings).is_err());
        }
    }

    #[test]
    fn clean_settings_file_uses_only_explicit_overrides() {
        let settings = json!({"max_settlements_per_player": 3, "unload_levels_cooldown": 40})
            .as_object()
            .unwrap()
            .clone();
        let actual = render_necesse_server_settings("", &settings).unwrap();
        assert!(actual.starts_with("SERVER = {\n"));
        assert!(actual.contains("maxSettlementsPerPlayer = 3,"));
        assert!(actual.contains("unloadLevelsCooldown = 40,"));
        assert!(!actual.contains("password"));
    }
}
