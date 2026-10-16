use super::*;
use crate::backups::save_import::tests::Fixture;

// Opaque synthetic payloads test migration scope, identity binding and byte
// preservation. They are deliberately not evidence of native game loading.
fn fixture(module: &str, settings: Value) -> Fixture {
    let mut fixture = Fixture::new();
    fs::remove_dir_all(&fixture.source).unwrap();
    fs::create_dir(&fixture.source).unwrap();
    fixture.context.record.summary.module_id = module.into();
    fixture.context.settings = serde_json::from_value(settings).unwrap();
    fixture.context.settings_json = serde_json::to_string(&fixture.context.settings).unwrap();
    fixture
}

fn put(root: &Path, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn checked(fixture: &Fixture) -> ServerPlan {
    plan(
        &fixture.context,
        &fixture.source,
        &io::inventory(&fixture.source, None).unwrap(),
    )
    .unwrap()
    .unwrap()
}

#[test]
fn save_import_server_worlds_rimworld_keeps_linked_assets_and_target_configuration() {
    let fixture = fixture("rimworld", json!({}));
    for directory in ["Users", "Saves", "Settlements", "Maps"] {
        fs::create_dir(fixture.source.join(directory)).unwrap();
    }
    put(
        &fixture.source,
        "WorldValuesFile.json",
        br#"{"Seed":"original","unknown":"keep"}"#,
    );
    put(
        &fixture.source,
        "Users/Alice.json",
        b"original account password hash",
    );
    put(&fixture.source, "Saves/Alice.rws", b"original colony");
    put(
        &fixture.source,
        "Settlements/42.json",
        br#"{"Username":"Alice"}"#,
    );
    put(
        &fixture.context.install,
        "Configs/ServerConfig.json",
        b"target host configuration",
    );
    let before = io::inventory(&fixture.source, None).unwrap();
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert_eq!(
        io::inventory(&fixture.context.target, None).unwrap(),
        before
    );
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), before);
    assert_eq!(
        fs::read(fixture.context.install.join("Configs/ServerConfig.json")).unwrap(),
        b"target host configuration"
    );
    fs::remove_file(fixture.source.join("WorldValuesFile.json")).unwrap();
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
}

#[test]
fn save_import_server_worlds_rust_replaces_world_and_player_set_without_host_takeover() {
    let fixture = fixture(
        "rust",
        json!({"level":"Procedural Map","level_url":"","world_size":3500,"seed":12345}),
    );
    for (path, bytes) in [
        ("proceduralmap.3500.12345.270.sav", "world"),
        ("proceduralmap.3500.12345.270.map", "map"),
        ("proceduralmap.3500.12345.270.sav.1", "native backup"),
        ("player.blueprints.5.db", "blueprints"),
        ("player.blueprints.5.db-wal", "native WAL"),
        ("player.blueprints.5.db-shm", "native SHM"),
        ("player.identities.5.db", "player identities"),
        ("sv.files.0.db", "sign images"),
        ("loadouts/a.json", "loadout"),
        ("cfg/server.cfg", "source RCON credentials"),
        ("companion.id", "source Rust+ server identity"),
        ("player.tokens.db", "source Rust+ pairing credentials"),
    ] {
        put(&fixture.source, path, bytes);
    }
    for path in [
        "player.states.4.db",
        "oldmap.269.sav",
        "oldmap.269.map",
        "loadouts/obsolete.json",
    ] {
        put(&fixture.context.target, path, b"old native payload");
    }
    for (path, bytes) in [
        ("cfg/server.cfg", "target RCON credentials"),
        ("companion.id", "target Rust+ identity"),
        ("player.tokens.db", "target pairing"),
        ("future-metadata.json", "unowned target"),
    ] {
        put(&fixture.context.target, path, bytes);
    }
    let before = io::inventory(&fixture.source, None).unwrap();
    let result = checked(&fixture);
    assert!(
        result.scopes[0]
            .clear
            .contains(&PathBuf::from("player.states.4.db"))
    );
    fixture.import(&fixture.preview().fingerprint).unwrap();
    for path in [
        "player.states.4.db",
        "oldmap.269.sav",
        "loadouts/obsolete.json",
    ] {
        assert!(!fixture.context.target.join(path).exists());
    }
    assert_eq!(
        fs::read(fixture.context.target.join("player.blueprints.5.db-wal")).unwrap(),
        b"native WAL"
    );
    assert_eq!(
        fs::read(fixture.context.target.join("cfg/server.cfg")).unwrap(),
        b"target RCON credentials"
    );
    assert_eq!(
        fs::read(fixture.context.target.join("companion.id")).unwrap(),
        b"target Rust+ identity"
    );
    assert_eq!(
        fs::read(fixture.context.target.join("player.tokens.db")).unwrap(),
        b"target pairing"
    );
    assert!(
        fixture
            .context
            .target
            .join("future-metadata.json")
            .is_file()
    );
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), before);
}

#[test]
fn save_import_server_worlds_rust_refuses_unmatched_map_and_copied_executables() {
    let mut fixture = fixture(
        "rust",
        json!({"level":"Procedural Map","level_url":"","world_size":3500,"seed":12345}),
    );
    put(
        &fixture.source,
        "proceduralmap.3500.12345.270.sav",
        b"world",
    );
    put(&fixture.source, "proceduralmap.3500.12345.270.map", b"map");
    let checked = fixture.preview();
    fixture.context.settings.insert("seed".into(), json!(42));
    assert!(fixture.import(&checked.fingerprint).is_err());
    fixture.context.settings.insert("seed".into(), json!(12345));
    put(&fixture.source, "loadouts/execute.dll", b"not a save");
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
}

