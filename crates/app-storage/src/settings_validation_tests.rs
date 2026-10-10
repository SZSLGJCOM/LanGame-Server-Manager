use crate::settings_validation::{SettingsValidationPhase, collect_settings_schema_diagnostics};
use crate::settings_value_formats::is_iso_calendar_date;
use serde_json::{Map, Value, json};

#[test]
fn dst_playstyle_conflicts_are_rejected_at_save_boundary() {
    let mut descriptor = super::test_descriptor(std::path::Path::new("."));
    descriptor.summary.id = "dontstarve".to_owned();
    descriptor.schema_json = Some(r#"{"properties":{}}"#.to_owned());
    let mut settings = Map::from_iter([
        ("game_mode".into(), json!("endless")),
        ("master_settings_preset".into(), json!("RELAXED")),
    ]);
    assert!(
        crate::settings_validation::validate_settings_against_schema(
            Some(&descriptor),
            &settings,
            SettingsValidationPhase::Complete
        )
        .is_err()
    );
    settings.insert("game_mode".into(), json!("survival"));
    crate::settings_validation::validate_settings_against_schema(
        Some(&descriptor),
        &settings,
        SettingsValidationPhase::Complete,
    )
    .unwrap();
}

#[test]
fn dst_editable_raw_master_presets_reject_legacy_mode_conflicts() {
    let mut descriptor = super::test_descriptor(std::path::Path::new("."));
    descriptor.summary.id = "dontstarve".to_owned();
    descriptor.schema_json = Some(r#"{"properties":{}}"#.to_owned());
    for raw in [
        "return { override_enabled = true, settings_preset = 'RELAXED', overrides = {} }",
        "return { override_enabled = true, preset = 'RELAXED', overrides = {} }",
    ] {
        let settings = Map::from_iter([
            ("game_mode".into(), json!("endless")),
            ("master_worldgenoverride_lua".into(), json!(raw)),
        ]);
        let error = crate::settings_validation::validate_settings_against_schema(
            Some(&descriptor),
            &settings,
            SettingsValidationPhase::Complete,
        )
        .expect_err("an explicit raw preset cannot conflict with legacy mode");
        assert!(error.to_string().contains("game_mode"));
    }

    for raw in [
        "return { override_enabled = true, overrides = {} }",
        "return make_world()",
    ] {
        let settings = Map::from_iter([
            ("game_mode".into(), json!("endless")),
            ("master_worldgenoverride_lua".into(), json!(raw)),
        ]);
        crate::settings_validation::validate_settings_against_schema(
            Some(&descriptor),
            &settings,
            SettingsValidationPhase::Complete,
        )
        .expect("an absent preset or an opaque script is not evidence of a conflict");
    }
}

#[test]
fn dst_mod_pipeline_rejects_raw_structured_conflicts_at_save_boundary() {
    let mut descriptor = super::test_descriptor(std::path::Path::new("."));
    descriptor.summary.id = "dontstarve".to_owned();
    descriptor.schema_json = Some(r#"{"properties":{}}"#.to_owned());
    for shard in ["master", "caves"] {
        for structured in ["enabled_workshop_mod_ids", "mod_configuration_options"] {
            let mut settings = Map::from_iter([
                ("enable_caves".to_owned(), json!(true)),
                (
                    format!("{shard}_modoverrides_lua"),
                    json!("return {custom=true}"),
                ),
                (
                    format!("{shard}_{structured}"),
                    if structured == "enabled_workshop_mod_ids" {
                        json!("2039181790")
                    } else {
                        json!({"2039181790":{"option":false}})
                    },
                ),
            ]);
            for phase in [
                SettingsValidationPhase::Creation,
                SettingsValidationPhase::Complete,
            ] {
                let error = crate::settings_validation::validate_settings_against_schema(
                    Some(&descriptor),
                    &settings,
                    phase,
                )
                .expect_err("raw overrides must not silently ignore structured settings");
                assert!(error.to_string().contains("modoverrides"));
            }
            settings.insert(
                format!("{shard}_modoverrides_lua"),
                json!("return {\r\n}\r\n"),
            );
            crate::settings_validation::validate_settings_against_schema(
                Some(&descriptor),
                &settings,
                SettingsValidationPhase::Complete,
            )
            .expect("default raw template allows structured settings");
        }
    }
}

#[test]
fn enshrouded_custom_role_errors_reach_the_save_boundary() {
    let mut descriptor = super::test_descriptor(std::path::Path::new("."));
    descriptor.summary.id = "enshrouded".to_owned();
    descriptor.schema_json =
        Some(r#"{"properties":{"custom_user_groups_json":{"type":"string"}}}"#.to_owned());
    let mut input = Map::from_iter([("custom_user_groups_json".to_owned(), json!("[42]"))]);
    for phase in [
        SettingsValidationPhase::Creation,
        SettingsValidationPhase::Complete,
    ] {
        assert!(
            crate::settings_validation::validate_settings_against_schema(
                Some(&descriptor),
                &input,
                phase
            )
            .is_err()
        );
        input.insert("custom_user_groups_json".to_owned(), json!("[]"));
        crate::settings_validation::validate_settings_against_schema(
            Some(&descriptor),
            &input,
            phase,
        )
        .unwrap();
        input.insert("custom_user_groups_json".to_owned(), json!("[42]"));
    }
}

#[test]
fn dst_data_collection_opt_out_normalizes_the_cluster_to_offline_mode() {
    let mut descriptor = super::test_descriptor(std::path::Path::new("."));
    descriptor.summary.id = "dontstarve".to_owned();
    descriptor.schema_json = Some(
        json!({"properties": {
            "disable_data_collection": {"type":"boolean", "default":false},
            "offline_cluster": {"type":"boolean", "default":false}
        }})
        .to_string(),
    );

    let normalized = crate::instances::normalize_complete_instance_settings(
        Some(&descriptor),
        Map::from_iter([
            ("disable_data_collection".to_owned(), json!(true)),
            ("offline_cluster".to_owned(), json!(false)),
        ]),
        "dst-normalization",
        "DST normalization",
        "0.0.0.0",
    )
    .expect("normalize DST settings");

    assert_eq!(normalized.get("offline_cluster"), Some(&json!(true)));
    assert_eq!(
        normalized.get("disable_data_collection"),
        Some(&json!(true))
    );
}

#[test]
fn valheim_password_minimum_is_enforced_before_save_and_start() {
    let mut descriptor = super::test_descriptor(std::path::Path::new("."));
    descriptor.summary.id = "valheim".to_owned();
    descriptor.schema_json = Some(include_str!("../../../modules/valheim/schema.json").to_owned());

    for (password, valid) in [
        ("", false),
        ("1234", false),
        ("12345", true),
        ("synthetic-long-password", true),
    ] {
        let result = crate::instances::normalize_complete_instance_settings(
            Some(&descriptor),
            Map::from_iter([("server_password".to_owned(), json!(password))]),
            "valheim-password-validation",
            "Valheim password validation",
            "0.0.0.0",
        );

        if valid {
            let settings = result.expect("a password of at least five characters must be accepted");
            assert_eq!(
                settings.get("server_password").and_then(Value::as_str),
                Some(password)
            );
        } else if let Err(crate::StorageError::InvalidModuleSetting { field, message, .. }) = result
        {
            assert_eq!(field, "server_password");
            assert_eq!(message, "must be at least 5 characters");
        } else {
            panic!("a short password must fail server_password minimum length validation");
        }
    }
}

#[test]
fn valheim_world_rules_accept_only_documented_argument_shapes() {
    let schema: Value = serde_json::from_str(include_str!("../../../modules/valheim/schema.json"))
        .expect("Valheim schema");
    for (key, value, valid) in [
        ("world_preset", "normal", true),
        ("world_modifiers", "", true),
        (
            "world_modifiers",
            "combat hard\r\ndeathpenalty casual;resources most,raids none\nportals veryhard",
            true,
        ),
        ("world_modifiers", "combat", false),
        ("world_modifiers", "combat hard extra", false),
        ("world_modifiers", "combat most", false),
        ("world_modifiers", "combat hard\n-password secret", false),
        ("world_set_keys", "playerevents\nnomap", true),
        ("world_set_keys", "players=8", false),
        ("world_set_keys", "nomap unexpected", false),
    ] {
        let settings = Map::from_iter([(String::from(key), Value::from(value))]);
        let diagnostics = collect_settings_schema_diagnostics(
            &schema,
            &settings,
            SettingsValidationPhase::Creation,
        );
        assert_eq!(
            diagnostics.iter().any(|diagnostic| diagnostic.field == key),
            !valid,
            "{key}: {value}"
        );
    }
}

#[test]
fn disabled_dst_caves_defer_native_semantics_but_keep_type_and_size_boundaries() {
    let mut descriptor = super::test_descriptor(std::path::Path::new("."));
    descriptor.schema_json = Some(json!({"properties": {
        "enable_caves": {"type":"boolean"},
        "caves_settings_preset": {"type":"string", "pattern":"^[A-Z_]+$", "maxLength":16},
        "caves_mod_configuration_options": {"type":"object", "properties": {"choice":{"type":"string", "enum":["default"]}}},
        "caves_modoverrides_lua": {"type":"string", "maxLength":32},
        "master_settings_preset": {"type":"string", "pattern":"^[A-Z_]+$"}
    }}).to_string());
    let mut settings = Map::from_iter([
        (String::from("enable_caves"), json!(false)),
        (String::from("caves_settings_preset"), json!("saved value")),
        (
            String::from("caves_mod_configuration_options"),
            json!({"choice":"saved choice"}),
        ),
        (
            String::from("caves_modoverrides_lua"),
            json!("return custom_mods()"),
        ),
    ]);
    let original = settings.clone();
    super::validate_settings_against_schema(
        Some(&descriptor),
        &settings,
        SettingsValidationPhase::Complete,
    )
    .unwrap();
    assert_eq!(
        settings, original,
        "inactive validation must not erase retained values"
    );
    settings.insert(String::from("enable_caves"), json!(true));
    assert!(
        super::validate_settings_against_schema(
            Some(&descriptor),
            &settings,
            SettingsValidationPhase::Complete
        )
        .is_err()
    );
    settings.insert(String::from("enable_caves"), json!(false));
    for invalid in [json!(42), json!("x".repeat(33))] {
        settings.insert(String::from("caves_modoverrides_lua"), invalid);
        assert!(
            super::validate_settings_against_schema(
                Some(&descriptor),
                &settings,
                SettingsValidationPhase::Complete
            )
            .is_err()
        );
    }
    settings = original;
    settings.insert(String::from("master_settings_preset"), json!("bad value"));
    assert!(
        super::validate_settings_against_schema(
            Some(&descriptor),
            &settings,
            SettingsValidationPhase::Complete
        )
        .is_err()
    );
    settings.remove("master_settings_preset");
    descriptor.summary.id = String::from("another_game");
    assert!(
        super::validate_settings_against_schema(
            Some(&descriptor),
            &settings,
            SettingsValidationPhase::Complete
        )
        .is_err()
    );
}

#[test]
fn iso_calendar_date_requires_a_real_calendar_day() {
    for valid in ["2024-02-29", "2026-08-10", "9999-12-31"] {
        assert!(is_iso_calendar_date(valid), "expected {valid} to be valid");
    }
    for invalid in [
        "0000-01-01",
        "2023-02-29",
        "2026-04-31",
        "2026-13-01",
        "2026-1-01",
        "not-a-date",
    ] {
        assert!(
            !is_iso_calendar_date(invalid),
            "expected {invalid} to be invalid"
        );
    }
}

#[test]
fn date_format_reports_invalid_nested_roster_dates() {
    let schema = json!({
        "type": "object",
        "properties": {
            "blacklist_entries": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "unbandate": { "type": "string", "format": "date" }
                    }
                }
            }
        }
    });
    let settings = serde_json::from_value::<Map<String, Value>>(json!({
        "blacklist_entries": [{ "unbandate": "2026-02-30" }]
    }))
    .expect("settings object");

    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.field == "blacklist_entries[0].unbandate" && diagnostic.code == "format"
    }));
}

