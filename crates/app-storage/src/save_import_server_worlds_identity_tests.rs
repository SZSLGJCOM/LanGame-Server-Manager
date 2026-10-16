use super::*;

#[test]
fn save_import_server_worlds_sons_keeps_complete_dedicated_slot_and_continue_binding() {
    let mut fixture = fixture(
        "sonsoftheforest",
        json!({"save_slot":3,"save_mode":"Continue"}),
    );
    put(
        &fixture.source,
        "DedicatedServer/Multiplayer/0000000003/SaveData.zip",
        b"PK\x03\x04opaque native world container",
    );
    put(
        &fixture.source,
        "DedicatedServer/Multiplayer/0000000003/SaveData.zip.bak",
        b"native backup",
    );
    put(
        &fixture.source,
        "dedicatedserver.cfg",
        b"source host credentials",
    );
    put(
        &fixture.context.target,
        "DedicatedServer/Multiplayer/0000000003/old-player.json",
        b"old player",
    );
    put(
        &fixture.context.target,
        "DedicatedServer/Multiplayer/0000000001/keep",
        b"other slot",
    );
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert!(
        fixture
            .context
            .target
            .join("DedicatedServer/Multiplayer/0000000003/SaveData.zip")
            .is_file()
    );
    assert!(
        !fixture
            .context
            .target
            .join("DedicatedServer/Multiplayer/0000000003/old-player.json")
            .exists()
    );
    assert!(!fixture.context.target.join("dedicatedserver.cfg").exists());
    assert!(
        fixture
            .context
            .target
            .join("DedicatedServer/Multiplayer/0000000001/keep")
            .is_file()
    );
    let preview = fixture.preview();
    fixture
        .context
        .settings
        .insert("save_mode".into(), json!("New"));
    assert!(fixture.import(&preview.fingerprint).is_err());
    fixture
        .context
        .settings
        .insert("save_mode".into(), json!("Continue"));
    fixture
        .context
        .settings
        .insert("save_slot".into(), json!(4));
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
fn save_import_server_worlds_theforest_requires_checkpoint_and_preserves_other_slots() {
    let mut fixture = fixture("theforest", json!({"save_slot":2,"init_type":"Continue"}));
    put(
        &fixture.source,
        "Multiplayer/Slot2/__RESUME__",
        b"original dedicated world",
    );
    put(
        &fixture.source,
        "Multiplayer/Slot2/guid",
        b"native slot identity",
    );
    put(
        &fixture.context.target,
        "Multiplayer/Slot2/old-player",
        b"old player",
    );
    put(
        &fixture.context.target,
        "Multiplayer/Slot1/__RESUME__",
        b"other slot",
    );
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert_eq!(
        fs::read(fixture.context.target.join("Multiplayer/Slot2/__RESUME__")).unwrap(),
        b"original dedicated world"
    );
    assert!(
        !fixture
            .context
            .target
            .join("Multiplayer/Slot2/old-player")
            .exists()
    );
    assert!(
        fixture
            .context
            .target
            .join("Multiplayer/Slot1/__RESUME__")
            .is_file()
    );
    fs::remove_file(fixture.source.join("Multiplayer/Slot2/__RESUME__")).unwrap();
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
    fixture
        .context
        .settings
        .insert("init_type".into(), json!("New"));
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
fn save_import_server_worlds_unturned_replaces_linked_level_and_players_only() {
    let mut fixture = fixture("unturned", json!({"map":"PEI"}));
    for (path, bytes) in [
        ("Level/PEI/Barricades.dat", "barricades"),
        ("Level/PEI/Structures.dat", "structures"),
        ("Level/PEI/Structures.dat~", "native backup"),
        (
            "Players/76561198000000000_0/PEI/Player.dat",
            "native character",
        ),
        ("Server/Commands.dat", "source password"),
        ("Config.txt", "source gameplay config"),
        ("WorkshopDownloadConfig.json", "source workshop"),
        ("Server/Blacklist.dat", "source bans"),
    ] {
        put(&fixture.source, path, bytes);
    }
    for (path, bytes) in [
        ("Level/PEI/obsolete.dat", "old level"),
        (
            "Players/76561198000000001_0/PEI/Player.dat",
            "old character",
        ),
        ("Server/Commands.dat", "target password"),
        ("Config.txt", "target gameplay"),
        ("Server/Blacklist.dat", "target bans"),
    ] {
        put(&fixture.context.target, path, bytes);
    }
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert!(
        !fixture
            .context
            .target
            .join("Level/PEI/obsolete.dat")
            .exists()
    );
    assert!(
        !fixture
            .context
            .target
            .join("Players/76561198000000001_0/PEI/Player.dat")
            .exists()
    );
    assert_eq!(
        fs::read(
            fixture
                .context
                .target
                .join("Players/76561198000000000_0/PEI/Player.dat")
        )
        .unwrap(),
        b"native character"
    );
    assert_eq!(
        fs::read(fixture.context.target.join("Server/Commands.dat")).unwrap(),
        b"target password"
    );
    assert_eq!(
        fs::read(fixture.context.target.join("Server/Blacklist.dat")).unwrap(),
        b"target bans"
    );
    fixture
        .context
        .settings
        .insert("map".into(), json!("Russia"));
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
}

fn windrose_fixture() -> Fixture {
    let fixture = fixture("windrose", json!({"world_island_id":"ORIGINAL"}));
    put(
        &fixture.source,
        "RocksDB_v2/0.10.0/Worlds/ORIGINAL/WorldDescription.json",
        br#"{"WorldDescription":{"islandId":"ORIGINAL","WorldName":"Original","unknown":42}}"#,
    );
    put(
        &fixture.source,
        "RocksDB_v2/0.10.0/Worlds/ORIGINAL/000001.sst",
        b"opaque native database",
    );
    put(
        &fixture.source,
        "RocksDB_v2_Backups/0.10.0/Worlds/ORIGINAL_Latest/world.bin",
        b"complete native backup",
    );
    put(&fixture.context.install, "R5/ServerDescription.json", br#"{"ServerDescription_Persistent":{"WorldIslandId":"ORIGINAL","PersistentServerId":"TARGET","Password":"fixture-password"}}"#);
    fixture
}

#[test]
fn save_import_server_worlds_windrose_retains_database_backup_and_target_identity() {
    let fixture = windrose_fixture();
    let before = io::inventory(&fixture.source, None).unwrap();
    let description = fixture.context.install.join("R5/ServerDescription.json");
    let native = fs::read(&description).unwrap();
    assert_eq!(checked(&fixture).guards, vec![description.clone()]);
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert_eq!(
        io::inventory(&fixture.context.target, None).unwrap(),
        before
    );
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), before);
    assert_eq!(fs::read(description).unwrap(), native);
}

#[test]
fn save_import_server_worlds_windrose_refuses_identity_changes_and_partial_worlds() {
    let fixture = windrose_fixture();
    let preview = fixture.preview();
    put(&fixture.context.install, "R5/ServerDescription.json", br#"{"ServerDescription_Persistent":{"WorldIslandId":"ORIGINAL","PersistentServerId":"CHANGED"}}"#);
    assert!(fixture.import(&preview.fingerprint).is_err());
    put(
        &fixture.context.install,
        "R5/ServerDescription.json",
        br#"{"ServerDescription_Persistent":{"WorldIslandId":"DIFFERENT"}}"#,
    );
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
    put(
        &fixture.context.install,
        "R5/ServerDescription.json",
        br#"{"ServerDescription_Persistent":{"WorldIslandId":"ORIGINAL"}}"#,
    );
    fs::remove_file(
        fixture
            .source
            .join("RocksDB_v2/0.10.0/Worlds/ORIGINAL/000001.sst"),
    )
    .unwrap();
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
    put(
        &fixture.source,
        "RocksDB_v2/0.10.0/Worlds/ORIGINAL/000001.sst",
        b"database",
    );
    put(
        &fixture.source,
        "RocksDB_v2/0.11.0/Worlds/ORIGINAL/WorldDescription.json",
        br#"{"WorldDescription":{"islandId":"ORIGINAL"}}"#,
    );
    assert!(
        plan(
            &fixture.context,
            &fixture.source,
            &io::inventory(&fixture.source, None).unwrap()
        )
        .is_err()
    );
}