#[test]
fn save_import_server_worlds_satisfactory_uses_native_server_sessions_and_keeps_identity() {
    let fixture = fixture("satisfactory", json!({}));
    put(
        &fixture.source,
        "ServerSettings.7777.sav",
        b"source secret identity",
    );
    put(&fixture.source, "server/Factory.sav", b"original factory");
    put(
        &fixture.source,
        "server/Factory_autosave_0.sav",
        b"native autosave",
    );
    put(
        &fixture.context.target,
        "ServerSettings.7777.sav",
        b"target secret identity",
    );
    put(
        &fixture.context.target,
        "server/Other.sav",
        b"unselected session",
    );
    let before = io::inventory(&fixture.source, None).unwrap();
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert_eq!(
        fs::read(fixture.context.target.join("server/Factory.sav")).unwrap(),
        b"original factory"
    );
    assert_eq!(
        fs::read(fixture.context.target.join("ServerSettings.7777.sav")).unwrap(),
        b"target secret identity"
    );
    assert!(fixture.context.target.join("server/Other.sav").is_file());
    assert!(!fixture.context.target.join("Factory.sav").exists());
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), before);
    // Directly selecting the source server/ directory has the same destination.
    let source = fixture.source.join("server");
    let plan = plan(
        &fixture.context,
        &source,
        &io::inventory(&source, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(
        plan.scopes[0]
            .mappings
            .iter()
            .any(|map| map.target == Path::new("server/Factory.sav"))
    );
}

#[test]
fn save_import_server_worlds_satisfactory_refuses_identity_only_and_ambiguous_names() {
    let fixture = fixture("satisfactory", json!({}));
    put(
        &fixture.source,
        "ServerSettings.7777.sav",
        b"source identity",
    );
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
    put(&fixture.source, "Factory.sav", b"one");
    put(&fixture.source, "server/Factory.sav", b"another");
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
}

fn sevendays_source(fixture: &Fixture, world: &str, prefixed: bool) {
    let root = if prefixed {
        fixture.source.join("Saves")
    } else {
        fixture.source.clone()
    };
    put(
        &root,
        format!("{world}/Game/main.ttw"),
        b"original native main",
    );
    put(
        &root,
        format!("{world}/Game/Region/r.0.0.7rg"),
        b"native region",
    );
    put(
        &root,
        format!("{world}/Game/Player/Steam_123.ttp"),
        b"native player",
    );
}

fn sevendays_map(root: &Path) {
    for marker in ["map_info.xml", "dtm.raw", "biomes.png", "prefabs.xml"] {
        put(root, marker, b"native map payload");
    }
}

#[test]
fn save_import_server_worlds_sevendays_migrates_selected_save_and_generated_map_together() {
    let fixture = fixture(
        "sevendaystodie",
        json!({"game_world":"OriginalMap","world_name":"Game"}),
    );
    sevendays_source(&fixture, "OriginalMap", true);
    sevendays_map(&fixture.source.join("GeneratedWorlds/OriginalMap"));
    put(&fixture.source, "serveradmin.xml", b"source access rules");
    let maps = instance_root(&fixture.context.record).join("GeneratedWorlds");
    put(&maps, "OriginalMap/old-region.bin", b"old map");
    put(&maps, "OtherMap/keep.bin", b"other map");
    put(
        &fixture.context.target,
        "OtherMap/OtherGame/main.ttw",
        b"other game",
    );
    put(
        &fixture.context.target,
        "OriginalMap/Game/Player/old.ttp",
        b"old player",
    );
    let before = io::inventory(&fixture.source, None).unwrap();
    let plan = checked(&fixture);
    assert_eq!(plan.scopes.len(), 2);
    assert_eq!(plan.scopes[1].target, maps);
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert!(maps.join("OriginalMap/dtm.raw").is_file());
    assert!(!maps.join("OriginalMap/old-region.bin").exists());
    assert!(maps.join("OtherMap/keep.bin").is_file());
    assert!(
        !fixture
            .context
            .target
            .join("OriginalMap/Game/Player/old.ttp")
            .exists()
    );
    assert!(
        fixture
            .context
            .target
            .join("OriginalMap/Game/Region/r.0.0.7rg")
            .is_file()
    );
    assert!(
        fixture
            .context
            .target
            .join("OtherMap/OtherGame/main.ttw")
            .is_file()
    );
    assert!(!fixture.context.target.join("serveradmin.xml").exists());
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), before);
}

#[test]
fn save_import_server_worlds_sevendays_builtin_saves_root_and_missing_map_boundary() {
    let mut fixture = fixture(
        "sevendaystodie",
        json!({"game_world":"Navezgane","world_name":"Game"}),
    );
    sevendays_source(&fixture, "Navezgane", false);
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
    sevendays_map(&fixture.context.install.join("Data/Worlds/Navezgane"));
    assert_eq!(checked(&fixture).scopes.len(), 1);
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert!(
        fixture
            .context
            .target
            .join("Navezgane/Game/main.ttw")
            .is_file()
    );
    fixture
        .context
        .settings
        .insert("game_world".into(), json!("RWG"));
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
}

#[path = "save_import_server_worlds_identity_tests.rs"]
mod identity_tests;
