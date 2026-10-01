use serde_json::{Map, Value};

use crate::player_access_normalization::{
    normalize_barotrauma_account, normalize_minecraft_banned_ip, normalize_minecraft_player_name,
    normalize_minecraft_uuid,
};

pub(crate) const DELIMITED_FIELDS_SCHEMA_KEY: &str = "x-lsgm-player-access-delimited-fields";
pub(crate) const ENTRY_SEPARATORS_SCHEMA_KEY: &str = "x-lsgm-player-access-entry-separators";

const SUPPORTED_DELIMITED_CODECS: &[&str] = &[
    "pipe_steam64",
    "csv_uuid_name",
    "minecraft_ip_csv",
    "barotrauma_account",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DelimitedEntryRequirement {
    Stored,
    MutationAdd,
    MutationRemove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedDelimitedEntry {
    pub stored: String,
    pub identity: String,
    pub parts: Vec<String>,
}

pub(crate) fn is_delimited_player_access_codec(codec: &str) -> bool {
    SUPPORTED_DELIMITED_CODECS.contains(&codec)
}

pub(crate) fn normalize_delimited_player_access_entry(
    codec: &str,
    field_schema: &Map<String, Value>,
    raw: &str,
    requirement: DelimitedEntryRequirement,
) -> Result<NormalizedDelimitedEntry, String> {
    if !is_delimited_player_access_codec(codec) {
        return Err(format!(
            "codec `{codec}` is not a delimited player-access codec"
        ));
    }
    let fields = field_schema
        .get(DELIMITED_FIELDS_SCHEMA_KEY)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!("{codec} requires an explicit {DELIMITED_FIELDS_SCHEMA_KEY} contract")
        })?;
    if fields.is_empty() {
        return Err(format!("{DELIMITED_FIELDS_SCHEMA_KEY} must not be empty"));
    }
    validate_delimited_contract(codec, fields)?;

    let delimiter = match codec {
        "pipe_steam64" => '|',
        "csv_uuid_name" | "minecraft_ip_csv" | "barotrauma_account" => ',',
        _ => unreachable!(),
    };
    let consume_rest = fields
        .last()
        .and_then(Value::as_object)
        .and_then(|field| field.get("consumeRest"))
        .and_then(Value::as_bool)
        == Some(true);
    let raw_parts = if consume_rest {
        raw.splitn(fields.len(), delimiter)
            .map(str::trim)
            .collect::<Vec<_>>()
    } else {
        raw.split(delimiter).map(str::trim).collect::<Vec<_>>()
    };
    if raw_parts.len() > fields.len() {
        return Err(format!(
            "entry has {} segments but this roster accepts at most {}",
            raw_parts.len(),
            fields.len()
        ));
    }

    let enforce_required = !matches!(requirement, DelimitedEntryRequirement::MutationRemove);
    let mut parts = Vec::with_capacity(raw_parts.len());
    for (index, raw_part) in raw_parts.iter().enumerate() {
        let field = fields[index]
            .as_object()
            .ok_or_else(|| format!("{DELIMITED_FIELDS_SCHEMA_KEY}[{index}] must be an object"))?;
        let name = field
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                format!("{DELIMITED_FIELDS_SCHEMA_KEY}[{index}].name must not be empty")
            })?;
        if raw_part.is_empty() {
            return Err(format!("segment `{name}` must not be empty when present"));
        }
        parts.push(normalize_segment(name, field, raw_part)?);
    }

    if enforce_required {
        for (index, field) in fields.iter().enumerate() {
            let field = field.as_object().ok_or_else(|| {
                format!("{DELIMITED_FIELDS_SCHEMA_KEY}[{index}] must be an object")
            })?;
            if field.get("required").and_then(Value::as_bool) == Some(true) && index >= parts.len()
            {
                let name = field
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                return Err(format!("required segment `{name}` is missing"));
            }
        }
    }

    let identity = parts
        .first()
        .cloned()
        .ok_or_else(|| String::from("identity segment is missing"))?;
    Ok(NormalizedDelimitedEntry {
        stored: parts.join(&delimiter.to_string()),
        identity,
        parts,
    })
}

