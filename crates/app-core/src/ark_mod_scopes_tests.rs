use super::*;
use serde_json::json;

fn settings() -> Value {
    json!({
        "server_name": "Shared room", "mod_ids_csv": "111111,222222", "passive_mod_ids_csv": "333333",
        "active_mod_ids": "111111,222222", "auto_managed_mod_ids": "111111\n222222\n999999",
        "asa_mod_configurations": {
            "111111": {"game_ini": "[One]\nGame=Shared", "game_user_settings_ini": "[One]\nUser=Shared"},
            "222222": {"game_ini": "[Two]\nGame=Shared"}
        },
        "curseforge_disabled_mod_ids": ["444444"], "curseforge_removed_mod_ids": ["555555"],
        "additional_maps": [
            {"id": "one", "map_name": "ScorchedEarth_WP", "name": "One", "enabled": true},
            {"id": "two", "map_name": "TheCenter_WP", "name": "Two", "enabled": true},
            {"id": "paused", "map_name": "TheIsland_WP", "name": "Paused", "enabled": false}
        ]
    })
}

#[test]
fn two_map_mod_targets_keep_other_mods_and_future_shared_changes_inherited() {
    let mut settings = settings();
    settings[OVERRIDES_FIELD] = json!({
        "main": {"mods": {"111111": {"active": true}}},
        "map-one": {"mods": {"111111": {"active": true}}},
        "map-two": {"mods": {"111111": {"active": false}}}
    });
    for (key, ids) in [
        ("main", "111111,222222"),
        ("map-one", "111111,222222"),
        ("map-two", "222222"),
    ] {
        assert_eq!(
            effective_settings("arksurvivalascended", &settings, key).unwrap()["mod_ids_csv"],
            ids
        );
    }
    settings["mod_ids_csv"] = json!("111111,777777");
    settings["asa_mod_configurations"]["222222"]["game_ini"] = json!("[Two]\nGame=Updated");
    for (key, ids) in [
        ("main", "111111,777777"),
        ("map-one", "111111,777777"),
        ("map-two", "777777"),
    ] {
        let effective = effective_settings("arksurvivalascended", &settings, key).unwrap();
        assert_eq!(effective["mod_ids_csv"], ids);
        assert_eq!(
            effective["asa_mod_configurations"]["222222"]["game_ini"],
            "[Two]\nGame=Updated"
        );
    }
    assert_eq!(
        effective_settings("arksurvivalascended", &settings, "map-paused").unwrap()["mod_ids_csv"],
        "111111,777777"
    );
    assert_eq!(settings["server_name"], "Shared room");
}

#[test]
fn configuration_inherits_each_unspecified_file_and_supports_explicit_empty_and_null() {
    let mut settings = settings();
    settings[OVERRIDES_FIELD] = json!({"map-one": {"mods": {"999999": {"configuration": {}}}}});
    assert_eq!(
        effective_settings("arksurvivalascended", &settings, "map-one").unwrap(),
        settings
    );
    settings[OVERRIDES_FIELD] =
        json!({"map-one": {"mods": {"111111": {"configuration": {"game_ini": ""}}}}});
    settings["asa_mod_configurations"]["111111"]["game_user_settings_ini"] =
        json!("[One]\nUser=Updated");
    let effective = effective_settings("arksurvivalascended", &settings, "map-one").unwrap();
    assert_eq!(
        effective["asa_mod_configurations"]["111111"]["game_ini"],
        ""
    );
    assert_eq!(
        effective["asa_mod_configurations"]["111111"]["game_user_settings_ini"],
        "[One]\nUser=Updated"
    );
    assert_eq!(
        effective["asa_mod_configurations"]["222222"],
        settings["asa_mod_configurations"]["222222"]
    );
    settings[OVERRIDES_FIELD]["map-one"]["mods"]["111111"]["configuration"] = Value::Null;
    assert!(effective_settings("arksurvivalascended", &settings, "map-one").unwrap()["asa_mod_configurations"].get("111111").is_none());
    settings[OVERRIDES_FIELD]["map-one"]["mods"]
        .as_object_mut()
        .unwrap()
        .remove("111111");
    assert_eq!(
        effective_settings("arksurvivalascended", &settings, "map-one").unwrap()["asa_mod_configurations"],
        settings["asa_mod_configurations"]
    );
}

