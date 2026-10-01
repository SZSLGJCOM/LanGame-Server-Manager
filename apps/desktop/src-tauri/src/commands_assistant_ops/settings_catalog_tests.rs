use super::*;

fn delivered_catalog(settings: &str, offset: usize) -> Value {
    let page = assistant_settings_catalog_evidence(settings, offset).unwrap();
    let envelope = json!({"ok": true, "data": page});
    let encoded = assistant_tool_result_text(&envelope);
    assert!(encoded.len() <= ASSISTANT_TOOL_RESULT_BYTES);
    let delivered: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        delivered, envelope,
        "catalog survives final encoding intact"
    );
    assert!(page["keys"].to_string().len() <= ASSISTANT_SETTINGS_CATALOG_PAGE_BYTES);
    page
}

#[test]
fn settings_catalog_lists_all_real_dst_default_keys_in_one_evidence_page() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../../../modules/dontstarve/schema.json"
    ))
    .unwrap();
    let properties = schema["properties"].as_object().unwrap();
    let mut expected = properties.keys().cloned().collect::<Vec<_>>();
    expected.push(String::from("bind_ip"));
    expected.sort_unstable();
    expected.dedup();
    let mut current = properties
        .iter()
        .map(|(key, property)| (key.clone(), property["default"].clone()))
        .collect::<serde_json::Map<_, _>>();
    current.insert(String::from("bind_ip"), json!("0.0.0.0"));
    let page = delivered_catalog(&Value::Object(current).to_string(), 0);
    assert_eq!(page["keys"], json!(expected));
    assert_eq!(page["totalKeys"], expected.len());
    assert_eq!(page["omittedSensitiveKeys"], 0);
    assert!(page["nextOffset"].is_null());
    for key in [
        "offline_cluster",
        "lan_only_cluster",
        "disable_data_collection",
        "master_world_size",
        "max_players",
        "bind_ip",
        "shard_layout",
        "islands_enabled_workshop_mod_ids",
        "volcano_enabled_workshop_mod_ids",
    ] {
        assert!(page["keys"].as_array().unwrap().contains(&json!(key)));
    }
}

#[test]
fn settings_catalog_pages_unicode_and_escaped_keys_without_loss_or_duplicates() {
    let current = (0..300)
        .rev()
        .map(|index| {
            (
                format!(
                    "setting_{index:04}_{}\"quoted\\suffix\u{1}",
                    "界".repeat(24)
                ),
                Value::Null,
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let expected = current.keys().cloned().collect::<Vec<_>>();
    let settings = Value::Object(current).to_string();
    let mut collected = Vec::new();
    let mut offset = 0;
    let mut pages = 0;
    loop {
        let page = delivered_catalog(&settings, offset);
        assert_eq!(page["totalKeys"], expected.len());
        assert_eq!(page["omittedSensitiveKeys"], 0);
        collected.extend(
            page["keys"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| key.as_str().unwrap().to_owned()),
        );
        pages += 1;
        assert!(pages <= expected.len());
        let Some(next) = page["nextOffset"].as_u64() else {
            break;
        };
        assert!(next as usize > offset);
        offset = next as usize;
    }
    assert!(pages > 1, "encoded byte budget must cause pagination");
    assert_eq!(collected, expected);
}

#[test]
fn settings_catalog_advances_over_sensitive_names_without_exposing_names_or_values() {
    let credential = ["synthetic", "catalog", "credential"].join("-");
    let sensitive_name = format!("password={credential}");
    let private_name = String::from("C:/fixture/private/setting");
    let mut current = (0..510)
        .map(|index| (format!("a{index:03}"), json!(credential)))
        .collect::<serde_json::Map<_, _>>();
    current.insert(String::from("admin_password"), json!(credential));
    current.insert(private_name.clone(), Value::Null);
    current.insert(sensitive_name.clone(), Value::Null);
    current.insert(
        String::from("z_last"),
        json!({"schema_only_candidate": true}),
    );
    let expected = current
        .keys()
        .filter(|key| *key != &sensitive_name && *key != &private_name)
        .cloned()
        .collect::<Vec<_>>();
    let settings = Value::Object(current).to_string();
    let first = delivered_catalog(&settings, 0);
    assert_eq!(first["totalKeys"], 514);
    assert_eq!(first["nextOffset"], 512);
    assert_eq!(first["omittedSensitiveKeys"], 1);
    let second = delivered_catalog(&settings, 512);
    assert_eq!(second["omittedSensitiveKeys"], 1);
    assert!(second["nextOffset"].is_null());
    let keys = [
        first["keys"].as_array().unwrap(),
        second["keys"].as_array().unwrap(),
    ]
    .into_iter()
    .flatten()
    .cloned()
    .collect::<Vec<_>>();
    assert_eq!(json!(keys), json!(expected));
    let text = json!([first, second]).to_string();
    for forbidden in [
        credential.as_str(),
        private_name.as_str(),
        "schema_only_candidate",
        "[REDACTED]",
    ] {
        assert!(!text.contains(forbidden));
    }
    assert!(text.contains("admin_password"));
}

#[test]
fn settings_catalog_rejects_unreadable_keys_and_invalid_input_without_echoing_them() {
    let long_key = "界".repeat(86);
    let settings = json!({long_key.clone(): null}).to_string();
    let error = assistant_settings_catalog_evidence(&settings, 0).unwrap_err();
    assert!(error.contains("256"));
    assert!(!error.contains(&long_key));
    for invalid in ["[]", "null", "not-json"] {
        assert!(assistant_settings_catalog_evidence(invalid, 0).is_err());
    }
    for offset in [1, usize::MAX] {
        let empty = delivered_catalog(r#"{"actual_key":null}"#, offset);
        assert_eq!(empty["keys"], json!([]));
        assert_eq!(empty["totalKeys"], 1);
        assert!(empty["nextOffset"].is_null());
    }
    let empty = delivered_catalog("{}", 0);
    assert_eq!(empty["keys"], json!([]));
    assert_eq!(empty["totalKeys"], 0);
    assert!(empty["nextOffset"].is_null());
}
