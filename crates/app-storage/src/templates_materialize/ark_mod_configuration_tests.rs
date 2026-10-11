use super::*;

fn configuration() -> Value {
    json!({"mod_ids_csv":"1254808,99", "asa_mod_configurations": {
        "1254808":{"game_user_settings_ini":"[BetterVariants]\nDebug=True\nDebugSpawns=False\n"},
        "99":{"game_ini":"[OtherMod]\nChoice=First\nChoice=Second\n"}
    }})
}

#[test]
fn ark_mod_configuration_writes_both_files_and_preserves_unmanaged_content() {
    let fixture = Fixture::new("arksurvivalascended");
    for dir in [&fixture.config, &fixture.live] {
        fs::write(dir.join("GameUserSettings.ini"), "[Unmanaged]\nKeep=1\n").unwrap();
    }
    fixture.write(&configuration()).unwrap().commit();
    for dir in [&fixture.config, &fixture.live] {
        let gus = fs::read_to_string(dir.join("GameUserSettings.ini")).unwrap();
        assert!(
            gus.contains("[BetterVariants]\nDebug=True\nDebugSpawns=False"),
            "{gus}"
        );
        assert!(gus.contains("Keep=1"));
        let game = fs::read_to_string(dir.join("Game.ini")).unwrap();
        assert!(game.contains("Choice=First\nChoice=Second"));
    }
}

#[test]
fn ark_mod_configuration_disable_reenable_and_remove_only_owned_entries() {
    let fixture = Fixture::new("arksurvivalascended");
    let mut settings = configuration();
    fixture.write(&settings).unwrap().commit();
    settings["mod_ids_csv"] = json!("99");
    fixture.write(&settings).unwrap().commit();
    for dir in [&fixture.config, &fixture.live] {
        assert!(
            !fs::read_to_string(dir.join("GameUserSettings.ini"))
                .unwrap()
                .contains("Debug=True")
        );
        assert!(
            fs::read_to_string(dir.join("Game.ini"))
                .unwrap()
                .contains("Choice=Second")
        );
    }
    settings["mod_ids_csv"] = json!("1254808,99");
    fixture.write(&settings).unwrap().commit();
    assert!(
        fs::read_to_string(fixture.live.join("GameUserSettings.ini"))
            .unwrap()
            .contains("Debug=True")
    );
    settings["asa_mod_configurations"]["1254808"] = json!({});
    fixture.write(&settings).unwrap().commit();
    assert!(
        !fs::read_to_string(fixture.live.join("GameUserSettings.ini"))
            .unwrap()
            .contains("Debug=True")
    );
}

#[test]
fn ark_mod_configuration_shared_native_arrays_append_and_detect_same_class() {
    let fixture = Fixture::new("arksurvivalascended");
    let mut settings = json!({"mod_ids_csv":"1,2", "override_named_engram_entries":"(EngramClassName=\"Native_C\",EngramHidden=True)",
    "asa_mod_configurations": {
        "1":{"game_ini":"[/Script/ShooterGame.ShooterGameMode]\nOverrideNamedEngramEntries=(EngramClassName=\"ModA_C\",EngramHidden=True)\nOverrideNamedEngramEntries=(EngramClassName=\"ModB_C\",EngramHidden=False)"},
        "2":{"game_ini":"[/Script/ShooterGame.ShooterGameMode]\nOverrideNamedEngramEntries=(EngramClassName=\"ModC_C\",EngramHidden=True)"}
    }});
    crate::templates::validate_asa_mod_configurations(settings.as_object().unwrap()).unwrap();
    fixture.write(&settings).unwrap().commit();
    let actual = fs::read_to_string(fixture.live.join("Game.ini")).unwrap();
    for name in ["Native_C", "ModA_C", "ModB_C", "ModC_C"] {
        assert!(actual.contains(name), "{actual}");
    }
    settings["asa_mod_configurations"]["2"]["game_ini"] = json!(
        "[/script/shootergame.shootergamemode]\nOverrideNamedEngramEntries=(EngramClassName=\"MODA_C\",EngramHidden=False)"
    );
    assert!(
        crate::templates::validate_asa_mod_configurations(settings.as_object().unwrap())
            .unwrap_err()
            .contains("conflicts")
    );
    assert!(fixture.write(&settings).is_err());
    assert_eq!(
        fs::read_to_string(fixture.live.join("Game.ini")).unwrap(),
        actual
    );
}