#[test]
fn platform_account_ids_are_validated_against_the_selected_namespace() {
    let schema = json!({
        "type": "object",
        "properties": {
            "users": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "platform": {
                            "type": "string",
                            "enum": ["Steam", "EOS", "XBL", "PSN"]
                        },
                        "userid": {
                            "type": "string",
                            "x-lsgm-player-access-platform-field": "platform"
                        }
                    }
                }
            }
        }
    });
    let settings = serde_json::from_value::<Map<String, Value>>(json!({
        "users": [
            { "platform": "Steam", "userid": "not-a-steam-id" },
            { "platform": "EOS", "userid": "0002604bc42244e099c1bf05145fb71f" },
            { "platform": "PSN", "userid": "Player-One_42" }
        ]
    }))
    .expect("settings object");

    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "platform_account_id")
            .map(|diagnostic| diagnostic.field.as_str())
            .collect::<Vec<_>>(),
        vec!["users[0].userid"]
    );
}

#[test]
fn raw_config_extras_cannot_override_managed_player_rosters() {
    let schema = json!({
        "type": "object",
        "properties": {
            "extra": {
                "type": "string",
                "format": "textarea",
                "x-lsgm-disallowed-line-prefixes": ["ownerid", "banid"]
            }
        }
    });
    let settings = serde_json::from_value::<Map<String, Value>>(json!({
        "extra": "# ownerid is documented here\n; banid is also a comment\nserver.description ok\n  BANID 76561198000000000"
    }))
    .expect("settings object");

    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "managed_directive")
            .map(|diagnostic| diagnostic.field.as_str())
            .collect::<Vec<_>>(),
        vec!["extra"]
    );
}