#[test]
fn active_passive_membership_and_loading_order_are_independent() {
    let mut settings = settings();
    settings[OVERRIDES_FIELD] = json!({"map-one": {
        "mods": {"111111": {"passive": true}, "333333": {"active": true},
            "222222": {"disabled_active": true}, "555555": {"removed": false}},
        "active_order": ["333333", "222222", "111111"]
    }});
    let effective = effective_settings("arksurvivalascended", &settings, "map-one").unwrap();
    assert_eq!(effective["mod_ids_csv"], "333333,222222,111111");
    assert_eq!(effective["passive_mod_ids_csv"], "333333,111111");
    assert_eq!(
        effective["curseforge_disabled_mod_ids"],
        json!(["444444", "222222"])
    );
    assert_eq!(effective["curseforge_removed_mod_ids"], json!([]));
}

#[test]
fn map_only_ids_append_in_lexical_order_after_shared_ids() {
    let mut settings = settings();
    settings[OVERRIDES_FIELD] = json!({"main": {"mods": {
        "928708": {"active": true}, "1254808": {"active": true}
    }}});
    assert_eq!(
        effective_settings("arksurvivalascended", &settings, "main").unwrap()["mod_ids_csv"],
        "111111,222222,1254808,928708"
    );
}

#[test]
fn evolved_mod_reference_forms_share_the_native_parser_and_preserve_download_intent() {
    let mut settings = settings();
    settings["active_mod_ids"] = json!(
        "workshop-111111;https://steamcommunity.com/sharedfiles/filedetails/?id=222222;mod-333333"
    );
    settings[OVERRIDES_FIELD] =
        json!({"main": {"mods": {"222222": {"active": false}, "666666": {"active": true}}}});
    let effective = effective_settings("arksurvivalevolved", &settings, "main").unwrap();
    assert_eq!(effective["active_mod_ids"], "111111,333333,666666");
    assert_eq!(
        effective["auto_managed_mod_ids"],
        settings["auto_managed_mod_ids"]
    );
    assert_eq!(
        crate::ark_mods::parse_evolved_mod_ids(settings["active_mod_ids"].as_str().unwrap()),
        ["111111", "222222", "333333"]
    );
}

#[test]
fn sparse_overrides_reject_unknown_targets_ids_fields_types_and_duplicate_order() {
    for overrides in [
        json!([]),
        json!({"map-missing": {"mods": {}}}),
        json!({"main": {}}),
        json!({"main": {"mods": {"01": {"active": true}}}}),
        json!({"main": {"mods": {"111111": {"active": "true"}}}}),
        json!({"main": {"mods": {"111111": {"admin_password": "bad"}}}}),
        json!({"main": {"mods": {"111111": {"configuration": {"other.ini": "bad"}}}}}),
        json!({"main": {"mods": {}, "active_order": ["111111", "111111"]}}),
    ] {
        let mut settings = settings();
        settings[OVERRIDES_FIELD] = overrides;
        assert!(validate_overrides("arksurvivalascended", &settings).is_err());
    }
    let mut settings = settings();
    settings[OVERRIDES_FIELD] = json!({"main": {"mods": {"111111": {"passive": true}}}});
    assert!(validate_overrides("arksurvivalevolved", &settings).is_err());
    settings[SNAPSHOTS_FIELD] = json!({"main": {}});
    assert!(effective_settings("arksurvivalascended", &settings, "main").is_err());
}

