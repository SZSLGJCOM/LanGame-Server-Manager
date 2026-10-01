use super::*;
use serde_json::json;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    module_id: &'static str,
    config: PathBuf,
    live: PathBuf,
    install: PathBuf,
    saves: PathBuf,
}

impl Fixture {
    fn new(module_id: &'static str) -> Self {
        let root = std::env::temp_dir().join(format!("ark-ini-{}", uuid::Uuid::new_v4()));
        let install = root.join("install");
        let live = install.join("ShooterGame/Saved/Config/WindowsServer");
        let config = root.join("instance/config");
        let saves = root.join("instance/saves");
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&config).unwrap();
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
            archives_root: root.join("instances").join(".trash"),
        };
        Self {
            root,
            paths,
            module_id,
            config,
            live,
            install,
            saves,
        }
    }

    fn write(&self, settings: &Value) -> Result<ManagedConfigMutation, StorageError> {
        self.write_with_status(settings, false)
    }

    fn write_with_status(
        &self,
        settings: &Value,
        running: bool,
    ) -> Result<ManagedConfigMutation, StorageError> {
        let settings = settings.as_object().unwrap();
        let input = ModuleTemplateRenderInput {
            config_dir: &self.config,
            install_root: &self.install,
            saves_dir: &self.saves,
            instance_id: "ark-ini-test",
            instance_name: "ARK test",
            module_id: self.module_id,
            bind_ip: "0.0.0.0",
            autostart: false,
            settings,
            ports: &[],
        };
        let context = ModuleSupportMaterializationContext {
            storage_paths: &self.paths,
            module_id: self.module_id,
            install_root: &self.install,
            shared_install_root: &self.install,
            config_dir: &self.config,
            saves_dir: &self.saves,
            instance_id: "ark-ini-test",
            instance_running: running,
            settings,
        };
        write_pending_instance_configuration(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../modules")
                .join(self.module_id)
                .join("templates"),
            &input,
            &context,
            &self.config.join("instance.json"),
            InstanceConfigInput {
                instance_id: input.instance_id,
                instance_name: input.instance_name,
                module_id: self.module_id,
                bind_ip: input.bind_ip,
                autostart: false,
                settings: settings.clone(),
                ports: &[],
            },
            None,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn ark_ini_save_preserves_both_sources_and_managed_deletions() {
    for module_id in ["arksurvivalascended", "arksurvivalevolved"] {
        let fixture = Fixture::new(module_id);
        fs::write(
            fixture.config.join("Game.ini"),
            concat!(
                "; config comment\n[/script/shootergame.shootergamemode]\n",
                "PerLevelStatsMultiplier_Player[99]=7\n",
                "NPCReplacements=(FromClassName=\"Old\",ToClassName=\"\")\n",
                "[Mod.Config]\nOnlyConfig=yes\nShared=config\nModRule=a\nModRule=b\n",
                "+ModArray=ConfigAdd\n",
            ),
        )
        .unwrap();
        fs::write(
            fixture.live.join("Game.ini"),
            concat!(
                "; live comment\n[/script/shootergame.shootergamemode]\n",
                "PerLevelStatsMultiplier_Player[99]=9\n",
                "NPCReplacements=(FromClassName=\"OldLive\",ToClassName=\"\")\n",
                "[Mod.Config]\nOnlyLive=yes\nShared=live\nModRule=c\nModRule=d\n",
                "-ModArray=LiveRemove\n",
            ),
        )
        .unwrap();
        fs::write(
            fixture.config.join("GameUserSettings.ini"),
            "[SessionSettings]\nSessionName=old\n[Mod.UI]\nTheme=blue\n",
        )
        .unwrap();
        fs::write(
            fixture.live.join("GameUserSettings.ini"),
            "; keep GUS\n[SessionSettings]\nSessionName=old live\n[Mod.UI]\nScale=2\n",
        )
        .unwrap();
        let settings = json!({"server_name":"Managed ARK", "per_level_stats_multiplier_player_integer":"[0]=2\n[12]=3", "npc_replacements":""});
        fixture.write(&settings).unwrap().commit();
        for directory in [&fixture.config, &fixture.live] {
            let game = fs::read_to_string(directory.join("Game.ini")).unwrap();
            assert!(game.contains("OnlyConfig=yes"), "{module_id}: {game}");
            assert!(game.contains("; config comment"));
            assert!(game.contains("PerLevelStatsMultiplier_Player[0]=2"));
            assert!(game.contains("PerLevelStatsMultiplier_Player[12]=3"));
            assert!(!game.contains("[99]"));
            assert!(!game.contains("NPCReplacements="));
            let gus = fs::read_to_string(directory.join("GameUserSettings.ini")).unwrap();
            assert!(gus.contains("SessionName=Managed ARK"));
            assert!(gus.contains("Theme=blue"));
        }
        let native = fs::read_to_string(fixture.live.join("Game.ini")).unwrap();
        assert!(native.contains("; live comment"));
        assert!(native.contains("OnlyLive=yes"));
        assert!(native.contains("Shared=live"));
        assert!(!native.contains("Shared=config"));
        assert!(native.contains("ModRule=c\nModRule=d"));
        assert!(native.contains("+ModArray=ConfigAdd"));
        assert!(native.contains("-ModArray=LiveRemove"));
        assert!(!native.contains("ModRule=a"));
        fixture.write(&settings).unwrap().commit();
        assert_eq!(
            fs::read_to_string(fixture.live.join("Game.ini")).unwrap(),
            native
        );
        fixture
            .write(&json!({"per_level_stats_multiplier_player_integer":""}))
            .unwrap()
            .commit();
        for directory in [&fixture.config, &fixture.live] {
            assert!(
                !fs::read_to_string(directory.join("Game.ini"))
                    .unwrap()
                    .contains("PerLevelStatsMultiplier_Player[")
            );
        }
    }
}

#[test]
fn ark_ini_raw_extra_overrides_managed_sequence_without_losing_duplicates() {
    for module_id in ["arksurvivalascended", "arksurvivalevolved"] {
        let fixture = Fixture::new(module_id);
        let settings = json!({
            "npc_replacements":"(FromClassName=\"Managed\",ToClassName=\"\")",
            "game_ini_extra":"NPCReplacements=(FromClassName=\"ModA\",ToClassName=\"\")\nNPCReplacements=(FromClassName=\"ModB\",ToClassName=\"\")\n[Mod.Private]\nCustom=1\nCustom=2\n",
            "server_name":"Managed",
            "game_user_settings_extra":"[SessionSettings]\nSessionName=Extra\n",
        });
        fixture.write(&settings).unwrap().commit();
        for directory in [&fixture.config, &fixture.live] {
            let game = fs::read_to_string(directory.join("Game.ini")).unwrap();
            assert_eq!(game.matches("NPCReplacements=").count(), 2, "{game}");
            assert!(!game.contains("FromClassName=\"Managed\""));
            assert!(game.contains("Custom=1\nCustom=2"));
            let gus = fs::read_to_string(directory.join("GameUserSettings.ini")).unwrap();
            assert_eq!(gus.matches("SessionName=").count(), 1);
            assert!(gus.contains("SessionName=Extra"));
        }
    }
}

#[test]
fn ark_ini_failed_pending_save_restores_config_and_live_bytes() {
    let fixture = Fixture::new("arksurvivalevolved");
    fixture
        .write(&json!({"server_name":"Before"}))
        .unwrap()
        .commit();
    let targets = [
        fixture.config.join("Game.ini"),
        fixture.config.join("GameUserSettings.ini"),
        fixture.live.join("Game.ini"),
        fixture.live.join("GameUserSettings.ini"),
        fixture.config.join("instance.json"),
    ];
    let before = targets
        .iter()
        .map(|path| fs::read(path).unwrap())
        .collect::<Vec<_>>();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.config.join("instance.json"));
    assert!(
        fixture
            .write(&json!({"server_name":"After", "game_ini_extra":"[MyMod]\nKeep=1\n"}))
            .is_err()
    );
    for (path, bytes) in targets.iter().zip(before) {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn ark_ini_rollback_preserves_concurrent_native_edits() {
    let fixture = Fixture::new("arksurvivalascended");
    fixture
        .write(&json!({"server_name":"Before"}))
        .unwrap()
        .commit();
    let config_path = fixture.config.join("GameUserSettings.ini");
    let original_config = fs::read(&config_path).unwrap();
    let pending = fixture.write(&json!({"server_name":"After"})).unwrap();
    let native_path = fixture.live.join("GameUserSettings.ini");
    let concurrent = "[Mod.Concurrent]\nKeep=external edit\n";
    fs::write(&native_path, concurrent).unwrap();
    assert!(pending.rollback().is_err());
    assert_eq!(fs::read_to_string(native_path).unwrap(), concurrent);
    assert_eq!(fs::read(config_path).unwrap(), original_config);
}

#[test]
fn ark_ini_direct_rerender_preserves_unknown_content_and_comments() {
    let fixture = Fixture::new("arksurvivalevolved");
    fs::write(
        fixture.config.join("Game.ini"),
        "\u{feff}; author comment\r\n[Private.Mod] ; section comment\r\nRule=one\r\nRule=two\r\n",
    )
    .unwrap();
    let settings = Map::new();
    render_module_templates(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/arksurvivalevolved/templates"),
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
    let native = fs::read_to_string(fixture.config.join("Game.ini")).unwrap();
    assert!(native.starts_with("\u{feff}; author comment\r\n"));
    assert!(native.contains("[Private.Mod] ; section comment\r\nRule=one\r\nRule=two\r\n"));
}

#[test]
fn ark_ini_invalid_native_file_rolls_back_rendered_configuration() {
    let fixture = Fixture::new("arksurvivalevolved");
    fixture
        .write(&json!({"server_name":"Before"}))
        .unwrap()
        .commit();
    let config_path = fixture.config.join("GameUserSettings.ini");
    let original_config = fs::read(&config_path).unwrap();
    let live_path = fixture.live.join("Game.ini");
    let malformed = "[Mod.Partial\nKeep=unfinished external edit\n";
    fs::write(&live_path, malformed).unwrap();
    assert!(fixture.write(&json!({"server_name":"After"})).is_err());
    assert_eq!(fs::read(config_path).unwrap(), original_config);
    assert_eq!(fs::read_to_string(live_path).unwrap(), malformed);
}

#[path = "ark_ini_encoding_tests.rs"]
mod encoding_tests;
#[path = "ark_ini_history_tests.rs"]
mod history_tests;

#[test]
fn ark_ini_prefixed_rules_canonicalize_native_assignment_spacing() {
    let settings = json!({"npc_replacements": concat!(
        "NPCReplacements = (FromClassName=\"A\",ToClassName=\"\")\n",
        "NPCReplacements=(FromClassName=\"B\",ToClassName=\"\")\n",
        "(FromClassName=\"C\",ToClassName=\"\")"
    )});
    let rendered = render_ark_prefixed_lines(
        settings.as_object().unwrap(),
        "npc_replacements",
        "NPCReplacements=",
    );
    assert_eq!(
        rendered,
        concat!(
            "NPCReplacements=(FromClassName=\"A\",ToClassName=\"\")\n",
            "NPCReplacements=(FromClassName=\"B\",ToClassName=\"\")\n",
            "NPCReplacements=(FromClassName=\"C\",ToClassName=\"\")\n"
        )
    );
}
