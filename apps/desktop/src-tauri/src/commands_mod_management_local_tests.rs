use super::*;

#[test]
fn first_disable_records_local_payload_and_enable_accepts_the_original_name() {
    let (root, target) = fixture("first-disable");
    write_package(&target, "Example");
    let original = fingerprint_tree(&target.target_path.join("Example")).unwrap();
    let scanned =
        crate::commands::commands_mods::mod_inventory::read_inventory(clone_target(&target))
            .unwrap();
    assert!(scanned.items[0].management.is_none());
    assert!(!target.target_path.join(ONLINE_MOD_MANIFEST).exists());

    let disabled = manage_package(clone_target(&target), "Example", "disable").unwrap();
    let item = &disabled.inventory.items[0];
    assert_eq!(
        item.management.as_ref().unwrap().package_id,
        "local-Example"
    );
    assert!(!item.management.as_ref().unwrap().enabled);
    assert!(!target.target_path.join("Example").exists());
    assert!(!Path::new(&item.path).starts_with(&target.target_path));
    assert_eq!(fingerprint_tree(Path::new(&item.path)).unwrap(), original);

    // A queued request can still carry the name from the preceding inventory.
    let enabled = manage_package(clone_target(&target), "Example", "enable").unwrap();
    assert!(
        enabled.inventory.items[0]
            .management
            .as_ref()
            .unwrap()
            .enabled
    );
    assert_eq!(
        fingerprint_tree(&target.target_path.join("Example")).unwrap(),
        original
    );
    assert_eq!(
        read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))
            .unwrap()
            .packages
            .len(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn first_delete_removes_recognizable_payload_and_preserves_other_local_bytes() {
    let (root, target) = fixture("first-delete");
    write_package(&target, "Example");
    for relative in ["operator-notes.md", "world.sav", "assets.bin"] {
        fs::write(target.target_path.join("Example").join(relative), relative).unwrap();
    }
    let mut retained_files = fingerprint_tree(&target.target_path.join("Example")).unwrap();
    retained_files.remove("plugin.dll");
    fs::write(target.target_path.join("unrelated.dll"), b"unrelated Mod").unwrap();

    let deleted = manage_package(clone_target(&target), "Example", "delete").unwrap();
    assert_eq!(deleted.removed_file_count, 1);
    assert_eq!(
        fingerprint_tree(Path::new(deleted.preserved_path.as_ref().unwrap())).unwrap(),
        retained_files
    );
    assert!(!target.target_path.join("Example").exists());
    assert_eq!(
        fs::read(target.target_path.join("unrelated.dll")).unwrap(),
        b"unrelated Mod"
    );
    assert!(
        read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))
            .unwrap()
            .packages
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn first_enable_registers_in_place_but_does_not_bypass_dependencies() {
    let (root, target) = fixture("first-enable");
    write_package(&target, "Consumer");
    let metadata = target.target_path.join("Consumer/manifest.json");
    fs::write(
        &metadata,
        br#"{"name":"Consumer","version_number":"2.0.0","dependencies":["Team-Missing-1.0.0"]}"#,
    )
    .unwrap();
    let original = fingerprint_tree(&target.target_path).unwrap();
    let error = manage_package(clone_target(&target), "Consumer", "enable").unwrap_err();
    assert!(error.contains("Team-Missing-1.0.0"), "{error}");
    assert_eq!(fingerprint_tree(&target.target_path).unwrap(), original);
    assert!(!target.target_path.join(ONLINE_MOD_MANIFEST).exists());

    // A broken existing install must still be disableable without a prior step.
    let disabled = manage_package(clone_target(&target), "Consumer", "disable").unwrap();
    let state = disabled.inventory.items[0].management.as_ref().unwrap();
    assert!(!state.enabled);
    assert_eq!(state.dependencies, ["Team-Missing-1.0.0"]);

    write_package(&target, "Independent");
    let original = fingerprint_tree(&target.target_path.join("Independent")).unwrap();
    manage_package(clone_target(&target), "Independent", "enable").unwrap();
    assert_eq!(
        fingerprint_tree(&target.target_path.join("Independent")).unwrap(),
        original
    );
    assert!(
        read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))
            .unwrap()
            .packages["local-Independent"]
            .enabled
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_first_action_rolls_back_files_without_leaving_ownership() {
    for action in ["enable", "disable", "delete"] {
        let (root, target) = fixture("first-action-rollback");
        write_package(&target, "Example");
        let original = fingerprint_tree(&target.target_path.join("Example")).unwrap();
        storage::fail_next_publication();
        let error = manage_package(clone_target(&target), "Example", action).unwrap_err();
        assert!(error.contains("rolled back"), "{action}: {error}");
        assert_eq!(
            fingerprint_tree(&target.target_path.join("Example")).unwrap(),
            original
        );
        assert!(
            read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))
                .unwrap()
                .packages
                .is_empty()
        );
        assert!(
            !ManagementPaths::new(&target.target_path)
                .unwrap()
                .journal
                .exists()
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn interrupted_first_action_recovers_files_and_final_ownership_together() {
    for action in ["disable", "delete"] {
        for moved in [false, true] {
            let (root, target) = fixture("first-action-recovery");
            write_package(&target, "Existing");
            manage_package(clone_target(&target), "Existing", "adopt").unwrap();
            write_package(&target, "Example");
            let paths = ManagementPaths::new(&target.target_path).unwrap();
            let mut original =
                read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
            let adopted = adopt_package(&target.target_path, "Example", &original).unwrap();
            let transaction = ManagementTransaction::new(
                "local-Example".into(),
                action,
                &mut original,
                Some(adopted),
            )
            .unwrap();
            let source = target.target_path.join("Example");
            let mut files = fingerprint_tree(&source).unwrap();
            let mut record = serde_json::to_value(transaction).unwrap();
            record["payload_files"] = serde_json::to_value(&files).unwrap();
            let destination = if action == "disable" {
                paths.disabled.join("Example")
            } else {
                paths
                    .root
                    .join("preserved")
                    .join(record["retained_name"].as_str().unwrap())
                    .join("Example")
            };
            fs::write(&paths.journal, serde_json::to_vec(&record).unwrap()).unwrap();
            if moved {
                fs::create_dir_all(destination.parent().unwrap()).unwrap();
                fs::rename(&source, &destination).unwrap();
            }

            recover_management(&target).unwrap();
            let recovered = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
            assert!(recovered.packages["local-Existing"].enabled);
            assert!(!source.exists());
            assert!(!paths.journal.exists());
            if action == "disable" {
                assert_eq!(recovered.packages.len(), 2);
                assert!(!recovered.packages["local-Example"].enabled);
                assert_eq!(fingerprint_tree(&destination).unwrap(), files);
                manage_package(clone_target(&target), "local-Example", "enable").unwrap();
                assert_eq!(fingerprint_tree(&source).unwrap(), files);
            } else {
                // First deletion has identical original/final ownership records.
                // Both pre-rename and post-rename recovery must still remove the
                // payload and preserve configuration outside the loading tree.
                assert_eq!(recovered.packages.len(), 1);
                files.remove("plugin.dll");
                assert_eq!(fingerprint_tree(&destination).unwrap(), files);
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn first_action_rejects_protected_or_non_mod_paths_without_recording_them() {
    let (root, target) = fixture("first-action-reject");
    write_package(&target, "0_TFP_Harmony");
    write_package(&target, "config");
    fs::create_dir(target.target_path.join("NotesOnly")).unwrap();
    fs::write(target.target_path.join("NotesOnly/readme.md"), b"notes").unwrap();
    let original = fingerprint_tree(&target.target_path).unwrap();
    for name in ["0_TFP_Harmony", "config", "NotesOnly", "../outside.dll"] {
        for action in ["enable", "disable", "delete"] {
            assert!(manage_package(clone_target(&target), name, action).is_err());
            assert_eq!(fingerprint_tree(&target.target_path).unwrap(), original);
            assert!(!target.target_path.join(ONLINE_MOD_MANIFEST).exists());
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_local_name_matching_another_package_key_cannot_select_the_wrong_mod() {
    let (root, target) = fixture("first-action-ambiguous");
    write_package(&target, "Other");
    manage_package(clone_target(&target), "Other", "adopt").unwrap();
    write_package(&target, "local-Other");
    let original = fingerprint_tree(&target.target_path).unwrap();
    for action in ["enable", "disable", "delete"] {
        let error = manage_package(clone_target(&target), "local-Other", action).unwrap_err();
        assert!(error.contains("another local path"), "{error}");
        assert_eq!(fingerprint_tree(&target.target_path).unwrap(), original);
    }
    // A target remains ambiguous after its own files have moved out of the
    // loading directory; a stale name must not select the other package key.
    manage_package(clone_target(&target), "local-Other", "adopt").unwrap();
    manage_package(clone_target(&target), "local-local-Other", "disable").unwrap();
    let original = fingerprint_tree(&target.target_path).unwrap();
    for action in ["enable", "disable", "delete"] {
        let error = manage_package(clone_target(&target), "local-Other", action).unwrap_err();
        assert!(error.contains("another local path"), "{error}");
        assert_eq!(fingerprint_tree(&target.target_path).unwrap(), original);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn first_delete_journal_validates_ownership_even_though_final_record_is_empty() {
    let (root, target) = fixture("first-delete-journal");
    write_package(&target, "Example");
    let mut original = InstalledPackages::default();
    let mut adopted = adopt_package(&target.target_path, "Example", &original).unwrap();
    adopted
        .files
        .insert("../outside.dll".into(), "0".repeat(64));
    assert!(
        ManagementTransaction::new(
            "local-Example".into(),
            "delete",
            &mut original,
            Some(adopted)
        )
        .is_err()
    );
    assert!(target.target_path.join("Example/plugin.dll").exists());
    assert!(!target.target_path.join(ONLINE_MOD_MANIFEST).exists());
    fs::remove_dir_all(root).unwrap();
}
