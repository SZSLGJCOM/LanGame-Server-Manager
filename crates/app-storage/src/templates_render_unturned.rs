use super::*;
use std::collections::BTreeMap;
use std::collections::HashSet;

pub(super) fn managed_native_paths() -> Result<HashSet<String>, StorageError> {
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/unturned/schema.json"))?;
    Ok(schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, property)| property.get("x-lsgm-native-type").is_some())
        .filter_map(|(_, property)| property.get("x-lsgm-source-key").and_then(Value::as_str))
        .map(str::to_ascii_lowercase)
        .collect())
}

fn invalid(field: &str, message: impl Into<String>) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: "unturned".into(),
        field: field.into(),
        message: message.into(),
    }
}

fn quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\\n")
    )
}

fn links(value: &Value) -> Result<String, StorageError> {
    let text = value
        .as_str()
        .ok_or_else(|| invalid("browser_links_json", "Expected a JSON array string."))?;
    if text.len() > 65536 {
        return Err(invalid("browser_links_json", "Links exceed 64 KiB."));
    }
    let parsed: Value = serde_json::from_str(text)
        .map_err(|_| invalid("browser_links_json", "Invalid links JSON."))?;
    let entries = parsed
        .as_array()
        .ok_or_else(|| invalid("browser_links_json", "Expected a JSON array."))?;
    if entries.len() > 32 {
        return Err(invalid(
            "browser_links_json",
            "At most 32 lobby links are supported.",
        ));
    }
    let mut result = String::from("[\n");
    for entry in entries {
        let object = entry
            .as_object()
            .ok_or_else(|| invalid("browser_links_json", "Each link must be an object."))?;
        if object.len() != 2 {
            return Err(invalid(
                "browser_links_json",
                "Each link must contain only Message and URL.",
            ));
        }
        let message = object
            .get("Message")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("browser_links_json", "Each link needs a Message string."))?;
        let url = object
            .get("URL")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("browser_links_json", "Each link needs a URL string."))?;
        if message.is_empty()
            || message.len() > 1024
            || message.contains('\0')
            || url.len() > 2048
            || !(url.starts_with("https://") || url.starts_with("http://"))
            || url.chars().any(char::is_whitespace)
            || url.chars().any(char::is_control)
        {
            return Err(invalid(
                "browser_links_json",
                "Use a nonempty label and an HTTP or HTTPS URL without whitespace.",
            ));
        }
        result.push_str(&format!(
            "\t\t{{\n\t\t\tMessage {}\n\t\t\tURL {}\n\t\t}}\n",
            quoted(message),
            quoted(url)
        ));
    }
    result.push_str("\t]");
    Ok(result)
}

fn native_value(key: &str, kind: &str, value: &Value) -> Result<String, StorageError> {
    match kind {
        "bool" if value.is_boolean() => Ok(value.to_string()),
        "float"
            if value
                .as_f64()
                .is_some_and(|n| n.is_finite() && n.abs() <= f32::MAX as f64) =>
        {
            Ok(value.to_string())
        }
        "uint" if value.as_u64().is_some_and(|n| n <= u32::MAX as u64) => Ok(value.to_string()),
        "int" if value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()) => Ok(value.to_string()),
        "EServerMonetizationTag"
            if value.as_str().is_some_and(|v| {
                matches!(
                    v,
                    "Unspecified" | "Any" | "None" | "NonGameplay" | "Monetized"
                )
            }) =>
        {
            Ok(quoted(value.as_str().unwrap_or_default()))
        }
        "string"
            if value
                .as_str()
                .is_some_and(|v| !v.contains('\0') && v.len() <= 65536) =>
        {
            Ok(quoted(value.as_str().unwrap_or_default()))
        }
        "Link[]" => links(value),
        _ => Err(invalid(key, format!("Invalid native {kind} value."))),
    }
}

