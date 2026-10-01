use super::*;

struct TextRosterCase {
    module_id: &'static str,
    field_key: &'static str,
    add_value: &'static str,
    remove_value: &'static str,
    expected_stored_value: &'static str,
    rendered_relative_path: &'static str,
    expected_rendered_fragment: &'static str,
    expected_sync_mode: PlayerAccessSyncMode,
    expected_add_action_id: Option<&'static str>,
    expected_remove_action_id: Option<&'static str>,
}

#[tokio::test]
async fn necesse_owner_mutation_rejects_names_the_native_owner_flag_ignores_atomically() {
    let (root, paths, created) =
        provision_real_module_instance("necesse", "Player Access Necesse Validation").await;
    let before = read_instance_details(&paths, &created.summary.id)
        .await
        .expect("read Necesse before invalid mutation");

    let error = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("owner_name"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!("Alice-1"),
            expected_value: None,
        },
    )
    .await
    .expect_err("hyphenated Necesse owner must be rejected");
    assert!(error.to_string().contains("schema pattern"));

    let after = read_instance_details(&paths, &created.summary.id)
        .await
        .expect("read Necesse after invalid mutation");
    assert_eq!(after.settings_json, before.settings_json);
    cleanup_root(&root);
}

#[tokio::test]
async fn valheim_conflicting_rosters_reject_canonical_identity_without_partial_write() {
    let (root, paths, created) =
        provision_real_module_instance("valheim", "Player Access Valheim Conflict").await;

    let unexpected_baseline = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("permitted_list"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!("Steam_Player_42"),
            expected_value: Some(json!("")),
        },
    )
    .await
    .expect_err("list mutation must reject a scalar expected value");
    assert!(
        unexpected_baseline
            .to_string()
            .contains("expectedValue is only valid")
    );

    let permitted = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("permitted_list"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!("Steam_Player_42"),
            expected_value: None,
        },
    )
    .await
    .expect("add permitted Valheim identity");
    let before_conflict = permitted.details.settings_json.clone();

    let error = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("banned_list"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!("steam_player_42"),
            expected_value: None,
        },
    )
    .await
    .expect_err("canonical permitted/banned conflict must be rejected");
    assert!(error.to_string().contains("permitted_list"));

    let after_conflict = read_instance_details(&paths, &created.summary.id)
        .await
        .expect("read Valheim after rejected conflict");
    assert_eq!(after_conflict.settings_json, before_conflict);

    let banned = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("banned_list"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!("Xbox_Banned_7"),
            expected_value: None,
        },
    )
    .await
    .expect("add distinct banned Valheim identity");
    let before_admin_conflict = banned.details.settings_json.clone();
    let error = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("admin_list"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!("xbox_banned_7"),
            expected_value: None,
        },
    )
    .await
    .expect_err("canonical admin/banned conflict must be rejected");
    assert!(error.to_string().contains("banned_list"));
    let after_admin_conflict = read_instance_details(&paths, &created.summary.id)
        .await
        .expect("read Valheim after rejected admin conflict");
    assert_eq!(after_admin_conflict.settings_json, before_admin_conflict);

    cleanup_root(&root);
}

