use super::*;
use crate::player_access_normalization::{
    normalize_ark_account_id, normalize_minecraft_banned_ip, normalize_minecraft_player_name,
    normalize_minecraft_uuid, normalize_terraria_banlist_entry, normalize_valheim_platform_id,
};

pub(super) fn render_minecraft_server_ip(settings: &Map<String, Value>) -> String {
    let Some(value) = lookup_materialized_setting_text(settings, "bind_ip") else {
        return String::new();
    };
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "0.0.0.0" || trimmed == "::" || trimmed == "[::]" {
        String::new()
    } else {
        String::from(trimmed)
    }
}

pub(super) fn split_delimited_entry(line: &str, delimiter: char, fields: usize) -> Vec<String> {
    line.splitn(fields, delimiter)
        .map(str::trim)
        .map(String::from)
        .collect()
}

pub(super) fn normalize_minecraft_reason(value: Option<&String>, default: &str) -> String {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(String::from)
        .unwrap_or_else(|| String::from(default))
}

pub(super) fn render_minecraft_ops_json(settings: &Map<String, Value>) -> String {
    let mut seen = HashSet::new();
    let entries = parse_config_lines(settings, "operator_entries")
        .into_iter()
        .filter_map(|line| {
            let parts = split_delimited_entry(&line, ',', 4);
            if parts.len() < 2 {
                return None;
            }

            let uuid = normalize_minecraft_uuid(&parts[0])?;
            let name = normalize_minecraft_player_name(&parts[1])?;
            if !seen.insert(uuid.clone()) {
                return None;
            }

            let level = parts
                .get(2)
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(4)
                .clamp(1, 4);
            let bypasses = parts
                .get(3)
                .and_then(|value| parse_loose_bool(value))
                .unwrap_or(false);

            Some(serde_json::json!({
                "uuid": uuid,
                "name": name,
                "level": level,
                "bypassesPlayerLimit": bypasses
            }))
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&entries).unwrap_or_else(|_| String::from("[]"))
}

pub(super) fn render_minecraft_named_uuid_json(settings: &Map<String, Value>, key: &str) -> String {
    let mut seen = HashSet::new();
    let entries = parse_config_lines(settings, key)
        .into_iter()
        .filter_map(|line| {
            let parts = split_delimited_entry(&line, ',', 2);
            if parts.len() < 2 {
                return None;
            }

            let uuid = normalize_minecraft_uuid(&parts[0])?;
            let name = normalize_minecraft_player_name(&parts[1])?;
            if !seen.insert(uuid.clone()) {
                return None;
            }

            Some(serde_json::json!({
                "uuid": uuid,
                "name": name
            }))
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&entries).unwrap_or_else(|_| String::from("[]"))
}

pub(super) fn render_minecraft_banned_players_json(settings: &Map<String, Value>) -> String {
    let mut seen = HashSet::new();
    let entries = parse_config_lines(settings, "banned_player_entries")
        .into_iter()
        .filter_map(|line| {
            let parts = split_delimited_entry(&line, ',', 3);
            if parts.len() < 2 {
                return None;
            }

            let uuid = normalize_minecraft_uuid(&parts[0])?;
            let name = normalize_minecraft_player_name(&parts[1])?;
            if !seen.insert(uuid.clone()) {
                return None;
            }
            let reason = normalize_minecraft_reason(parts.get(2), "Banned by server operator");

            Some(serde_json::json!({
                "uuid": uuid,
                "name": name,
                "created": "",
                "source": "LanGame",
                "expires": "forever",
                "reason": reason
            }))
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&entries).unwrap_or_else(|_| String::from("[]"))
}

pub(super) fn render_minecraft_banned_ips_json(settings: &Map<String, Value>) -> String {
    let mut seen = HashSet::new();
    let entries = parse_config_lines(settings, "banned_ip_entries")
        .into_iter()
        .filter_map(|line| {
            let parts = split_delimited_entry(&line, ',', 2);
            if parts.is_empty() {
                return None;
            }

            let address = normalize_minecraft_banned_ip(&parts[0])?;
            if !seen.insert(address.clone()) {
                return None;
            }
            let reason = normalize_minecraft_reason(parts.get(1), "Banned by server operator");

            Some(serde_json::json!({
                "ip": address,
                "created": "",
                "source": "LanGame",
                "expires": "forever",
                "reason": reason
            }))
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&entries).unwrap_or_else(|_| String::from("[]"))
}

pub(super) fn render_rust_user_lines(
    settings: &Map<String, Value>,
    key: &str,
    command: &str,
) -> String {
    let mut seen = HashSet::new();
    parse_config_lines(settings, key)
        .into_iter()
        .filter_map(|line| {
            let parts = split_delimited_entry(&line, '|', 3);
            let steam_id = parts.first().and_then(|part| normalize_steam64_id(part))?;
            if !seen.insert(steam_id.clone()) {
                return None;
            }
            let name = parts.get(1).map(String::as_str).unwrap_or("");
            let reason = parts.get(2).map(String::as_str).unwrap_or("");
            Some(format!(
                "{command} {} \"{}\" \"{}\"",
                steam_id,
                escape_source_quoted(name),
                escape_source_quoted(reason)
            ))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn render_rust_ban_lines(settings: &Map<String, Value>) -> String {
    let mut seen = HashSet::new();
    parse_config_lines(settings, "banned_entries")
        .into_iter()
        .filter_map(|line| {
            let parts = split_delimited_entry(&line, '|', 2);
            let steam_id = parts.first().and_then(|part| normalize_steam64_id(part))?;
            if !seen.insert(steam_id.clone()) {
                return None;
            }
            let reason = parts
                .get(1)
                .map(String::as_str)
                .unwrap_or("Banned by server operator");
            Some(format!(
                "banid {} \"LanGame\" \"{}\"",
                steam_id,
                escape_source_quoted(reason)
            ))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn render_rust_skip_queue_lines(settings: &Map<String, Value>) -> String {
    let mut seen = HashSet::new();
    parse_config_lines(settings, "skip_queue_entries")
        .into_iter()
        .filter_map(|line| {
            let parts = split_delimited_entry(&line, '|', 3);
            let steam_id = parts.first().and_then(|part| normalize_steam64_id(part))?;
            if !seen.insert(steam_id.clone()) {
                return None;
            }

            let name = parts.get(1).map(String::as_str).unwrap_or("").trim();
            let note = parts.get(2).map(String::as_str).unwrap_or("").trim();

            if name.is_empty() {
                Some(format!("global.skipqueueid {steam_id}"))
            } else if note.is_empty() {
                Some(format!(
                    "global.skipqueueid {steam_id} \"{}\"",
                    escape_source_quoted(name)
                ))
            } else {
                Some(format!(
                    "global.skipqueueid {steam_id} \"{}\" \"{}\"",
                    escape_source_quoted(name),
                    escape_source_quoted(note)
                ))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn rust_setting_text(settings: &Map<String, Value>, key: &str) -> Option<String> {
    lookup_materialized_setting_text(settings, key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn render_rust_optional_setting_line(
    settings: &Map<String, Value>,
    key: &str,
    command: &str,
) -> String {
    let Some(value) = rust_setting_text(settings, key) else {
        return String::new();
    };
    format!("{command} {value}")
}

pub(super) fn render_rust_optional_quoted_setting_line(
    settings: &Map<String, Value>,
    key: &str,
    command: &str,
) -> String {
    let Some(value) = rust_setting_text(settings, key) else {
        return String::new();
    };
    format!("{command} \"{}\"", escape_source_quoted(&value))
}

pub(super) fn rust_has_level_url(settings: &Map<String, Value>) -> bool {
    rust_setting_text(settings, "level_url").is_some()
}

pub(super) fn render_rust_seed_line(settings: &Map<String, Value>) -> String {
    if rust_has_level_url(settings) {
        String::new()
    } else {
        render_rust_optional_setting_line(settings, "seed", "server.seed")
    }
}

pub(super) fn render_rust_world_size_line(settings: &Map<String, Value>) -> String {
    if rust_has_level_url(settings) {
        String::new()
    } else {
        render_rust_optional_setting_line(settings, "world_size", "server.worldsize")
    }
}

pub(super) fn render_rust_wipe_unix_override_line(settings: &Map<String, Value>) -> String {
    let Some(value) = rust_setting_text(settings, "wipe_unix_timestamp_override") else {
        return String::new();
    };

    match value.parse::<i64>() {
        Ok(parsed) if parsed > 0 => format!("wipetimer.wipeUnixTimestampOverride {parsed}"),
        _ => String::new(),
    }
}

pub(super) fn render_unturned_optional_line(
    settings: &Map<String, Value>,
    key: &str,
    command: &str,
) -> String {
    let Some(value) = lookup_materialized_setting_text(settings, key) else {
        return String::new();
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{command} {trimmed}")
    }
}

pub(super) fn render_unturned_owner_line(settings: &Map<String, Value>) -> String {
    let Some(raw) = lookup_materialized_setting_text(settings, "owner_steam_id") else {
        return String::new();
    };

    normalize_steam64_id(&raw)
        .map(|steam_id| format!("Owner {steam_id}"))
        .unwrap_or_default()
}

pub(super) fn render_unturned_admin_lines(settings: &Map<String, Value>) -> String {
    parse_steam64_lines(settings, "admin_steam_ids")
        .into_iter()
        .map(|steam_id| format!("Admin {steam_id}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn render_unturned_config_text_value(
    settings: &Map<String, Value>,
    schema_defaults: &Map<String, Value>,
    key: &str,
    fallback_key: Option<&str>,
) -> String {
    let lookup = |setting_key: &str| {
        lookup_materialized_setting_text(settings, setting_key)
            .or_else(|| lookup_materialized_setting_text(schema_defaults, setting_key))
    };
    let value = lookup(key).unwrap_or_default();
    let resolved = if value.trim().is_empty() {
        fallback_key.and_then(lookup).unwrap_or_default()
    } else {
        value
    };

    let escaped = resolved
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

pub(super) fn render_unturned_enabled_line(
    settings: &Map<String, Value>,
    key: &str,
    command: &str,
) -> String {
    let value = if lookup_boolean_setting(settings, key) {
        "Enabled"
    } else {
        "Disabled"
    };
    format!("{command} {value}")
}

pub(super) fn render_valheim_platform_id_lines(settings: &Map<String, Value>, key: &str) -> String {
    let mut seen = HashSet::new();
    parse_config_lines(settings, key)
        .into_iter()
        .filter_map(|line| normalize_valheim_platform_id(&line))
        .filter(|line| seen.insert(line.to_ascii_lowercase()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn render_workshop_file_ids_json(settings: &Map<String, Value>, key: &str) -> String {
    let ids = parse_workshop_id_list(settings, key)
        .into_iter()
        .filter_map(|id| id.parse::<u64>().ok())
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&ids).unwrap_or_else(|_| String::from("[]"))
}

pub(super) fn render_conan_modlist_lines(pak_file_names: &[String]) -> String {
    let mut seen = HashSet::new();
    pak_file_names
        .iter()
        .filter_map(|name| normalize_conan_pak_file_name(name))
        .filter(|name| seen.insert(name.to_ascii_lowercase()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_conan_pak_file_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || !trimmed.to_ascii_lowercase().ends_with(".pak")
    {
        return None;
    }
    Some(String::from(trimmed))
}

pub(super) fn parse_config_lines(settings: &Map<String, Value>, key: &str) -> Vec<String> {
    let Some(raw) = lookup_materialized_setting_text(settings, key) else {
        return Vec::new();
    };

    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("//"))
        .map(String::from)
        .collect()
}

pub(super) fn normalize_steam64_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.len() == 17 && value.chars().all(|character| character.is_ascii_digit()) {
        Some(String::from(value))
    } else {
        None
    }
}

pub(super) fn parse_steam64_lines(settings: &Map<String, Value>, key: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    parse_config_lines(settings, key)
        .into_iter()
        .filter_map(|line| normalize_steam64_id(&line))
        .filter(|line| seen.insert(line.clone()))
        .collect()
}

pub(super) fn parse_steam64_values_from_text(raw: &str) -> Vec<String> {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut seen = HashSet::new();
    normalized
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("//"))
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(normalize_steam64_id)
        .filter(|entry| seen.insert(entry.clone()))
        .collect()
}

pub(super) fn parse_loose_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub(super) fn escape_source_quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn lookup_setting_array<'a>(
    settings: &'a Map<String, Value>,
    key: &str,
) -> Option<&'a Vec<Value>> {
    settings.get(key)?.as_array()
}

pub(super) fn escape_xml_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(super) fn normalize_sevendaystodie_steam_id(value: Option<&Value>) -> Option<String> {
    let text = value.and_then(|value| match value {
        Value::String(text) => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    })?;

    if (17..=20).contains(&text.len()) && text.chars().all(|character| character.is_ascii_digit()) {
        Some(text)
    } else {
        None
    }
}

fn normalize_sevendaystodie_user_identity(entry: &Map<String, Value>) -> Option<(String, String)> {
    let platform = entry.get("platform")?.as_str()?.trim();
    let platform = match platform.to_ascii_lowercase().as_str() {
        "steam" => "Steam",
        "eos" => "EOS",
        "xbl" => "XBL",
        "psn" => "PSN",
        _ => return None,
    };
    let userid = entry.get("userid")?.as_str()?.trim();
    if userid.is_empty()
        || userid.len() > 64
        || !userid
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
        || (platform == "Steam"
            && (userid.len() != 17 || !userid.bytes().all(|byte| byte.is_ascii_digit())))
    {
        return None;
    }

    Some((platform.to_string(), userid.to_string()))
}

pub(super) fn normalize_sevendaystodie_text(value: Option<&Value>, default: &str) -> String {
    let text = value
        .map(stringify_template_value)
        .unwrap_or_else(|| String::from(default));
    let trimmed = text.trim();

    if trimmed.is_empty() {
        escape_xml_attribute(default)
    } else {
        escape_xml_attribute(trimmed)
    }
}

pub(super) fn normalize_sevendaystodie_level(value: Option<&Value>, default: i64) -> i64 {
    let level = match value {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(default),
        Some(Value::String(text)) => text.trim().parse::<i64>().unwrap_or(default),
        Some(Value::Bool(boolean)) => {
            if *boolean {
                1
            } else {
                0
            }
        }
        _ => default,
    };

    level.clamp(0, 1000)
}

pub(super) fn render_sevendaystodie_admin_user_lines(settings: &Map<String, Value>) -> String {
    let Some(entries) = lookup_setting_array(settings, "admin_users") else {
        return String::new();
    };

    let mut seen = HashSet::new();
    let mut lines = Vec::new();

    for entry in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some((platform, userid)) = normalize_sevendaystodie_user_identity(entry) else {
            continue;
        };

        if seen.insert(format!(
            "{}:{}",
            platform.to_ascii_lowercase(),
            userid.to_ascii_lowercase()
        )) {
            let name = normalize_sevendaystodie_text(entry.get("name"), "");
            let permission_level = normalize_sevendaystodie_level(entry.get("permission_level"), 0);
            lines.push(format!(
                "    <user platform=\"{platform}\" userid=\"{userid}\" name=\"{name}\" permission_level=\"{permission_level}\" />"
            ));
        }
    }

    lines.join("\n")
}

pub(super) fn render_sevendaystodie_admin_group_lines(settings: &Map<String, Value>) -> String {
    let Some(entries) = lookup_setting_array(settings, "admin_groups") else {
        return String::new();
    };

    let mut seen = HashSet::new();
    let mut lines = Vec::new();

    for entry in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some(steam_id) = normalize_sevendaystodie_steam_id(entry.get("steam_id")) else {
            continue;
        };

        if seen.insert(steam_id.to_ascii_lowercase()) {
            let name = normalize_sevendaystodie_text(entry.get("name"), "");
            let permission_level_default =
                normalize_sevendaystodie_level(entry.get("permission_level_default"), 1000);
            let permission_level_mod =
                normalize_sevendaystodie_level(entry.get("permission_level_mod"), 0);
            lines.push(format!(
                "    <group steamID=\"{steam_id}\" name=\"{name}\" permission_level_default=\"{permission_level_default}\" permission_level_mod=\"{permission_level_mod}\" />"
            ));
        }
    }

    lines.join("\n")
}

pub(super) fn render_sevendaystodie_whitelist_user_lines(settings: &Map<String, Value>) -> String {
    let Some(entries) = lookup_setting_array(settings, "whitelist_users") else {
        return String::new();
    };

    let mut seen = HashSet::new();
    let mut lines = Vec::new();

    for entry in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some((platform, userid)) = normalize_sevendaystodie_user_identity(entry) else {
            continue;
        };

        if seen.insert(format!(
            "{}:{}",
            platform.to_ascii_lowercase(),
            userid.to_ascii_lowercase()
        )) {
            let name = normalize_sevendaystodie_text(entry.get("name"), "");
            lines.push(format!(
                "    <user platform=\"{platform}\" userid=\"{userid}\" name=\"{name}\" />"
            ));
        }
    }

    lines.join("\n")
}

pub(super) fn render_sevendaystodie_whitelist_group_lines(settings: &Map<String, Value>) -> String {
    let Some(entries) = lookup_setting_array(settings, "whitelist_groups") else {
        return String::new();
    };

    let mut seen = HashSet::new();
    let mut lines = Vec::new();

    for entry in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some(steam_id) = normalize_sevendaystodie_steam_id(entry.get("steam_id")) else {
            continue;
        };

        if seen.insert(steam_id.to_ascii_lowercase()) {
            let name = normalize_sevendaystodie_text(entry.get("name"), "");
            lines.push(format!(
                "    <group steamID=\"{steam_id}\" name=\"{name}\" />"
            ));
        }
    }

    lines.join("\n")
}

pub(super) fn render_sevendaystodie_blacklist_lines(settings: &Map<String, Value>) -> String {
    let Some(entries) = lookup_setting_array(settings, "blacklist_entries") else {
        return String::new();
    };

    let mut seen = HashSet::new();
    let mut lines = Vec::new();

    for entry in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some((platform, userid)) = normalize_sevendaystodie_user_identity(entry) else {
            continue;
        };

        if seen.insert(format!(
            "{}:{}",
            platform.to_ascii_lowercase(),
            userid.to_ascii_lowercase()
        )) {
            let name = normalize_sevendaystodie_text(entry.get("name"), "");
            let unbandate = normalize_sevendaystodie_text(entry.get("unbandate"), "9999-12-31");
            let reason = normalize_sevendaystodie_text(entry.get("reason"), "LanGame");
            lines.push(format!(
                "    <blacklisted platform=\"{platform}\" userid=\"{userid}\" name=\"{name}\" unbandate=\"{unbandate}\" reason=\"{reason}\" />"
            ));
        }
    }

    lines.join("\n")
}

pub(super) fn render_sevendaystodie_permission_lines(settings: &Map<String, Value>) -> String {
    let Some(entries) = lookup_setting_array(settings, "command_permissions") else {
        return String::new();
    };

    let mut seen = HashSet::new();
    let mut lines = Vec::new();

    for entry in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let command = normalize_sevendaystodie_text(entry.get("cmd"), "");
        if command.is_empty() {
            continue;
        }

        if seen.insert(command.to_ascii_lowercase()) {
            let permission_level = normalize_sevendaystodie_level(entry.get("permission_level"), 0);
            lines.push(format!(
                "    <permission cmd=\"{command}\" permission_level=\"{permission_level}\" />"
            ));
        }
    }

    lines.join("\n")
}

pub(super) const ENSHROUDED_SERVER_TAG_FLAGS: &[&str] = &[
    "English",
    "German",
    "French",
    "Italian",
    "Japanese",
    "Korean",
    "Polish",
    "Portuguese",
    "Russian",
    "Spanish",
    "Thai",
    "Turkish",
    "Ukrainian",
    "Chinese",
    "Taiwanese",
    "LookingForPlayers",
    "BaseBuilding",
    "Exploration",
    "Roleplay",
];

pub(super) fn render_ark_prefixed_lines(
    settings: &Map<String, Value>,
    key: &str,
    prefix: &str,
) -> String {
    let Some(raw) = lookup_setting_text(settings, key) else {
        return String::new();
    };

    let lines = raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            if let Some((native_key, value)) = line.split_once('=')
                && prefix
                    .strip_suffix('=')
                    .is_some_and(|key| native_key.trim() == key)
            {
                format!("{prefix}{}", value.trim_start())
            } else if line.starts_with(prefix) {
                String::from(line)
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>();

    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

pub(super) fn render_ark_steam64_lines(settings: &Map<String, Value>, key: &str) -> String {
    parse_steam64_lines(settings, key).join("\n")
}

pub(super) fn render_ark_account_id_lines(settings: &Map<String, Value>, key: &str) -> String {
    let mut seen = HashSet::new();
    parse_config_lines(settings, key)
        .into_iter()
        .filter_map(|line| normalize_ark_account_id(&line))
        .filter(|line| seen.insert(line.to_ascii_lowercase()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn render_projectzomboid_welcome_message(settings: &Map<String, Value>) -> String {
    let Some(raw) = lookup_setting_text(settings, "welcome_message") else {
        return String::new();
    };

    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized
        .split('\n')
        .map(|line| line.trim())
        .collect::<Vec<_>>();

    lines.join(" <LINE> ")
}

pub(super) fn render_projectzomboid_workshop_items(settings: &Map<String, Value>) -> String {
    parse_workshop_id_list(settings, "workshop_items").join(";")
}

pub(super) fn render_projectzomboid_semicolon_list(
    settings: &Map<String, Value>,
    key: &str,
) -> String {
    let Some(raw) = lookup_setting_text(settings, key) else {
        return String::new();
    };

    normalize_projectzomboid_semicolon_list(&raw).join(";")
}

pub(super) fn normalize_projectzomboid_semicolon_list(raw: &str) -> Vec<String> {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut seen = HashSet::new();
    let mut entries = Vec::new();

    for entry in normalized
        .split(['\n', ';'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty() && !entry.starts_with('#') && !entry.starts_with("//"))
    {
        if seen.insert(entry.to_ascii_lowercase()) {
            entries.push(String::from(entry));
        }
    }

    entries
}

/// Compare the exact IDs that the native `Mods=` writer emits, allowing only a
/// change of order. Case changes and additions/removals require separate intent.
pub fn projectzomboid_mod_reorder_preserves_ids(before: &str, after: &str) -> bool {
    let mut original = normalize_projectzomboid_semicolon_list(before);
    let mut proposed = normalize_projectzomboid_semicolon_list(after);
    original.sort_unstable();
    proposed.sort_unstable();
    original == proposed
}

pub(super) fn render_terraria_seed_line(settings: &Map<String, Value>) -> String {
    render_terraria_optional_text_line(settings, "seed", "seed")
}

pub(super) fn render_terraria_special_seed_line(settings: &Map<String, Value>) -> String {
    let Some(raw) = lookup_setting_text(settings, "special_seed") else {
        return String::new();
    };

    match raw.trim().to_ascii_lowercase().as_str() {
        "celebration" => String::from("seed_celebration=1"),
        "theconstant" => String::from("seed_theconstant=1"),
        "notthebees" => String::from("seed_notthebees=1"),
        "notraps" => String::from("seed_notraps=1"),
        "fortheworthy" => String::from("seed_fortheworthy=1"),
        "remix" => String::from("seed_remix=1"),
        "drunk" => String::from("seed_drunk=1"),
        "zenith" => String::from("seed_zenith=1"),
        _ => String::new(),
    }
}

pub(super) fn render_terraria_lobby_line(settings: &Map<String, Value>) -> String {
    if !lookup_boolean_setting(settings, "steam") {
        return String::new();
    }

    let Some(raw) = lookup_setting_text(settings, "lobby") else {
        return String::new();
    };

    match raw.trim().to_ascii_lowercase().as_str() {
        "friends" => String::from("lobby=friends"),
        "private" => String::from("lobby=private"),
        _ => String::new(),
    }
}

pub(super) fn render_terraria_banlist_lines(settings: &Map<String, Value>) -> String {
    let mut seen = HashSet::new();
    parse_config_lines(settings, "banlist_entries")
        .into_iter()
        .filter_map(|entry| normalize_terraria_banlist_entry(&entry))
        .filter(|entry| seen.insert(entry.to_ascii_lowercase()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn render_terraria_enabled_line(
    settings: &Map<String, Value>,
    key: &str,
    config_key: &str,
) -> String {
    if lookup_boolean_setting(settings, key) {
        format!("{config_key}=1")
    } else {
        String::new()
    }
}

pub(super) fn render_terraria_optional_text_line(
    settings: &Map<String, Value>,
    key: &str,
    config_key: &str,
) -> String {
    let Some(raw) = lookup_setting_text(settings, key) else {
        return String::new();
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    format!("{config_key}={trimmed}")
}

pub(super) fn render_terraria_optional_i64_line(
    settings: &Map<String, Value>,
    key: &str,
    config_key: &str,
) -> String {
    let Some(value) = lookup_numeric_setting_i64(settings, key) else {
        return String::new();
    };

    format!("{config_key}={value}")
}

pub(super) fn render_enshrouded_banned_accounts_json(
    settings: &Map<String, Value>,
) -> Result<String, StorageError> {
    let ids = crate::player_access_normalization::parse_enshrouded_banned_account_ids(settings)
        .map_err(|message| StorageError::InvalidModuleSetting {
            module_id: String::from("enshrouded"),
            field: String::from("banned_player_ids"),
            message,
        })?;
    let mut entries = Vec::<Value>::new();

    for account_id in ids {
        let mut ban_date = Map::new();
        ban_date.insert(
            String::from("value"),
            Value::Number(serde_json::Number::from(0_u64)),
        );

        let mut entry = Map::new();
        entry.insert(
            String::from("accountId"),
            Value::Number(serde_json::Number::from(account_id)),
        );
        entry.insert(String::from("displayName"), Value::String(String::new()));
        entry.insert(String::from("characterName"), Value::String(String::new()));
        entry.insert(String::from("banDate"), Value::Object(ban_date));
        entries.push(Value::Object(entry));
    }

    Ok(serde_json::to_string(&entries)?)
}

pub(super) fn render_enshrouded_extra_user_groups_json_entries(
    settings: &Map<String, Value>,
) -> String {
    let Some(raw) = lookup_setting_text(settings, "custom_user_groups_json") else {
        return String::new();
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let Ok(parsed) = serde_json::from_str::<Value>(trimmed) else {
        return String::new();
    };

    let groups = match parsed {
        Value::Array(items) => items
            .into_iter()
            .filter(|item| item.is_object())
            .collect::<Vec<_>>(),
        Value::Object(_) => vec![parsed],
        _ => Vec::new(),
    };

    let rendered = groups
        .into_iter()
        .filter_map(|group| serde_json::to_string_pretty(&group).ok())
        .map(|group| {
            group
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>();

    if rendered.is_empty() {
        String::new()
    } else {
        format!(",\n{}", rendered.join(",\n"))
    }
}

pub(super) fn render_enshrouded_tags_json(settings: &Map<String, Value>) -> String {
    let Some(raw) = lookup_setting_text(settings, "server_tags") else {
        return String::from("[]");
    };

    let canonical_flags = ENSHROUDED_SERVER_TAG_FLAGS
        .iter()
        .map(|value| (value.to_ascii_lowercase(), *value))
        .collect::<HashMap<_, _>>();
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut seen = HashSet::new();
    let mut entries = Vec::<String>::new();

    for value in normalized
        .split(['\n', ','])
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if value.starts_with('#') || value.starts_with("//") {
            continue;
        }

        let Some(canonical) = canonical_flags.get(&value.to_ascii_lowercase()) else {
            continue;
        };

        let candidate = String::from(*canonical);
        if seen.insert(candidate.clone()) {
            entries.push(candidate);
        }
    }

    serde_json::to_string(&entries).unwrap_or_else(|_| String::from("[]"))
}

pub(super) fn render_enshrouded_minutes_as_ns(
    settings: &Map<String, Value>,
    key: &str,
    default_minutes: u64,
) -> String {
    let minutes = lookup_numeric_setting_u64(settings, key).unwrap_or(default_minutes);
    let nanoseconds = minutes.saturating_mul(60).saturating_mul(1_000_000_000);
    nanoseconds.to_string()
}

pub(super) fn lookup_numeric_setting_u64(settings: &Map<String, Value>, key: &str) -> Option<u64> {
    settings.get(key).and_then(|value| match value {
        Value::Number(number) => number.as_u64().or_else(|| {
            number
                .as_i64()
                .and_then(|value| (value >= 0).then_some(value as u64))
        }),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        Value::Bool(boolean) => Some(if *boolean { 1 } else { 0 }),
        _ => None,
    })
}

pub(super) fn lookup_numeric_setting_i64(settings: &Map<String, Value>, key: &str) -> Option<i64> {
    settings.get(key).and_then(|value| match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok())),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        Value::Bool(boolean) => Some(if *boolean { 1 } else { 0 }),
        _ => None,
    })
}

pub(super) fn lookup_boolean_setting(settings: &Map<String, Value>, key: &str) -> bool {
    settings
        .get(key)
        .map(|value| match value {
            Value::Bool(boolean) => *boolean,
            Value::Number(number) => number
                .as_i64()
                .map(|value| value != 0)
                .or_else(|| number.as_u64().map(|value| value != 0))
                .unwrap_or(false),
            Value::String(text) => !matches!(
                text.trim().to_ascii_lowercase().as_str(),
                "" | "0" | "false" | "off" | "no"
            ),
            _ => false,
        })
        .unwrap_or(false)
}

pub(super) fn lookup_template_path_json(
    instance_root: &Path,
    install_root: &Path,
    config_dir: &Path,
    data_dir: &Path,
    logs_dir: &Path,
    saves_dir: &Path,
    path: &str,
) -> Option<String> {
    let value = match path {
        "install_root" => install_root.to_string_lossy().into_owned(),
        "instance_root" => instance_root.to_string_lossy().into_owned(),
        "config_dir" => config_dir.to_string_lossy().into_owned(),
        "data_dir" => data_dir.to_string_lossy().into_owned(),
        "logs_dir" => logs_dir.to_string_lossy().into_owned(),
        "saves_dir" => saves_dir.to_string_lossy().into_owned(),
        _ => return None,
    };

    serde_json::to_string(&value).ok()
}