#[test]
fn rust_server_cfg_extra_cannot_override_managed_runtime_authority() {
    let schema: Value = serde_json::from_str(include_str!("../../../modules/rust/schema.json"))
        .expect("parse Rust schema");
    let settings = serde_json::from_value::<Map<String, Value>>(json!({
        "server_cfg_extra": "# server.port 29015\nserver.description safe\nSERVER.IP 127.0.0.1\nserver.identity other\nserver.port=29015\nserver.queryport 29017\nrcon.port 29016"
    }))
    .expect("settings object");

    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "managed_directive")
            .map(|diagnostic| diagnostic.field.as_str())
            .collect::<Vec<_>>(),
        vec![
            "server_cfg_extra",
            "server_cfg_extra",
            "server_cfg_extra",
            "server_cfg_extra",
            "server_cfg_extra"
        ]
    );
}

#[test]
fn delimited_rosters_reject_renderer_fallback_metadata() {
    let schema = json!({
        "type":"object",
        "properties":{
            "operators":{
                "type":"string",
                "format":"textarea",
                "x-lsgm-player-access-codec":"csv_uuid_name",
                "x-lsgm-player-access-delimited-fields":[
                    {"name":"uuid","format":"minecraft_uuid","required":true},
                    {"name":"name","format":"minecraft_name","required":true},
                    {"name":"level","format":"integer","required":false,"minimum":1,"maximum":4},
                    {"name":"bypass","format":"boolean","required":false}
                ]
            }
        }
    });
    let settings = serde_json::from_value::<Map<String, Value>>(json!({
        "operators":"a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11,Alex,9,true"
    }))
    .expect("settings object");

    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.field == "operators" && diagnostic.code == "player_access_entry"
    }));
}

