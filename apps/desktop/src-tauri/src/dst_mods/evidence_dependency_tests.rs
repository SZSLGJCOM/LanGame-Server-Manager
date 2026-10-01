use super::dst_mod_dependency_names;
use serde_json::{Value, json};

#[test]
fn dst_mod_dependency_names_preserves_direct_candidates_without_guessing_display_names() {
    let metadata = json!({"mod_dependencies": [
        {"Display Library": true, "declared_library": false, "workshop": "workshop-123456"},
        {"declared_library": false, "local addon.v1": false, "汉化 包.v1": false}
    ]});
    assert_eq!(
        dst_mod_dependency_names(&metadata),
        [
            "declared_library",
            "workshop-123456",
            "local addon.v1",
            "汉化 包.v1"
        ]
    );
}

#[test]
fn dst_mod_dependency_names_requires_strict_boolean_false_for_directory_keys() {
    let metadata = json!({"mod_dependencies": [{
        "directory": false, "display_name": true, "text": "false", "numeric": 0,
        "missing": null, "list": [], "table": {}, "workshop": false
    }]});
    assert_eq!(dst_mod_dependency_names(&metadata), ["directory"]);
}

#[test]
fn dst_mod_dependency_names_accepts_only_explicit_workshop_directory_identifiers() {
    for value in [
        json!("123456"),
        json!(123456),
        json!("workshop-"),
        json!("workshop-123x"),
        json!("workshop-123/../other"),
        json!("https://example.invalid/workshop-123456"),
        json!("local_folder"),
        json!(false),
        json!(true),
        Value::Null,
    ] {
        let metadata = json!({"mod_dependencies": [{"workshop": value}]});
        assert!(dst_mod_dependency_names(&metadata).is_empty(), "{metadata}");
    }
    let metadata = json!({"mod_dependencies": [{"workshop": "workshop-123456"}]});
    assert_eq!(dst_mod_dependency_names(&metadata), ["workshop-123456"]);
}

#[test]
fn dst_mod_dependency_names_ignores_malformed_groups_without_flattening_or_recursing() {
    for metadata in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"mod_dependencies": "local_folder"}),
        json!({"mod_dependencies": {"local_folder": false}}),
        json!({"mod_dependencies": ["local_folder", 42, null, false, ["local_folder"]]}),
        json!({"mod_dependencies": [{"nested": {"local_folder": false}}]}),
    ] {
        assert!(dst_mod_dependency_names(&metadata).is_empty(), "{metadata}");
    }
    let metadata = json!({"mod_dependencies": [null, {"actual_directory": false}]});
    assert_eq!(dst_mod_dependency_names(&metadata), ["actual_directory"]);
}

#[test]
fn dst_mod_dependency_names_reuses_directory_safety_checks() {
    for name in [
        "",
        ".",
        "..",
        "../other",
        "C:\\mods",
        "folder/name",
        "folder\\name",
        "name:stream",
        "name.",
        "name ",
        "workshop-12x",
        "CON",
        "nul.txt",
        "COM1",
        "LPT9.lua",
        "CONIN$",
        "COM¹",
        "bad\nname",
        "bad\tname",
        &"a".repeat(129),
    ] {
        let metadata = json!({"mod_dependencies": [{(name): false}]});
        assert!(dst_mod_dependency_names(&metadata).is_empty(), "{name:?}");
    }
}

#[test]
fn dst_mod_dependency_names_deduplicates_and_caps_five_in_stable_group_order() {
    let metadata = json!({"mod_dependencies": [
        {"z_first_group": false},
        {"z_first_group": false, "beta": false, "alpha": false},
        {"workshop": "workshop-123456", "fourth": false},
        {"last": false}
    ]});
    assert_eq!(
        dst_mod_dependency_names(&metadata),
        [
            "z_first_group",
            "alpha",
            "beta",
            "fourth",
            "workshop-123456"
        ]
    );
}