#[tokio::test]
async fn scalar_player_access_replace_and_clear_reject_stale_expected_values() {
    let (root, paths, created) =
        provision_real_module_instance("unturned", "Player Access Scalar CAS").await;
    let first_owner = "76561198000000001";
    let second_owner = "76561198000000002";

    let missing_baseline = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("owner_steam_id"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!(first_owner),
            expected_value: None,
        },
    )
    .await
    .expect_err("scalar mutation without an expected value must be rejected");
    assert!(
        missing_baseline
            .to_string()
            .contains("expectedValue is required")
    );

    let first = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("owner_steam_id"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!(first_owner),
            expected_value: Some(json!("")),
        },
    )
    .await
    .expect("set initial scalar owner");
    assert_eq!(
        serde_json::from_str::<Value>(&first.details.settings_json).unwrap()["owner_steam_id"],
        json!(first_owner)
    );

    let stale_replace = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("owner_steam_id"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!(second_owner),
            expected_value: Some(json!("")),
        },
    )
    .await
    .expect_err("stale scalar replace must be rejected");
    assert!(
        stale_replace
            .to_string()
            .contains("changed while this edit was pending")
    );

    let replaced = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("owner_steam_id"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!(second_owner),
            expected_value: Some(json!(first_owner)),
        },
    )
    .await
    .expect("replace scalar owner with a fresh baseline");
    assert_eq!(
        serde_json::from_str::<Value>(&replaced.details.settings_json).unwrap()["owner_steam_id"],
        json!(second_owner)
    );

    let stale_clear = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("owner_steam_id"),
            operation: PlayerAccessMutationOperation::Remove,
            value: json!(first_owner),
            expected_value: Some(json!(first_owner)),
        },
    )
    .await
    .expect_err("stale scalar clear must be rejected");
    assert!(
        stale_clear
            .to_string()
            .contains("changed while this edit was pending")
    );

    let after_stale_clear = read_instance_details(&paths, &created.summary.id)
        .await
        .expect("read scalar owner after rejected stale clear");
    assert_eq!(
        serde_json::from_str::<Value>(&after_stale_clear.settings_json).unwrap()["owner_steam_id"],
        json!(second_owner)
    );

    let cleared = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("owner_steam_id"),
            operation: PlayerAccessMutationOperation::Remove,
            value: json!(second_owner),
            expected_value: Some(json!(second_owner)),
        },
    )
    .await
    .expect("clear scalar owner with a fresh baseline");
    assert_eq!(
        serde_json::from_str::<Value>(&cleared.details.settings_json).unwrap()["owner_steam_id"],
        json!("")
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn sevendaystodie_player_access_mutation_persists_and_materializes_serveradmin_xml() {
    let (root, paths, created) =
        provision_real_module_instance("sevendaystodie", "Player Access 7DTD Integration").await;
    let steam_id = "76561198000000001";

    let added = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("blacklist_entries"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!({"platform":"Steam","userid":steam_id}),
            expected_value: None,
        },
    )
    .await
    .expect("add 7DTD blacklist entry");

    assert_eq!(
        added.persistent_status,
        PlayerAccessPersistentStatus::Updated
    );
    assert_eq!(added.live_target, format!("Steam_{steam_id}"));
    assert_eq!(
        added
            .sync
            .mutation_action_id(PlayerAccessMutationOperation::Add),
        None
    );
    let added_settings: Value =
        serde_json::from_str(&added.details.settings_json).expect("parse updated 7DTD settings");
    assert_eq!(
        added_settings["blacklist_entries"],
        json!([{
            "platform": "Steam",
            "userid": steam_id,
            "name": "",
            "unbandate": "9999-12-31",
            "reason": "LanGame"
        }])
    );

    let config_root = Path::new(&created.config_file_path)
        .parent()
        .expect("7DTD config root");
    let rendered_path = config_root.join("serveradmin.xml");
    let materialized_path = Path::new(&added.details.saves_path).join("serveradmin.xml");
    let expected_blacklist_line = format!(
        "    <blacklisted platform=\"Steam\" userid=\"{steam_id}\" name=\"\" unbandate=\"9999-12-31\" reason=\"LanGame\" />"
    );
    let rendered = fs::read_to_string(&rendered_path).expect("rendered 7DTD serveradmin.xml");
    let materialized =
        fs::read_to_string(&materialized_path).expect("materialized 7DTD serveradmin.xml");
    assert!(rendered.contains("<users>"));
    assert!(rendered.contains("<commands>"));
    assert!(!rendered.contains("<admins>"));
    assert!(!rendered.contains("<permissions>"));
    assert!(
        rendered.contains(&expected_blacklist_line),
        "rendered serveradmin.xml did not contain the persistent blacklist entry:\n{rendered}"
    );
    assert_eq!(materialized, rendered);

    let removed = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("blacklist_entries"),
            operation: PlayerAccessMutationOperation::Remove,
            value: json!({"platform":"Steam","userid":steam_id}),
            expected_value: None,
        },
    )
    .await
    .expect("remove 7DTD blacklist entry");

    assert_eq!(
        removed.persistent_status,
        PlayerAccessPersistentStatus::Updated
    );
    assert_eq!(removed.live_target, format!("Steam_{steam_id}"));
    assert_eq!(
        removed
            .sync
            .mutation_action_id(PlayerAccessMutationOperation::Remove),
        Some("unban_player")
    );
    let removed_settings: Value = serde_json::from_str(&removed.details.settings_json)
        .expect("parse 7DTD settings after removal");
    assert_eq!(removed_settings["blacklist_entries"], json!([]));

    let rendered_after_remove =
        fs::read_to_string(&rendered_path).expect("updated rendered 7DTD serveradmin.xml");
    let materialized_after_remove =
        fs::read_to_string(&materialized_path).expect("updated materialized 7DTD serveradmin.xml");
    assert!(!rendered_after_remove.contains(steam_id));
    assert_eq!(materialized_after_remove, rendered_after_remove);

    let reloaded = read_instance_details(&paths, &created.summary.id)
        .await
        .expect("reload 7DTD instance details");
    let reloaded_settings: Value =
        serde_json::from_str(&reloaded.settings_json).expect("parse reloaded 7DTD settings");
    assert_eq!(reloaded_settings["blacklist_entries"], json!([]));

    cleanup_root(&root);
}

