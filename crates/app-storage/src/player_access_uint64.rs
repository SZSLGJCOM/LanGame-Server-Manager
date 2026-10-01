use std::collections::HashSet;

use serde_json::{Map, Value};

pub(crate) fn normalize_uint64_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u64>().ok()?;
    Some(value.to_string())
}

pub(crate) fn parse_enshrouded_banned_account_ids(
    settings: &Map<String, Value>,
) -> Result<Vec<u64>, String> {
    let Some(value) = settings.get("banned_player_ids") else {
        return Ok(Vec::new());
    };
    let raw = value
        .as_str()
        .ok_or_else(|| String::from("native account hashes must be a string list"))?;
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for line in raw.split(['\r', '\n']).map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        for entry in line
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let normalized = normalize_uint64_id(entry).ok_or_else(|| {
                String::from("native account hashes must contain only decimal digits in 0..18446744073709551615; Steam IDs are not converted")
            })?;
            let id = normalized
                .parse::<u64>()
                .map_err(|error| error.to_string())?;
            if seen.insert(id) {
                ids.push(id);
            }
        }
    }
    Ok(ids)
}