#[test]
fn ark_mod_configuration_raw_override_releases_replaced_native_class() {
    let fixture = Fixture::new("arksurvivalascended");
    let settings = json!({"mod_ids_csv":"1",
        "override_named_engram_entries":"(EngramClassName=\"NativeA_C\",EngramHidden=True)",
        "game_ini_extra":"OverrideNamedEngramEntries=(EngramClassName=\"RawB_C\",EngramHidden=True)",
        "asa_mod_configurations":{"1":{"game_ini":"[/Script/ShooterGame.ShooterGameMode]\nOverrideNamedEngramEntries=(EngramClassName=\"NativeA_C\",EngramHidden=False)"}}
    });
    crate::templates::validate_asa_mod_configurations(settings.as_object().unwrap()).unwrap();
    fixture.write(&settings).unwrap().commit();
    let actual = fs::read_to_string(fixture.live.join("Game.ini")).unwrap();
    assert!(actual.contains("EngramClassName=\"RawB_C\",EngramHidden=True"));
    assert!(actual.contains("EngramClassName=\"NativeA_C\",EngramHidden=False"));
    assert!(!actual.contains("EngramClassName=\"NativeA_C\",EngramHidden=True"));
}

#[test]
fn ark_mod_configuration_rejects_ambiguous_fragments_destinations_and_conflicts() {
    for invalid in [
        json!({"../42":{"game_ini":""}}),
        json!({"1":{"path":"../../Other.ini"}}),
        json!({"1":{"game_ini":"Key=NoSection"}}),
        json!({"1":{"game_ini":"[Mod]\nKey=bad\u{0000}"}}),
        json!({"1":{"game_ini":"x".repeat(65537)}}),
        json!({"1":{"game_user_settings_ini":"[SessionSettings]\nSessionName=override"}}),
        json!({"1":{"game_ini":"[Shared]\nValue=1"},"2":{"game_ini":"[shared]\nvalue=2"}}),
    ] {
        let settings = json!({"mod_ids_csv":"1,2", "asa_mod_configurations":invalid});
        assert!(
            crate::templates::validate_asa_mod_configurations(settings.as_object().unwrap())
                .is_err(),
            "{settings}"
        );
    }
    let settings = json!({"mod_ids_csv":"1", "game_ini_extra":"[Shared]\nValue=raw",
        "asa_mod_configurations":{"1":{"game_ini":"[shared]\nVALUE=mod"}}});
    assert!(
        crate::templates::validate_asa_mod_configurations(settings.as_object().unwrap())
            .unwrap_err()
            .contains("conflicts")
    );
}

#[test]
fn ark_mod_configuration_transaction_failure_keeps_ini_and_ownership() {
    let fixture = Fixture::new("arksurvivalascended");
    let mut settings = configuration();
    fixture.write(&settings).unwrap().commit();
    let paths = [
        fixture.config.join("instance.json"),
        fixture.config.join("ark-native-ownership.json"),
        fixture.config.join("GameUserSettings.ini"),
        fixture.live.join("GameUserSettings.ini"),
    ];
    let before = paths
        .iter()
        .map(|path| fs::read(path).unwrap())
        .collect::<Vec<_>>();
    settings["asa_mod_configurations"]["1254808"]["game_user_settings_ini"] =
        json!("[BetterVariants]\nDebug=False");
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.config.join("instance.json"));
    assert!(fixture.write(&settings).is_err());
    for (path, bytes) in paths.iter().zip(before) {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