pub(crate) fn render_unturned_native_config(
    settings: &Map<String, Value>,
    defaults: &Map<String, Value>,
) -> Result<String, StorageError> {
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/unturned/schema.json"))?;
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("schema", "Unturned schema has no properties."))?;
    let mut groups: BTreeMap<&str, Vec<(&str, String)>> = BTreeMap::new();
    for (key, property) in properties {
        let Some(kind) = property.get("x-lsgm-native-type").and_then(Value::as_str) else {
            continue;
        };
        let Some(path) = property.get("x-lsgm-source-key").and_then(Value::as_str) else {
            continue;
        };
        let Some((group, field)) = path.split_once('.') else {
            continue;
        };
        let Some(value) = settings
            .get(key)
            .or_else(|| defaults.get(key))
            .filter(|v| !v.is_null())
        else {
            continue;
        };
        let text = if key == "browser_desc_hint" || key == "browser_desc_full" {
            native_value(key, kind, value)?;
            if let Some(welcome) = settings
                .get("welcome_message")
                .or_else(|| defaults.get("welcome_message"))
            {
                native_value("welcome_message", "string", welcome)?;
            }
            render_unturned_config_text_value(settings, defaults, key, Some("welcome_message"))
        } else {
            native_value(key, kind, value)?
        };
        groups.entry(group).or_default().push((field, text));
    }
    let mut result = String::from("Version 1\n");
    for (group, entries) in groups {
        result.push_str(&format!("{group}\n{{\n"));
        for (field, value) in entries {
            // Native DAT tokenizes an inline '[' as a scalar; collections start on the next line.
            if value.starts_with('[') {
                result.push_str(&format!("\t{field}\n\t{value}\n"));
            } else {
                result.push_str(&format!("\t{field} {value}\n"));
            }
        }
        result.push_str("}\n");
    }
    Ok(result)
}

pub(crate) fn validate_unturned_native_settings(
    settings: &Map<String, Value>,
) -> Result<(), StorageError> {
    render_unturned_native_config(settings, &Map::new()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_links_and_scalar_text_without_creating_configuration_nodes() {
        let value = serde_json::json!(
            "[{\"Message\":\"hello\\n}\\nServer\\n{\",\"URL\":\"https://example.test/\\\"q\"}]"
        );
        let text = links(&value).unwrap();
        assert!(text.contains("Message \"hello\\n}\\nServer\\n{\""));
        assert!(text.contains("URL \"https://example.test/\\\"q\""));
        for value in [
            serde_json::json!("{}"),
            serde_json::json!("[{\"Message\":\"x\",\"URL\":\"file:///x\"}]"),
            serde_json::json!("[{\"Message\":\"x\",\"URL\":\"https://x\\ny\"}]"),
        ] {
            assert!(links(&value).is_err());
        }
    }

    #[test]
    fn enforces_native_numeric_width_and_preserves_explicit_zero_false() {
        assert_eq!(
            native_value("field", "uint", &serde_json::json!(0)).unwrap(),
            "0"
        );
        assert_eq!(
            native_value("field", "bool", &serde_json::json!(false)).unwrap(),
            "false"
        );
        assert!(native_value("field", "uint", &serde_json::json!(-1)).is_err());
        assert!(native_value("field", "uint", &serde_json::json!(4294967296u64)).is_err());
        assert!(native_value("field", "int", &serde_json::json!(2147483648i64)).is_err());
        assert!(native_value("field", "float", &serde_json::json!(1e100)).is_err());
    }

    #[test]
    fn emits_native_category_blocks_with_explicit_overrides_only() {
        let settings = serde_json::json!({"native_items_spawn_chance": 0.25, "native_players_health_default": 0, "native_gameplay_friendly_fire": false, "native_vehicles_armor_multiplier": null});
        let text =
            render_unturned_native_config(settings.as_object().unwrap(), &Map::new()).unwrap();
        assert!(text.contains("Items\n{\n\tSpawn_Chance 0.25\n}"));
        assert!(text.contains("Health_Default 0"));
        assert!(text.contains("Friendly_Fire false"));
        assert!(!text.contains("Armor_Multiplier"));
        assert_eq!(
            render_unturned_native_config(&Map::new(), &Map::new()).unwrap(),
            "Version 1\n"
        );
        assert!(!text.contains("Items.Spawn_Chance"));
        let settings = serde_json::json!({"browser_links_json": "[{\"Message\":\"Rules\",\"URL\":\"https://example.test\"}]"});
        let text =
            render_unturned_native_config(settings.as_object().unwrap(), &Map::new()).unwrap();
        assert!(text.contains("\tLinks\n\t[\n\t\t{"));
        assert!(!text.contains("Links ["));
    }

    #[test]
    fn rejects_invalid_description_fallbacks_and_multibyte_limits() {
        let settings =
            serde_json::json!({"browser_desc_hint": "", "welcome_message": "invalid\u{0}"});
        assert!(validate_unturned_native_settings(settings.as_object().unwrap()).is_err());
        assert!(native_value("field", "string", &Value::String("界".repeat(21846))).is_err());
        let value =
            serde_json::json!([{"Message": "界".repeat(342), "URL": "https://example.test"}])
                .to_string();
        assert!(links(&Value::String(value)).is_err());
        for value in [
            "[{\"Message\":\"x\",\"URL\":\"https://example.test\",\"Extra\":1}]",
            "[null]",
        ] {
            assert!(links(&Value::String(value.into())).is_err());
        }
    }
}
