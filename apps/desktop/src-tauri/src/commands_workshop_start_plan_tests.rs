use super::super::{ManagedModState, ManualModInventoryItem};
use super::*;
use serde_json::json;

fn collect(module_id: &str, settings: Value) -> Vec<String> {
    collect_start_workshop_item_ids(module_id, &settings, None).expect("valid native settings")
}

fn inventory(module_id: &str) -> ManualModInventoryResult {
    ManualModInventoryResult {
        instance_id: "one".into(),
        module_id: module_id.into(),
        source_label: "Workshop".into(),
        target_label: "Mods".into(),
        target_path: "D:/instance/Mods".into(),
        target_exists: true,
        items: Vec::new(),
    }
}

fn item(name: &str, package_name: Option<&str>) -> ManualModInventoryItem {
    ManualModInventoryItem {
        name: name.into(),
        path: format!("D:/instance/Mods/{name}"),
        item_type: "directory".into(),
        inferred_id: package_name.map(String::from),
        file_count: 1,
        total_bytes: 3,
        modified_unix_ms: None,
        management: None,
    }
}

#[test]
fn native_settings_use_current_load_or_download_lists() {
    for (module_id, key) in [
        ("projectzomboid", "workshop_items"),
        ("unturned", "workshop_file_ids"),
        ("barotrauma", "mod_workshop_ids"),
        ("conanexiles", "mod_workshop_ids"),
        ("soulmask", "mod_workshop_ids"),
    ] {
        let settings = json!({
            key: "200000;100000,workshop-200000\nhttps://steamcommunity.com/sharedfiles/filedetails/?id=300000\n# 400000\n-- 500000",
            "auto_managed_mod_ids": "600000",
            "steam_workshop_disabled_mod_ids": ["700000"],
            "steam_workshop_collections": [{"id":"800000", "title":"Old", "member_ids":["900000"]}]
        });
        assert_eq!(
            collect(module_id, settings),
            ["100000", "200000", "300000"],
            "{module_id}"
        );
    }
}

#[test]
fn ark_refresh_uses_the_native_active_mods_reference_and_separator_contract() {
    assert_eq!(
        collect(
            "arksurvivalevolved",
            json!({
                "active_mod_ids": "100000 200000;workshop-300000、400000，https://example.invalid/123456?id=500000\n#800000 --900000",
                "auto_managed_mod_ids": "600000"
            })
        ),
        ["100000", "200000", "300000", "400000", "500000"]
    );
}

#[test]
fn ark_refreshes_the_enabled_maps_effective_mod_union() {
    let settings = json!({
        "active_mod_ids": "100000,200000",
        "additional_maps": [
            {"id":"desert", "map_name":"ScorchedEarth_P", "name":"Desert", "enabled":true},
            {"id":"center", "map_name":"TheCenter", "name":"Center", "enabled":true},
            {"id":"paused", "map_name":"Ragnarok", "name":"Paused", "enabled":false}
        ],
        "ark_mod_map_overrides": {
            "main": {"mods":{"300000":{"active":true}}},
            "map-desert": {"mods":{"300000":{"active":true},"400000":{"active":true}}},
            "map-paused": {"mods":{"500000":{"active":true}}}
        }
    });
    assert_eq!(
        collect("arksurvivalevolved", settings.clone()),
        ["100000", "200000", "300000", "400000"]
    );
    let mut disabled_on_all_maps = settings;
    for key in ["main", "map-desert", "map-center"] {
        if !disabled_on_all_maps["ark_mod_map_overrides"][key].is_object() {
            disabled_on_all_maps["ark_mod_map_overrides"][key] = json!({"mods":{}});
        }
        for id in ["100000", "200000"] {
            disabled_on_all_maps["ark_mod_map_overrides"][key]["mods"][id] =
                json!({"active":false});
        }
    }
    assert_eq!(
        collect("arksurvivalevolved", disabled_on_all_maps),
        ["300000", "400000"]
    );
}

#[test]
fn empty_or_client_only_workflows_have_no_start_refresh() {
    for module_id in [
        "minecraft",
        "rimworld",
        "arksurvivalascended",
        "conanexiles",
    ] {
        assert!(collect(module_id, json!({})).is_empty());
    }
    assert!(collect("rimworld", json!({"workshop_items":"100000"})).is_empty());
}