#[tokio::test]
async fn sevendaystodie_player_access_current_settings_materialize_without_conversion() {
    let (root, paths, created) =
        provision_real_module_instance("sevendaystodie", "Player Access 7DTD Current Integration")
            .await;
    let config_path = Path::new(&created.config_file_path);
    let mut document: Value = serde_json::from_str(
        &fs::read_to_string(config_path).expect("read current 7DTD instance config"),
    )
    .expect("parse current 7DTD instance config");
    document["settings"]["admin_users"] = json!([{
        "platform": "Steam", "userid": "76561198000000001",
        "name": "Admin",
        "permission_level": 0
    }]);
    document["settings"]["whitelist_users"] = json!([{
        "platform": "Steam", "userid": "76561198000000002",
        "name": "Friend"
    }]);
    document["settings"]["blacklist_entries"] = json!([
        {
            "platform": "Steam", "userid": "76561198000000003",
            "unbandate": "9999-12-31",
            "reason": "Permanent current ban"
        },
        {
            "platform": "Steam", "userid": "76561198000000004",
            "unbandate": "2025-01-01"
        }
    ]);
    fs::write(
        config_path,
        serde_json::to_vec_pretty(&document).expect("serialize current 7DTD instance config"),
    )
    .expect("write current 7DTD instance config");

    let materialized = materialize_instance_configuration(&paths, &created.summary.id)
        .await
        .expect("materialize normalized current 7DTD settings");
    let settings: Value =
        serde_json::from_str(&materialized.settings_json).expect("parse normalized settings");
    assert_eq!(
        settings["admin_users"][0],
        json!({
            "platform": "Steam",
            "userid": "76561198000000001",
            "name": "Admin",
            "permission_level": 0
        })
    );
    assert_eq!(
        settings["whitelist_users"][0]["userid"],
        json!("76561198000000002")
    );
    assert_eq!(
        settings["blacklist_entries"][0]["unbandate"],
        json!("9999-12-31")
    );
    assert_eq!(
        settings["blacklist_entries"][1]["unbandate"],
        json!("2025-01-01")
    );

    let persisted: Value = serde_json::from_str(
        &fs::read_to_string(config_path).expect("read normalized 7DTD instance config"),
    )
    .expect("parse normalized 7DTD instance config");
    assert!(
        persisted["settings"]["admin_users"][0]
            .get("steam_id")
            .is_none()
    );
    let rendered = fs::read_to_string(
        Path::new(&materialized.config_file_path)
            .parent()
            .expect("7DTD config root")
            .join("serveradmin.xml"),
    )
    .expect("rendered normalized serveradmin.xml");
    assert!(rendered.contains(
        "<blacklisted platform=\"Steam\" userid=\"76561198000000003\" name=\"\" unbandate=\"9999-12-31\" reason=\"Permanent current ban\" />"
    ));
    assert!(rendered.contains(
        "<blacklisted platform=\"Steam\" userid=\"76561198000000004\" name=\"\" unbandate=\"2025-01-01\" reason=\"LanGame\" />"
    ));

    cleanup_root(&root);
}

