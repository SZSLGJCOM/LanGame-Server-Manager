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

#[test]
fn asa_native_user_settings_version_and_rcon_credentials_survive_utf16_materialization() {
    let fixture = Fixture::new("arksurvivalascended");
    let settings = json!({
        "server_name":"ASA | Native", "admin_password":"fixture-admin-password",
        "server_password":"fixture-server-password", "rcon_enabled":true, "max_players":4
    });
    let settings = settings.as_object().unwrap();
    let ports = [
        app_core::PortBinding {
            name: "game".into(),
            protocol: "udp".into(),
            port: 7777,
        },
        app_core::PortBinding {
            name: "query".into(),
            protocol: "udp".into(),
            port: 27015,
        },
        app_core::PortBinding {
            name: "rcon".into(),
            protocol: "tcp".into(),
            port: 27021,
        },
    ];
    let input = ModuleTemplateRenderInput {
        config_dir: &fixture.config,
        install_root: &fixture.install,
        saves_dir: &fixture.saves,
        instance_id: "asa-version-probe",
        instance_name: "ASA version probe",
        module_id: fixture.module_id,
        bind_ip: "0.0.0.0",
        autostart: false,
        settings,
        ports: &ports,
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
        settings,
    };
    let write = || {
        write_pending_instance_configuration(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../modules/arksurvivalascended/templates"),
            &input,
            &context,
            &fixture.config.join("instance.json"),
            InstanceConfigInput {
                instance_id: input.instance_id,
                instance_name: input.instance_name,
                module_id: fixture.module_id,
                bind_ip: input.bind_ip,
                autostart: false,
                settings: settings.clone(),
                ports: &ports,
            },
            None,
        )
        .unwrap()
        .commit();
    };
    let assert_native_settings = |path: &Path| {
        let bytes = fs::read(path).unwrap();
        let text = if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
            let (pairs, remainder) = body.as_chunks::<2>();
            assert!(remainder.is_empty());
            String::from_utf16(
                &pairs
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        } else {
            String::from_utf8(bytes).unwrap()
        };
        for (section, key, value) in [
            (
                "/script/shootergame.shootergameusersettings",
                "Version",
                "5",
            ),
            (
                "serversettings",
                "ServerAdminPassword",
                "fixture-admin-password",
            ),
            (
                "serversettings",
                "ServerPassword",
                "fixture-server-password",
            ),
            ("serversettings", "RCONPort", "27021"),
            ("sessionsettings", "SessionName", "ASA | Native"),
        ] {
            let mut current_section = "";
            let mut entries = Vec::new();
            for line in text.lines() {
                if let Some(header) = line
                    .strip_prefix('[')
                    .and_then(|line| line.strip_suffix(']'))
                {
                    current_section = header;
                } else if current_section.eq_ignore_ascii_case(section)
                    && line
                        .split_once('=')
                        .is_some_and(|(actual, _)| actual.eq_ignore_ascii_case(key))
                {
                    entries.push(line.to_owned());
                }
            }
            assert_eq!(
                entries,
                [format!("{key}={value}")],
                "{}: {section}.{key}",
                path.display()
            );
        }
        text
    };
    write();
    let config_path = fixture.config.join("GameUserSettings.ini");
    let live_path = fixture.live.join("GameUserSettings.ini");
    assert_native_settings(&config_path);
    let native = assert_native_settings(&live_path);
    // Model only the game's documented UTF-16 writeback boundary. Native
    // deletion on an invalid version still requires separate live acceptance.
    let native = format!("{native}\n[Native.UserSettings]\nUnmanaged=keep\n").replace('\n', "\r\n");
    let utf16 = [0xfeff_u16]
        .into_iter()
        .chain(native.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    fs::write(&live_path, utf16).unwrap();
    write();
    assert_native_settings(&config_path);
    let native = assert_native_settings(&live_path);
    assert!(native.contains("Unmanaged=keep"));
    assert_eq!(&fs::read(&live_path).unwrap()[..2], &[0xff, 0xfe]);
}
