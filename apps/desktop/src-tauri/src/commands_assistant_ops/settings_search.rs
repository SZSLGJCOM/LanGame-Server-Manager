const ASSISTANT_SETTINGS_SEARCH_PAGE: usize = 10;
const ASSISTANT_SETTINGS_SEARCH_QUERY_BYTES: usize = 128;
const ASSISTANT_SETTINGS_SEARCH_TERMS: usize = 8;

fn assistant_search_settings_evidence(
    settings: &str,
    schema: Option<&str>,
    query: &str,
    offset: usize,
) -> Result<Value, String> {
    if query.len() > ASSISTANT_SETTINGS_SEARCH_QUERY_BYTES {
        return Err(String::from(
            "Setting search accepts at most 128 query bytes.",
        ));
    }
    let normalized_query = normalize_assistant_settings_search_text(query);
    let terms = normalized_query.split_whitespace().collect::<Vec<_>>();
    if terms.is_empty() || terms.len() > ASSISTANT_SETTINGS_SEARCH_TERMS {
        return Err(String::from(
            "Setting search requires between 1 and 8 alphanumeric terms.",
        ));
    }
    let current: Value = serde_json::from_str(settings)
        .map_err(|_| String::from("Instance settings are not valid JSON."))?;
    let current = current
        .as_object()
        .ok_or_else(|| String::from("Instance settings must be an object."))?;
    let parsed_schema: Value = schema
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| String::from("Setting schema is not valid JSON."))?
        .unwrap_or_default();
    let properties = parsed_schema.get("properties").and_then(Value::as_object);
    let available = current
        .keys()
        .chain(properties.into_iter().flat_map(|value| value.keys()))
        .collect::<std::collections::BTreeSet<_>>();
    // Search only labels and source metadata. Current values and schema defaults
    // must not become an oracle for discovering secrets through match counts.
    let matches = available
        .iter()
        .filter(|key| {
            let property = properties.and_then(|value| value.get(key.as_str()));
            let mut text = key.to_string();
            for field in [
                "title",
                "description",
                "x-lsgm-source",
                "x-lsgm-source-key",
                "x-lsgm-source-surface",
            ] {
                if let Some(value) = property
                    .and_then(|value| value.get(field))
                    .and_then(Value::as_str)
                {
                    text.push(' ');
                    text.push_str(value);
                }
            }
            let text = normalize_assistant_settings_search_text(&text);
            terms.iter().all(|term| text.contains(*term))
        })
        .map(|key| key.as_str().to_owned())
        .collect::<Vec<_>>();
    let selected = matches
        .iter()
        .skip(offset)
        .take(ASSISTANT_SETTINGS_SEARCH_PAGE)
        .cloned()
        .collect::<Vec<_>>();
    let end = offset.saturating_add(selected.len());
    // Empty keys mean "list all settings" to the existing reader, so an empty
    // search page must be constructed explicitly instead of forwarding it.
    let mut evidence = if selected.is_empty() {
        json!({"entries": [], "totalKeys": available.len()})
    } else {
        assistant_settings_evidence(settings, schema, &selected, 0)?
    };
    evidence["totalMatches"] = json!(matches.len());
    evidence["nextOffset"] = json!((end < matches.len()).then_some(end));
    evidence["matching"] = json!({
        "mode": "all_terms",
        "termCount": terms.len(),
        "description": "Every normalized query term must occur in the setting key or schema labels/source metadata. Matching uses case-insensitive literal substrings, not semantic similarity. Current values and defaults are not searched."
    });
    if matches.is_empty() {
        evidence["hint"] = json!(
            "No setting matched every query term. Retry with fewer terms or an exact native configuration filename. Zero matches does not establish that the setting is unsupported."
        );
    }
    Ok(evidence)
}

fn normalize_assistant_settings_search_text(text: &str) -> String {
    text.chars()
        .flat_map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
            .to_lowercase()
        })
        .collect()
}

fn assistant_file_setting_candidates(schema: Option<&str>, file: &str) -> Result<Value, String> {
    let filename = file.rsplit('/').next().unwrap_or(file);
    let evidence = assistant_search_settings_evidence("{}", schema, filename, 0)?;
    let entries = evidence["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| {
            let property = &entry["schema"];
            let text = |field| {
                truncate_assistant_prompt_text(property[field].as_str().unwrap_or_default(), 384)
            };
            json!({"key": entry["key"], "type": property["type"],
                "title": text("title"), "description": text("description"),
                "source": text("x-lsgm-source"), "sourceKey": text("x-lsgm-source-key")})
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"entries": entries, "totalMatches": evidence["totalMatches"],
        "nextOffset": evidence["nextOffset"],
        "note": "Metadata matches for this filename. Confirm the file/shard scope and read the setting before proposing customize_config. Search settings with more specific terms if needed."}),
    )
}

#[cfg(test)]
#[path = "settings_search_tests.rs"]
mod settings_search_tests;
