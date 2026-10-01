fn checked_restore_fixture(
    label: &str,
) -> (PathBuf, StoredInstanceRecord, PreparedInstanceBackupRestore) {
    let root = test_root(label);
    let record = test_record(&root);
    fs::create_dir_all(&record.saves_dir).unwrap();
    fs::write(record.saves_dir.join("world.db"), b"earlier save").unwrap();
    let backup = create_instance_backup_with_prefix(
        &record,
        &record.saves_dir,
        &record.summary.id,
        "saves",
        InstanceBackupKind::Manual,
    )
    .unwrap();
    fs::write(record.saves_dir.join("world.db"), b"current save").unwrap();
    let prepared = PreparedInstanceBackupRestore {
        dst_settings_sha256: None,
        source_sha256: backup_tree_sha256(&Path::new(&backup.backup_path).join("saves")).unwrap(),
        target_sha256: backup_tree_sha256(&record.saves_dir).unwrap(),
        target_path: record.saves_dir.clone(),
        backup,
    };
    (root, record, prepared)
}

#[test]
fn checked_restore_rejects_changed_backup_even_when_its_size_is_unchanged() {
    let (root, record, prepared) = checked_restore_fixture("checked-source");
    fs::write(
        Path::new(&prepared.backup.backup_path).join("saves/world.db"),
        b"changed save",
    )
    .unwrap();
    let result = restore_instance_backup_transaction_checked(
        &record,
        &record.saves_dir,
        &record.summary.id,
        &prepared.backup.backup_id,
        Some(&prepared),
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read(record.saves_dir.join("world.db")).unwrap(),
        b"current save"
    );
    assert_eq!(
        collect_instance_backups(&record, &record.summary.id)
            .unwrap()
            .len(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_restore_rejects_new_current_saves_and_a_different_backup_identity() {
    let (root, record, prepared) = checked_restore_fixture("checked-target");
    fs::write(record.saves_dir.join("world.db"), b"latest world").unwrap();
    assert!(
        restore_instance_backup_transaction_checked(
            &record,
            &record.saves_dir,
            &record.summary.id,
            &prepared.backup.backup_id,
            Some(&prepared)
        )
        .is_err()
    );
    assert_eq!(
        fs::read(record.saves_dir.join("world.db")).unwrap(),
        b"latest world"
    );
    let mut changed = prepared.backup.clone();
    changed.instance_id = "another-instance".into();
    assert!(
        prepared
            .validate(
                &changed,
                &record.saves_dir,
                &Path::new(&prepared.backup.backup_path).join("saves")
            )
            .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_restore_preserves_safeguard_and_reads_back_exact_source_content() {
    let (root, record, prepared) = checked_restore_fixture("checked-success");
    let result = restore_instance_backup_transaction_checked(
        &record,
        &record.saves_dir,
        &record.summary.id,
        &prepared.backup.backup_id,
        Some(&prepared),
    )
    .unwrap();
    assert_eq!(
        fs::read(record.saves_dir.join("world.db")).unwrap(),
        b"earlier save"
    );
    assert_eq!(
        backup_tree_sha256(&record.saves_dir).unwrap(),
        prepared.source_sha256
    );
    assert_eq!(
        fs::read(Path::new(&result.safeguard_backup_path).join("saves/world.db")).unwrap(),
        b"current save"
    );
    assert_eq!(result.backup_id, prepared.backup.backup_id);
    assert_eq!(result.instance_id, record.summary.id);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_restore_refuses_changed_staging_before_replacing_current_saves() {
    let (root, record, prepared) = checked_restore_fixture("checked-copy");
    let source = Path::new(&prepared.backup.backup_path).join("saves");
    let error = replace_directory_from_source_checked(
        &source,
        &record.saves_dir,
        &mut |source, _| {
            fs::write(source, b"modified copy")?;
            Ok(())
        },
        publish_new_directory,
        |staging, target| prepared.validate_contents(staging, target),
    );
    assert!(error.is_err());
    assert_eq!(
        fs::read(record.saves_dir.join("world.db")).unwrap(),
        b"current save"
    );
    assert!(
        !fs::read_dir(record.saves_dir.parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".restore-publish"))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn backup_confirmation_hash_tracks_names_and_empty_directories() {
    let (root, record, _) = checked_restore_fixture("checked-tree");
    let original = backup_tree_sha256(&record.saves_dir).unwrap();
    fs::create_dir(record.saves_dir.join("empty")).unwrap();
    assert_ne!(backup_tree_sha256(&record.saves_dir).unwrap(), original);
    fs::remove_dir(record.saves_dir.join("empty")).unwrap();
    fs::rename(
        record.saves_dir.join("world.db"),
        record.saves_dir.join("renamed.db"),
    )
    .unwrap();
    assert_ne!(backup_tree_sha256(&record.saves_dir).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn historical_ark_primary_backup_restores_without_replacing_sibling_maps() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let root = test_root(edition);
        let mut record = test_record(&root);
        record.summary.module_id = edition.into();
        let saved = root.join("runtime/ShooterGame/Saved");
        let primary = saved.join(&record.summary.id);
        let sibling = saved.join(format!("{}-map-desert", record.summary.id));
        fs::create_dir_all(&primary).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        fs::write(primary.join("TheIsland.ark"), b"old primary").unwrap();
        fs::write(sibling.join("ScorchedEarth.ark"), b"live sibling").unwrap();
        let backup = create_instance_backup_with_prefix(
            &record,
            &primary,
            &record.summary.id,
            "saves",
            InstanceBackupKind::Manual,
        )
        .unwrap();
        fs::write(primary.join("TheIsland.ark"), b"current primary").unwrap();
        let target = backup_restore_scope(&record, &saved, &backup);
        assert_eq!(target, primary);
        let prepared = PreparedInstanceBackupRestore {
            dst_settings_sha256: None,
            source_sha256: backup_tree_sha256(&Path::new(&backup.backup_path).join("saves")).unwrap(),
            target_sha256: backup_tree_sha256(&target).unwrap(),
            target_path: target,
            backup,
        };
        let result = restore_instance_backup_transaction_checked(
            &record,
            &saved,
            &record.summary.id,
            &prepared.backup.backup_id,
            Some(&prepared),
        )
        .unwrap();
        assert_eq!(fs::read(primary.join("TheIsland.ark")).unwrap(), b"old primary");
        assert_eq!(
            fs::read(sibling.join("ScorchedEarth.ark")).unwrap(),
            b"live sibling"
        );
        assert_eq!(
            fs::read(Path::new(&result.safeguard_backup_path).join("saves/TheIsland.ark"))
                .unwrap(),
            b"current primary"
        );
        assert!(!saved.join("TheIsland.ark").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
