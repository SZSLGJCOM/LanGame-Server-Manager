use super::*;
use serde_json::json;

fn file(path: &str, size: u64, modified: u64) -> SaveFileEntry {
    SaveFileEntry {
        relative_path: path.into(),
        is_directory: false,
        size_bytes: size,
        modified_at_unix_ms: Some(modified),
    }
}

fn dir(path: &str) -> SaveFileEntry {
    SaveFileEntry {
        relative_path: path.into(),
        is_directory: true,
        size_bytes: 0,
        modified_at_unix_ms: Some(1),
    }
}

fn settings(value: serde_json::Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

fn slot<'a>(slots: &'a [SaveSlot], name: &str) -> &'a SaveSlot {
    slots
        .iter()
        .find(|slot| slot.name == name)
        .unwrap_or_else(|| panic!("missing slot {name}: {slots:?}"))
}

/// Every listed file belongs to exactly one slot.
fn assert_partition(entries: &[SaveFileEntry], slots: &[SaveSlot]) {
    for entry in entries.iter().filter(|entry| !entry.is_directory) {
        let owners = slots
            .iter()
            .filter(|slot| {
                slot.paths.iter().any(|root| {
                    entry.relative_path == *root
                        || entry.relative_path.starts_with(&format!("{root}/"))
                })
            })
            .count();
        assert_eq!(
            owners, 1,
            "{} belongs to {owners} slots",
            entry.relative_path
        );
    }
}

#[test]
fn theforest_slots_mark_the_configured_save_slot() {
    let entries = vec![
        dir("Multiplayer"),
        dir("Multiplayer/Slot1"),
        file("Multiplayer/Slot1/__RESUME__", 10, 5),
        dir("Multiplayer/Slot3"),
        file("Multiplayer/Slot3/__RESUME__", 30, 9),
        file("Multiplayer/Slot3/players", 2, 4),
        file("Multiplayer/readme.txt", 1, 6),
        file("notes.txt", 1, 1),
    ];
    let (native, slots) =
        group_save_slots("theforest", &settings(json!({ "save_slot": 3 })), &entries);
    assert!(native);
    let current = slot(&slots, "Slot3");
    assert!(current.current && current.kind == "native");
    assert_eq!(
        (
            current.size_bytes,
            current.file_count,
            current.modified_at_unix_ms
        ),
        (32, 2, Some(9))
    );
    assert_eq!(
        current.label,
        Some(SaveSlotLabel {
            kind: "save_slot",
            value: "3".into()
        })
    );
    assert!(!slot(&slots, "Slot1").current);
    assert_eq!(slots[0].name, "Slot3", "the current slot is listed first");
    assert_eq!(slot(&slots, "").kind, "files");
    let beside = slot(&slots, "Multiplayer");
    assert_eq!(
        (beside.kind, beside.paths.as_slice()),
        (
            "directory",
            ["Multiplayer/readme.txt".to_owned()].as_slice()
        )
    );
    assert_partition(&entries, &slots);
}

#[test]
fn sons_of_the_forest_uses_the_ten_digit_dedicated_slot() {
    let entries = vec![
        dir("DedicatedServer"),
        dir("DedicatedServer/Multiplayer"),
        dir("DedicatedServer/Multiplayer/0000000042"),
        file("DedicatedServer/Multiplayer/0000000042/SaveData.zip", 7, 2),
    ];
    let (_, slots) = group_save_slots(
        "sonsoftheforest",
        &settings(json!({ "save_slot": 42 })),
        &entries,
    );
    let current = slot(&slots, "0000000042");
    assert!(current.current);
    assert_eq!(current.label.as_ref().unwrap().value, "42");
    assert_eq!(sons_slot_dir(42), "DedicatedServer/Multiplayer/0000000042");
    assert_eq!(theforest_slot_dir(2), "Multiplayer/Slot2");
}

#[test]
fn seven_days_requires_the_native_marker_and_matches_world_and_game() {
    let entries = vec![
        dir("Navezgane"),
        dir("Navezgane/MyGame"),
        file("Navezgane/MyGame/main.ttw", 5, 3),
        dir("Navezgane/Other"),
        file("Navezgane/Other/readme", 1, 1),
    ];
    let (_, slots) = group_save_slots(
        "sevendaystodie",
        &settings(json!({ "game_world": "Navezgane", "world_name": "MyGame" })),
        &entries,
    );
    assert!(slot(&slots, "Navezgane/MyGame").current);
    // Without main.ttw the directory is not a native slot and falls back to its top-level row.
    assert_eq!(slot(&slots, "Navezgane").kind, "directory");
    assert_partition(&entries, &slots);
}

