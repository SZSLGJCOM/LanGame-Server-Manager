use super::*;

fn copy_native_templates(fixture: &MapFixture) {
    for name in ["Game.ini.hbs", "GameUserSettings.ini.hbs"] {
        fs::copy(
            repo_root()
                .join("modules")
                .join(&fixture.details.summary.module_id)
                .join("templates")
                .join(name),
            fixture.descriptor.root.join("templates").join(name),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn ark_per_mod_targets_persist_and_inherit_other_mods_and_each_shared_ini_file() {
    let mut fixture = MapFixture::new("arksurvivalascended").await;
    fixture.add_two_maps().await;
    copy_native_templates(&fixture);
    let mut settings = fixture.settings();
    settings["mod_ids_csv"] = json!("1254808,941450");
    settings["asa_mod_configurations"] = json!({
        "1254808": {"game_ini": "[Targeted]\nGame=Shared\n", "game_user_settings_ini": "[Targeted]\nUser=Shared\n"},
        "941450": {"game_user_settings_ini": "[OtherMod]\nRate=First\n"}
    });
    settings["ark_mod_map_overrides"] = json!({
        "main": {"mods": {"1254808": {"active": true, "configuration": {"game_user_settings_ini": "[Targeted]\nUser=Targeted\n"}}}},
        "map-desert": {"mods": {"1254808": {"active": true, "configuration": {"game_user_settings_ini": "[Targeted]\nUser=Targeted\n"}}}},
        "map-center": {"mods": {"1254808": {"active": false}}}
    });
    fixture.save(&settings).await;
    fixture.details = read_instance_details(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        fixture.settings()["ark_mod_map_overrides"],
        settings["ark_mod_map_overrides"]
    );
    for (key, expected) in [
        ("main", "1254808,941450"),
        ("map-desert", "1254808,941450"),
        ("map-center", "941450"),
    ] {
        let effective = app_core::ark_mod_scopes::effective_settings(
            "arksurvivalascended",
            &fixture.settings(),
            key,
        )
        .unwrap();
        assert_eq!(effective["mod_ids_csv"], expected);
    }
    // Changes to an unrelated shared Mod, a newly added Mod and an unoverridden
    // INI file must all reach the same maps without rewriting their target rows.
    settings["mod_ids_csv"] = json!("1254808,1715971");
    settings["asa_mod_configurations"]["1254808"]["game_ini"] = json!("[Targeted]\nGame=Updated\n");
    settings["asa_mod_configurations"]["1254808"]["game_user_settings_ini"] =
        json!("[Targeted]\nUser=NewShared\n");
    settings["asa_mod_configurations"]["941450"]["game_user_settings_ini"] =
        json!("[OtherMod]\nRate=Updated\n");
    settings["asa_mod_configurations"]["1715971"] =
        json!({"game_user_settings_ini": "[NewMod]\nEnabled=True\n"});
    fixture.save(&settings).await;
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    fixture.details = read_instance_details(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    let directory = Path::new(&fixture.created.config_file_path)
        .parent()
        .unwrap()
        .join("ark-maps");
    for (key, expected) in [
        ("main", "1254808,1715971"),
        ("map-desert", "1254808,1715971"),
        ("map-center", "1715971"),
    ] {
        let effective = app_core::ark_mod_scopes::effective_settings(
            "arksurvivalascended",
            &fixture.settings(),
            key,
        )
        .unwrap();
        assert_eq!(effective["mod_ids_csv"], expected);
        assert_eq!(
            effective["asa_mod_configurations"]["941450"]["game_user_settings_ini"],
            "[OtherMod]\nRate=Updated\n"
        );
        let user_ini =
            fs::read_to_string(directory.join(key).join("GameUserSettings.ini")).unwrap();
        let game_ini = fs::read_to_string(directory.join(key).join("Game.ini")).unwrap();
        assert!(user_ini.contains("[NewMod]") && user_ini.contains("Enabled=True"));
        assert!(!user_ini.contains("Rate=First") && !user_ini.contains("Rate=Updated"));
        if key == "map-center" {
            assert!(!user_ini.contains("User=Targeted") && !game_ini.contains("Game=Updated"));
        } else {
            assert!(user_ini.contains("User=Targeted") && !user_ini.contains("User=NewShared"));
            assert!(game_ini.contains("Game=Updated"));
        }
    }
    settings["ark_mod_map_overrides"]["main"]["mods"]["1254808"]["configuration"] = Value::Null;
    fixture.save(&settings).await;
    assert!(
        !fs::read_to_string(directory.join("main/Game.ini"))
            .unwrap()
            .contains("Game=Updated")
    );
    assert!(
        !fs::read_to_string(directory.join("main/GameUserSettings.ini"))
            .unwrap()
            .contains("User=Targeted")
    );
    assert!(
        fs::read_to_string(directory.join("map-desert/GameUserSettings.ini"))
            .unwrap()
            .contains("User=Targeted")
    );
}

#[tokio::test]
async fn ark_whole_map_snapshot_conversion_is_read_only_until_the_next_managed_write() {
    let mut fixture = MapFixture::new("arksurvivalascended").await;
    fixture.add_two_maps().await;
    copy_native_templates(&fixture);
    let mut settings = fixture.settings();
    settings["mod_ids_csv"] = json!("1254808,941450");
    settings["passive_mod_ids_csv"] = json!("1715971");
    settings["asa_mod_configurations"] = json!({"1254808": {
        "game_ini": "[Converted]\nGame=Shared\n", "game_user_settings_ini": "[Converted]\nUser=Shared\n"
    }});
    settings["ark_mod_map_overrides"] = json!({
        "map-desert": {"mods": {"1254808": {"configuration": {"game_ini": "", "game_user_settings_ini": "[Converted]\nUser=Desert\n"}}}, "active_order": ["941450", "1254808"]},
        "map-center": {"mods": {"1254808": {"active": false, "configuration": null}, "941450": {"active": false}, "1715971": {"passive": false}}}
    });
    settings["additional_maps"][1]["enabled"] = json!(false);
    fixture.save(&settings).await;
    let config_path = PathBuf::from(&fixture.created.config_file_path);
    let directory = config_path.parent().unwrap().join("ark-maps");
    let desert_ini = directory.join("map-desert/GameUserSettings.ini");
    let mut text = fs::read_to_string(&desert_ini).unwrap();
    text.push_str("\n[UserPreserved]\nKeep=ExistingIsolatedValue\n");
    fs::write(&desert_ini, text).unwrap();
    let mut document: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    document["settings"]
        .as_object_mut()
        .unwrap()
        .remove("ark_mod_map_overrides");
    document["settings"]["ark_map_mod_overrides"] = json!({
        "map-desert": {"mod_ids_csv": "941450,1254808", "passive_mod_ids_csv": "1715971",
            "asa_mod_configurations": {"1254808": {"game_user_settings_ini": "[Converted]\nUser=Desert\n"}}},
        "map-center": {}
    });
    let original = serde_json::to_vec_pretty(&document).unwrap();
    fs::write(&config_path, &original).unwrap();
    fixture.details = read_instance_details(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        fs::read(&config_path).unwrap(),
        original,
        "reading must not rewrite a historical document"
    );
    let converted = fixture.settings();
    assert!(converted.get("ark_map_mod_overrides").is_none());
    assert_eq!(
        converted["ark_mod_map_overrides"]["map-desert"]["active_order"],
        json!(["941450", "1254808"])
    );
    assert_eq!(
        app_core::ark_mod_scopes::effective_settings(
            "arksurvivalascended",
            &converted,
            "map-center"
        )
        .unwrap()["mod_ids_csv"],
        ""
    );
    let expected = fixture.details.settings_json.clone();
    let mut upgraded = converted.clone();
    upgraded["server_name"] = json!("Converted primary room");
    fixture.details =
        update_instance_if_current(&fixture.paths, fixture.input(&upgraded), &expected)
            .await
            .unwrap();
    let after_conversion = fs::read(&config_path).unwrap();
    assert!(
        update_instance_if_current(&fixture.paths, fixture.input(&upgraded), &expected)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&config_path).unwrap(), after_conversion);
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    let persisted: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    assert!(persisted["settings"].get("ark_map_mod_overrides").is_none());
    assert_eq!(
        persisted["settings"]["ark_mod_map_overrides"],
        converted["ark_mod_map_overrides"]
    );
    assert!(
        fs::read_to_string(&desert_ini)
            .unwrap()
            .contains("Keep=ExistingIsolatedValue")
    );
    assert!(
        !fs::read_to_string(directory.join("map-desert/Game.ini"))
            .unwrap()
            .contains("Game=Shared")
    );
    let mut next = upgraded;
    next["mod_ids_csv"] = json!("1254808,941450,777777");
    fixture.save(&next).await;
    assert_eq!(
        app_core::ark_mod_scopes::effective_settings(
            "arksurvivalascended",
            &fixture.settings(),
            "map-desert"
        )
        .unwrap()["mod_ids_csv"],
        "941450,1254808,777777"
    );
    assert_eq!(
        app_core::ark_mod_scopes::effective_settings(
            "arksurvivalascended",
            &fixture.settings(),
            "map-center"
        )
        .unwrap()["mod_ids_csv"],
        "777777"
    );
}

#[tokio::test]
async fn ark_shared_config_edits_validate_effective_conflicts_on_paused_map_overrides() {
    let mut fixture = MapFixture::new("arksurvivalascended").await;
    fixture.add_two_maps().await;
    let mut settings = fixture.settings();
    settings["additional_maps"][0]["enabled"] = json!(false);
    settings["mod_ids_csv"] = json!("1254808,941450");
    settings["asa_mod_configurations"] = json!({
        "1254808": {"game_user_settings_ini": "[First]\nA=1\n"},
        "941450": {"game_user_settings_ini": "[Second]\nB=1\n"}
    });
    settings["ark_mod_map_overrides"] = json!({"map-desert": {"mods": {"1254808": {
        "configuration": {"game_user_settings_ini": "[Collision]\nValue=Local\n"}
    }}}});
    fixture.save(&settings).await;
    let path = Path::new(&fixture.created.config_file_path);
    let before = fs::read(path).unwrap();
    settings["asa_mod_configurations"]["941450"]["game_user_settings_ini"] =
        json!("[Collision]\nValue=Shared\n");
    assert!(matches!(
        update_instance(&fixture.paths, fixture.input(&settings)).await,
        Err(StorageError::InvalidModuleSetting { .. })
    ));
    assert_eq!(fs::read(path).unwrap(), before);
}
