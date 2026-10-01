use std::net::IpAddr;

use serde_json::{Map, Value};

use crate::StorageError;
use crate::settings_value_formats::is_date_or_local_datetime;

#[path = "player_access_humanitz.rs"]
mod humanitz;
pub(crate) use humanitz::{
    is_humanitz_net_id, is_stored_humanitz_identity, parse_humanitz_roster,
    validate_humanitz_roster_update, validate_humanitz_rosters,
};

const SEVEN_DAYS_MODULE_ID: &str = "sevendaystodie";
#[path = "player_access_uint64.rs"]
mod uint64;
pub(crate) use uint64::{normalize_uint64_id, parse_enshrouded_banned_account_ids};
const SEVEN_DAYS_PERMANENT_UNBAN_DATE: &str = "9999-12-31";
const SEVEN_DAYS_OBJECT_ROSTER_FIELDS: &[&str] = &[
    "admin_users",
    "admin_groups",
    "whitelist_users",
    "whitelist_groups",
    "blacklist_entries",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlayerAccessNormalizationDiagnostic {
    pub field: String,
    pub message: String,
}

pub(crate) fn normalize_module_player_access_settings(
    module_id: &str,
    settings: &mut Map<String, Value>,
) -> Result<bool, Vec<PlayerAccessNormalizationDiagnostic>> {
    if module_id == "humanitz" {
        for key in humanitz::HUMANITZ_ROSTER_FIELDS {
            parse_humanitz_roster(settings, key)
                .map_err(|message| vec![normalization_diagnostic(key, &message)])?;
        }
    }
    if module_id == "enshrouded" {
        parse_enshrouded_banned_account_ids(settings)
            .map_err(|message| vec![normalization_diagnostic("banned_player_ids", &message)])?;
    }
    if module_id == SEVEN_DAYS_MODULE_ID {
        return normalize_sevendaystodie_player_access_settings(settings);
    }

    Ok(false)
}

pub(crate) fn normalize_module_player_access_settings_strict(
    module_id: &str,
    settings: &mut Map<String, Value>,
) -> Result<bool, StorageError> {
    normalize_module_player_access_settings(module_id, settings).map_err(|diagnostics| {
        let mut diagnostics = diagnostics.into_iter();
        let diagnostic = diagnostics
            .next()
            .unwrap_or(PlayerAccessNormalizationDiagnostic {
                field: String::from("player_access"),
                message: String::from("player-access settings could not be normalized"),
            });
        let remaining = diagnostics.count();
        let message = if remaining == 0 {
            diagnostic.message
        } else {
            format!(
                "{}; {remaining} additional player-access normalization error(s) remain",
                diagnostic.message
            )
        };
        StorageError::InvalidModuleSetting {
            module_id: String::from(module_id),
            field: diagnostic.field,
            message,
        }
    })
}

fn normalize_sevendaystodie_player_access_settings(
    settings: &mut Map<String, Value>,
) -> Result<bool, Vec<PlayerAccessNormalizationDiagnostic>> {
    let mut normalized = settings.clone();
    let mut diagnostics = Vec::new();

    for field in SEVEN_DAYS_OBJECT_ROSTER_FIELDS {
        let Some(value) = normalized.get_mut(*field) else {
            continue;
        };
        let Some(entries) = value.as_array_mut() else {
            diagnostics.push(normalization_diagnostic(field, "roster must be an array"));
            continue;
        };

        for (index, entry) in entries.iter_mut().enumerate() {
            let path = format!("{field}[{index}]");
            let Some(entry) = entry.as_object_mut() else {
                diagnostics.push(normalization_diagnostic(
                    &path,
                    "roster entry must be an object",
                ));
                continue;
            };

            normalize_object_roster_strings(entry, &path, &mut diagnostics);

            if matches!(
                *field,
                "admin_users" | "whitelist_users" | "blacklist_entries"
            ) {
                normalize_sevendaystodie_user_identity(entry, &path, &mut diagnostics);
            }
            if *field == "blacklist_entries" {
                normalize_sevendaystodie_unban_date(entry, &path, &mut diagnostics);
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let changed = normalized != *settings;
    if changed {
        *settings = normalized;
    }
    Ok(changed)
}

fn normalize_object_roster_strings(
    entry: &mut Map<String, Value>,
    path: &str,
    diagnostics: &mut Vec<PlayerAccessNormalizationDiagnostic>,
) {
    for (key, value) in entry.iter_mut() {
        let Value::String(text) = value else {
            continue;
        };
        if text.chars().any(char::is_control) {
            diagnostics.push(normalization_diagnostic(
                &format!("{path}.{key}"),
                "player-access string metadata must not contain control characters",
            ));
            continue;
        }
        *text = text.trim().to_string();
    }
}

fn normalize_sevendaystodie_user_identity(
    entry: &mut Map<String, Value>,
    path: &str,
    diagnostics: &mut Vec<PlayerAccessNormalizationDiagnostic>,
) {
    if entry.contains_key("steam_id") {
        diagnostics.push(normalization_diagnostic(
            &format!("{path}.steam_id"),
            "user entries require platform and userid; steam_id is only valid for group entries",
        ));
        return;
    }
    let Some(user_id) = entry.get("userid").and_then(trimmed_string) else {
        diagnostics.push(normalization_diagnostic(
            &format!("{path}.userid"),
            "userid must be a non-empty string",
        ));
        return;
    };
    let Some(platform) = entry.get("platform").and_then(trimmed_string) else {
        diagnostics.push(normalization_diagnostic(
            &format!("{path}.platform"),
            "platform must be a non-empty string",
        ));
        return;
    };
    if platform.eq_ignore_ascii_case("steam") && !is_steam64(&user_id) {
        diagnostics.push(normalization_diagnostic(
            &format!("{path}.userid"),
            "Steam userid must be a 17-digit Steam64 ID",
        ));
        return;
    }

    let platform = if platform.eq_ignore_ascii_case("steam") {
        String::from("Steam")
    } else {
        platform
    };
    entry.insert(String::from("platform"), Value::String(platform));
    entry.insert(String::from("userid"), Value::String(user_id));
}

fn normalize_sevendaystodie_unban_date(
    entry: &mut Map<String, Value>,
    path: &str,
    diagnostics: &mut Vec<PlayerAccessNormalizationDiagnostic>,
) {
    let field = format!("{path}.unbandate");
    let Some(value) = entry.get("unbandate") else {
        entry.insert(
            String::from("unbandate"),
            Value::String(String::from(SEVEN_DAYS_PERMANENT_UNBAN_DATE)),
        );
        return;
    };

    match normalize_sevendaystodie_unban_date_value(value) {
        Ok(date) => {
            entry.insert(String::from("unbandate"), Value::String(date));
        }
        Err(message) => diagnostics.push(normalization_diagnostic(&field, message)),
    }
}

fn normalize_sevendaystodie_unban_date_value(value: &Value) -> Result<String, &'static str> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| is_date_or_local_datetime(text))
        .map(String::from)
        .ok_or("unbandate must be a real calendar date in YYYY-MM-DD or YYYY-MM-DD HH:mm:ss format")
}

fn trimmed_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(String::from)
}

fn is_steam64(value: &str) -> bool {
    value.len() == 17 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn normalization_diagnostic(field: &str, message: &str) -> PlayerAccessNormalizationDiagnostic {
    PlayerAccessNormalizationDiagnostic {
        field: String::from(field),
        message: String::from(message),
    }
}

pub(crate) fn normalize_ark_account_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 128
        || value.contains("{{")
        || value.contains("}}")
        || value.contains(',')
        || value.contains('|')
        || value.contains(';')
        || value.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || !(character.is_ascii_alphanumeric()
                    || character == '_'
                    || character == '-'
                    || character == ':')
        })
    {
        return None;
    }

    Some(String::from(value))
}

