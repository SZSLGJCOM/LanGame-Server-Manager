use super::*;
use serde_json::json;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    install: PathBuf,
    config: PathBuf,
    live: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("satisfactory-ini-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/lgs.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let install = root.join("install");
        let config = paths.instances_root.join("selected/config");
        let live = paths
            .instances_root
            .join("selected/data/Saved/Config/WindowsServer");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&live).unwrap();
        fs::write(
            config.join(SATISFACTORY_ENGINE_INI_FILE),
            "[Engine]\nManaged=new\n",
        )
        .unwrap();
        fs::write(
            config.join(SATISFACTORY_GAME_INI_FILE),
            "[Game]\nManaged=new\n",
        )
        .unwrap();
        for name in [SATISFACTORY_ENGINE_INI_FILE, SATISFACTORY_GAME_INI_FILE] {
            fs::write(live.join(name), "[Unmanaged]\nKeep=original\n").unwrap();
        }
        Self {
            root,
            paths,
            install,
            config,
            live,
        }
    }

    fn user_settings(&self) -> PathBuf {
        self.live.join(USER_SETTINGS_FILE)
    }

    fn apply(&self, settings: Value) -> Result<(), StorageError> {
        let context = ModuleSupportMaterializationContext {
            storage_paths: &self.paths,
            module_id: "satisfactory",
            install_root: &self.install,
            shared_install_root: &self.install,
            config_dir: &self.config,
            saves_dir: &self.root,
            instance_id: "selected",
            instance_running: false,
            settings: settings.as_object().unwrap(),
        };
        // Exercise module dispatch and the same commit/rollback boundary used by
        // the other materialization tests, rather than only the map parser.
        super::super::materialize_module_support_files(&context)
    }

    fn assert_support_files_unchanged(&self) {
        for name in [SATISFACTORY_ENGINE_INI_FILE, SATISFACTORY_GAME_INI_FILE] {
            assert_eq!(
                fs::read(self.live.join(name)).unwrap(),
                b"[Unmanaged]\nKeep=original\n"
            );
        }
    }

    fn apply_normalized(
        &self,
        descriptor: &app_modules::ModuleDescriptor,
        persisted: Map<String, Value>,
    ) -> Map<String, Value> {
        let settings = crate::instances::normalize_complete_instance_settings(
            Some(descriptor),
            persisted,
            "selected",
            "Native settings",
            "127.0.0.1",
        )
        .unwrap();
        let input = ModuleTemplateRenderInput {
            config_dir: &self.config,
            install_root: &self.install,
            saves_dir: &self.root,
            instance_id: "selected",
            instance_name: "Native settings",
            module_id: "satisfactory",
            bind_ip: "127.0.0.1",
            autostart: false,
            settings: &settings,
            ports: &descriptor.default_ports,
        };
        let context = ModuleSupportMaterializationContext {
            storage_paths: &self.paths,
            module_id: "satisfactory",
            install_root: &self.install,
            shared_install_root: &self.install,
            config_dir: &self.config,
            saves_dir: &self.root,
            instance_id: "selected",
            instance_running: false,
            settings: &settings,
        };
        let config_path = self.config.join("instance.json");
        write_pending_instance_configuration(
            &descriptor.root.join("templates"),
            &input,
            &context,
            &config_path,
            InstanceConfigInput {
                instance_id: input.instance_id,
                instance_name: input.instance_name,
                module_id: input.module_id,
                bind_ip: input.bind_ip,
                autostart: false,
                settings: settings.clone(),
                ports: input.ports,
            },
            None,
        )
        .unwrap()
        .commit();
        let saved: Value = serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
        saved["settings"].as_object().unwrap().clone()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn satisfactory_options_merge_preserves_native_and_other_instance_settings() {
    let fixture = Fixture::new();
    let original = concat!(
        "\u{feff}; native settings\r\n[Preceding]\r\nKeep=before\r\n",
        "[/Script/FactoryGame.FGGameUserSettings]\r\n",
        "mIntValues=((\"FG.DSAutoPause\", 1),(\"FG.NetworkQuality\", 1),",
        "(\"FG.SendGameplayData\", 1),(\"FG.FutureOption\", -17),",
        "(\"FicsitRemoteMonitoring.Server.uWS.Port\", 8081),",
        "(\"FicsitRemoteMonitoring.Server.uWS.Autostart\", 1))\r\n",
        "mStringValues=((\"Unrelated\", \"untouched\"))\r\n",
        "; keep this comment\r\n[OtherSection]\r\nSetting=value\r\n"
    );
    fs::write(fixture.user_settings(), original).unwrap();
    let other = fixture
        .paths
        .instances_root
        .join("other/data/Saved/Config/WindowsServer/GameUserSettings.ini");
    let shared = fixture
        .install
        .join("FactoryGame/Saved/Config/WindowsServer/GameUserSettings.ini");
    for path in [&other, &shared] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, original).unwrap();
    }

    fixture
        .apply(json!({
            "auto_pause_when_empty": false, "network_quality": 2, "send_gameplay_data": false
        }))
        .unwrap();
    let output = fs::read_to_string(fixture.user_settings()).unwrap();
    assert!(output.starts_with('\u{feff}'));
    for expected in [
        "(\"FG.DSAutoPause\", 0)",
        "(\"FG.NetworkQuality\", 2)",
        "(\"FG.SendGameplayData\", 0)",
        "(\"FG.FutureOption\", -17)",
        "(\"FicsitRemoteMonitoring.Server.uWS.Port\", 8081)",
        "(\"FicsitRemoteMonitoring.Server.uWS.Autostart\", 1)",
        "mStringValues=((\"Unrelated\", \"untouched\"))",
        "; keep this comment",
        "[OtherSection]\nSetting=value",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    assert_eq!(output.matches("mIntValues=").count(), 1);
    for path in [other, shared] {
        assert_eq!(fs::read(path).unwrap(), original.as_bytes());
    }
    fixture.apply(json!({"network_quality": 3})).unwrap();
    let output = fs::read_to_string(fixture.user_settings()).unwrap();
    assert!(output.contains("(\"FG.NetworkQuality\", 3)"));
    assert!(output.contains("(\"FG.DSAutoPause\", 0)"));
    assert!(output.contains("(\"FG.SendGameplayData\", 0)"));
}

