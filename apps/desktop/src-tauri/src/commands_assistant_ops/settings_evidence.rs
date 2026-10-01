fn assistant_settings_evidence(
    settings: &str,
    schema: Option<&str>,
    keys: &[String],
    offset: usize,
) -> Result<Value, String> {
    if keys.len() > 40 || keys.iter().any(|key| key.len() > 256) {
        return Err(String::from("Read at most 40 setting keys at a time."));
    }
    // Keep original setting names during redaction; generic key/value entries lose
    // the secret-bearing key that the provider boundary needs to recognize.
    let settings: Value = serde_json::from_str(&redact_assistant_provider_text(settings))
        .map_err(|error| error.to_string())?;
    let settings = settings
        .as_object()
        .ok_or_else(|| String::from("Instance settings must be an object."))?;
    let schema: Value = schema
        .map(serde_json::from_str)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let properties = schema.get("properties").and_then(Value::as_object);
    let available = settings
        .keys()
        .chain(properties.into_iter().flat_map(|value| value.keys()))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let selected = if keys.is_empty() {
        available.iter().skip(offset).take(20).collect::<Vec<_>>()
    } else {
        keys.iter().collect()
    };
    let entries = selected
        .iter()
        .map(|key| {
            let property = properties.and_then(|value| value.get(key.as_str()));
            json!({"key": key, "exists": settings.contains_key(key.as_str()),
                "schemaExists": property.is_some(), "value": settings.get(key.as_str()),
                "schema": property.map(|schema| crate::assistant::redact_assistant_setting_schema(key.as_str(), schema))})
        })
        .collect::<Vec<_>>();
    // Patch support follows the current settings object, not schema metadata.
    // Missing values and present JSON nulls must remain distinguishable.
    let unknown_keys = selected
        .iter()
        .filter(|key| !settings.contains_key(key.as_str()))
        .collect::<Vec<_>>();
    let mut evidence = json!({"entries": entries, "unknownKeys": unknown_keys,
        "totalKeys": available.len(),
        "nextOffset": if keys.is_empty() && offset.saturating_add(selected.len()) < available.len() { Some(offset + selected.len()) } else { None }});
    if !unknown_keys.is_empty() {
        evidence["hint"] = json!(
            "Keys with exists=false are absent from current settings and cannot be patched; a schema-only entry is a discovery candidate. Use list_settings for actual names, search_settings for a short key or description, or read_settings with empty keys and nextOffset to page values. A null schema alone does not mean an existing setting is unsupported."
        );
    }
    Ok(evidence)
}
