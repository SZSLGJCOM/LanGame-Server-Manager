use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulg-{:012x}",
            Uuid::new_v4().as_u128() & 0x0000_ffff_ffff_ffff
        ));
        fs::create_dir_all(&path).expect("create test root");
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn storage_paths(root: &Path) -> StoragePaths {
    StoragePaths {
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
    }
}

fn context<'a>(
    paths: &'a StoragePaths,
    module_id: &'a str,
    install_root: &'a Path,
    config_dir: &'a Path,
    settings: &'a Map<String, Value>,
) -> ModuleSupportMaterializationContext<'a> {
    ModuleSupportMaterializationContext {
        storage_paths: paths,
        module_id,
        install_root,
        shared_install_root: install_root,
        config_dir,
        saves_dir: install_root,
        instance_id: "unreal-test",
        instance_running: false,
        settings,
    }
}

#[test]
fn vrising_empty_rosters_remove_native_overrides_and_nonempty_rosters_are_preserved() {
    let root = TestRoot::new();
    let paths = storage_paths(&root.0);
    let config = root.0.join("instance/config");
    let rendered = config.join("Settings");
    let native = root.0.join("instance/Settings");
    fs::create_dir_all(&rendered).unwrap();
    for name in [VRISING_HOST_SETTINGS_FILE, VRISING_GAME_SETTINGS_FILE] {
        fs::write(rendered.join(name), "{}").unwrap();
    }
    let settings = Map::new();
    for roster in ["76561190000000001\n", "\n"] {
        for name in [VRISING_ADMIN_LIST_FILE, VRISING_BAN_LIST_FILE] {
            fs::write(rendered.join(name), roster).unwrap();
        }
        let mut files = ManagedConfigMutation::new("vrising");
        materialize_vrising_support_files(
            &context(&paths, "vrising", &root.0, &config, &settings),
            &mut files,
        )
        .unwrap();
        files.commit();
        for name in [VRISING_ADMIN_LIST_FILE, VRISING_BAN_LIST_FILE] {
            if roster.trim().is_empty() {
                assert!(!native.join(name).exists());
            } else {
                assert_eq!(fs::read_to_string(native.join(name)).unwrap(), roster);
            }
        }
    }
}

#[test]
fn sons_of_the_forest_prepares_native_configuration_and_launcher_identity_transactionally() {
    let root = TestRoot::new();
    let paths = storage_paths(&root.0);
    let install = root.0.join("install");
    let config = root.0.join("config");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join(SONS_OF_THE_FOREST_OWNER_WHITELIST_FILE), "").unwrap();
    fs::write(
        config.join("dedicatedserver.cfg"),
        r#"{"ServerName":"Instance server","SkipNetworkAccessibilityTest":true,"GameSettings":{"Structure.Damage":false}}"#,
    )
    .unwrap();
    let native_config = root.0.join("dedicatedserver.cfg");
    let original = r#"{"ServerName":"Native default","SkipNetworkAccessibilityTest":false,"FutureSetting":17,"GameSettings":{"Future.Toggle":true}}"#;
    fs::write(&native_config, original).unwrap();
    let settings = Map::new();
    let mut files = ManagedConfigMutation::new("sonsoftheforest");
    materialize_sonsoftheforest_support_files(
        &context(&paths, "sonsoftheforest", &install, &config, &settings),
        &mut files,
    )
    .expect("prepare first native launch");
    assert_eq!(
        fs::read(install.join("steam_appid.txt")).unwrap(),
        b"1326470"
    );
    let native: Value = serde_json::from_slice(&fs::read(&native_config).unwrap()).unwrap();
    assert_eq!(native["ServerName"], "Instance server");
    assert_eq!(native["SkipNetworkAccessibilityTest"], true);
    assert_eq!(native["FutureSetting"], 17);
    assert_eq!(native["GameSettings"]["Structure.Damage"], false);
    assert_eq!(native["GameSettings"]["Future.Toggle"], true);
    files.rollback().expect("roll back preparation");
    assert!(!install.join("steam_appid.txt").exists());
    assert_eq!(fs::read_to_string(native_config).unwrap(), original);
}