#[test]
fn scalar_steam64_roster_rejects_multiple_newline_values() {
    let schema = json!({
        "type":"object",
        "properties":{
            "owner":{
                "type":"string",
                "x-lsgm-player-access-codec":"steam64"
            }
        }
    });
    let settings = serde_json::from_value::<Map<String, Value>>(json!({
        "owner":"76561198000000001\n76561198000000002"
    }))
    .expect("settings object");

    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.field == "owner" && diagnostic.code == "player_access_entry"
    }));
}

#[test]
fn conan_structured_native_values_reject_malformed_ini_tuples() {
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../modules/conanexiles/schema.json");
    let schema: Value =
        serde_json::from_str(&std::fs::read_to_string(schema_path).expect("read Conan schema"))
            .expect("parse Conan schema");
    let settings = serde_json::from_value::<Map<String, Value>>(json!({
        "building_pvp_allowed_structure_ids": "80901,80111",
        "item_repair_durability_loss_by_repairkit_tier": "(0.2,0.1)",
        "server_transfer_allowed_servers": "127.0.0.1:7777"
    }))
    .expect("settings object");

    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "pattern")
            .map(|diagnostic| diagnostic.field.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "building_pvp_allowed_structure_ids",
            "item_repair_durability_loss_by_repairkit_tier",
            "server_transfer_allowed_servers",
        ])
    );
}
