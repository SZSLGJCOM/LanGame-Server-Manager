use super::*;

#[test]
fn settings_evidence_distinguishes_missing_nullable_and_schema_only_keys() {
    let settings = json!({"nullable_setting": null, "bind_ip": "0.0.0.0"});
    let schema = json!({"properties": {
        "nullable_setting": {"type": ["string", "null"]},
        "schema_candidate": {"type": "boolean"}
    }});
    let requested = [
        "nullable_setting",
        "bind_ip",
        "missing_setting",
        "schema_candidate",
    ]
    .map(String::from);
    let result = assistant_settings_evidence(
        &settings.to_string(),
        Some(&schema.to_string()),
        &requested,
        0,
    )
    .unwrap();
    let entries = result["entries"].as_array().unwrap();
    assert_eq!(entries[0]["value"], Value::Null);
    assert_eq!(entries[0]["exists"], true);
    assert_eq!(entries[0]["schemaExists"], true);
    assert_eq!(entries[1]["value"], "0.0.0.0");
    assert_eq!(entries[1]["exists"], true);
    assert_eq!(entries[1]["schemaExists"], false);
    assert_eq!(entries[1]["schema"], Value::Null);
    assert_eq!(entries[2]["value"], Value::Null);
    assert_eq!(entries[2]["exists"], false);
    assert_eq!(entries[2]["schemaExists"], false);
    assert_eq!(entries[3]["value"], Value::Null);
    assert_eq!(entries[3]["exists"], false);
    assert_eq!(entries[3]["schemaExists"], true);
    assert_eq!(entries[3]["schema"]["type"], "boolean");
    assert_eq!(
        result["unknownKeys"],
        json!(["missing_setting", "schema_candidate"])
    );
    let hint = result["hint"].as_str().expect("missing-key recovery hint");
    assert!(hint.contains("search_settings"));
    assert!(hint.contains("empty keys"));
    assert!(hint.contains("schema-only"));
    assert!(hint.len() < 512);
    assert_eq!(result["totalKeys"], 3);
    assert!(result["nextOffset"].is_null());
}

#[test]
fn settings_evidence_keeps_pagination_and_presence_independent_of_value() {
    let settings = (0..21)
        .map(|index| (format!("setting_{index:02}"), Value::Null))
        .collect::<serde_json::Map<_, _>>();
    let settings = Value::Object(settings).to_string();
    for (offset, count, next) in [(0, 20, Some(20)), (20, 1, None)] {
        let page = assistant_settings_evidence(&settings, None, &[], offset).unwrap();
        let entries = page["entries"].as_array().unwrap();
        assert_eq!(entries.len(), count);
        assert!(entries.iter().all(|entry| entry["exists"] == true));
        assert!(entries.iter().all(|entry| entry["value"].is_null()));
        assert!(entries.iter().all(|entry| entry["schemaExists"] == false));
        assert_eq!(page["unknownKeys"], json!([]));
        assert_eq!(page["nextOffset"], json!(next));
        assert!(page.get("hint").is_none());
    }
    assert!(assistant_settings_evidence(&settings, None, &vec!["key".into(); 41], 0).is_err());
    assert!(assistant_settings_evidence(&settings, None, &["x".repeat(257)], 0).is_err());
}

#[test]
fn settings_evidence_presence_does_not_expose_sensitive_values_or_schema_defaults() {
    let credential = ["synthetic", "provider", "credential"].join("-");
    let settings = json!({"admin_password": credential});
    let schema = json!({"properties": {
        "admin_password": {"type": "string", "default": credential, "examples": [credential]}
    }});
    let evidence = assistant_settings_evidence(
        &settings.to_string(),
        Some(&schema.to_string()),
        &[String::from("admin_password")],
        0,
    )
    .unwrap();
    let delivered = assistant_tool_result_text(&json!({"ok": true, "data": evidence}));
    assert!(!delivered.contains(&credential));
    let result: Value = serde_json::from_str(&delivered).unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(result["data"]["entries"][0]["exists"], true);
    assert_eq!(result["data"]["entries"][0]["schemaExists"], true);
    assert_eq!(result["data"]["entries"][0]["value"], "[REDACTED]");
    assert_eq!(result["data"]["unknownKeys"], json!([]));
}
