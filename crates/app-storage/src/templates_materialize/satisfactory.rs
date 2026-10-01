use std::collections::HashSet;
use std::io::{ErrorKind, Read};

use super::managed_config_merge::{materialization_error, merge_ini_documents, parse_ini_document};
use super::*;

const USER_SETTINGS_FILE: &str = "GameUserSettings.ini";
const USER_SETTINGS_SECTION: &str = "/Script/FactoryGame.FGGameUserSettings";
const MAX_USER_SETTINGS_BYTES: usize = 256 * 1024;
const MAX_INT_OPTIONS: usize = 4096;

pub(super) fn materialize_satisfactory_support_files(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }

    let data_root = context
        .storage_paths
        .instances_root
        .join(context.instance_id)
        .join("data");
    let target_config_root = data_root.join("Saved/Config/WindowsServer");
    // Validate the native option map before any support file is changed. All
    // writes then share the caller's conflict checks and rollback transaction.
    let options_plan = plan_user_settings(
        &target_config_root.join(USER_SETTINGS_FILE),
        context.settings,
    )?;

    for directory in ["profile/AppData/Local", "profile/AppData/Roaming"] {
        let path = data_root.join(directory);
        fs::create_dir_all(&path).map_err(|source| StorageError::CreatePath { path, source })?;
    }
    let engine_source = context.config_dir.join(SATISFACTORY_ENGINE_INI_FILE);
    let engine_destination = target_config_root.join(SATISFACTORY_ENGINE_INI_FILE);
    let game_source = context.config_dir.join(SATISFACTORY_GAME_INI_FILE);
    let game_destination = target_config_root.join(SATISFACTORY_GAME_INI_FILE);
    merge_rendered_ini_files(
        &[
            ManagedIniFile {
                source_path: &engine_source,
                destination_path: &engine_destination,
                removed_sections: &[],
            },
            ManagedIniFile {
                source_path: &game_source,
                destination_path: &game_destination,
                removed_sections: &[],
            },
        ],
        "satisfactory",
        files,
    )?;
    if let Some(plan) = options_plan {
        files.apply(vec![plan])?;
    }
    Ok(())
}

fn plan_user_settings(
    path: &Path,
    settings: &Map<String, Value>,
) -> Result<Option<ManagedConfigMergePlan>, StorageError> {
    let mut updates = Vec::new();
    for (setting, native_key) in [
        ("auto_pause_when_empty", "FG.DSAutoPause"),
        ("network_quality", "FG.NetworkQuality"),
        ("weather_preset", "FG.WeatherPreset"),
        ("send_gameplay_data", "FG.SendGameplayData"),
    ] {
        let Some(value) = settings.get(setting) else {
            continue;
        };
        let value = match setting {
            "network_quality" => value.as_i64().filter(|value| (0..=3).contains(value)),
            "weather_preset" => value.as_i64().filter(|value| (0..=6).contains(value)),
            _ => value.as_bool().map(i64::from),
        }
        .ok_or_else(|| {
            materialization_error(
                "satisfactory",
                path,
                format!("invalid explicit value for {setting}"),
            )
        })?;
        updates.push((native_key, value as i32));
    }
    // Only explicit overrides own native entries; other edits preserve them.
    if updates.is_empty() {
        return Ok(None);
    }

    let original = read_user_settings(path)?;
    let existing = std::str::from_utf8(original.as_deref().unwrap_or_default()).map_err(|_| {
        materialization_error(
            "satisfactory",
            path,
            "refusing to replace non-UTF-8 existing INI".to_owned(),
        )
    })?;
    let merged = merge_user_settings(existing, &updates)
        .map_err(|message| materialization_error("satisfactory", path, message))?;
    if merged.len() > MAX_USER_SETTINGS_BYTES {
        return Err(materialization_error(
            "satisfactory",
            path,
            "merged GameUserSettings.ini exceeds the configuration size limit".to_owned(),
        ));
    }
    Ok(Some(ManagedConfigMergePlan {
        destination_path: path.to_owned(),
        replacement: merged.into_bytes(),
        original,
    }))
}

fn read_user_settings(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StorageError::ReadConfig {
                path: path.to_owned(),
                source,
            });
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_USER_SETTINGS_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadConfig {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() > MAX_USER_SETTINGS_BYTES {
        return Err(materialization_error(
            "satisfactory",
            path,
            "GameUserSettings.ini exceeds the configuration size limit".to_owned(),
        ));
    }
    Ok(Some(bytes))
}