#[test]
fn astroneer_groups_versions_by_the_shared_filename_parser() {
    let entries = vec![
        file("SAVE_1$2026.01.02-03.04.05.savegame", 10, 1),
        file("SAVE_1$2026.02.02-03.04.05.savegame", 20, 2),
        file("Other$2026.01.02-03.04.05.savegame", 5, 3),
        file("not-a-save.txt", 1, 4),
    ];
    let (_, slots) = group_save_slots("astroneer", &Map::new(), &entries);
    let active = slot(&slots, "SAVE_1");
    assert!(active.current, "SAVE_1 is the native default active save");
    assert_eq!(
        (active.file_count, active.size_bytes, active.paths.len()),
        (2, 30, 2)
    );
    assert!(!slot(&slots, "Other").current);
    assert_partition(&entries, &slots);
}

#[test]
fn rust_matches_the_configured_map_prefix_and_keeps_sidecars_together() {
    let entries = vec![
        file("proceduralmap.4000.12.256.sav", 100, 5),
        file("proceduralmap.4000.12.256.sav.1", 90, 4),
        file("proceduralmap.4000.12.256.map", 50, 3),
        file("hapisisland.sav", 10, 2),
        file("player.blueprints.5.db", 3, 1),
    ];
    let config = settings(json!({ "level": "Procedural Map", "world_size": 4000, "seed": 12 }));
    let (_, slots) = group_save_slots("rust", &config, &entries);
    let world = slot(&slots, "proceduralmap.4000.12.256");
    assert!(world.current);
    assert_eq!(world.file_count, 3);
    assert!(!slot(&slots, "hapisisland").current);
    assert_eq!(
        rust_world_prefix(
            &settings(json!({ "level_url": "https://x" })),
            "Procedural Map"
        ),
        Ok(String::new())
    );
    assert_eq!(
        rust_world_prefix(&settings(json!({ "seed": 1 })), "Barren"),
        Err("world_size")
    );
    let named = settings(json!({ "level": "HapisIsland" }));
    assert!(slot(&group_save_slots("rust", &named, &entries).1, "hapisisland").current);
    assert_partition(&entries, &slots);
}

#[test]
fn windrose_unturned_and_valheim_layouts_name_their_worlds() {
    let windrose = vec![
        dir("RocksDB_v2"),
        dir("RocksDB_v2/0.10"),
        dir("RocksDB_v2/0.10/Worlds"),
        dir("RocksDB_v2/0.10/Worlds/island-a"),
        file(
            "RocksDB_v2/0.10/Worlds/island-a/WorldDescription.json",
            4,
            1,
        ),
    ];
    let (_, slots) = group_save_slots(
        "windrose",
        &settings(json!({ "world_island_id": "island-a" })),
        &windrose,
    );
    assert!(slot(&slots, "island-a").current);

    let unturned = vec![
        dir("Level"),
        dir("Level/PEI"),
        file("Level/PEI/Barricades.dat", 1, 1),
        dir("Players"),
        file("Players/p.dat", 1, 1),
    ];
    let (_, slots) = group_save_slots("unturned", &settings(json!({ "map": "PEI" })), &unturned);
    assert!(slot(&slots, "PEI").current);
    assert_eq!(slot(&slots, "Players").kind, "directory");

    let valheim = vec![
        dir("worlds_local"),
        file("worlds_local/Dedicated.fwl", 1, 1),
        file("worlds_local/Dedicated.db", 9, 2),
        dir("worlds_local/Modern"),
        file("worlds_local/Modern/_main.1.ok", 1, 3),
    ];
    let (_, slots) = group_save_slots(
        "valheim",
        &settings(json!({ "world_name": "Dedicated" })),
        &valheim,
    );
    let dedicated = slot(&slots, "Dedicated");
    assert!(dedicated.current && dedicated.file_count == 2);
    assert_eq!(slot(&slots, "Modern").kind, "native");
    assert_partition(&valheim, &slots);
}

#[test]
fn games_without_a_slot_model_fall_back_to_top_level_rows() {
    let entries = vec![
        dir("0"),
        dir("0/WORLD"),
        file("0/WORLD/Level.sav", 8, 2),
        file("loose.txt", 1, 1),
    ];
    let (native, slots) = group_save_slots("palworld", &Map::new(), &entries);
    assert!(!native);
    assert_eq!(slots.len(), 2);
    assert_eq!(slot(&slots, "0").kind, "directory");
    assert!(slots.iter().all(|slot| !slot.current));
    assert_partition(&entries, &slots);
}

#[test]
fn shared_native_name_rules_reject_path_like_settings() {
    for bad in [
        "", "..", "a/b", "a\\b", "C:", "trail.", "trail ", "nul\u{0}",
    ] {
        assert!(!is_native_name(bad), "{bad:?}");
    }
    assert!(is_native_name("My World"));
    let traversal = settings(json!({ "map": "../Level" }));
    let entries = vec![dir("Level"), dir("Level/PEI"), file("Level/PEI/a", 1, 1)];
    assert!(
        group_save_slots("unturned", &traversal, &entries)
            .1
            .iter()
            .all(|slot| !slot.current)
    );
    assert_eq!(
        configured_save_slot(&settings(json!({ "save_slot": 9 })), Some(5)),
        None
    );
}