pub(crate) fn normalize_barotrauma_account(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let upper = trimmed.to_ascii_uppercase();
    if let Some(rest) = upper.strip_prefix("STEAM_") {
        let parts = rest.split(':').collect::<Vec<_>>();
        if parts.len() == 3
            && parts[1].len() == 1
            && matches!(parts[1], "0" | "1")
            && !parts[2].is_empty()
            && parts[2].chars().all(|character| character.is_ascii_digit())
        {
            return Some(format!("STEAM_1:{}:{}", parts[1], parts[2]));
        }
    }

    if upper.starts_with("[U:1:") && upper.ends_with(']') {
        let account = &upper[5..upper.len() - 1];
        if let Ok(account_id) = account.parse::<u64>() {
            return Some(format!("STEAM_1:{}:{}", account_id % 2, account_id / 2));
        }
    }

    if trimmed.len() == 17 && trimmed.chars().all(|character| character.is_ascii_digit()) {
        let steam64 = trimmed.parse::<u64>().ok()?;
        const STEAM64_BASE: u64 = 76_561_197_960_265_728;
        if steam64 <= STEAM64_BASE {
            return None;
        }
        let account_id = steam64 - STEAM64_BASE;
        return Some(format!("STEAM_1:{}:{}", account_id % 2, account_id / 2));
    }

    None
}

pub(crate) fn normalize_dst_klei_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.len() < 4
        || value.len() > 64
        || !value.to_ascii_uppercase().starts_with("KU_")
        || value.contains("{{")
        || value.contains("}}")
        || value.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, ',' | '|' | '"' | '\'' | '[' | ']' | '{' | '}')
        })
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return None;
    }

    Some(format!("KU_{}", &value[3..]))
}

pub(crate) fn normalize_valheim_platform_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty()
        || value.contains(',')
        || value.contains('|')
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return None;
    }

    Some(String::from(value))
}

pub(crate) fn normalize_minecraft_uuid(raw: &str) -> Option<String> {
    let value = raw.trim().to_ascii_lowercase();
    let compact = match value.len() {
        32 if value.bytes().all(|byte| byte.is_ascii_hexdigit()) => value,
        36 => {
            let valid = value.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            });
            if !valid {
                return None;
            }
            value.replace('-', "")
        }
        _ => return None,
    };

    Some(format!(
        "{}-{}-{}-{}-{}",
        &compact[0..8],
        &compact[8..12],
        &compact[12..16],
        &compact[16..20],
        &compact[20..32]
    ))
}

