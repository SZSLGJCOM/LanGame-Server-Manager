pub(crate) fn redact_assistant_file_text(file_key: &str, text: &str) -> String {
    if assistant_key_is_sensitive(file_key) {
        return String::from(ASSISTANT_REDACTED_VALUE);
    }
    // The file reader preserves a UTF-8 BOM for exact replacements. Parse its
    // document without the BOM, while retaining the original safe source bytes.
    let json_text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if let Ok(original) = serde_json::from_str::<Value>(json_text) {
        let mut redacted = original.clone();
        redact_assistant_json_value(&mut redacted, None);
        // Value discards earlier duplicate keys. Equality therefore proves the
        // original bytes safe only after checking every map before key collapse.
        let literal_is_safe = redacted == original
            && serde_json::from_str::<AssistantUniqueJsonKeys>(json_text).is_ok();
        return if literal_is_safe {
            text.to_owned()
        } else {
            serde_json::to_string_pretty(&redacted)
                .unwrap_or_else(|_| String::from(ASSISTANT_REDACTED_VALUE))
        };
    }
    redact_assistant_provider_lines(text)
}

struct AssistantUniqueJsonKeys;

impl<'de> serde::Deserialize<'de> for AssistantUniqueJsonKeys {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(AssistantUniqueJsonKeysVisitor)
    }
}

struct AssistantUniqueJsonKeysVisitor;

impl<'de> serde::de::Visitor<'de> for AssistantUniqueJsonKeysVisitor {
    type Value = AssistantUniqueJsonKeys;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JSON with unique keys in every object")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(AssistantUniqueJsonKeys)
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(AssistantUniqueJsonKeys)
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(AssistantUniqueJsonKeys)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(AssistantUniqueJsonKeys)
    }

    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(AssistantUniqueJsonKeys)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(AssistantUniqueJsonKeys)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        while sequence
            .next_element::<AssistantUniqueJsonKeys>()?
            .is_some()
        {}
        Ok(AssistantUniqueJsonKeys)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut keys = std::collections::HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                // Do not echo a potentially sensitive key or source fragment.
                return Err(serde::de::Error::custom("duplicate JSON object key"));
            }
            map.next_value::<AssistantUniqueJsonKeys>()?;
        }
        Ok(AssistantUniqueJsonKeys)
    }
}

#[cfg(test)]
#[path = "assistant_file_redaction_tests.rs"]
mod file_redaction_tests;