#[test]
fn satisfactory_instances_materialize_independent_native_configurations() {
    let root = TestRoot::new();
    let paths = storage_paths(&root.0);
    let install = root.0.join("install");
    fs::create_dir_all(&install).expect("shared installation");
    let settings = Map::new();
    for (instance_id, max_players) in [("alpha", 8), ("bravo", 16)] {
        let config = paths.instances_root.join(instance_id).join("config");
        fs::create_dir_all(&config).expect("instance config");
        fs::write(
            config.join("Engine.ini"),
            "[SystemSettings]\nnet.MaxInternetClientRate=120000\n",
        )
        .unwrap();
        fs::write(
            config.join("Game.ini"),
            format!("[/Script/Engine.GameSession]\nMaxPlayers={max_players}\n"),
        )
        .unwrap();
        materialize_satisfactory_support_files(
            &ModuleSupportMaterializationContext {
                instance_id,
                ..context(&paths, "satisfactory", &install, &config, &settings)
            },
            &mut ManagedConfigMutation::new("satisfactory-isolation"),
        )
        .expect("materialize instance");
    }
    for (instance_id, max_players) in [("alpha", 8), ("bravo", 16)] {
        let native = paths
            .instances_root
            .join(instance_id)
            .join("data/Saved/Config/WindowsServer/Game.ini");
        assert!(
            fs::read_to_string(native)
                .unwrap()
                .contains(&format!("MaxPlayers={max_players}"))
        );
    }
    assert!(
        !install
            .join("FactoryGame/Saved/Config/WindowsServer")
            .exists()
    );
}