#[tokio::test]
async fn sevendaystodie_composite_player_identity_add_edit_remove_supports_steam_and_eos() {
    let (root, paths, created) = provision_real_module_instance(
        "sevendaystodie",
        "Player Access 7DTD Composite Integration",
    )
    .await;
    let steam_id = "76561198000000001";
    let eos_id = "00000000000000000000000000000001";

    for value in [
        json!({"platform":"Steam","userid":steam_id,"name":"Steam Player"}),
        json!({"platform":"Steam","userid":steam_id,"name":"Steam Player Updated"}),
        json!({"platform":"EOS","userid":eos_id,"name":"EOS Player"}),
        json!({"platform":"EOS","userid":eos_id,"name":"EOS Player Updated"}),
    ] {
        apply_instance_player_access_mutation(
            &paths,
            ApplyInstancePlayerAccessMutationInput {
                instance_id: created.summary.id.clone(),
                field_key: String::from("blacklist_entries"),
                operation: PlayerAccessMutationOperation::Add,
                value,
                expected_value: None,
            },
        )
        .await
        .expect("add or edit composite 7DTD identity");
    }

    let edited = read_instance_details(&paths, &created.summary.id)
        .await
        .expect("read edited composite identities");
    let edited_settings: Value =
        serde_json::from_str(&edited.settings_json).expect("parse edited composite identities");
    let entries = edited_settings["blacklist_entries"]
        .as_array()
        .expect("blacklist array");
    assert_eq!(
        entries.len(),
        2,
        "platform is part of the canonical identity"
    );
    assert_eq!(entries[0]["name"], json!("Steam Player Updated"));
    assert_eq!(entries[1]["name"], json!("EOS Player Updated"));

    let rendered_path = Path::new(&edited.config_file_path)
        .parent()
        .expect("7DTD config root")
        .join("serveradmin.xml");
    let rendered = fs::read_to_string(&rendered_path).expect("render composite blacklist");
    assert!(rendered.contains(&format!(
        "<blacklisted platform=\"Steam\" userid=\"{steam_id}\" name=\"Steam Player Updated\""
    )));
    assert!(rendered.contains(&format!(
        "<blacklisted platform=\"EOS\" userid=\"{eos_id}\" name=\"EOS Player Updated\""
    )));

    for (platform, userid) in [("Steam", steam_id), ("EOS", eos_id)] {
        apply_instance_player_access_mutation(
            &paths,
            ApplyInstancePlayerAccessMutationInput {
                instance_id: created.summary.id.clone(),
                field_key: String::from("blacklist_entries"),
                operation: PlayerAccessMutationOperation::Remove,
                value: json!({"platform":platform,"userid":userid}),
                expected_value: None,
            },
        )
        .await
        .expect("remove composite 7DTD identity");
    }

    let removed = read_instance_details(&paths, &created.summary.id)
        .await
        .expect("read removed composite identities");
    let removed_settings: Value =
        serde_json::from_str(&removed.settings_json).expect("parse removed composite identities");
    assert_eq!(removed_settings["blacklist_entries"], json!([]));
    assert!(
        !fs::read_to_string(&rendered_path)
            .expect("read blacklist after removals")
            .contains(steam_id)
    );
    assert!(
        !fs::read_to_string(&rendered_path)
            .expect("read blacklist after EOS removal")
            .contains(eos_id)
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn rimworld_retired_whitelist_mutation_is_rejected() {
    let (root, paths, created) =
        provision_real_module_instance("rimworld", "Player Access RimWorld Integration").await;
    let result = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: String::from("whitelisted_users"),
            operation: PlayerAccessMutationOperation::Add,
            value: json!("Alice"),
            expected_value: None,
        },
    )
    .await;
    assert!(
        result.is_err(),
        "retired whitelist cannot become a successful no-op"
    );
    assert!(
        !instance_private_runtime_root(&created)
            .join("Configs/WhitelistConfig.json")
            .exists()
    );
    cleanup_root(&root);
}