#[test]
fn satisfactory_missing_option_fields_do_not_read_or_create_native_settings() {
    let fixture = Fixture::new();
    fixture.apply(json!({})).unwrap();
    assert!(!fixture.user_settings().exists());
    let existing = b"\xff\xfeinvalid unmanaged bytes";
    fs::write(fixture.user_settings(), existing).unwrap();
    fixture
        .apply(json!({"server_name": "another name"}))
        .unwrap();
    assert_eq!(fs::read(fixture.user_settings()).unwrap(), existing);
}

#[test]
fn satisfactory_options_create_only_explicit_native_values() {
    let fixture = Fixture::new();
    fixture.apply(json!({"network_quality": 0})).unwrap();
    assert_eq!(
        fs::read_to_string(fixture.user_settings()).unwrap(),
        "[/Script/FactoryGame.FGGameUserSettings]\nmIntValues=((\"FG.NetworkQuality\", 0))\n"
    );
    fixture
        .apply(json!({"auto_pause_when_empty": true, "send_gameplay_data": true}))
        .unwrap();
    let output = fs::read_to_string(fixture.user_settings()).unwrap();
    assert!(output.contains("(\"FG.NetworkQuality\", 0)"));
    assert!(output.contains("(\"FG.DSAutoPause\", 1)"));
    assert!(output.contains("(\"FG.SendGameplayData\", 1)"));
}

#[test]
fn satisfactory_malformed_or_ambiguous_native_options_leave_all_files_unchanged() {
    let fixture = Fixture::new();
    for invalid in [
        "mIntValues=((\"FG.Unknown\", 1)",
        "mIntValues=((\"FG.Unknown\", 1),)",
        "mIntValues=((\"FG.Unknown\", 1)) trailing",
        "mIntValues=((FG.Unknown, 1))",
        "mIntValues=((\"\", 1))",
        "mIntValues=((\"FG.Unknown\", 2147483648))",
        "mIntValues=((\"FG.Unknown\", 1.5))",
        "mIntValues=((\"FG.Unknown\", \"1\"))",
        "mIntValues=((\"FG.Unknown\", 1),(\"fg.unknown\", 2))",
        "mIntValues=()\nmIntValues=()",
        "mIntValues=()\nMINTVALUES=()",
        "+mIntValues=((\"FG.Unknown\", 1))",
        "+ mIntValues=((\"FG.Unknown\", 1))",
        "!mIntValues=ClearArray",
        "mIntValues[0]=1",
        "mIntValues=()\n[/script/factorygame.fggameusersettings]\nmIntValues=()",
        "mIntValues=()\n[Broken",
        "\u{feff}mIntValues=((\"FG.FutureOption\", 17))",
        "mIntValues=()\n[Other]\n\u{feff}[/Script/FactoryGame.FGGameUserSettings]",
        "mIntValues=((\"Bad\\qEscape\", 1))",
        "mIntValues=((\"Unterminated\\\", 1))",
    ] {
        let original = format!("[{USER_SETTINGS_SECTION}]\n{invalid}\n");
        fs::write(fixture.user_settings(), &original).unwrap();
        assert!(
            fixture
                .apply(json!({"auto_pause_when_empty": false}))
                .is_err(),
            "{invalid}"
        );
        assert_eq!(
            fs::read(fixture.user_settings()).unwrap(),
            original.as_bytes()
        );
        fixture.assert_support_files_unchanged();
    }
}

