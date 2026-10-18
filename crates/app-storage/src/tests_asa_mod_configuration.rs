use super::*;

#[tokio::test]
async fn asa_mod_configuration_storage_cas_restart_and_running_guard() {
    let mut fixture = MapFixture::new("arksurvivalascended").await;
    let initial_json = fixture.details.settings_json.clone();
    let mut settings = fixture.settings();
    settings["mod_ids_csv"] = json!("1254808");
    settings["asa_mod_configurations"] =
        json!({"1254808":{"game_user_settings_ini":"[BetterVariants]\nDebug=True\n"}});
    fixture.details =
        update_instance_if_current(&fixture.paths, fixture.input(&settings), &initial_json)
            .await
            .unwrap();
    fixture.details = read_instance_details(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        fixture.settings()["asa_mod_configurations"],
        settings["asa_mod_configurations"]
    );
    let config = PathBuf::from(&fixture.created.config_file_path);
    let before = fs::read(&config).unwrap();
    assert!(
        update_instance_if_current(&fixture.paths, fixture.input(&settings), &initial_json)
            .await
            .is_err()
    );
    let mut invalid = settings.clone();
    invalid["asa_mod_configurations"]["1254808"]["game_user_settings_ini"] = json!("Debug=False");
    assert!(matches!(
        update_instance(&fixture.paths, fixture.input(&invalid)).await,
        Err(StorageError::InvalidModuleSetting { .. })
    ));
    assert_eq!(fs::read(&config).unwrap(), before);
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    let live_ini = instance_private_runtime_root(&fixture.created)
        .join("ShooterGame/Saved/Config/WindowsServer/GameUserSettings.ini");
    assert!(
        fs::read_to_string(&live_ini)
            .unwrap()
            .contains("Debug=True")
    );

    let run_log = fixture.root.join("mod-config-running.log");
    fs::write(&run_log, b"synthetic registered process, not executed").unwrap();
    let run = record_started_test_instance(
        &fixture.paths,
        &fixture.created.summary.id,
        9_999_997,
        &run_log.to_string_lossy(),
    )
    .await
    .unwrap();
    // Starting legitimately regenerates instance.json (including its timestamp).
    // The rejected edit must preserve the state immediately before that edit.
    let before_running_edit = fs::read(&config).unwrap();
    let mut changed = settings.clone();
    changed["asa_mod_configurations"]["1254808"]["game_user_settings_ini"] =
        json!("[BetterVariants]\nDebug=False");
    assert!(matches!(
        update_instance(&fixture.paths, fixture.input(&changed)).await,
        Err(StorageError::InvalidModuleSetting { .. })
    ));
    assert_eq!(fs::read(&config).unwrap(), before_running_edit);
    assert!(
        fs::read_to_string(&live_ini)
            .unwrap()
            .contains("Debug=True")
    );
    mark_instance_process_stopped(
        &fixture.paths,
        &fixture.created.summary.id,
        run.run_id,
        Some(0),
        false,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn ark_mod_map_overrides_materialize_independent_ini_files_and_preserve_unknown_content() {
    for module_id in ["arksurvivalascended", "arksurvivalevolved"] {
        let mut fixture = MapFixture::new(module_id).await;
        fixture.add_two_maps().await;
        for name in ["Game.ini.hbs", "GameUserSettings.ini.hbs"] {
            fs::copy(
                repo_root()
                    .join("modules")
                    .join(module_id)
                    .join("templates")
                    .join(name),
                fixture.descriptor.root.join("templates").join(name),
            )
            .unwrap();
        }
        let asa = module_id == "arksurvivalascended";
        let field = if asa { "mod_ids_csv" } else { "active_mod_ids" };
        let mut settings = fixture.settings();
        settings[field] = json!("1254808");
        if asa {
            settings["asa_mod_configurations"] = json!({"1254808": {
                "game_user_settings_ini": "[ScopedMod]\nValue=Shared\n"
            }});
        }
        fixture.save(&settings).await;
        let native = instance_private_runtime_root(&fixture.created)
            .join("ShooterGame/Saved/Config/WindowsServer/GameUserSettings.ini");
        let mut native_text = fs::read_to_string(&native).unwrap();
        native_text.push_str("\n; retained native note\n[Unmanaged]\nKeep=Native\n");
        fs::write(&native, native_text).unwrap();
        let mut desert =
            json!({"mods": {"1254808": {"active": false}, "941450": {"active": true}}});
        let mut center = json!({"mods": {"1254808": {"active": false}}});
        if asa {
            desert["mods"]["1254808"]["configuration"] = Value::Null;
            center["mods"]["1254808"]["configuration"] = Value::Null;
            desert["mods"]["941450"]["configuration"] = json!({
                "game_user_settings_ini": "[ScopedMod]\nValue=Desert\n"
            });
        }
        settings["ark_mod_map_overrides"] = json!({"map-desert": desert, "map-center": center});
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
            ("main", "1254808"),
            ("map-desert", "941450"),
            ("map-center", ""),
        ] {
            let path = directory.join(key).join("GameUserSettings.ini");
            let text = fs::read_to_string(&path).unwrap();
            assert!(
                text.contains("Keep=Native"),
                "{module_id}/{key} must retain unowned native content"
            );
            if expected.is_empty() {
                assert!(!text.contains("1254808") && !text.contains("941450"));
            } else if !asa {
                assert!(
                    text.contains(&format!("ActiveMods={expected}")),
                    "{module_id}/{key}: {text}"
                );
            }
            if asa {
                match key {
                    "main" => {
                        assert!(text.contains("Value=Shared") && !text.contains("Value=Desert"))
                    }
                    "map-desert" => {
                        assert!(text.contains("Value=Desert") && !text.contains("Value=Shared"))
                    }
                    _ => assert!(!text.contains("Value=Shared") && !text.contains("Value=Desert")),
                }
            }
            let projected =
                app_core::ark_maps::project_process(&fixture.details, Some(key)).unwrap();
            let effective: Value = serde_json::from_str(&projected.settings_json).unwrap();
            assert_eq!(
                effective[field], expected,
                "{module_id}/{key} must project its own native Mod list"
            );
            let rcon = projected
                .ports
                .iter()
                .find(|port| port.name == "rcon")
                .unwrap()
                .port;
            assert!(text.contains(&format!("RCONPort={rcon}")));
            assert!(directory.join(key).join("Game.ini").is_file());
        }
        // The game may rewrite an isolated INI using UTF-16. A later edit must
        // preserve its encoding and unowned values while removing old Mod keys.
        let desert_ini = directory.join("map-desert/GameUserSettings.ini");
        let text = fs::read_to_string(&desert_ini)
            .unwrap()
            .replace("\r\n", "\n")
            .replace('\n', "\r\n")
            + "\r\n[MapPrivate]\r\nKeep=独立\r\n";
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(&desert_ini, bytes).unwrap();
        settings["ark_mod_map_overrides"]["map-desert"]["mods"]["941450"]["active"] = json!(false);
        if asa {
            settings["ark_mod_map_overrides"]["map-desert"]["mods"]["941450"]["configuration"] =
                Value::Null;
        }
        fixture.save(&settings).await;
        let bytes = fs::read(&desert_ini).unwrap();
        assert!(bytes.starts_with(&[0xff, 0xfe]));
        let units = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let text = String::from_utf16(&units).unwrap();
        assert!(text.contains("Keep=独立") && text.contains("Keep=Native"));
        assert!(!text.contains("Value=Desert") && !text.contains("Value=Shared"));
        assert!(!text.contains("941450") && !text.contains("1254808"));
        assert!(text.contains("\r\n"));
        // Clearing every override returns to the existing shared native path.
        settings["ark_mod_map_overrides"] = json!({});
        fixture.save(&settings).await;
        let projected = app_core::ark_maps::project_process(&fixture.details, None).unwrap();
        let projected: Value = serde_json::from_str(&projected.settings_json).unwrap();
        assert!(projected.get("_managed_ark_ini_directory").is_none());
        assert_eq!(projected[field], "1254808");
        let shared = fs::read_to_string(&native).unwrap();
        if asa {
            assert!(shared.contains("Value=Shared"));
        } else {
            assert!(shared.contains("ActiveMods=1254808"));
        }
        assert!(
            desert_ini.is_file(),
            "unused isolated files are retained, not deleted"
        );
    }
}

#[tokio::test]
async fn ark_mod_map_overrides_validate_paused_maps_and_do_not_mutate_on_rejection() {
    let mut fixture = MapFixture::new("arksurvivalascended").await;
    fixture.add_two_maps().await;
    let mut settings = fixture.settings();
    settings["additional_maps"][0]["enabled"] = json!(false);
    fixture.save(&settings).await;
    let config = Path::new(&fixture.created.config_file_path);
    let before = fs::read(config).unwrap();
    for mods in [
        json!({"1254808": {"active": true, "configuration": {"game_ini": "MissingSection=1"}}}),
        json!({"1254808": {"active": true, "configuration": {"game_user_settings_ini": "[ServerSettings]\nServerPassword=Changed"}}}),
        json!({"01": {"removed": true}}),
        json!({"123": {"disabled_active": "invalid"}}),
        json!({"1254808": {"server_name": "Unexpected override"}}),
    ] {
        let mut invalid = settings.clone();
        invalid["ark_mod_map_overrides"] = json!({"map-desert": {"mods": mods}});
        assert!(matches!(
            update_instance(&fixture.paths, fixture.input(&invalid)).await,
            Err(StorageError::InvalidModuleSetting { .. })
        ));
        assert_eq!(fs::read(config).unwrap(), before);
    }
}

#[tokio::test]
async fn ark_mod_map_overrides_are_read_only_while_any_map_is_running() {
    for module_id in ["arksurvivalascended", "arksurvivalevolved"] {
        let mut fixture = MapFixture::new(module_id).await;
        let mut settings = fixture.settings();
        settings["ark_mod_map_overrides"] =
            json!({"main": {"mods": {"941450": {"active": false}}}});
        fixture.save(&settings).await;
        let log = fixture.root.join("map-snapshot-registered.log");
        fs::write(&log, b"synthetic process registration only").unwrap();
        let run = record_started_test_instance(
            &fixture.paths,
            &fixture.created.summary.id,
            9_999_996,
            &log.to_string_lossy(),
        )
        .await
        .unwrap();
        let config = Path::new(&fixture.created.config_file_path);
        let before = fs::read(config).unwrap();
        settings["ark_mod_map_overrides"]["main"]["mods"]["941450"]["active"] = json!(true);
        assert!(matches!(
            update_instance(&fixture.paths, fixture.input(&settings)).await,
            Err(StorageError::InvalidModuleSetting { .. })
        ));
        assert_eq!(fs::read(config).unwrap(), before);
        let mut historical_input = settings.clone();
        historical_input
            .as_object_mut()
            .unwrap()
            .remove("ark_mod_map_overrides");
        historical_input["ark_map_mod_overrides"] = if module_id == "arksurvivalascended" {
            json!({"main": {"mod_ids_csv": "941450"}})
        } else {
            json!({"main": {"active_mod_ids": "941450"}})
        };
        assert!(matches!(
            update_instance(&fixture.paths, fixture.input(&historical_input)).await,
            Err(StorageError::InvalidModuleSetting { .. })
        ));
        assert_eq!(fs::read(config).unwrap(), before);
        mark_instance_process_stopped(
            &fixture.paths,
            &fixture.created.summary.id,
            run.run_id,
            Some(0),
            false,
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn ark_mod_map_overrides_roll_back_previous_ini_writes_when_a_later_map_is_unreadable() {
    let mut fixture = MapFixture::new("arksurvivalascended").await;
    fixture.add_two_maps().await;
    for name in ["Game.ini.hbs", "GameUserSettings.ini.hbs"] {
        fs::copy(
            repo_root()
                .join("modules/arksurvivalascended/templates")
                .join(name),
            fixture.descriptor.root.join("templates").join(name),
        )
        .unwrap();
    }
    let mut settings = fixture.settings();
    settings["mod_ids_csv"] = json!("1254808");
    settings["asa_mod_configurations"] = json!({"1254808": {
        "game_user_settings_ini": "[ScopedMod]\nValue=Shared\n"
    }});
    settings["ark_mod_map_overrides"] = json!({
        "map-desert": {"mods": {"1254808": {"configuration": {
            "game_user_settings_ini": "[ScopedMod]\nValue=Desert\n"
        }}}},
        "map-center": {"mods": {"1254808": {"active": false, "configuration": null}}}
    });
    fixture.save(&settings).await;
    let config_dir = Path::new(&fixture.created.config_file_path)
        .parent()
        .unwrap();
    let live_dir = instance_private_runtime_root(&fixture.created)
        .join("ShooterGame/Saved/Config/WindowsServer");
    for (path, note) in [
        (live_dir.join("GameUserSettings.ini"), "SharedUserValue"),
        (
            config_dir.join("ark-maps/main/GameUserSettings.ini"),
            "MainUserValue",
        ),
        (
            config_dir.join("ark-maps/map-desert/Game.ini"),
            "DesertUserValue",
        ),
    ] {
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str(&format!("\n; user-owned note\n[UserOwned]\nKeep={note}\n"));
        fs::write(path, text).unwrap();
    }
    // A directory in place of a native input fails reliably on Windows without
    // administrator privileges or ACL manipulation. Its contents are user data.
    let blocked = config_dir.join("ark-maps/map-center/Game.ini");
    fs::remove_file(&blocked).unwrap();
    fs::create_dir(&blocked).unwrap();
    let retained = blocked.join("keep-user-content.txt");
    fs::write(&retained, b"retained fault-target contents").unwrap();
    let mut paths = vec![PathBuf::from(&fixture.created.config_file_path)];
    for directory in [
        config_dir.to_owned(),
        live_dir,
        config_dir.join("ark-maps/main"),
        config_dir.join("ark-maps/map-desert"),
        config_dir.join("ark-maps/map-center"),
    ] {
        for name in [
            "Game.ini",
            "GameUserSettings.ini",
            "ark-native-ownership.json",
        ] {
            let path = directory.join(name);
            if path.is_file() {
                paths.push(path);
            }
        }
    }
    let before = paths
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect::<Vec<_>>();
    let prior_settings = fixture.details.settings_json.clone();
    settings["server_name"] = json!("Attempted shared rename");
    settings["asa_mod_configurations"]["1254808"]["game_user_settings_ini"] =
        json!("[ScopedMod]\nValue=AttemptedShared\n");
    settings["ark_mod_map_overrides"]["map-desert"]["mods"]["1254808"]["configuration"]["game_user_settings_ini"] =
        json!("[ScopedMod]\nValue=AttemptedDesert\n");
    assert!(
        update_instance(&fixture.paths, fixture.input(&settings))
            .await
            .is_err()
    );
    for (path, bytes) in before {
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "rollback must restore {}",
            path.display()
        );
    }
    assert!(blocked.is_dir());
    assert_eq!(
        fs::read(retained).unwrap(),
        b"retained fault-target contents"
    );
    assert_eq!(
        read_instance_details(&fixture.paths, &fixture.created.summary.id)
            .await
            .unwrap()
            .settings_json,
        prior_settings
    );
}
