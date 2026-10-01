use super::*;

#[test]
fn ark_ini_extra_deletion_uses_exact_keys_and_keeps_unmanaged_indexes() {
    for module_id in ["arksurvivalascended", "arksurvivalevolved"] {
        let fixture = Fixture::new(module_id);
        let original = "[MyMod]\nCustom[0]=original\nCustom[1]=unmanaged\n; keep this note\nIndependent=keep\n";
        for directory in [&fixture.config, &fixture.live] {
            fs::write(directory.join("Game.ini"), original).unwrap();
        }
        fixture
            .write(&json!({"game_ini_extra":"[MyMod]\nCustom[0]=managed\n"}))
            .unwrap()
            .commit();
        fixture
            .write(&json!({"game_ini_extra":""}))
            .unwrap()
            .commit();
        for directory in [&fixture.config, &fixture.live] {
            let actual = fs::read_to_string(directory.join("Game.ini")).unwrap();
            assert!(!actual.contains("Custom[0]="), "{actual}");
            assert!(actual.contains("Custom[1]=unmanaged"));
            assert!(actual.contains("Independent=keep"));
            assert!(actual.contains("; keep this note"));
        }
    }
}

#[test]
fn ark_ini_running_edits_keep_materialized_ownership_until_stop() {
    let fixture = Fixture::new("arksurvivalascended");
    fixture.write(&json!({"game_ini_extra":"[MyMod]\nA=1\n", "game_user_settings_extra":"[MyMod]\nGusA=1\n"})).unwrap().commit();
    let marker = fixture.config.join("ark-native-ownership.json");
    let materialized_ownership = fs::read(&marker).unwrap();
    for key in ["B", "C"] {
        fixture
            .write_with_status(
                &json!({"game_ini_extra":format!("[MyMod]\n{key}=1\n")}),
                true,
            )
            .unwrap()
            .commit();
        assert_eq!(fs::read(&marker).unwrap(), materialized_ownership);
        assert!(
            fs::read_to_string(fixture.live.join("Game.ini"))
                .unwrap()
                .contains("A=1")
        );
    }
    fixture
        .write(&json!({"game_ini_extra":"[MyMod]\nC=1\n"}))
        .unwrap()
        .commit();
    for directory in [&fixture.config, &fixture.live] {
        let actual = fs::read_to_string(directory.join("Game.ini")).unwrap();
        assert!(!actual.contains("\nA=1"));
        assert!(!actual.contains("\nB=1"));
        assert!(actual.contains("\nC=1"));
        assert!(
            !fs::read_to_string(directory.join("GameUserSettings.ini"))
                .unwrap()
                .contains("GusA=")
        );
    }
}

#[test]
fn ark_ini_ownership_rolls_back_with_files_and_rejects_corruption() {
    let fixture = Fixture::new("arksurvivalevolved");
    fixture
        .write(&json!({"game_ini_extra":"[MyMod]\nOwned=1\n"}))
        .unwrap()
        .commit();
    let marker = fixture.config.join("ark-native-ownership.json");
    let before = fs::read(&marker).unwrap();
    let native = fs::read(fixture.live.join("Game.ini")).unwrap();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.config.join("instance.json"));
    assert!(fixture.write(&json!({"game_ini_extra":""})).is_err());
    assert_eq!(fs::read(&marker).unwrap(), before);
    assert_eq!(fs::read(fixture.live.join("Game.ini")).unwrap(), native);
    fs::write(&marker, "{broken ownership").unwrap();
    assert!(fixture.write(&json!({"game_ini_extra":""})).is_err());
    assert_eq!(fs::read(fixture.live.join("Game.ini")).unwrap(), native);
    assert_eq!(fs::read_to_string(marker).unwrap(), "{broken ownership");
}

#[test]
fn ark_ini_bootstraps_prior_extra_before_first_running_edit() {
    let fixture = Fixture::new("arksurvivalevolved");
    fixture
        .write(&json!({"game_ini_extra":"[MyMod]\nOriginal=1\n"}))
        .unwrap()
        .commit();
    fs::remove_file(fixture.config.join("ark-native-ownership.json")).unwrap();
    fixture
        .write_with_status(&json!({"game_ini_extra":"[MyMod]\nPending=2\n"}), true)
        .unwrap()
        .commit();
    fixture
        .write(&json!({"game_ini_extra":""}))
        .unwrap()
        .commit();
    assert!(
        !fs::read_to_string(fixture.live.join("Game.ini"))
            .unwrap()
            .contains("Original=")
    );
}