#[test]
fn satisfactory_rejects_oversize_non_utf8_and_invalid_explicit_values() {
    let fixture = Fixture::new();
    for bytes in [vec![b' '; MAX_USER_SETTINGS_BYTES + 1], vec![0xff, 0xfe]] {
        fs::write(fixture.user_settings(), &bytes).unwrap();
        assert!(fixture.apply(json!({"network_quality": 2})).is_err());
        assert_eq!(fs::read(fixture.user_settings()).unwrap(), bytes);
        fixture.assert_support_files_unchanged();
    }
    fs::write(fixture.user_settings(), "[Other]\nKey=keep\n").unwrap();
    for invalid in [
        json!({"network_quality": -1}),
        json!({"network_quality": 4}),
        json!({"network_quality": 1.5}),
        json!({"network_quality": "2"}),
        json!({"auto_pause_when_empty": 0}),
        json!({"send_gameplay_data": null}),
    ] {
        assert!(fixture.apply(invalid).is_err());
        assert_eq!(
            fs::read(fixture.user_settings()).unwrap(),
            b"[Other]\nKey=keep\n"
        );
        fixture.assert_support_files_unchanged();
    }
}

#[test]
fn satisfactory_complete_map_parser_preserves_escaped_names_and_bounds_entries() {
    let existing = format!(
        "[{USER_SETTINGS_SECTION}]\nmIntValues=((\"Mod.\\\"Name\\\\Path\", -2147483648))  \n"
    );
    let result = merge_user_settings(&existing, &[("FG.NetworkQuality", 2)]).unwrap();
    assert!(result.contains("(\"Mod.\\\"Name\\\\Path\", -2147483648)"));
    let unicode = format!("[{USER_SETTINGS_SECTION}]\nmIntValues=((\"模组.設定\", 17))\n");
    let result = merge_user_settings(&unicode, &[("FG.NetworkQuality", 2)]).unwrap();
    assert!(result.contains("(\"模组.設定\", 17)"));
    let entries = (0..MAX_INT_OPTIONS)
        .map(|index| format!("(\"Mod.{index}\", 0)"))
        .collect::<Vec<_>>()
        .join(",");
    let full = format!("[{USER_SETTINGS_SECTION}]\nmIntValues=({entries})\n");
    assert!(merge_user_settings(&full, &[("FG.NetworkQuality", 1)]).is_err());
    assert!(parse_int_map(&format!("({entries},(\"Extra\", 0))")).is_err());
}

#[test]
fn satisfactory_native_option_plan_rejects_concurrent_changes_and_rolls_back() {
    let fixture = Fixture::new();
    let original = b"[Other]\nKey=original\n";
    fs::write(fixture.user_settings(), original).unwrap();
    let settings = json!({"network_quality": 2});
    let plan = plan_user_settings(&fixture.user_settings(), settings.as_object().unwrap())
        .unwrap()
        .unwrap();
    let changed = b"[Other]\nKey=changed by native server\n";
    fs::write(fixture.user_settings(), changed).unwrap();
    let mut files = ManagedConfigMutation::new("satisfactory");
    assert!(files.apply(vec![plan]).is_err());
    files.rollback().unwrap();
    assert_eq!(fs::read(fixture.user_settings()).unwrap(), changed);

    let plan = plan_user_settings(&fixture.user_settings(), settings.as_object().unwrap())
        .unwrap()
        .unwrap();
    let mut files = ManagedConfigMutation::new("satisfactory");
    files.apply(vec![plan]).unwrap();
    files.rollback().unwrap();
    assert_eq!(fs::read(fixture.user_settings()).unwrap(), changed);
}