#[test]
fn abiotic_merge_keeps_unknown_keys_and_replaces_repeated_moderators() {
    let root = TestRoot::new();
    let paths = storage_paths(&root.0);
    let config = root.0.join("config");
    let install = root.0.join("install");
    let settings = Map::new();
    let sandbox = install
        .join("AbioticFactor/Saved/Config/WindowsServer/LanGame/unreal-test-SandboxSettings.ini");
    let admin = install.join("AbioticFactor/Saved/SaveGames/Server/LanGame/unreal-test-Admin.ini");
    fs::create_dir_all(&config).expect("config");
    fs::create_dir_all(sandbox.parent().expect("sandbox parent")).expect("sandbox parent");
    fs::create_dir_all(admin.parent().expect("admin parent")).expect("admin parent");
    fs::write(
        config.join("SandboxSettings.ini"),
        "[SandboxSettings]\nGameDifficulty=3\n",
    )
    .expect("sandbox source");
    fs::write(
        config.join("Admin.ini"),
        "[Moderators]\nModerator=111\nModerator=222\n",
    )
    .expect("admin source");
    fs::write(
        &sandbox,
        "[SandboxSettings]\nGameDifficulty=1\nFutureOption=keep\n",
    )
    .expect("sandbox destination");
    fs::write(
        &admin,
        "[Moderators]\nModerator=old\nFutureRosterOption=keep\nModerator=duplicate\n",
    )
    .expect("admin destination");

    materialize_unreal_large_support_files(
        &context(&paths, "abioticfactor", &install, &config, &settings),
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("materialize abiotic");

    let sandbox = fs::read_to_string(sandbox).expect("read sandbox");
    assert!(sandbox.contains("GameDifficulty=3"));
    assert!(sandbox.contains("FutureOption=keep"));
    let admin = fs::read_to_string(admin).expect("read admin");
    assert_eq!(admin.matches("Moderator=").count(), 2);
    assert!(admin.contains("Moderator=111"));
    assert!(admin.contains("Moderator=222"));
    assert!(admin.contains("FutureRosterOption=keep"));
}

#[test]
fn abiotic_invalid_second_file_leaves_every_native_file_unchanged() {
    let root = TestRoot::new();
    let paths = storage_paths(&root.0);
    let config = root.0.join("config");
    let install = root.0.join("install");
    let settings = Map::new();
    let sandbox = install
        .join("AbioticFactor/Saved/Config/WindowsServer/LanGame/unreal-test-SandboxSettings.ini");
    let admin = install.join("AbioticFactor/Saved/SaveGames/Server/LanGame/unreal-test-Admin.ini");
    fs::create_dir_all(&config).expect("config");
    fs::create_dir_all(sandbox.parent().expect("sandbox parent")).expect("sandbox parent");
    fs::create_dir_all(admin.parent().expect("admin parent")).expect("admin parent");
    fs::write(
        config.join("SandboxSettings.ini"),
        "[SandboxSettings]\nGameDifficulty=3\n",
    )
    .expect("sandbox source");
    fs::write(config.join("Admin.ini"), "[broken\nModerator=111\n").expect("invalid admin source");
    let sandbox_original = b"[SandboxSettings]\nGameDifficulty=1\n";
    let admin_original = b"[Moderators]\nModerator=old\n";
    fs::write(&sandbox, sandbox_original).expect("sandbox destination");
    fs::write(&admin, admin_original).expect("admin destination");

    materialize_unreal_large_support_files(
        &context(&paths, "abioticfactor", &install, &config, &settings),
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect_err("invalid batch member must fail");

    assert_eq!(fs::read(sandbox).expect("read sandbox"), sandbox_original);
    assert_eq!(fs::read(admin).expect("read admin"), admin_original);
}

#[test]
fn conan_merge_preserves_unknown_keys_across_all_three_native_inis() {
    let root = TestRoot::new();
    let paths = storage_paths(&root.0);
    let config = root.0.join("config");
    let install = root.0.join("install");
    let target = install.join("ConanSandbox/Saved/Config/WindowsServer");
    let settings = Map::new();
    fs::create_dir_all(&config).expect("config");
    fs::create_dir_all(&target).expect("target");
    for (name, rendered, existing) in [
        (
            "Engine.ini",
            "[OnlineSubsystem]\nServerPassword=\n",
            "[OnlineSubsystem]\nServerPassword=old\nFutureEngine=keep\n",
        ),
        (
            "Game.ini",
            "[/Script/Engine.GameSession]\nMaxPlayers=70\n",
            "[/Script/Engine.GameSession]\nMaxPlayers=10\nFutureGame=keep\n",
        ),
        (
            "ServerSettings.ini",
            "[ServerSettings]\nServerName=海风流放地\n",
            "[ServerSettings]\nServerName=old\nFutureServer=keep\n",
        ),
    ] {
        fs::write(config.join(name), rendered).expect("rendered INI");
        fs::write(target.join(name), existing).expect("native INI");
    }

    materialize_unreal_large_support_files(
        &context(&paths, "conanexiles", &install, &config, &settings),
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("materialize Conan");

    assert!(
        fs::read_to_string(target.join("Engine.ini"))
            .expect("engine")
            .contains("FutureEngine=keep")
    );
    assert!(
        fs::read_to_string(target.join("Game.ini"))
            .expect("game")
            .contains("FutureGame=keep")
    );
    let server = fs::read_to_string(target.join("ServerSettings.ini")).expect("server");
    assert!(server.contains("ServerName=海风流放地"));
    assert!(server.contains("FutureServer=keep"));
}

#[test]
fn humanitz_mixed_batch_preserves_ini_unknowns_and_text_bytes() {
    let root = TestRoot::new();
    let paths = storage_paths(&root.0);
    let config = root.0.join("config");
    let install = root.0.join("install");
    let settings = Map::new();
    let target = install.join("HumanitZServer");
    fs::create_dir_all(&config).expect("config");
    fs::create_dir_all(&target).expect("target");
    fs::write(
        config.join("GameServerSettings.ini"),
        "[Host Settings]\nServerName=海风群岛 ⚓\n[World Settings]\nZombieAmount=4\n",
    )
    .expect("settings source");
    fs::write(
        target.join("GameServerSettings.ini"),
        "[Host Settings]\nServerName=old\nFutureSetting=keep\n[World Settings]\nZombieAmount=1\n",
    )
    .expect("settings destination");
    for (name, bytes) in [
        ("WelcomeMessage.txt", b"Welcome, survivor!\n".as_slice()),
        ("AdminList.txt", b"111\r\n222\n".as_slice()),
        ("F_MVPAccess.txt", b"333\n".as_slice()),
        ("F_ReservedSlots.txt", b"444\n".as_slice()),
        ("F_BannedPlayers.txt", b"555\n".as_slice()),
    ] {
        fs::write(config.join(name), bytes).expect("text source");
    }

    fs::write(target.join("F_MVPAccess.txt"), b"historical roster\r\n").unwrap();
    materialize_unreal_large_support_files(
        &context(&paths, "humanitz", &install, &config, &settings),
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("materialize HumanitZ");

    let settings = fs::read_to_string(target.join("GameServerSettings.ini")).expect("settings");
    assert!(settings.contains("ServerName=海风群岛 ⚓"));
    assert!(settings.contains("FutureSetting=keep"));
    assert_eq!(
        fs::read(target.join("AdminList.txt")).expect("admin list"),
        b"111\r\n222\n"
    );
    assert_eq!(
        fs::read(target.join("F_MVPAccess.txt")).unwrap(),
        b"historical roster\r\n"
    );
}

#[test]
fn soulmask_merge_preserves_nested_unknown_profile_fields() {
    let root = TestRoot::new();
    let paths = storage_paths(&root.0);
    let config = root.0.join("config");
    let install = root.0.join("install");
    let settings = Map::new();
    let target = install.join("WS/Saved/GameplaySettings/GameXishu.json");
    fs::create_dir_all(&config).expect("config");
    fs::create_dir_all(target.parent().expect("target parent")).expect("target parent");
    fs::write(
        config.join("GameXishu.json"),
        r#"{"0":{"setting":2},"1":{},"2":{}}"#,
    )
    .expect("source");
    fs::write(
        &target,
        r#"{"0":{"setting":1,"future":{"keep":true}},"futureProfile":{"enabled":true}}"#,
    )
    .expect("destination");

    materialize_unreal_large_support_files(
        &context(&paths, "soulmask", &install, &config, &settings),
        &mut ManagedConfigMutation::new("fixture"),
    )
    .expect("materialize Soulmask");

    let merged: Value =
        serde_json::from_slice(&fs::read(target).expect("read target")).expect("parse merged JSON");
    assert_eq!(merged["0"]["setting"], 2);
    assert_eq!(merged["0"]["future"]["keep"], true);
    assert_eq!(merged["futureProfile"]["enabled"], true);
}