fn merge_user_settings(existing: &str, updates: &[(&str, i32)]) -> Result<String, String> {
    let body = existing.strip_prefix('\u{feff}').unwrap_or(existing);
    // The shared INI parser and merger differ in their handling of interior
    // BOMs. Reject ambiguous input rather than leave a second option map behind.
    if body.contains('\u{feff}') {
        return Err("unexpected byte-order mark inside GameUserSettings.ini".to_owned());
    }
    let document = parse_ini_document(existing)
        .map_err(|message| format!("refusing to replace malformed existing INI: {message}"))?;
    let mut in_settings = false;
    let mut seen_section = false;
    let mut native_map = None;
    for line in body.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            in_settings = line[1..line.len() - 1]
                .trim()
                .eq_ignore_ascii_case(USER_SETTINGS_SECTION);
            if in_settings && std::mem::replace(&mut seen_section, true) {
                return Err("duplicate FGGameUserSettings sections are ambiguous".to_owned());
            }
            continue;
        }
        if !in_settings || line.starts_with([';', '#']) || line.starts_with("//") {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let base_key = key.trim_start_matches(['+', '-', '.', '!']).trim();
        if base_key.eq_ignore_ascii_case("mIntValues") {
            if key != base_key || native_map.replace(value).is_some() {
                return Err("duplicate or operator-prefixed mIntValues are ambiguous".to_owned());
            }
        } else if base_key
            .split_once('[')
            .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case("mIntValues"))
        {
            return Err("indexed mIntValues assignments are ambiguous".to_owned());
        }
    }
    let mut entries = parse_int_map(native_map.unwrap_or("()"))?;
    for &(name, value) in updates {
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
        {
            entry.value = value;
        } else {
            if entries.len() == MAX_INT_OPTIONS {
                return Err("native integer option map exceeds the entry limit".to_owned());
            }
            entries.push(IntOption {
                name: name.to_owned(),
                quoted_name: format!("\"{name}\""),
                value,
            });
        }
    }
    let pairs = entries
        .iter()
        .map(|entry| format!("({}, {})", entry.quoted_name, entry.value))
        .collect::<Vec<_>>()
        .join(",");
    let rendered = format!("[{USER_SETTINGS_SECTION}]\nmIntValues=({pairs})\n");
    let rendered_document = parse_ini_document(&rendered)?;
    Ok(merge_ini_documents(document, rendered_document, &[]))
}

struct IntOption {
    name: String,
    quoted_name: String,
    value: i32,
}

fn parse_int_map(mut input: &str) -> Result<Vec<IntOption>, String> {
    const INVALID: &str = "malformed native integer option map";
    fn consume(input: &mut &str, expected: char) -> Result<(), String> {
        *input = input.trim_start().strip_prefix(expected).ok_or(INVALID)?;
        Ok(())
    }
    fn quoted_name(input: &mut &str) -> Result<(String, String), String> {
        *input = input.trim_start();
        let original = *input;
        consume(input, '"')?;
        let mut name = String::new();
        let mut escaped = false;
        for (index, ch) in input.char_indices() {
            if ch.is_control() {
                return Err(INVALID.to_owned());
            }
            if escaped {
                if !matches!(ch, '\\' | '"') {
                    return Err(INVALID.to_owned());
                }
                name.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                if name.is_empty() {
                    return Err(INVALID.to_owned());
                }
                *input = &input[index + 1..];
                return Ok((name, original[..index + 2].to_owned()));
            } else {
                name.push(ch);
            }
        }
        Err(INVALID.to_owned())
    }

    consume(&mut input, '(')?;
    let mut entries = Vec::new();
    let mut names = HashSet::new();
    if input.trim() == ")" {
        return Ok(entries);
    }
    loop {
        if entries.len() == MAX_INT_OPTIONS {
            return Err("native integer option map exceeds the entry limit".to_owned());
        }
        consume(&mut input, '(')?;
        let (name, quoted_name) = quoted_name(&mut input)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err("duplicate native integer option names are ambiguous".to_owned());
        }
        consume(&mut input, ',')?;
        let end = input.find(')').ok_or(INVALID)?;
        let literal = input[..end].trim();
        let digits = literal.strip_prefix(['-', '+']).unwrap_or(literal);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(INVALID.to_owned());
        }
        let value = literal.parse::<i32>().map_err(|_| INVALID.to_owned())?;
        input = &input[end..];
        consume(&mut input, ')')?;
        entries.push(IntOption {
            name,
            quoted_name,
            value,
        });
        input = input.trim_start();
        if input.trim() == ")" {
            return Ok(entries);
        }
        consume(&mut input, ',')?;
    }
}

#[cfg(test)]
#[path = "satisfactory_tests.rs"]
mod tests;