#[test]
fn terraria_only_refreshes_its_selected_tmodloader_runtime() {
    for runtime in [None, Some("vanilla")] {
        let mut settings = json!({"tmodloader_workshop_item_ids":"100000"});
        if let Some(runtime) = runtime {
            settings["server_runtime"] = json!(runtime);
        }
        assert!(collect("terraria", settings).is_empty());
    }
    assert_eq!(
        collect(
            "terraria",
            json!({"server_runtime":"tmodloader", "tmodloader_workshop_item_ids":"100000"})
        ),
        ["100000"]
    );
}

#[test]
fn dst_uses_only_active_shard_enablement_and_honors_removal() {
    let settings = json!({
        "master_enabled_workshop_mod_ids":"100000,200000",
        "caves_enabled_workshop_mod_ids":"300000",
        "islands_enabled_workshop_mod_ids":"400000",
        "volcano_enabled_workshop_mod_ids":"500000",
        "shared_workshop_mod_ids":"600000",
        "shared_workshop_collection_ids":"700000",
        "master_mod_configuration_options":{"800000":{}},
        "dst_removed_workshop_mod_ids":["200000"],
        "enable_caves":false
    });
    assert_eq!(collect("dontstarve", settings.clone()), ["100000"]);
    let mut caves = settings.clone();
    caves["enable_caves"] = json!(true);
    assert_eq!(collect("dontstarve", caves), ["100000", "300000"]);
    let mut islands = settings;
    islands["shard_layout"] = json!("island_adventures");
    assert_eq!(
        collect("dontstarve", islands),
        ["100000", "300000", "400000", "500000"]
    );
}

#[test]
fn dst_raw_static_declarations_reuse_materializer_precedence() {
    let settings = json!({
        "master_enabled_workshop_mod_ids":"900000",
        "master_modoverrides_lua":"return { [\"workshop-100000\"] = { enabled = true, configuration_options = { n = 1 } }, [\"workshop-200000\"] = { enabled = false }, [\"workshop-300000\"] = {}, [\"local-server-mod\"] = { enabled = true }, client_mods_disabled = true }",
        "caves_modoverrides_lua":"return dynamic_table()",
        "enable_caves":false
    });
    assert_eq!(collect("dontstarve", settings), ["100000"]);
}

#[test]
fn dst_unknown_active_lua_fails_without_returning_partial_ids() {
    let error = collect_start_workshop_item_ids(
        "dontstarve",
        &json!({
            "master_enabled_workshop_mod_ids":"100000",
            "caves_modoverrides_lua":"return dynamic_table()",
            "enable_caves":true
        }),
        None,
    )
    .expect_err("unknown native script cannot confirm a complete refresh plan");
    assert!(error.contains("Cannot determine enabled Workshop Mods"));
    assert!(error.contains("caves"));
}

#[test]
fn dst_managed_ids_are_not_limited_by_static_lua_evidence_budgets() {
    let ids = (200000..200150)
        .map(|id| id.to_string())
        .collect::<Vec<_>>();
    let settings = json!({
        "master_enabled_workshop_mod_ids":ids.join("\n"),
        "master_modoverrides_lua":"return {\r\n}\r\n",
        "master_worldgenoverride_lua":"x".repeat(150_000),
        "enable_caves":true,
        "caves_modoverrides_lua":"return { [\"workshop-300000\"] = { enabled = true }, [\"workshop-400000\"] = { enabled = false } }",
        "caves_enabled_workshop_mod_ids":"500000"
    });
    let collected = collect("dontstarve", settings);
    assert_eq!(collected.len(), 151);
    assert_eq!(&collected[..150], ids);
    assert_eq!(collected[150], "300000");
}

#[test]
fn native_active_workshop_list_is_authoritative_over_retained_disabled_markers() {
    for module_id in ["barotrauma", "conanexiles", "soulmask"] {
        assert_eq!(
            collect(
                module_id,
                json!({
                    "mod_workshop_ids":"100000",
                    "steam_workshop_disabled_mod_ids":["100000", "200000"]
                })
            ),
            ["100000"]
        );
    }
}

#[test]
fn palworld_matches_enabled_package_names_and_excludes_removed_payloads() {
    let mut local = inventory("palworld");
    local.items = vec![
        item("100000", Some("EnabledPackage")),
        item("200000", Some("DisabledPackage")),
        item("300000", Some("EnabledPackage")),
        item("400000", None),
        item("LocalPackage", Some("EnabledPackage")),
    ];
    let settings = json!({
        "mod_package_names":" enabledpackage ",
        "steam_workshop_removed_mod_ids":["300000"],
        "steam_workshop_collections":[{"id":"500000", "title":"Retained", "member_ids":["200000", "300000", "600000"]}]
    });
    assert_eq!(
        collect_start_workshop_item_ids("palworld", &settings, Some(&local)).unwrap(),
        ["100000"]
    );
}