#[test]
fn ark_ini_direct_render_tracks_config_and_live_ownership_independently() {
    let fixture = Fixture::new("arksurvivalascended");
    fixture
        .write(&json!({"game_ini_extra":"[MyMod]\nOriginal=1\n"}))
        .unwrap()
        .commit();
    let settings = Map::new();
    render_module_templates(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/arksurvivalascended/templates"),
        &ModuleTemplateRenderInput {
            config_dir: &fixture.config,
            install_root: &fixture.install,
            saves_dir: &fixture.saves,
            instance_id: "ark-rerender",
            instance_name: "ARK rerender",
            module_id: fixture.module_id,
            bind_ip: "0.0.0.0",
            autostart: false,
            settings: &settings,
            ports: &[],
        },
    )
    .unwrap();
    assert!(
        !fs::read_to_string(fixture.config.join("Game.ini"))
            .unwrap()
            .contains("Original=")
    );
    assert!(
        fs::read_to_string(fixture.live.join("Game.ini"))
            .unwrap()
            .contains("Original=")
    );
    fixture.write(&json!({})).unwrap().commit();
    assert!(
        !fs::read_to_string(fixture.live.join("Game.ini"))
            .unwrap()
            .contains("Original=")
    );
}

#[test]
fn ark_ini_asa_clears_old_engine_session_player_limit_alias() {
    let fixture = Fixture::new("arksurvivalascended");
    for directory in [&fixture.config, &fixture.live] {
        fs::write(
            directory.join("GameUserSettings.ini"),
            "[/Script/Engine.GameSession]\nMaxPlayers=999\nCustom=keep\n",
        )
        .unwrap();
    }
    fixture.write(&json!({"max_players":42})).unwrap().commit();
    for directory in [&fixture.config, &fixture.live] {
        let actual = fs::read_to_string(directory.join("GameUserSettings.ini")).unwrap();
        assert!(!actual.contains("MaxPlayers=999"));
        assert!(actual.contains("MaxPlayers=42"));
        assert!(actual.contains("Custom=keep"));
        assert_eq!(actual.matches("MaxPlayers=").count(), 1);
    }
}

#[test]
fn ark_ini_actual_template_keys_update_live_without_claiming_preserved_unknowns() {
    let fixture = Fixture::new("arksurvivalascended");
    fixture.write(&json!({})).unwrap().commit();
    fs::write(
        fixture.live.join("GameUserSettings.ini"),
        "[ServerSettings]\nRCONEnabled=true\n[Template.Custom]\nGenerated=old\nUnknown=keep\n",
    )
    .unwrap();
    let settings = Map::new();
    let input = ModuleTemplateRenderInput {
        config_dir: &fixture.config,
        install_root: &fixture.install,
        saves_dir: &fixture.saves,
        instance_id: "ark-custom-template",
        instance_name: "ARK custom template",
        module_id: fixture.module_id,
        bind_ip: "0.0.0.0",
        autostart: false,
        settings: &settings,
        ports: &[],
    };
    let context = ModuleSupportMaterializationContext {
        storage_paths: &fixture.paths,
        module_id: fixture.module_id,
        install_root: &fixture.install,
        shared_install_root: &fixture.install,
        config_dir: &fixture.config,
        saves_dir: &fixture.saves,
        instance_id: input.instance_id,
        instance_running: false,
        settings: &settings,
    };
    for (generated, expected) in [
        (
            "[ServerSettings]\nRCONEnabled=false\n[Template.Custom]\nGenerated=new\n",
            Some("Generated=new"),
        ),
        ("[ServerSettings]\nRCONEnabled=false\n", None),
    ] {
        let mut pending = ManagedConfigMutation::new(fixture.module_id);
        ark_ini::write_rendered(
            &input,
            &fixture.config.join("GameUserSettings.ini"),
            generated.to_owned(),
            &mut pending,
        )
        .unwrap();
        ark_ini::materialize(&context, &mut pending).unwrap();
        pending.commit();
        let native = fs::read_to_string(fixture.live.join("GameUserSettings.ini")).unwrap();
        assert!(native.contains("RCONEnabled=false"));
        assert!(!native.contains("RCONEnabled=true"));
        assert!(native.contains("Unknown=keep"));
        match expected {
            Some(value) => assert!(native.contains(value)),
            None => assert!(!native.contains("Generated=")),
        }
    }
}