#[test]
fn snapshot_conversion_preserves_order_empty_maps_markers_and_per_file_differences() {
    let mut settings = settings();
    settings[OVERRIDES_FIELD] = json!({}); // Newly merged schema default is not a second source.
    settings[SNAPSHOTS_FIELD] = json!({"map-one": {
        "mod_ids_csv": "222222,111111", "passive_mod_ids_csv": "444444",
        "curseforge_disabled_mod_ids": ["666666"], "curseforge_removed_mod_ids": ["777777"],
        "asa_mod_configurations": {"111111": {"game_user_settings_ini": "[One]\nUser=Shared"}}
    }, "map-paused": {}});
    assert!(convert_map_snapshots("arksurvivalascended", &mut settings).unwrap());
    assert!(settings.get(SNAPSHOTS_FIELD).is_none());
    let scope = &settings[OVERRIDES_FIELD]["map-one"];
    assert_eq!(scope["active_order"], json!(["222222", "111111"]));
    assert_eq!(
        scope["mods"]["111111"]["configuration"],
        json!({"game_ini": ""})
    );
    assert_eq!(scope["mods"]["222222"]["configuration"], Value::Null);
    let effective = effective_settings("arksurvivalascended", &settings, "map-one").unwrap();
    assert_eq!(effective["mod_ids_csv"], "222222,111111");
    assert_eq!(effective["passive_mod_ids_csv"], "444444");
    assert_eq!(effective["curseforge_disabled_mod_ids"], json!(["666666"]));
    assert_eq!(effective["curseforge_removed_mod_ids"], json!(["777777"]));
    let empty = effective_settings("arksurvivalascended", &settings, "map-paused").unwrap();
    assert_eq!(empty["mod_ids_csv"], "");
    assert_eq!(empty["passive_mod_ids_csv"], "");
    assert_eq!(empty["asa_mod_configurations"], json!({}));
    settings["mod_ids_csv"] = json!("111111,222222,888888");
    settings["asa_mod_configurations"]["111111"]["game_user_settings_ini"] =
        json!("[One]\nUser=NewShared");
    let effective = effective_settings("arksurvivalascended", &settings, "map-one").unwrap();
    assert_eq!(effective["mod_ids_csv"], "222222,111111,888888");
    assert_eq!(
        effective["asa_mod_configurations"]["111111"]["game_user_settings_ini"],
        "[One]\nUser=NewShared"
    );
    assert_eq!(
        effective_settings("arksurvivalascended", &settings, "map-paused").unwrap()["mod_ids_csv"],
        "888888"
    );
    assert!(!convert_map_snapshots("arksurvivalascended", &mut settings).unwrap());
}

#[test]
fn conversion_rejects_competing_sources_and_unrecognized_references_without_mutation() {
    for (field, value) in [
        (OVERRIDES_FIELD, json!({"main": {"mods": {}}})),
        ("mod_ids_csv", json!("111111,unknown-reference")),
    ] {
        let mut settings = settings();
        settings[SNAPSHOTS_FIELD] = json!({"main": {}});
        settings[field] = value;
        let original = settings.clone();
        assert!(convert_map_snapshots("arksurvivalascended", &mut settings).is_err());
        assert_eq!(settings, original);
    }
    let mut settings = settings();
    settings[SNAPSHOTS_FIELD] = json!({"main": {"active_mod_ids": "workshop-222222,https://steamcommunity.com/?id=111111"}});
    convert_map_snapshots("arksurvivalevolved", &mut settings).unwrap();
    assert_eq!(
        effective_settings("arksurvivalevolved", &settings, "main").unwrap()["active_mod_ids"],
        "222222,111111"
    );
}

#[test]
fn evolved_scope_ids_match_native_minimum_length_and_noncanonical_conversion_is_rejected() {
    for overrides in [
        json!({"main": {"mods": {"5": {"active": true}}}}),
        json!({"main": {"mods": {}, "active_order": ["5"]}}),
    ] {
        let mut settings = settings();
        settings[OVERRIDES_FIELD] = overrides;
        assert!(validate_overrides("arksurvivalevolved", &settings).is_err());
    }
    let mut settings = settings();
    settings["active_mod_ids"] = json!("000123");
    settings[SNAPSHOTS_FIELD] = json!({"main": {}});
    let original = settings.clone();
    assert!(convert_map_snapshots("arksurvivalevolved", &mut settings).is_err());
    assert_eq!(settings, original);
}

#[test]
fn evolved_projection_preserves_native_wide_delimiters_reference_priority_and_ignored_tokens() {
    for (raw, expected) in [
        ("111111;222222", "111111,222222,333333"),
        ("111111 222222", "111111,222222,333333"),
        (
            "workshop-111111，https://steamcommunity.com/999999/?id=222222",
            "111111,222222,333333",
        ),
        ("# 400000\n-- 500000", "400000,500000,333333"),
        ("#400000\n--500000\nignored-reference", "333333"),
    ] {
        let mut settings = settings();
        settings["active_mod_ids"] = json!(raw);
        settings[OVERRIDES_FIELD] = json!({"main": {"mods": {"333333": {"active": true}}}});
        assert_eq!(
            effective_settings("arksurvivalevolved", &settings, "main").unwrap()["active_mod_ids"],
            expected
        );
    }
}
