const ASSISTANT_SETTINGS_CATALOG_PAGE_BYTES: usize = 8 * 1024;
const ASSISTANT_SETTINGS_CATALOG_PAGE_KEYS: usize = 512;
const ASSISTANT_SETTINGS_CATALOG_KEY_BYTES: usize = 256;

fn assistant_settings_catalog_evidence(settings: &str, offset: usize) -> Result<Value, String> {
    let current: Value = serde_json::from_str(settings)
        .map_err(|_| String::from("Instance settings are not valid JSON."))?;
    let current = current
        .as_object()
        .ok_or_else(|| String::from("Instance settings must be an object."))?;
    let mut available = current.keys().collect::<Vec<_>>();
    available.sort_unstable();
    let mut keys = Vec::new();
    let mut encoded_bytes = 2;
    let mut next_offset = offset.min(available.len());
    let mut omitted_sensitive_keys = 0;
    for (index, key) in available
        .iter()
        .enumerate()
        .skip(offset)
        .take(ASSISTANT_SETTINGS_CATALOG_PAGE_KEYS)
    {
        if key.len() > ASSISTANT_SETTINGS_CATALOG_KEY_BYTES {
            return Err(String::from(
                "A setting key exceeds the 256-byte read limit; no truncated key was supplied.",
            ));
        }
        let encoded_key = serde_json::to_string(key).map_err(|error| error.to_string())?;
        // Check the name as a JSON string, matching its eventual array context.
        // Never publish a redacted replacement as if it were a real setting key.
        let redacted: Value =
            serde_json::from_str(&redact_assistant_provider_text(&encoded_key))
                .map_err(|_| String::from("Setting names could not be safely encoded."))?;
        if redacted.as_str() != Some(key.as_str()) {
            omitted_sensitive_keys += 1;
            next_offset = index + 1;
            continue;
        }
        let needed = encoded_key.len() + usize::from(!keys.is_empty());
        if encoded_bytes + needed > ASSISTANT_SETTINGS_CATALOG_PAGE_BYTES {
            break;
        }
        encoded_bytes += needed;
        keys.push(*key);
        next_offset = index + 1;
    }
    Ok(json!({
        "keys": keys,
        "totalKeys": available.len(),
        "nextOffset": (next_offset < available.len()).then_some(next_offset),
        "omittedSensitiveKeys": omitted_sensitive_keys,
        "note": "Only current setting keys are listed; values and schema-only candidates are omitted. Changes still require task and setting validation. Read exact keys with read_settings for their values and schema. omittedSensitiveKeys counts names withheld on this page; nextOffset advances over original keys."
    }))
}

#[cfg(test)]
#[path = "settings_catalog_tests.rs"]
mod settings_catalog_tests;