#[tokio::test]
async fn real_module_text_rosters_persist_render_and_materialize_player_access_mutations() {
    let cases = [
        TextRosterCase {
            module_id: "arksurvivalevolved",
            field_key: "priority_join_list",
            add_value: "76561198000000010",
            remove_value: "76561198000000010",
            expected_stored_value: "76561198000000010",
            rendered_relative_path: "PlayersJoinNoCheckList.txt",
            expected_rendered_fragment: "76561198000000010",
            expected_sync_mode: PlayerAccessSyncMode::Direct,
            expected_add_action_id: Some("allow_no_check"),
            expected_remove_action_id: Some("disallow_no_check"),
        },
        TextRosterCase {
            module_id: "humanitz",
            field_key: "banned_player_steam_ids",
            add_value: "|FEDCBA9876543210fedcba9876543210",
            remove_value: "|FEDCBA9876543210fedcba9876543210",
            expected_stored_value: "|FEDCBA9876543210fedcba9876543210",
            rendered_relative_path: "F_BannedPlayers.txt",
            expected_rendered_fragment: "|FEDCBA9876543210fedcba9876543210",
            expected_sync_mode: PlayerAccessSyncMode::Direct,
            expected_add_action_id: Some("ban_player"),
            expected_remove_action_id: Some("unban_player"),
        },
        TextRosterCase {
            module_id: "minecraft",
            field_key: "banned_player_entries",
            add_value: "A0EEBC999C0B4EF8BB6D6BB9BD380A11,Alex,Integration test",
            remove_value: "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
            expected_stored_value: "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11,Alex,Integration test",
            rendered_relative_path: "banned-players.json",
            expected_rendered_fragment: "\"uuid\": \"a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11\"",
            expected_sync_mode: PlayerAccessSyncMode::Restart,
            expected_add_action_id: None,
            expected_remove_action_id: None,
        },
        TextRosterCase {
            module_id: "rust",
            field_key: "banned_entries",
            add_value: "76561198000000012|Integration test",
            remove_value: "76561198000000012",
            expected_stored_value: "76561198000000012|Integration test",
            rendered_relative_path: "bans.cfg",
            expected_rendered_fragment: "banid 76561198000000012 \"LanGame\" \"Integration test\"",
            expected_sync_mode: PlayerAccessSyncMode::Direct,
            expected_add_action_id: Some("ban_steamid"),
            expected_remove_action_id: Some("unban_steamid"),
        },
        TextRosterCase {
            module_id: "terraria",
            field_key: "banlist_entries",
            add_value: "Troublemaker",
            remove_value: "Troublemaker",
            expected_stored_value: "Troublemaker",
            rendered_relative_path: "banlist.txt",
            expected_rendered_fragment: "Troublemaker",
            expected_sync_mode: PlayerAccessSyncMode::Restart,
            expected_add_action_id: None,
            expected_remove_action_id: None,
        },
        TextRosterCase {
            module_id: "unturned",
            field_key: "admin_steam_ids",
            add_value: "76561198000000014",
            remove_value: "76561198000000014",
            expected_stored_value: "76561198000000014",
            rendered_relative_path: "Commands.dat",
            expected_rendered_fragment: "Admin 76561198000000014",
            expected_sync_mode: PlayerAccessSyncMode::Direct,
            expected_add_action_id: Some("add_admin"),
            expected_remove_action_id: Some("remove_admin"),
        },
        TextRosterCase {
            module_id: "vrising",
            field_key: "ban_list",
            add_value: "76561198000000013",
            remove_value: "76561198000000013",
            expected_stored_value: "76561198000000013",
            rendered_relative_path: "Settings/banlist.txt",
            expected_rendered_fragment: "76561198000000013",
            expected_sync_mode: PlayerAccessSyncMode::Restart,
            expected_add_action_id: None,
            expected_remove_action_id: None,
        },
    ];

    for case in cases {
        let instance_name = format!("Player Access {} Integration", case.module_id);
        let (root, paths, created) =
            provision_real_module_instance(case.module_id, &instance_name).await;

        let added = apply_instance_player_access_mutation(
            &paths,
            ApplyInstancePlayerAccessMutationInput {
                instance_id: created.summary.id.clone(),
                field_key: String::from(case.field_key),
                operation: PlayerAccessMutationOperation::Add,
                value: json!(case.add_value),
                expected_value: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{} add mutation failed: {error}", case.module_id));

        assert_eq!(
            added.persistent_status,
            PlayerAccessPersistentStatus::Updated,
            "{} add mutation status",
            case.module_id
        );
        assert_eq!(added.sync.mode, case.expected_sync_mode);
        assert_eq!(
            added
                .sync
                .mutation_action_id(PlayerAccessMutationOperation::Add),
            case.expected_add_action_id
        );
        let added_settings: Value = serde_json::from_str(&added.details.settings_json)
            .unwrap_or_else(|error| panic!("{} settings JSON: {error}", case.module_id));
        assert_eq!(
            added_settings[case.field_key],
            json!(case.expected_stored_value),
            "{} persisted roster value",
            case.module_id
        );

        let config_root = Path::new(&created.config_file_path)
            .parent()
            .expect("instance config root");
        let rendered_path = config_root.join(case.rendered_relative_path);
        let rendered = fs::read_to_string(&rendered_path).unwrap_or_else(|error| {
            panic!(
                "{} rendered roster file {}: {error}",
                case.module_id,
                rendered_path.display()
            )
        });
        assert!(
            rendered.contains(case.expected_rendered_fragment),
            "{} rendered roster file did not contain `{}`:\n{}",
            case.module_id,
            case.expected_rendered_fragment,
            rendered
        );
        assert_materialized_roster_matches(
            &paths,
            &created,
            case.module_id,
            &rendered_path,
            &rendered,
        );

        let removed = apply_instance_player_access_mutation(
            &paths,
            ApplyInstancePlayerAccessMutationInput {
                instance_id: created.summary.id.clone(),
                field_key: String::from(case.field_key),
                operation: PlayerAccessMutationOperation::Remove,
                value: json!(case.remove_value),
                expected_value: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{} remove mutation failed: {error}", case.module_id));

        assert_eq!(
            removed.persistent_status,
            PlayerAccessPersistentStatus::Updated,
            "{} remove mutation status",
            case.module_id
        );
        assert_eq!(
            removed
                .sync
                .mutation_action_id(PlayerAccessMutationOperation::Remove),
            case.expected_remove_action_id
        );
        let removed_settings: Value = serde_json::from_str(&removed.details.settings_json)
            .unwrap_or_else(|error| {
                panic!("{} settings JSON after removal: {error}", case.module_id)
            });
        assert_eq!(
            removed_settings[case.field_key],
            json!(""),
            "{} roster should be empty after removal",
            case.module_id
        );

        let rendered_after_remove = fs::read_to_string(&rendered_path).unwrap_or_else(|error| {
            panic!(
                "{} rendered roster file after removal {}: {error}",
                case.module_id,
                rendered_path.display()
            )
        });
        assert!(
            !rendered_after_remove.contains(case.expected_rendered_fragment),
            "{} rendered roster retained the removed entry:\n{}",
            case.module_id,
            rendered_after_remove
        );
        assert_materialized_roster_matches(
            &paths,
            &created,
            case.module_id,
            &rendered_path,
            &rendered_after_remove,
        );

        let reloaded = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap_or_else(|error| panic!("{} reload failed: {error}", case.module_id));
        let reloaded_settings: Value = serde_json::from_str(&reloaded.settings_json)
            .unwrap_or_else(|error| panic!("{} reloaded settings JSON: {error}", case.module_id));
        assert_eq!(reloaded_settings[case.field_key], json!(""));

        cleanup_root(&root);
    }
}

async fn provision_real_module_instance(
    module_id: &str,
    instance_name: &str,
) -> (PathBuf, StoragePaths, InstanceProvisioning) {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");

    fs::create_dir_all(paths.database_path.parent().expect("test database parent"))
        .expect("create test database root");
    fs::create_dir_all(paths.games_root.join(module_id)).expect("create fake install root");
    initialize_database(&paths)
        .await
        .expect("initialize player-access integration database");

    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .expect("discover repository modules")
        .into_iter()
        .find(|descriptor| descriptor.summary.id == module_id)
        .unwrap_or_else(|| panic!("repository module `{module_id}` is missing"));
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap_or_else(|error| panic!("sync module `{module_id}`: {error}"));

    replenish_test_library(&paths, &descriptor).await;

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from(instance_name),
            module_id: String::from(module_id),
        },
    )
    .await
    .unwrap_or_else(|error| panic!("create `{module_id}` integration instance: {error}"));

    (root, paths, created)
}

fn assert_materialized_roster_matches(
    _paths: &StoragePaths,
    created: &InstanceProvisioning,
    module_id: &str,
    rendered_path: &Path,
    rendered: &str,
) {
    let instance_root = Path::new(&created.config_file_path)
        .parent()
        .and_then(Path::parent)
        .expect("instance root");
    let runtime_root = resolve_instance_runtime_root(instance_root).unwrap();
    let materialized_path = match module_id {
        "arksurvivalevolved" => Some(
            runtime_root
                .join("ShooterGame")
                .join("Binaries")
                .join("Win64")
                .join("PlayersJoinNoCheckList.txt"),
        ),
        "humanitz" => Some(
            runtime_root
                .join("HumanitZServer")
                .join("F_BannedPlayers.txt"),
        ),
        "minecraft" => Some(instance_root.join("banned-players.json")),
        "rust" => Some(
            runtime_root
                .join("server")
                .join(&created.summary.id)
                .join("cfg")
                .join("bans.cfg"),
        ),
        "terraria" => None,
        "unturned" => Some(
            runtime_root
                .join("Servers")
                .join(&created.summary.id)
                .join("Server")
                .join("Commands.dat"),
        ),
        "vrising" => Some(instance_root.join("Settings").join("banlist.txt")),
        _ => panic!("unexpected player-access integration module `{module_id}`"),
    };

    let Some(materialized_path) = materialized_path else {
        return;
    };
    assert_ne!(materialized_path, rendered_path);
    if module_id == "vrising" && rendered.trim().is_empty() {
        assert!(
            !materialized_path.exists(),
            "empty native V Rising overrides must be absent"
        );
        return;
    }
    let materialized = fs::read_to_string(&materialized_path).unwrap_or_else(|error| {
        panic!(
            "{module_id} materialized roster file {}: {error}",
            materialized_path.display()
        )
    });
    assert_eq!(
        materialized, rendered,
        "{module_id} materialized roster drifted from rendered configuration"
    );
}

#[path = "tests_seven_days_ban_native.rs"]
mod seven_days_ban_native;

#[tokio::test]
async fn humanitz_net_id_rosters_preserve_old_ids_and_only_dispatch_complete_new_ids() {
    let (root, paths, created) =
        provision_real_module_instance("humanitz", "HumanitZ NetID preservation").await;
    let original = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let old = "76561198000000001";
    let full = "0123456789abcdef0123456789ABCDEF|FEDCBA9876543210fedcba9876543210";
    let product_only = "|FEDCBA9876543210fedcba9876543210";
    let config_path = Path::new(&original.config_file_path);
    let mut document: Value = serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
    document["settings"]["banned_player_steam_ids"] = json!(old);
    fs::write(config_path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let native = instance_private_runtime_root(&created).join("HumanitZServer/F_BannedPlayers.txt");
    fs::write(&native, old).unwrap();
    for target in [full, product_only] {
        let added = apply_instance_player_access_mutation(
            &paths,
            ApplyInstancePlayerAccessMutationInput {
                instance_id: created.summary.id.clone(),
                field_key: "banned_player_steam_ids".to_owned(),
                operation: PlayerAccessMutationOperation::Add,
                value: json!(target),
                expected_value: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(added.live_target, target);
        assert_eq!(added.sync.mode, PlayerAccessSyncMode::Direct);
        assert_eq!(
            added
                .sync
                .mutation_action_id(PlayerAccessMutationOperation::Add),
            Some("ban_player")
        );
    }
    let loaded = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&loaded.settings_json).unwrap();
    assert_eq!(
        settings["banned_player_steam_ids"],
        json!(format!("{old}\n{full}\n{product_only}"))
    );
    let native_before = fs::read(&native).unwrap();
    assert_eq!(
        String::from_utf8(native_before.clone()).unwrap().trim(),
        format!("{old}\n{full}\n{product_only}")
    );
    for invalid in [
        old,
        "FEDCBA9876543210fedcba9876543210",
        "0123|4567",
        "Host_Offline",
    ] {
        assert!(
            apply_instance_player_access_mutation(
                &paths,
                ApplyInstancePlayerAccessMutationInput {
                    instance_id: created.summary.id.clone(),
                    field_key: "banned_player_steam_ids".to_owned(),
                    operation: PlayerAccessMutationOperation::Add,
                    value: json!(invalid),
                    expected_value: None,
                }
            )
            .await
            .is_err()
        );
        assert_eq!(
            read_instance_details(&paths, &created.summary.id)
                .await
                .unwrap()
                .settings_json,
            loaded.settings_json
        );
        assert_eq!(fs::read(&native).unwrap(), native_before);
    }
    let removed = apply_instance_player_access_mutation(
        &paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: created.summary.id.clone(),
            field_key: "banned_player_steam_ids".to_owned(),
            operation: PlayerAccessMutationOperation::Remove,
            value: json!(old),
            expected_value: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        removed.persistent_status,
        PlayerAccessPersistentStatus::Updated
    );
    assert_eq!(removed.sync.mode, PlayerAccessSyncMode::Restart);
    assert_eq!(
        removed
            .sync
            .mutation_action_id(PlayerAccessMutationOperation::Remove),
        None,
        "a retained Steam identity must not dispatch an RCON unban target"
    );
    let loaded = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&loaded.settings_json).unwrap();
    assert_eq!(
        settings["banned_player_steam_ids"],
        json!(format!("{full}\n{product_only}"))
    );
    assert_eq!(
        fs::read_to_string(&native).unwrap().trim(),
        format!("{full}\n{product_only}")
    );
    cleanup_root(&root);
}

#[tokio::test]
async fn humanitz_whole_settings_save_only_retains_preexisting_steam_ids_in_the_same_roster() {
    let (root, paths, created) =
        provision_real_module_instance("humanitz", "HumanitZ whole settings boundary").await;
    let original = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let config_path = Path::new(&original.config_file_path);
    let native = instance_private_runtime_root(&created).join("HumanitZServer/F_BannedPlayers.txt");
    let original_config = fs::read(config_path).unwrap();
    let original_native = fs::read(&native).unwrap();
    let old = "76561198000000001";
    let full = "0123456789abcdef0123456789ABCDEF|FEDCBA9876543210fedcba9876543210";
    let save = |settings: &Map<String, Value>| UpdateInstanceInput {
        id: original.summary.id.clone(),
        bind_ip: original.summary.bind_ip.clone(),
        auto_backup_on_stop: original.auto_backup_on_stop,
        backup_retention_count: original.backup_retention_count,
        settings_json: serde_json::to_string(settings).unwrap(),
        ports: original.ports.clone(),
    };
    let mut settings: Map<String, Value> = serde_json::from_str(&original.settings_json).unwrap();
    for key in [
        "admin_steam_ids",
        "reserved_player_steam_ids",
        "banned_player_steam_ids",
    ] {
        let mut incoming = settings.clone();
        incoming.insert(key.to_owned(), json!(old));
        let error = update_instance(&paths, save(&incoming))
            .await
            .expect_err("whole settings saves must reject newly introduced Steam IDs");
        assert!(
            matches!(error, StorageError::InvalidModuleSetting { ref field, .. } if field == key)
        );
        assert_eq!(fs::read(config_path).unwrap(), original_config);
        assert_eq!(fs::read(&native).unwrap(), original_native);
        assert_eq!(
            read_instance_details(&paths, &created.summary.id)
                .await
                .unwrap()
                .settings_json,
            original.settings_json
        );
    }

    // Reproduce a real persisted roster from the previous Steam-ID implementation.
    let mut document: Value = serde_json::from_slice(&original_config).unwrap();
    document["settings"]["banned_player_steam_ids"] = json!(old);
    fs::write(config_path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    fs::write(&native, old).unwrap();
    settings.insert("banned_player_steam_ids".to_owned(), json!(old));
    settings.insert("server_name".to_owned(), json!("Retained roster"));
    update_instance(&paths, save(&settings)).await.unwrap();
    assert_eq!(fs::read_to_string(&native).unwrap().trim(), old);
    let retained_config = fs::read(config_path).unwrap();
    let retained_native = fs::read(&native).unwrap();

    let mut moved = settings.clone();
    moved.insert("banned_player_steam_ids".to_owned(), json!(""));
    moved.insert("admin_steam_ids".to_owned(), json!(old));
    assert!(
        update_instance(&paths, save(&moved)).await.is_err(),
        "a Steam ID stored in one roster must not be newly granted another roster's role"
    );
    assert_eq!(fs::read(config_path).unwrap(), retained_config);
    assert_eq!(fs::read(&native).unwrap(), retained_native);

    settings.insert("banned_player_steam_ids".to_owned(), json!(full));
    update_instance(&paths, save(&settings)).await.unwrap();
    assert_eq!(fs::read_to_string(&native).unwrap().trim(), full);
    let removed_config = fs::read(config_path).unwrap();
    let removed_native = fs::read(&native).unwrap();
    settings.insert(
        "banned_player_steam_ids".to_owned(),
        json!(format!("{full}\n{old}")),
    );
    assert!(
        update_instance(&paths, save(&settings)).await.is_err(),
        "an explicitly removed Steam ID must not become addable again"
    );
    assert_eq!(fs::read(config_path).unwrap(), removed_config);
    assert_eq!(fs::read(&native).unwrap(), removed_native);
    cleanup_root(&root);
}

#[tokio::test]
async fn humanitz_creation_cannot_treat_steam_ids_in_schema_defaults_as_existing_rosters() {
    let (root, paths, created) =
        provision_real_module_instance("humanitz", "HumanitZ creation boundary").await;
    let mut descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "humanitz")
        .unwrap();
    replenish_test_library(&paths, &descriptor).await;
    let original_schema: Value =
        serde_json::from_str(descriptor.schema_json.as_ref().unwrap()).unwrap();
    let instances_before = list_instances(&paths).await.unwrap();
    let config_before = fs::read(&created.config_file_path).unwrap();
    for key in [
        "admin_steam_ids",
        "reserved_player_steam_ids",
        "banned_player_steam_ids",
    ] {
        let mut schema = original_schema.clone();
        schema["properties"][key]["default"] = json!("76561198000000001");
        descriptor.schema_json = Some(schema.to_string());
        let error = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: format!("Rejected HumanitZ {key}"),
                module_id: "humanitz".to_owned(),
            },
        )
        .await
        .expect_err("new instances have no persisted Steam roster baseline");
        assert!(
            matches!(error, StorageError::InvalidModuleSetting { ref field, .. } if field == key)
        );
        assert_eq!(
            list_instances(&paths).await.unwrap().len(),
            instances_before.len()
        );
        assert_eq!(fs::read(&created.config_file_path).unwrap(), config_before);
    }
    cleanup_root(&root);
}
