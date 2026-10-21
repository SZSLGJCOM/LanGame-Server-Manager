use super::*;

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "langame-save-listing-{label}-{}",
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn test_record(root: &Path) -> StoredInstanceRecord {
    let instance_root = root.join("instance");
    StoredInstanceRecord {
        summary: InstanceSummary {
            id: String::from("instance-1"),
            name: String::from("Listing Test"),
            module_id: String::from("test"),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: String::from("127.0.0.1"),
            port_count: 0,
            autostart: false,
        },
        config_dir: instance_root.join("config"),
        saves_dir: instance_root.join("saves"),
        runtime_mode: String::from("independent"),
        program_install_root: Some(instance_root.join("runtime")),
        auto_backup_on_stop: false,
        backup_retention_count: 3,
    }
}

#[cfg(unix)]
fn create_directory_link(link: &Path, target: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn create_directory_link(link: &Path, target: &Path) {
    let output = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "failed to create test junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn relative_paths(listing: &SaveFileListing) -> Vec<&str> {
    listing
        .entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect()
}

#[test]
fn lists_nested_files_with_sizes_in_name_order() {
    let root = temp_root("nested");
    let saves = root.join("saves");
    fs::create_dir_all(saves.join("world/region")).unwrap();
    fs::write(saves.join("b.dat"), b"12345").unwrap();
    fs::write(saves.join("world/level.dat"), b"abc").unwrap();
    fs::write(saves.join("world/region/r.0.mca"), b"x").unwrap();

    let listing = list_directory_tree(&saves, 100).unwrap();
    assert!(listing.exists && !listing.truncated);
    assert_eq!(
        relative_paths(&listing),
        [
            "b.dat",
            "world",
            "world/level.dat",
            "world/region",
            "world/region/r.0.mca"
        ]
    );
    let file = listing
        .entries
        .iter()
        .find(|entry| entry.relative_path == "b.dat")
        .unwrap();
    assert_eq!((file.size_bytes, file.is_directory), (5, false));
    assert!(file.modified_at_unix_ms.is_some());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_root_is_reported_as_absent_not_as_an_error() {
    let root = temp_root("missing");
    let listing = list_directory_tree(&root.join("saves"), 10).unwrap();
    assert!(!listing.exists && listing.entries.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn entry_cap_truncates_with_a_stable_prefix() {
    let root = temp_root("cap");
    let saves = root.join("saves");
    fs::create_dir_all(&saves).unwrap();
    for index in 0..5 {
        fs::write(saves.join(format!("{index}.sav")), b"s").unwrap();
    }
    let listing = list_directory_tree(&saves, 3).unwrap();
    assert!(listing.truncated);
    assert_eq!(relative_paths(&listing), ["0.sav", "1.sav", "2.sav"]);
    let exact = list_directory_tree(&saves, 5).unwrap();
    assert!(!exact.truncated && exact.entries.len() == 5);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn links_and_junctions_inside_the_root_are_never_followed() {
    let root = temp_root("link-inside");
    let saves = root.join("saves");
    let outside = root.join("outside");
    fs::create_dir_all(&saves).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.db"), b"outside").unwrap();
    fs::write(saves.join("world.db"), b"world").unwrap();
    create_directory_link(&saves.join("escape"), &outside);

    let listing = list_directory_tree(&saves, 100).unwrap();
    assert_eq!(relative_paths(&listing), ["world.db"]);
    assert_eq!(listing.skipped_links, ["escape"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_linked_root_is_rejected() {
    let root = temp_root("link-root");
    let target = root.join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("secret.db"), b"outside").unwrap();
    let saves = root.join("saves");
    create_directory_link(&saves, &target);

    assert!(matches!(
        list_directory_tree(&saves, 100),
        Err(StorageError::UnsafeManagedPath { .. })
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn backup_ids_with_traversal_or_separators_are_rejected() {
    let root = temp_root("traversal");
    let record = test_record(&root);
    fs::create_dir_all(instance_backup_root(&record)).unwrap();
    let mut rejected = vec!["..", "../saves", "a/b", " padded", ""];
    if cfg!(windows) {
        rejected.extend(["a\\b", "C:\\Windows", "\\\\server\\share"]);
    }
    for backup_id in rejected {
        assert!(
            validated_backup_path(&record, backup_id).is_err(),
            "accepted backup id {backup_id:?}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_backup_linked_out_of_the_backup_root_is_rejected() {
    let root = temp_root("backup-link");
    let record = test_record(&root);
    let backups = instance_backup_root(&record);
    let outside = root.join("outside");
    fs::create_dir_all(&backups).unwrap();
    fs::create_dir_all(&outside).unwrap();
    create_directory_link(&backups.join("saves-1"), &outside);

    assert!(validated_backup_path(&record, "saves-1").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn slots_are_built_only_from_the_confined_capped_listing() {
    let root = temp_root("slots");
    let saves = root.join("saves");
    let outside = root.join("outside");
    fs::create_dir_all(saves.join("Multiplayer/Slot2")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(saves.join("Multiplayer/Slot2/__RESUME__"), b"resume").unwrap();
    fs::write(outside.join("huge.bin"), vec![0u8; 4096]).unwrap();
    // A junction disguised as another native slot is neither entered nor counted.
    create_directory_link(&saves.join("Multiplayer").join("Slot5"), &outside);

    let listing = list_directory_tree(&saves, 100).unwrap();
    let settings = serde_json::json!({ "save_slot": 2 })
        .as_object()
        .cloned()
        .unwrap();
    let (native, grouped) = slots::group_save_slots("theforest", &settings, &listing.entries);
    assert!(native);
    assert_eq!(listing.skipped_links, ["Multiplayer/Slot5"]);
    assert_eq!(grouped.len(), 1, "{grouped:?}");
    let current = grouped.iter().find(|slot| slot.current).unwrap();
    assert_eq!(
        (
            current.name.as_str(),
            current.size_bytes,
            current.file_count
        ),
        ("Slot2", 6, 1)
    );
    assert!(grouped.iter().all(|slot| slot.name != "Slot5"));

    let capped = list_directory_tree(&saves, 2).unwrap();
    assert!(capped.truncated);
    let (_, partial) = slots::group_save_slots("theforest", &settings, &capped.entries);
    assert!(
        partial.iter().all(|slot| slot.file_count == 0),
        "capped totals cover listed entries only"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn serialized_listing_always_carries_every_collection_and_the_slot_model() {
    let root = temp_root("contract");
    let listing = list_directory_tree(&root.join("missing"), 10).unwrap();
    let value = serde_json::to_value(&listing).unwrap();
    for key in ["entries", "skipped_links", "slots"] {
        assert_eq!(
            value[key],
            serde_json::json!([]),
            "{key} must serialize as an empty array"
        );
    }
    assert_eq!(value["slot_model"], "fallback");
    let keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.len(), 9, "{keys:?}");
    fs::remove_dir_all(root).ok();
}
