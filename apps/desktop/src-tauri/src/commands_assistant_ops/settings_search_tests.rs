use super::*;

#[test]
fn settings_search_discovers_native_worldgen_mapping_beyond_initial_schema_pages() {
    let schema = include_str!("../../../../../modules/dontstarve/schema.json");
    let parsed: Value = serde_json::from_str(schema).unwrap();
    let keys = parsed["properties"].as_object().unwrap();
    assert!(keys.len() > 300);
    assert!(
        keys.keys()
            .position(|key| key == "master_worldgenoverride_lua")
            .unwrap()
            > 160
    );
    let settings = json!({"master_worldgenoverride_lua": "return { preset = }"});
    let result = assistant_search_settings_evidence(
        &settings.to_string(),
        Some(schema),
        "WORLDGENOVERRIDE.LUA",
        0,
    )
    .unwrap();
    let master = result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key"] == "master_worldgenoverride_lua")
        .expect("native filename discovers the modeled setting in one read");
    assert_eq!(master["value"], settings["master_worldgenoverride_lua"]);
    assert_eq!(
        master["schema"]["x-lsgm-source-key"],
        "worldgenoverride.lua:raw_lua"
    );
    assert!(result["entries"].as_array().unwrap().len() <= 10);
}

#[test]
fn settings_search_explains_overconstrained_native_mod_queries_without_changing_matches() {
    let schema = include_str!("../../../../../modules/dontstarve/schema.json");
    let settings = json!({
        "master_modoverrides_lua": "return { ['local_fixture'] = { enabled = true } }"
    });
    let no_match = assistant_search_settings_evidence(
        &settings.to_string(),
        Some(schema),
        "mod enable enablement loaded registered mods modlist modstate",
        0,
    )
    .unwrap();
    assert_eq!(no_match["entries"], json!([]));
    assert_eq!(no_match["totalMatches"], 0);
    assert!(no_match["nextOffset"].is_null());
    assert_eq!(no_match["matching"]["mode"], "all_terms");
    assert_eq!(no_match["matching"]["termCount"], 8);
    let hint = no_match["hint"].as_str().expect("zero-match recovery hint");
    assert!(hint.contains("fewer terms"));
    assert!(hint.contains("configuration filename"));
    assert!(hint.contains("does not establish"));

    let by_filename = assistant_search_settings_evidence(
        &settings.to_string(),
        Some(schema),
        "modoverrides.lua",
        0,
    )
    .unwrap();
    let master = by_filename["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key"] == "master_modoverrides_lua")
        .expect("the native filename still discovers the editable setting");
    assert_eq!(master["value"], settings["master_modoverrides_lua"]);
    assert_eq!(by_filename["matching"]["mode"], "all_terms");
    assert_eq!(by_filename["matching"]["termCount"], 2);
    assert_eq!(master["exists"], true);
    assert!(by_filename["totalMatches"].as_u64().unwrap() > 0);
    assert!(
        by_filename["unknownKeys"]
            .as_array()
            .unwrap()
            .contains(&json!("caves_modoverrides_lua"))
    );
    let hint = by_filename["hint"].as_str().unwrap();
    assert!(hint.contains("schema-only"));
    assert!(!hint.contains("No setting matched"));
}

#[test]
fn settings_search_normalizes_unicode_and_punctuation_and_requires_every_term() {
    let settings = json!({"unmodeled_worldgen": "kept"});
    let schema = json!({"properties": {
        "mapped_field": {"title": "Überblick", "description": "主世界配置", "x-lsgm-source": "MASTER_WORLDGEN", "x-lsgm-source-key": "worldgenoverride.lua:raw_lua"},
        "unrelated_field": {"title": "Überblick", "description": "洞穴配置", "x-lsgm-source": "caves_worldgen"}
    }});
    let result = assistant_search_settings_evidence(
        &settings.to_string(),
        Some(&schema.to_string()),
        "ÜBERBLICK / 主世界配置 MASTER-worldgen",
        0,
    )
    .unwrap();
    assert_eq!(result["totalMatches"], 1);
    assert_eq!(result["entries"][0]["key"], "mapped_field");
    assert_eq!(result["entries"][0]["value"], Value::Null);

    let by_key =
        assistant_search_settings_evidence(&settings.to_string(), None, "unmodeled worldgen", 0)
            .unwrap();
    assert_eq!(by_key["entries"][0]["value"], "kept");
}

