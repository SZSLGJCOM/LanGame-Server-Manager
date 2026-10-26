use super::*;

const OPTIONAL_WORLD_SETTINGS: [(&str, &str); 3] = [
    ("backpack_cleanup_days", "BackpackCleanup"),
    ("weapon_durability_multiplier", "WeaponDurabilityMulti"),
    ("headshot_only_mode", "HeadshotOnlyMode"),
];

fn optional_native_value(settings: &Map<String, Value>, key: &str) -> Option<String> {
    match (key, settings.get(key)) {
        ("headshot_only_mode", Some(Value::String(value))) => match value.as_str() {
            "enabled" => Some(String::from("true")),
            "disabled" => Some(String::from("false")),
            _ => None,
        },
        (_, Some(Value::Number(value))) => Some(value.to_string()),
        _ => None,
    }
}

pub(super) fn render_optional_world_settings(settings: &Map<String, Value>) -> String {
    // Omitted lines stay unmanaged during INI reconciliation. Clearing an override
    // therefore keeps the last native value rather than resetting an existing world.
    OPTIONAL_WORLD_SETTINGS
        .iter()
        .filter_map(|(key, native)| {
            optional_native_value(settings, key).map(|value| format!("{native}={value}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn validate_optional_world_settings(
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    let mut in_world_settings = true; // settings_extra follows the World Settings section.
    let mut seen = [false; OPTIONAL_WORLD_SETTINGS.len()];
    for line in parse_config_lines(settings, "settings_extra") {
        if line.starts_with(';') {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            in_world_settings = section.trim().eq_ignore_ascii_case("World Settings");
            continue;
        }
        if !in_world_settings {
            continue;
        }
        let Some((native_key, _)) = line.split_once('=') else {
            continue;
        };
        let Some((index, (key, native))) = OPTIONAL_WORLD_SETTINGS
            .iter()
            .enumerate()
            .find(|(_, (_, native))| native_key.trim().eq_ignore_ascii_case(native))
        else {
            continue;
        };
        let message = if optional_native_value(settings, key).is_some() {
            Some(format!(
                "{native} is also set by {key}; clear one of the two overrides before saving"
            ))
        } else if seen[index] {
            Some(format!(
                "{native} appears more than once in [World Settings]; keep a single assignment"
            ))
        } else {
            None
        };
        if let Some(message) = message {
            return Err(StorageError::InvalidModuleSetting {
                module_id: String::from("humanitz"),
                field: String::from("settings_extra"),
                message,
            });
        }
        seen[index] = true;
    }
    Ok(())
}
