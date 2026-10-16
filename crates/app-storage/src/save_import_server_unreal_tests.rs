use super::*;
use crate::backups::save_import::tests::Fixture;

const SQLITE: &[u8] = b"SQLite format 3\0opaque native game data";

fn fixture(module: &str, settings: Value) -> Fixture {
    let mut fixture = Fixture::new();
    fs::remove_dir_all(&fixture.source).unwrap();
    fs::create_dir(&fixture.source).unwrap();
    fixture.context.record.summary.module_id = module.into();
    fixture.context.settings = serde_json::from_value(settings).unwrap();
    fixture
}

fn put(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn checked(fixture: &Fixture) -> ServerPlan {
    let tree = io::inventory(&fixture.source, None).unwrap();
    plan(&fixture.context, &fixture.source, &tree)
        .unwrap()
        .unwrap()
}

fn rejected(fixture: &Fixture) -> bool {
    let tree = io::inventory(&fixture.source, None).unwrap();
    plan(&fixture.context, &fixture.source, &tree).is_err()
}

#[test]
fn ase_maps_original_server_directory_and_clears_stale_native_characters_only() {
    let fixture = fixture("arksurvivalevolved", json!({"map_name":"TheIsland"}));
    for name in [
        "TheIsland.ark",
        "76561198000000000.arkprofile",
        "72.arktribe",
        "TheIsland_AntiCorruptionBackup.bak",
    ] {
        put(
            &fixture.source,
            &format!("original-server/{name}"),
            b"native bytes",
        );
    }
    put(
        &fixture.source,
        "Config/WindowsServer/GameUserSettings.ini",
        b"source password",
    );
    for name in [
        "example/old.arkprofile",
        "example/previous.ark",
        "example/OtherMap_01.10.2026_07.53.04.arkrbf",
        "example/OtherMap_AntiCorruptionBackup.bak",
        "example/cluster/upload",
        "Config/WindowsServer/GameUserSettings.ini",
        "example/unrelated.txt",
    ] {
        put(&fixture.context.target, name, b"existing target data");
    }
    let result = checked(&fixture);
    let scope = &result.scopes[0];
    assert_eq!(scope.target, fixture.context.target);
    assert_eq!(scope.mappings.len(), 4);
    assert!(scope.mappings.iter().any(|mapping| mapping.source
        == Path::new("original-server/TheIsland.ark")
        && mapping.target == Path::new("example/TheIsland.ark")));
    assert!(
        scope
            .clear
            .contains(&PathBuf::from("example/old.arkprofile"))
    );
    assert!(!scope.clear.contains(&PathBuf::from("example/previous.ark")));
    fixture.import(&fixture.preview().fingerprint).unwrap();
    for path in [
        "previous.ark",
        "OtherMap_01.10.2026_07.53.04.arkrbf",
        "OtherMap_AntiCorruptionBackup.bak",
    ] {
        assert_eq!(
            fs::read(fixture.context.target.join("example").join(path)).unwrap(),
            b"existing target data"
        );
    }
    assert!(
        !fixture
            .context
            .target
            .join("example/old.arkprofile")
            .exists()
    );
    assert!(
        !scope
            .clear
            .iter()
            .any(|path| path.to_string_lossy().contains("cluster")
                || path.to_string_lossy().contains("Config")
                || path.ends_with("unrelated.txt"))
    );
}

#[test]
fn asa_keeps_native_map_subdirectory_without_importing_cluster_data() {
    let fixture = fixture("arksurvivalascended", json!({"map_name":"TheIsland_WP"}));
    put(
        &fixture.source,
        "old-id/TheIsland_WP/TheIsland_WP.ark",
        b"world",
    );
    put(
        &fixture.source,
        "old-id/TheIsland_WP/Account.arkprofile",
        b"player",
    );
    put(
        &fixture.source,
        "old-id/TheIsland_WP/TheIsland_WP_01.10.2026_07.53.04.arkrbf",
        b"backup",
    );
    put(
        &fixture.source,
        "old-id/cluster/upload",
        b"separate cluster upload",
    );
    put(
        &fixture.source,
        "old-id/72.arktribe",
        b"native parent companion",
    );
    let result = checked(&fixture);
    assert_eq!(result.scopes[0].mappings.len(), 4);
    assert!(
        result.scopes[0]
            .mappings
            .iter()
            .all(|mapping| mapping.target.starts_with("example/TheIsland_WP")
                || mapping.target == Path::new("example/72.arktribe"))
    );
    assert!(
        !result.scopes[0]
            .mappings
            .iter()
            .any(|mapping| mapping.source.to_string_lossy().contains("cluster"))
    );
}

#[test]
fn ark_refuses_ambiguous_main_worlds_and_local_host_conversion() {
    let fixture = fixture("arksurvivalevolved", json!({"map_name":"TheIsland"}));
    put(&fixture.source, "one/TheIsland.ark", b"one");
    put(&fixture.source, "two/TheIsland.ark", b"two");
    assert!(rejected(&fixture));
    fs::remove_dir_all(fixture.source.join("two")).unwrap();
    put(&fixture.source, "one/localplayer.arkprofile", b"solo host");
    assert!(rejected(&fixture));
}

#[test]
fn ark_world_selection_is_map_specific_and_refuses_shared_flat_source() {
    assert!(ark_world_file(
        "TheIsland_01.10.2026_07.53.04.arkrbf",
        "TheIsland"
    ));
    for name in [
        "TheIsland_WP_01.10.2026_07.53.04.arkrbf",
        "TheIsland_fake.arkrbf",
        "OtherMap.ark",
        "OtherMap_NewLaunchBackup.bak",
    ] {
        assert!(!ark_world_file(name, "TheIsland"));
    }
    let fixture = fixture("arksurvivalevolved", json!({"map_name":"TheIsland"}));
    put(&fixture.source, "TheIsland.ark", b"one map");
    put(&fixture.source, "OtherMap.ark", b"other map");
    put(
        &fixture.source,
        "account.arkprofile",
        b"unknown shared character",
    );
    assert!(rejected(&fixture));
}

#[test]
fn soulmask_selects_current_map_from_multi_map_source_and_rejects_other_map_leaf() {
    let mut fixture = fixture("soulmask", json!({}));
    put(
        &fixture.source,
        "Worlds/Dedicated/Level01_Main/world.db",
        SQLITE,
    );
    put(
        &fixture.source,
        "Worlds/Dedicated/Level02_Main/world.db",
        SQLITE,
    );
    put(
        &fixture.context.target,
        "Worlds/Dedicated/Level02_Main/world.db",
        b"other target map",
    );
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert_eq!(
        fs::read(
            fixture
                .context
                .target
                .join("Worlds/Dedicated/Level02_Main/world.db")
        )
        .unwrap(),
        b"other target map"
    );
    fixture.source = fixture.source.join("Worlds/Dedicated/Level02_Main");
    assert!(rejected(&fixture));
}

#[test]
fn conan_selects_original_databases_and_excludes_host_configuration() {
    let fixture = fixture("conanexiles", json!({}));
    put(&fixture.source, "game_0.db", SQLITE);
    put(&fixture.source, "game_0_backup_1.db", SQLITE);
    put(
        &fixture.source,
        "Config/WindowsServer/ServerSettings.ini",
        b"source settings",
    );
    put(&fixture.source, "serveruid.txt", b"source server identity");
    let result = checked(&fixture);
    assert_eq!(result.scopes[0].mappings.len(), 2);
    assert!(
        result.scopes[0]
            .clear
            .contains(&PathBuf::from("game_0.db-wal"))
    );
    assert!(result.scopes[0].mappings.iter().all(|mapping| {
        mapping
            .target
            .extension()
            .is_some_and(|extension| extension == "db")
    }));
    put(
        &fixture.source,
        "game_0.db-wal",
        b"uncommitted world updates",
    );
    assert!(rejected(&fixture));
}

#[test]
fn conan_legacy_database_keeps_its_name_and_removes_newer_shadow_database() {
    let fixture = fixture("conanexiles", json!({}));
    put(&fixture.source, "Game.db", SQLITE);
    let result = checked(&fixture);
    assert_eq!(result.scopes[0].mappings[0].target, Path::new("Game.db"));
    assert!(result.scopes[0].clear.contains(&PathBuf::from("game_0.db")));
}

#[test]
fn humanitz_keeps_world_index_player_tree_and_shared_native_saves_together() {
    let fixture = fixture("humanitz", json!({"save_name":"DedicatedSaveMP"}));
    for path in [
        "SaveGames/SaveList/Default/Save_DedicatedSaveMP.sav",
        "SaveGames/SaveList/SaveCache.sav",
        "SaveGames/DedSave_ResGlobal.sav",
        "SaveGames/SaveList/Default/DedicatedSaveMP/FoliageData.sav",
        "SaveGames/SaveList/Default/DedicatedSaveMP/NodeAIData.sav",
    ] {
        put(&fixture.source, path, b"GVASnative save data");
    }
    put(
        &fixture.source,
        "Config/WindowsServer/Engine.ini",
        b"host settings",
    );
    let result = checked(&fixture);
    assert_eq!(result.scopes[0].mappings[0].source, Path::new("SaveGames"));
    assert_eq!(result.scopes[0].mappings[0].target, Path::new("SaveGames"));
    assert!(result.scopes[0].mappings[0].replace);
    fs::remove_file(fixture.source.join("SaveGames/SaveList/SaveCache.sav")).unwrap();
    assert!(rejected(&fixture));
}

fn nightingale_fixture() -> Fixture {
    let fixture = fixture("nightingale", json!({}));
    put(
        &fixture.source,
        "Config/WindowsServer/ServerState.ini",
        b"[/Script/NWX.NWXServerState]\nCurrentDeploy=b-server--mtr-frt-0000\n",
    );
    put(
        &fixture.source,
        "Config/WindowsServer/GameUserSettings.ini",
        b"source host settings",
    );
    put(
        &fixture.source,
        "Offline/DedicatedServer/Deploy/b-server--mtr-frt-0000/record.json",
        br#"{"id":"b-server--mtr-frt-0000","map":"mtr_frt_0000"}"#,
    );
    put(
        &fixture.source,
        "Offline/DedicatedServer/Deploy/b-server--mtr-frt-0000/state",
        b"\x1f\x8b\x08native realm state",
    );
    put(
        &fixture.source,
        "Offline/SteamAccount/Profiles/player.json",
        b"native character profile",
    );
    fixture
}

#[test]
fn nightingale_imports_realm_characters_and_matching_current_deploy_only() {
    let fixture = nightingale_fixture();
    let result = checked(&fixture);
    let mappings = &result.scopes[0].mappings;
    assert_eq!(mappings.len(), 2);
    assert!(
        mappings
            .iter()
            .any(|mapping| mapping.source == Path::new("Offline")
                && mapping.target == Path::new("Offline")
                && mapping.replace)
    );
    assert!(
        mappings
            .iter()
            .any(|mapping| mapping.target == Path::new("Config/WindowsServer/ServerState.ini"))
    );
    assert!(
        !mappings
            .iter()
            .any(|mapping| mapping.target == Path::new("Config"))
    );
    put(
        &fixture.source,
        "Offline/DedicatedServer/Deploy/b-server--mtr-frt-0000/record.json",
        br#"{"id":"another-realm"}"#,
    );
    assert!(rejected(&fixture));
}

#[test]
fn nightingale_refuses_unrelated_host_settings_and_traversal_in_selection_file() {
    let fixture = nightingale_fixture();
    put(
        &fixture.source,
        "Config/WindowsServer/ServerState.ini",
        b"[/Script/NWX.NWXServerState]\nCurrentDeploy=../other\n",
    );
    assert!(rejected(&fixture));
    put(&fixture.source, "Config/WindowsServer/ServerState.ini", b"[/Script/NWX.NWXServerState]\nCurrentDeploy=b-server--mtr-frt-0000\nPassword=fixture-password\n");
    assert!(rejected(&fixture));
}

#[test]
fn zomboid_selects_complete_world_without_taking_over_login_database() {
    let fixture = fixture("projectzomboid", json!({}));
    put(
        &fixture.source,
        "Saves/Multiplayer/original/map_ver.bin",
        b"native map version",
    );
    put(
        &fixture.source,
        "Saves/Multiplayer/original/players.db",
        SQLITE,
    );
    put(
        &fixture.source,
        "Saves/Multiplayer/original/vehicles.db",
        SQLITE,
    );
    put(
        &fixture.source,
        "Saves/Multiplayer/original/map_1_1.bin",
        b"world chunk",
    );
    put(&fixture.source, "db/original.db", SQLITE);
    put(&fixture.source, "Server/original.ini", b"server settings");
    let result = checked(&fixture);
    assert_eq!(result.scopes[0].target, fixture.context.target);
    assert_eq!(
        result.scopes[0].mappings[0].source,
        Path::new("Saves/Multiplayer/original")
    );
    assert!(result.scopes[0].mappings[0].target.as_os_str().is_empty());
    assert!(
        result.warnings[0].contains("login accounts")
            || result.warnings[0].contains("Login accounts")
    );
}

#[test]
fn scum_and_soulmask_keep_only_the_native_world_scopes() {
    for (module, source_file, expected_source, expected_target) in [
        ("scum", "SaveFiles/SCUM.db", "SaveFiles", "SaveFiles"),
        (
            "soulmask",
            "Worlds/Dedicated/Level01_Main/world.db",
            "Worlds/Dedicated/Level01_Main",
            "Worlds/Dedicated/Level01_Main",
        ),
    ] {
        let fixture = fixture(module, json!({}));
        put(&fixture.source, source_file, SQLITE);
        put(
            &fixture.source,
            "Config/WindowsServer/ServerSettings.ini",
            b"host settings",
        );
        put(
            &fixture.source,
            "GameplaySettings/GameXishu.json",
            b"managed gameplay settings",
        );
        let result = checked(&fixture);
        assert_eq!(result.scopes[0].mappings.len(), 1);
        let mapping = &result.scopes[0].mappings[0];
        assert_eq!(mapping.source, Path::new(expected_source));
        assert_eq!(mapping.target, Path::new(expected_target));
        assert!(mapping.replace);
        put(
            &fixture.source,
            &format!("{expected_source}/unexpected.ps1"),
            b"untrusted code",
        );
        assert!(rejected(&fixture));
    }
}

#[test]
fn adapter_refuses_unknown_module_unsafe_names_and_non_native_databases() {
    let mut fixture = fixture("minecraft", json!({}));
    let tree = io::inventory(&fixture.source, None).unwrap();
    assert!(
        plan(&fixture.context, &fixture.source, &tree)
            .unwrap()
            .is_none()
    );
    fixture.context.record.summary.module_id = "humanitz".into();
    fixture
        .context
        .settings
        .insert("save_name".into(), json!("../other"));
    assert!(rejected(&fixture));
    fixture.context.record.summary.module_id = "scum".into();
    put(&fixture.source, "SCUM.db", b"not a SQLite save");
    assert!(rejected(&fixture));
}