fn validate_delimited_contract(codec: &str, fields: &[Value]) -> Result<(), String> {
    let expected_identity_format = match codec {
        "pipe_steam64" => "steam64",
        "csv_uuid_name" => "minecraft_uuid",
        "minecraft_ip_csv" => "ip",
        "barotrauma_account" => "barotrauma_account",
        _ => unreachable!(),
    };
    for (index, field) in fields.iter().enumerate() {
        let field = field
            .as_object()
            .ok_or_else(|| format!("{DELIMITED_FIELDS_SCHEMA_KEY}[{index}] must be an object"))?;
        let name = field
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                format!("{DELIMITED_FIELDS_SCHEMA_KEY}[{index}].name must not be empty")
            })?;
        let format = field
            .get("format")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if !matches!(
            format,
            "steam64"
                | "minecraft_uuid"
                | "minecraft_name"
                | "ip"
                | "barotrauma_account"
                | "text"
                | "integer"
                | "boolean"
        ) {
            return Err(format!(
                "segment `{name}` declares unsupported format `{format}`"
            ));
        }
        if !field.get("required").is_some_and(Value::is_boolean) {
            return Err(format!("segment `{name}` must declare boolean `required`"));
        }
        if field.get("consumeRest").and_then(Value::as_bool) == Some(true)
            && (index + 1 != fields.len() || format != "text")
        {
            return Err(format!(
                "segment `{name}` may consume the delimiter remainder only when it is the final text segment"
            ));
        }
    }
    let first = fields[0].as_object().expect("contract field was validated");
    if first.get("format").and_then(Value::as_str) != Some(expected_identity_format)
        || first.get("required").and_then(Value::as_bool) != Some(true)
    {
        return Err(format!(
            "{codec} identity segment must be required format `{expected_identity_format}`"
        ));
    }
    Ok(())
}

pub(crate) fn split_player_access_text_entries(
    codec: &str,
    field_schema: &Map<String, Value>,
    raw: &str,
) -> Vec<String> {
    let comma_separated = player_access_text_accepts_comma(codec, field_schema);
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    normalized
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("//"))
        .flat_map(|line| {
            if comma_separated {
                line.split(',').collect::<Vec<_>>()
            } else {
                vec![line]
            }
        })
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(String::from)
        .collect()
}

pub(crate) fn player_access_text_accepts_comma(
    codec: &str,
    field_schema: &Map<String, Value>,
) -> bool {
    matches!(codec, "steam64" | "uint64")
        && field_schema
            .get(ENTRY_SEPARATORS_SCHEMA_KEY)
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some("comma")))
}

fn normalize_segment(name: &str, field: &Map<String, Value>, raw: &str) -> Result<String, String> {
    let format = field
        .get("format")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    match format {
        "steam64" => normalize_steam64(raw)
            .ok_or_else(|| format!("segment `{name}` must be a 17-digit Steam64 ID")),
        "minecraft_uuid" => normalize_minecraft_uuid(raw)
            .ok_or_else(|| format!("segment `{name}` must be a valid Minecraft UUID")),
        "minecraft_name" => normalize_minecraft_player_name(raw).ok_or_else(|| {
            format!("segment `{name}` must be a 1-16 character Minecraft player name")
        }),
        "ip" => normalize_minecraft_banned_ip(raw)
            .ok_or_else(|| format!("segment `{name}` must be a valid IPv4 or IPv6 address")),
        "barotrauma_account" => normalize_barotrauma_account(raw).ok_or_else(|| {
            format!(
                "segment `{name}` must be a Steam2, Steam3 individual account, or SteamID64 value"
            )
        }),
        "text" => normalize_safe_text(name, field, raw),
        "integer" => normalize_integer(name, field, raw),
        "boolean" => normalize_boolean(name, raw),
        _ => Err(format!(
            "segment `{name}` declares unsupported format `{format}`"
        )),
    }
}

fn normalize_steam64(raw: &str) -> Option<String> {
    let value = raw.trim();
    (value.len() == 17 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| String::from(value))
}

fn normalize_safe_text(
    name: &str,
    field: &Map<String, Value>,
    raw: &str,
) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty()
        || value.contains("{{")
        || value.contains("}}")
        || value.chars().any(char::is_control)
    {
        return Err(format!("segment `{name}` contains unsafe text"));
    }
    if let Some(max_length) = field.get("maxLength").and_then(Value::as_u64)
        && value.chars().count() as u64 > max_length
    {
        return Err(format!(
            "segment `{name}` must be at most {max_length} characters"
        ));
    }
    Ok(String::from(value))
}

fn normalize_integer(name: &str, field: &Map<String, Value>, raw: &str) -> Result<String, String> {
    let value = raw
        .parse::<i64>()
        .map_err(|_| format!("segment `{name}` must be an integer"))?;
    if let Some(minimum) = field.get("minimum").and_then(Value::as_i64)
        && value < minimum
    {
        return Err(format!("segment `{name}` must be at least {minimum}"));
    }
    if let Some(maximum) = field.get("maximum").and_then(Value::as_i64)
        && value > maximum
    {
        return Err(format!("segment `{name}` must be at most {maximum}"));
    }
    Ok(value.to_string())
}

fn normalize_boolean(name: &str, raw: &str) -> Result<String, String> {
    let value = match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => return Err(format!("segment `{name}` must be a boolean")),
    };
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn comma_rosters_filter_whole_comment_lines_before_splitting_values() {
        let schema = json!({
            "x-lsgm-player-access-entry-separators": ["newline", "comma"]
        });
        let entries = split_player_access_text_entries(
            "steam64",
            schema.as_object().expect("schema object"),
            "# ignored,76561198000000001\n76561198000000002, 76561198000000003\n// ignored,76561198000000004",
        );
        assert_eq!(entries, ["76561198000000002", "76561198000000003"]);
    }
}