#[test]
fn settings_search_pages_matches_stably_and_does_not_fall_back_to_all_settings() {
    let properties = (0..25)
        .rev()
        .map(|index| {
            (
                format!("setting_{index:02}"),
                json!({"title": "World generation", "type": "integer"}),
            )
        })
        .chain([(String::from("unrelated"), json!({"title": "Port"}))])
        .collect::<serde_json::Map<_, _>>();
    let schema = json!({"properties": properties}).to_string();
    let mut collected = Vec::new();
    for (offset, count, next) in [(0, 10, Some(10)), (10, 10, Some(20)), (20, 5, None)] {
        let page =
            assistant_search_settings_evidence("{}", Some(&schema), "world", offset).unwrap();
        let entries = page["entries"].as_array().unwrap();
        assert_eq!(entries.len(), count);
        assert_eq!(page["totalMatches"], 25);
        assert_eq!(page["totalKeys"], 26);
        assert_eq!(page["nextOffset"], json!(next));
        collected.extend(
            entries
                .iter()
                .map(|entry| entry["key"].as_str().unwrap().to_owned()),
        );
    }
    assert_eq!(
        collected,
        (0..25)
            .map(|index| format!("setting_{index:02}"))
            .collect::<Vec<_>>()
    );
    for offset in [25, usize::MAX] {
        let page =
            assistant_search_settings_evidence("{}", Some(&schema), "world", offset).unwrap();
        assert_eq!(page["entries"], json!([]));
        assert_eq!(page["totalMatches"], 25);
        assert_eq!(page["nextOffset"], Value::Null);
        assert!(page.get("hint").is_none());
    }
    let missing =
        assistant_search_settings_evidence("{}", Some(&schema), "no_matching_setting", 0).unwrap();
    assert_eq!(missing["entries"], json!([]));
    assert_eq!(missing["totalMatches"], 0);
    assert_eq!(missing["nextOffset"], Value::Null);
}

#[test]
fn settings_search_never_matches_current_values_or_schema_defaults() {
    let credential = ["Synthetic", "Secret", "Value42"].concat();
    let schema_credential = ["Synthetic", "Default", "Value42"].concat();
    let settings = json!({"admin_password": credential, "description": "PrivateCurrentValue42"});
    let schema = json!({"properties": {
        "admin_password": {"type": "string", "title": "Administrator password", "default": schema_credential,
            "examples": [schema_credential], "enum": [schema_credential], "const": schema_credential}
    }});
    for query in [
        credential.as_str(),
        "PrivateCurrentValue42",
        schema_credential.as_str(),
    ] {
        let result = assistant_search_settings_evidence(
            &settings.to_string(),
            Some(&schema.to_string()),
            query,
            0,
        )
        .unwrap();
        assert_eq!(result["totalMatches"], 0);
        assert_eq!(result["entries"], json!([]));
        assert_eq!(result["matching"]["mode"], "all_terms");
        assert!(result["hint"].is_string());
        assert!(!result.to_string().contains(query));
    }

    let result = assistant_search_settings_evidence(
        &settings.to_string(),
        Some(&schema.to_string()),
        "administrator password",
        0,
    )
    .unwrap();
    assert_eq!(result["entries"][0]["key"], "admin_password");
    assert_eq!(result["entries"][0]["value"], "[REDACTED]");
    assert!(!result.to_string().contains(&credential));
    assert!(!result.to_string().contains(&schema_credential));
    assert_eq!(result["entries"][0]["schema"]["type"], "string");
    assert_eq!(
        result["entries"][0]["schema"]["title"],
        "Administrator password"
    );
    for keyword in ["default", "examples", "enum", "const"] {
        assert_eq!(result["entries"][0]["schema"][keyword], "[REDACTED]");
    }
    let provider_text = assistant_tool_result_text(&json!({"ok": true, "data": result}));
    assert!(!provider_text.contains(&credential));
    assert!(!provider_text.contains(&schema_credential));
}

#[test]
fn settings_search_rejects_invalid_queries_without_echoing_them() {
    for query in [
        String::new(),
        String::from("   "),
        String::from("._-:/"),
        "SyntheticPrivateQuery".repeat(8),
        "界".repeat(43),
        String::from("one two three four five six seven eight secret_ninth"),
    ] {
        let error = assistant_search_settings_evidence("{}", None, &query, 0).unwrap_err();
        if !query.is_empty() {
            assert!(!error.contains(&query));
        }
        assert!(!error.contains("secret_ninth"));
    }
    for query in [
        "a".repeat(128),
        String::from("one two three four five six seven eight"),
    ] {
        assert!(assistant_search_settings_evidence("{}", None, &query, 0).is_ok());
    }
}