#[test]
fn satisfactory_normalization_round_trips_preserve_native_owned_options() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "satisfactory")
        .unwrap();
    let defaults = collect_schema_defaults_from_schema_json(
        descriptor.schema_json.as_deref(),
        SchemaDefaultContext {
            instance_id: Some("selected"),
            instance_name: Some("Native settings"),
        },
    )
    .unwrap();
    // Schema defaults remain documented, but do not establish native ownership.
    assert_eq!(defaults["auto_pause_when_empty"], json!(true));
    assert_eq!(defaults["network_quality"], json!(1));
    assert_eq!(defaults["send_gameplay_data"], json!(true));
    let fixture = Fixture::new();
    let original = concat!(
        "[/Script/FactoryGame.FGGameUserSettings]\n",
        "mIntValues=((\"FG.DSAutoPause\", 0),(\"FG.NetworkQuality\", 3),",
        "(\"FG.SendGameplayData\", 0),(\"FG.FutureOption\", 17),",
        "(\"FicsitRemoteMonitoring.Server.uWS.Port\", 8081))\n"
    );
    fs::write(fixture.user_settings(), original).unwrap();
    let mut persisted = fixture.apply_normalized(&descriptor, Map::new());
    for key in [
        "auto_pause_when_empty",
        "network_quality",
        "send_gameplay_data",
    ] {
        assert!(!persisted.contains_key(key), "normalization injected {key}");
    }
    assert_eq!(
        fs::read(fixture.user_settings()).unwrap(),
        original.as_bytes()
    );
    persisted.insert("max_players".to_owned(), json!(8));
    persisted = fixture.apply_normalized(&descriptor, persisted);
    assert_eq!(
        fs::read(fixture.user_settings()).unwrap(),
        original.as_bytes()
    );
    persisted.insert("network_quality".to_owned(), json!(2));
    persisted = fixture.apply_normalized(&descriptor, persisted);
    assert!(!persisted.contains_key("auto_pause_when_empty"));
    assert!(!persisted.contains_key("send_gameplay_data"));
    assert_eq!(persisted["network_quality"], json!(2));
    let changed = fs::read(fixture.user_settings()).unwrap();
    let expected = original.replace("(\"FG.NetworkQuality\", 3)", "(\"FG.NetworkQuality\", 2)");
    assert_eq!(changed, expected.as_bytes());
    persisted = fixture.apply_normalized(&descriptor, persisted);
    assert_eq!(fs::read(fixture.user_settings()).unwrap(), changed);
    persisted.insert("auto_pause_when_empty".to_owned(), json!(true));
    persisted.insert("send_gameplay_data".to_owned(), json!(true));
    let persisted = fixture.apply_normalized(&descriptor, persisted);
    assert_eq!(persisted["auto_pause_when_empty"], json!(true));
    assert_eq!(persisted["send_gameplay_data"], json!(true));
    let changed = fs::read_to_string(fixture.user_settings()).unwrap();
    assert!(changed.contains("(\"FG.DSAutoPause\", 1)"));
    assert!(changed.contains("(\"FG.SendGameplayData\", 1)"));
}

#[test]
fn satisfactory_merge_growth_limit_rejects_without_changing_any_file() {
    let fixture = Fixture::new();
    let mut original = format!("[{USER_SETTINGS_SECTION}]\nmIntValues=()\n;");
    original.push_str(&"x".repeat(MAX_USER_SETTINGS_BYTES - original.len()));
    fs::write(fixture.user_settings(), &original).unwrap();
    assert!(fixture.apply(json!({"network_quality": 2})).is_err());
    assert_eq!(
        fs::read(fixture.user_settings()).unwrap(),
        original.as_bytes()
    );
    fixture.assert_support_files_unchanged();
}

#[test]
fn satisfactory_weather_presets_write_and_preserve_only_explicit_overrides() {
    let fixture = Fixture::new();
    fs::write(
        fixture.user_settings(),
        "[/Script/FactoryGame.FGGameUserSettings]\nmIntValues=((\"FG.WeatherPreset\", 4),(\"FG.FutureOption\", -17))\n",
    )
    .unwrap();
    fixture.apply(json!({"network_quality": 2})).unwrap();
    assert!(
        fs::read_to_string(fixture.user_settings())
            .unwrap()
            .contains("(\"FG.WeatherPreset\", 4)")
    );
    for value in 0..=6 {
        fixture.apply(json!({"weather_preset": value})).unwrap();
        let saved = fs::read_to_string(fixture.user_settings()).unwrap();
        assert!(saved.contains(&format!("(\"FG.WeatherPreset\", {value})")));
        assert_eq!(saved.matches("FG.WeatherPreset").count(), 1);
        assert!(saved.contains("(\"FG.FutureOption\", -17)"));
        assert!(saved.contains("(\"FG.NetworkQuality\", 2)"));
    }
    let saved = fs::read(fixture.user_settings()).unwrap();
    for invalid in [json!(-1), json!(7), json!(1.5), json!("2"), Value::Null] {
        assert!(fixture.apply(json!({"weather_preset": invalid})).is_err());
        assert_eq!(fs::read(fixture.user_settings()).unwrap(), saved);
    }
}