pub(crate) fn normalize_minecraft_player_name(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 16
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }

    Some(String::from(value))
}

pub(crate) fn normalize_minecraft_banned_ip(raw: &str) -> Option<String> {
    raw.trim()
        .parse::<IpAddr>()
        .ok()
        .map(|address| address.to_string())
}

pub(crate) fn normalize_terraria_banlist_entry(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 128
        || value.contains("{{")
        || value.contains("}}")
        || value.chars().any(|character| character.is_control())
    {
        return None;
    }

    Some(String::from(value))
}

#[cfg(test)]
mod sevendaystodie_tests {
    use super::*;
    use serde_json::json;

    fn settings(value: Value) -> Map<String, Value> {
        value.as_object().expect("settings object").clone()
    }

    #[test]
    fn retired_user_fields_are_rejected_without_partial_changes() {
        for entry in [
            json!({"steam_id": "76561198000000001"}),
            json!({"steam_id": "76561198000000001", "platform": "Steam", "userid": "76561198000000002"}),
        ] {
            let original = settings(json!({"admin_users": [entry]}));
            let mut candidate = original.clone();
            let diagnostics =
                normalize_module_player_access_settings("sevendaystodie", &mut candidate)
                    .expect_err("retired fields must not be migrated");
            assert_eq!(candidate, original);
            assert_eq!(diagnostics[0].field, "admin_users[0].steam_id");
        }
    }

    #[test]
    fn invalid_calendar_date_is_diagnosed_without_partial_normalization() {
        let original = settings(json!({
            "admin_users": [{"platform": "Steam", "userid": "76561198000000001"}],
            "blacklist_entries": [{
                "platform": "Steam", "userid": "76561198000000002",
                "unbandate": "2023-02-29"
            }]
        }));
        let mut candidate = original.clone();

        let diagnostics = normalize_module_player_access_settings("sevendaystodie", &mut candidate)
            .expect_err("invalid date must be diagnosed");

        assert_eq!(candidate, original, "normalization must be atomic");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].field, "blacklist_entries[0].unbandate");
        assert!(diagnostics[0].message.contains("real calendar date"));
    }

    #[test]
    fn numeric_and_out_of_range_expiry_values_are_rejected() {
        for value in [
            json!(-1),
            json!(0),
            json!(1735689600),
            json!(1735689600000_i64),
            json!("1735689600"),
            json!(u64::MAX),
        ] {
            assert!(normalize_sevendaystodie_unban_date_value(&value).is_err());
        }
        assert_eq!(
            normalize_sevendaystodie_unban_date_value(&json!("9999-12-31")),
            Ok(String::from("9999-12-31"))
        );
    }

    #[test]
    fn current_rosters_are_idempotent_and_missing_expiry_gets_explicit_default() {
        let mut settings = settings(json!({
            "admin_users": [{"platform": "steam", "userid": "76561198000000001"}],
            "blacklist_entries": [{"platform": "EOS", "userid": "account-1"}]
        }));

        assert!(
            normalize_module_player_access_settings("sevendaystodie", &mut settings)
                .expect("normalize current entries")
        );
        assert_eq!(settings["admin_users"][0]["platform"], json!("Steam"));
        assert_eq!(
            settings["blacklist_entries"][0]["unbandate"],
            json!("9999-12-31")
        );
        assert!(
            !normalize_module_player_access_settings("sevendaystodie", &mut settings)
                .expect("second normalization")
        );
    }

    #[test]
    fn object_roster_string_metadata_is_trimmed_and_controls_fail_atomically() {
        let mut trimmed_settings = settings(json!({
            "blacklist_entries": [{
                "platform": " Steam ",
                "userid": " 76561198000000001 ",
                "name": "  Alice  ",
                "unbandate": " 9999-12-31 ",
                "reason": "  Trusted  "
            }]
        }));
        normalize_module_player_access_settings("sevendaystodie", &mut trimmed_settings)
            .expect("trim object roster metadata");
        assert_eq!(
            trimmed_settings["blacklist_entries"][0]["name"],
            json!("Alice")
        );
        assert_eq!(
            trimmed_settings["blacklist_entries"][0]["reason"],
            json!("Trusted")
        );

        for key in ["name", "reason"] {
            let original = settings(json!({
                "blacklist_entries": [{
                    "platform": "Steam",
                    "userid": "76561198000000001",
                    "name": "Alice",
                    "unbandate": "9999-12-31",
                    "reason": "Trusted"
                }]
            }));
            let mut candidate = original.clone();
            candidate["blacklist_entries"][0][key] = json!("unsafe\u{85}");
            let unchanged = candidate.clone();
            let diagnostics =
                normalize_module_player_access_settings("sevendaystodie", &mut candidate)
                    .expect_err("control metadata must be rejected");
            assert_eq!(candidate, unchanged);
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.field.ends_with(key))
            );
        }
    }
}

#[cfg(test)]
#[path = "player_access_input_tests.rs"]
mod input_tests;
