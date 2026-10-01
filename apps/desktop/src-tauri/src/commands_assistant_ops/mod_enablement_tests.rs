use super::*;

#[test]
fn installed_mod_evidence_distinguishes_declaration_from_runtime_enablement() {
    let projection = json!({"shards": [
        {"shard": "master", "active": true, "analysisStatus": "known",
         "declaredEnabledModNames": ["consumer"], "declaredDisabledModNames": ["disabled"],
         "declaredUnspecifiedModNames": ["options_only"]},
        {"shard": "caves", "active": false, "analysisStatus": "unknown",
         "declaredEnabledModNames": null, "declaredDisabledModNames": null,
         "declaredUnspecifiedModNames": null}
    ]});
    for (name, expected) in [
        ("consumer", "enabled"),
        ("dependency", "not_declared"),
        ("disabled", "disabled"),
        ("options_only", "unspecified"),
    ] {
        let states = assistant_mod_declared_enablement(&projection, name);
        assert_eq!(states[0]["declaredState"], expected);
        assert_eq!(states[0]["active"], true);
        assert_eq!(states[1]["declaredState"], "unknown");
        assert_eq!(states[1]["active"], false);
        let wire: Value = serde_json::from_str(&assistant_tool_result_text(&json!({
            "ok": true, "data": {"entries": [{"folderName": name, "configuredEnablementByShard": states}]}
        }))).unwrap();
        assert_eq!(
            wire["data"]["entries"][0]["configuredEnablementByShard"][0]["declaredState"],
            expected
        );
    }
}

#[test]
fn incomplete_mod_declaration_never_becomes_a_disabled_or_empty_mod_list() {
    for shard in [
        json!({"shard": "master", "active": true, "analysisStatus": "unknown"}),
        json!({"shard": "master", "active": true, "analysisStatus": "known"}),
        json!({"shard": "master", "active": true, "analysisStatus": "known", "declaredEnabledModNames": []}),
        json!({"shard": "master", "active": true, "analysisStatus": "known",
            "declaredEnabledModNames": [null], "declaredDisabledModNames": [], "declaredUnspecifiedModNames": []}),
    ] {
        let states = assistant_mod_declared_enablement(&json!({"shards": [shard]}), "dependency");
        assert_eq!(states[0]["declaredState"], "unknown");
    }
}

#[test]
fn workshop_aliases_do_not_become_proof_that_a_directory_is_disabled() {
    let projection = json!({"shards": [{"shard": "master", "active": true, "analysisStatus": "known",
        "declaredEnabledModNames": ["123456"], "declaredDisabledModNames": [], "declaredUnspecifiedModNames": []}]});
    let states = assistant_mod_declared_enablement(&projection, "workshop-123456");
    assert_eq!(states[0]["declaredState"], "unknown");
    assert_eq!(states[0]["matching"], "exact_directory");
}
