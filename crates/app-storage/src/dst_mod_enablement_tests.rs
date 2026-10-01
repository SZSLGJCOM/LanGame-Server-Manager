use super::*;

#[test]
fn island_adventures_requires_two_author_mods_in_every_enabled_world() {
    let mut settings = json!({"shard_layout":"island_adventures"});
    for spec in app_core::dst_shards::DST_SHARDS {
        settings[format!("{}_enabled_workshop_mod_ids", spec.process_key)] =
            json!("3435352667,1467214795");
    }
    validate_dst_shard_mod_requirements(&settings).unwrap();
    let evidence = inspect_dst_mod_enablement(&settings).unwrap();
    assert_eq!(evidence["shards"].as_array().unwrap().len(), 4);
    settings["volcano_enabled_workshop_mod_ids"] = json!("3435352667");
    let error = validate_dst_shard_mod_requirements(&settings).unwrap_err();
    assert!(error.contains("volcano") && error.contains("1467214795"));
    settings["volcano_enabled_workshop_mod_ids"] = json!("3435352667,1467214795,1505270912");
    assert!(
        validate_dst_shard_mod_requirements(&settings)
            .unwrap_err()
            .contains("conflicts")
    );
    validate_dst_shard_mod_requirements(&json!({})).unwrap();
}
use serde_json::json;

fn shard(result: &Value, name: &str) -> Value {
    result["shards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["shard"] == name)
        .unwrap()
        .clone()
}

fn assert_unknown(entry: &Value) {
    assert_eq!(entry["analysisStatus"], "unknown");
    for field in [
        "declaredEnabledModNames",
        "declaredDisabledModNames",
        "declaredUnspecifiedModNames",
        "clientModsDisabled",
    ] {
        assert!(
            entry[field].is_null(),
            "{field} must not imply empty known state"
        );
    }
    assert!(!entry["reason"].as_str().unwrap().is_empty());
}

#[test]
fn dst_mod_enablement_projects_static_declarations_per_shard_without_execution() {
    let result = inspect_dst_mod_enablement(&json!({
        "enable_caves": true,
        "master_modoverrides_lua": r#"-- Mod declarations
return {
    ['local_active'] = {enabled = true, configuration_options = {custom_label = 'private_option'}},
    ['local_disabled'] = {enabled = false},
    ['local_unspecified'] = {configuration_options = {}},
    ['local_nil'] = {enabled = nil},
    client_mods_disabled = true,
}"#,
        "caves_modoverrides_lua": "return { ['cave_mod'] = {enabled=true} }"
    }))
    .unwrap();
    let master = shard(&result, "master");
    assert_eq!(master["active"], true);
    assert_eq!(master["analysisStatus"], "known");
    assert_eq!(master["declaredEnabledModNames"], json!(["local_active"]));
    assert_eq!(
        master["declaredDisabledModNames"],
        json!(["local_disabled"])
    );
    assert_eq!(
        master["declaredUnspecifiedModNames"],
        json!(["local_nil", "local_unspecified"])
    );
    assert_eq!(master["clientModsDisabled"], true);
    let caves = shard(&result, "caves");
    assert_eq!(caves["active"], true);
    assert_eq!(caves["declaredEnabledModNames"], json!(["cave_mod"]));
    assert!(!result.to_string().contains("private_option"));
    assert!(result["limitation"].as_str().unwrap().contains("not proof"));
}

#[test]
fn dst_mod_enablement_uses_materializer_raw_precedence_and_structured_fallback() {
    for (raw, expected) in [
        ("return { ['local_raw']={enabled=true} }", vec!["local_raw"]),
        ("return {}", vec![]),
        (
            "return {\r\n}\r\n",
            vec!["workshop-123456", "workshop-654321"],
        ),
        ("  ", vec!["workshop-123456", "workshop-654321"]),
    ] {
        let result = inspect_dst_mod_enablement(&json!({
            "master_modoverrides_lua": raw,
            "master_enabled_workshop_mod_ids": "123456\n654321\n123456",
            "caves_enabled_workshop_mod_ids": "987654"
        }))
        .unwrap();
        assert_eq!(
            shard(&result, "master")["declaredEnabledModNames"],
            json!(expected)
        );
        let caves = shard(&result, "caves");
        assert_eq!(caves["active"], false);
        assert_eq!(caves["declaredEnabledModNames"], json!(["workshop-987654"]));
    }
    let result = inspect_dst_mod_enablement(&json!({"enable_caves": "true"})).unwrap();
    assert_eq!(shard(&result, "caves")["active"], false);
}

#[test]
fn dst_mod_enablement_marks_executable_ambiguous_and_unsupported_lua_unknown() {
    for source in [
        "local enabled = true; return {mod={enabled=enabled}}",
        "return build_mods()",
        "while true do end",
        "return {mod={enabled=true}, ['mod']={enabled=false}}",
        "return {mod={enabled=true, enabled=false}}",
        "return {mod={enabled=1}}",
        "return {mod={enabled='true'}}",
        "return {mod=false}",
        "return {[123456]={enabled=true}}",
        "return {client_mods_disabled='true'}",
    ] {
        let result =
            inspect_dst_mod_enablement(&json!({"master_modoverrides_lua": source})).unwrap();
        assert_unknown(&shard(&result, "master"));
        assert_eq!(shard(&result, "caves")["analysisStatus"], "known");
    }
}

#[test]
fn dst_mod_enablement_bounds_total_input_and_returns_no_partial_name_lists() {
    let oversized = format!(
        "return {{mod={{enabled=true,note='{}'}}}}",
        "x".repeat(128 * 1024)
    );
    let result =
        inspect_dst_mod_enablement(&json!({"master_modoverrides_lua": oversized})).unwrap();
    assert_unknown(&shard(&result, "master"));
    assert_unknown(&shard(&result, "caves"));

    let make_mods = |count: usize, name_prefix: &str| {
        format!(
            "return {{{}}}",
            (0..count)
                .map(|index| format!("['{name_prefix}_{index:03}']={{enabled=true}}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let at_limit = inspect_dst_mod_enablement(&json!({
        "master_modoverrides_lua": make_mods(50, "master"),
        "caves_modoverrides_lua": make_mods(50, "caves")
    }))
    .unwrap();
    assert_eq!(
        shard(&at_limit, "master")["declaredEnabledModNames"]
            .as_array()
            .unwrap()
            .len(),
        50
    );
    assert_eq!(
        shard(&at_limit, "caves")["declaredEnabledModNames"]
            .as_array()
            .unwrap()
            .len(),
        50
    );

    for settings in [
        json!({"master_modoverrides_lua": make_mods(101, "mod")}),
        json!({"master_modoverrides_lua": make_mods(51, "master"), "caves_modoverrides_lua": make_mods(50, "caves")}),
        json!({"master_modoverrides_lua": make_mods(50, &"long".repeat(21))}),
        json!({"master_modoverrides_lua": make_mods(50, &"\\001".repeat(40))}),
    ] {
        let result = inspect_dst_mod_enablement(&settings).unwrap();
        assert_unknown(&shard(&result, "master"));
        assert_unknown(&shard(&result, "caves"));
        assert!(result.to_string().len() < 2048);
    }
}

#[test]
fn dst_mod_enablement_requires_a_settings_object_and_does_not_echo_invalid_input() {
    for settings in [Value::Null, json!([]), json!("private invalid input")] {
        let error = inspect_dst_mod_enablement(&settings).unwrap_err();
        assert!(error.contains("object"));
        assert!(!error.contains("private invalid input"));
    }
}
