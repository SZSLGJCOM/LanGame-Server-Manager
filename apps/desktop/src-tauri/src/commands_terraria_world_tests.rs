use super::*;
use app_core::{InstanceStatus, InstanceSummary};
use serde_json::json;
use std::{fs, path::PathBuf};

struct Fixture {
    root: PathBuf,
    details: InstanceDetails,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-terraria-world-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let saves = root.join("saves");
        fs::create_dir(&saves).unwrap();
        let details = InstanceDetails {
            summary: InstanceSummary {
                id: "terraria-fixture".into(),
                name: "Fixture".into(),
                module_id: "terraria".into(),
                status: InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: "127.0.0.1".into(),
                port_count: 0,
                autostart: false,
            },
            saves_path: saves.to_str().unwrap().into(),
            config_file_path: root.join("config/instance.json").to_str().unwrap().into(),
            backup_uses_declared_saves_path: true,
            auto_backup_on_stop: false,
            backup_retention_count: 3,
            settings_json: json!({
                "server_runtime": "vanilla",
                "world_file": "岛.wld",
                "tmodloader_enabled_mod_names": "BarnacleMod"
            })
            .to_string(),
            ports: vec![],
            active_run: None,
        };
        Self { root, details }
    }
    fn write(&self, name: &str, bytes: &[u8]) {
        fs::write(Path::new(&self.details.saves_path).join(name), bytes).unwrap();
    }
    fn set(&mut self, key: &str, value: &str) {
        let mut json: Value = serde_json::from_str(&self.details.settings_json).unwrap();
        json[key] = value.into();
        self.details.settings_json = json.to_string();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn world_header(version: i32) -> Vec<u8> {
    // Desktop FileMetadata, followed by the section count and stream positions.
    let mut bytes = vec![0; 256];
    bytes[..4].copy_from_slice(&version.to_le_bytes());
    bytes[4..12].copy_from_slice(b"relogic\x02");
    bytes[24..26].copy_from_slice(&11i16.to_le_bytes());
    bytes[26..30].copy_from_slice(&128i32.to_le_bytes());
    bytes
}

#[test]
fn too_new_selected_world_gets_an_unused_filename_without_writing_either_world() {
    let mut game = Fixture::new();
    let original = world_header(326);
    game.write("岛.wld", &original);
    game.write("岛-mods.twld.bak2", b"preserve rollback");
    game.write("岛-mods-2.wld", &world_header(279));
    let archives = game.root.join("tmodloader/Worlds/Backups");
    fs::create_dir_all(&archives).unwrap();
    // The game prunes backups by substring, even when the .wld was removed.
    let archive = archives.join("2026-10-13-岛-mods-30.zip");
    fs::write(&archive, b"preserve native archive").unwrap();
    let preview = conflict(&game.details).unwrap().unwrap();
    assert_eq!(
        (preview.world_version, preview.max_world_version),
        (326, 279)
    );
    assert_eq!(preview.suggested_world_file, "岛-mods-4.wld");
    assert!(!game.root.join("saves/岛-mods-4.wld").exists());
    assert!(ensure_compatible_for_start(&game.details).is_err());
    game.set("world_file", &preview.suggested_world_file);
    assert!(conflict(&game.details).unwrap().is_none());
    assert!(ensure_fresh_for_start(&game.details, &preview.suggested_world_file).is_ok());
    game.write("岛-mods-4.wld", &world_header(279));
    assert!(ensure_fresh_for_start(&game.details, &preview.suggested_world_file).is_err());
    assert_eq!(fs::read(game.root.join("saves/岛.wld")).unwrap(), original);
    assert_eq!(fs::read(archive).unwrap(), b"preserve native archive");
}

#[test]
fn compatible_old_files_and_absent_slots_pass_but_corruption_is_not_a_version_conflict() {
    let game = Fixture::new();
    assert!(conflict(&game.details).unwrap().is_none());
    game.write("岛.wld", &world_header(279));
    assert!(conflict(&game.details).unwrap().is_none());
    let mut legacy = vec![0x20; 100];
    legacy[..4].copy_from_slice(&102i32.to_le_bytes());
    game.write("岛.wld", &legacy);
    assert!(conflict(&game.details).unwrap().is_none());
    for data in [
        vec![],
        vec![0xff; 100],
        b"This is not a Terraria world".repeat(3),
    ] {
        game.write("岛.wld", &data);
        assert!(conflict(&game.details).is_err());
    }
}

#[test]
fn custom_runtime_uses_its_own_terraria_base() {
    let mut game = Fixture::new();
    game.write("岛.wld", &world_header(326));
    let custom = game.root.join("custom-runtime");
    fs::create_dir(&custom).unwrap();
    game.set("tmodloader_runtime_dir", custom.to_str().unwrap());
    let manifest = custom.join("tModLoader.deps.json");
    fs::write(&manifest, br#"{"libraries":{"tModLoader/1.4.5.8":{}}}"#).unwrap();
    assert!(conflict(&game.details).unwrap().is_none());
    fs::write(&manifest, br#"{"libraries":{"tModLoader/1.4.4.9":{}}}"#).unwrap();
    assert!(conflict(&game.details).unwrap().is_some());
    fs::write(&manifest, b"truncated").unwrap();
    assert!(conflict(&game.details).is_err());
}

#[test]
fn protect_modded_world_on_vanilla_and_block_orphaned_sidecars_and_invalid_paths() {
    let mut game = Fixture::new();
    game.write("岛.twld", b"modded items");
    assert!(conflict(&game.details).is_err());
    game.set("tmodloader_enabled_mod_names", "");
    game.write("岛.wld", &world_header(279));
    assert!(
        ensure_compatible_for_start(&game.details)
            .unwrap_err()
            .contains("模组数据")
    );
    for filename in [
        "../outside.wld",
        "outside:path.wld",
        "CONIN$.wld",
        "COM¹.wld",
        "world.wld.bak",
        "",
    ] {
        game.set("world_file", filename);
        assert!(conflict(&game.details).is_err());
    }
}