#[test]
fn squad_only_uses_live_payloads_inside_the_selected_target() {
    let mut local = inventory("squad");
    let mut disabled = item("200000", None);
    disabled.management = Some(ManagedModState {
        package_id: "disabled".into(),
        enabled: false,
        dependencies: Vec::new(),
        dependents: Vec::new(),
        dependency_status: "unknown".into(),
    });
    let mut outside = item("300000", None);
    outside.path = "D:/steamcmd/cache/300000".into();
    let mut sibling = item("400000", None);
    sibling.path = "D:/instance/Mods-other/400000".into();
    let mut missing = item("500000", None);
    missing.file_count = 0;
    let mut actual = item("100000", None);
    actual.path = "d:\\INSTANCE\\mods\\100000".into();
    local.items = vec![
        actual,
        disabled,
        outside,
        sibling,
        missing,
        item("LocalMod", None),
    ];
    assert_eq!(
        collect_start_workshop_item_ids("squad", &json!({}), Some(&local)).unwrap(),
        ["100000"]
    );
}

#[test]
fn nested_metadata_numbers_cannot_add_another_workshop_item() {
    let mut local = inventory("palworld");
    let mut nested = item("Package-200000", Some("Package"));
    nested.path = "D:/instance/Mods/100000/Package-200000".into();
    local.items.push(nested);
    assert_eq!(
        collect_start_workshop_item_ids(
            "palworld",
            &json!({"mod_package_names":"Package"}),
            Some(&local)
        )
        .unwrap(),
        ["100000"]
    );
}

#[test]
fn locally_imported_names_with_embedded_digits_are_not_workshop_ownership() {
    for module_id in ["palworld", "squad"] {
        let mut local = inventory(module_id);
        local.items = vec![
            item("LocalPlugin123456", Some("EnabledPackage")),
            item("Revision20261017", Some("EnabledPackage")),
            item("123456-LocalPlugin", Some("EnabledPackage")),
            item("123456", Some("EnabledPackage")),
        ];
        assert_eq!(
            collect_start_workshop_item_ids(
                module_id,
                &json!({"mod_package_names":"EnabledPackage"}),
                Some(&local)
            )
            .unwrap(),
            ["123456"]
        );
    }
}

#[test]
fn native_lists_ignore_invalid_tokens_without_discarding_valid_ids() {
    assert_eq!(
        collect(
            "conanexiles",
            json!({
                "mod_workshop_ids":"not-a-workshop,100000\n000000;18446744073709551616;../other;workshop-200000"
            })
        ),
        ["100000", "200000"]
    );
}

#[test]
fn invalid_setting_types_and_membership_markers_fail_with_context() {
    for value in [json!(true), json!(["100000"]), Value::Null] {
        let error = collect_start_workshop_item_ids(
            "conanexiles",
            &json!({"mod_workshop_ids":value}),
            None,
        )
        .unwrap_err();
        assert!(error.contains("mod_workshop_ids"));
    }
    let local = inventory("palworld");
    assert!(
        collect_start_workshop_item_ids(
            "palworld",
            &json!({"steam_workshop_removed_mod_ids":["../100000"]}),
            Some(&local)
        )
        .unwrap_err()
        .contains("steam_workshop_removed_mod_ids")
    );
}

#[test]
fn inventory_must_match_module_and_have_safe_relative_paths() {
    let mut local = inventory("squad");
    assert!(collect_start_workshop_item_ids("palworld", &json!({}), Some(&local)).is_err());
    assert!(collect_start_workshop_item_ids("squad", &json!({}), None).is_err());
    let mut traversal = item("100000", None);
    traversal.path = "D:/instance/Mods/../100000".into();
    local.items.push(traversal);
    assert!(
        collect_start_workshop_item_ids("squad", &json!({}), Some(&local))
            .unwrap_err()
            .contains("relative components")
    );
    local.target_exists = false;
    assert!(
        collect_start_workshop_item_ids("squad", &json!({}), Some(&local))
            .unwrap()
            .is_empty()
    );
}
