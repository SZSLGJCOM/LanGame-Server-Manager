#[test]
fn backup_restore_interruption_child() {
    let Some(root) = std::env::var_os("LANGAME_BACKUP_RESTORE_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let source = PathBuf::from(std::env::var_os("LANGAME_BACKUP_RESTORE_CRASH_SOURCE").unwrap());
    let target = std::env::var_os("LANGAME_BACKUP_RESTORE_CRASH_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("instance/saves"));
    let stage = std::env::var("LANGAME_BACKUP_RESTORE_CRASH_STAGE").unwrap();
    replace_directory_from_source_with_publish(&source, &target, &mut |_, _| Ok(()), |from, to| {
        if stage == "published" {
            publish_new_directory(from, to)?;
        }
        // Termination deliberately skips destructors after the live directory
        // was displaced, exactly as a service crash between the two renames.
        std::process::exit(73)
    })
    .unwrap();
    panic!("the interruption probe did not terminate at publication");
}

#[test]
fn backup_restore_recovers_interruption_before_safeguarding_current_world() {
    for stage in ["displaced", "published"] {
        let root = test_root("restore-process-interruption");
        let record = test_record(&root);
        fs::create_dir_all(&record.saves_dir).unwrap();
        fs::write(record.saves_dir.join("world.db"), b"earlier world").unwrap();
        let backup = create_instance_backup_with_prefix(
            &record,
            &record.saves_dir,
            &record.summary.id,
            "saves",
            InstanceBackupKind::Manual,
        )
        .unwrap();
        fs::write(record.saves_dir.join("world.db"), b"current world").unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "backups::tests::backup_restore_interruption_child",
                "--nocapture",
            ])
            .env("LANGAME_BACKUP_RESTORE_CRASH_ROOT", &root)
            .env(
                "LANGAME_BACKUP_RESTORE_CRASH_SOURCE",
                Path::new(&backup.backup_path).join("saves"),
            )
            .env("LANGAME_BACKUP_RESTORE_CRASH_STAGE", stage)
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(73),
            "{}",
            String::from_utf8_lossy(&child.stdout)
        );
        let restored = restore_instance_backup_transaction(
            &record,
            &record.saves_dir,
            &record.summary.id,
            &backup.backup_id,
        )
        .unwrap();
        assert_eq!(
            fs::read(record.saves_dir.join("world.db")).unwrap(),
            b"earlier world"
        );
        assert_eq!(
            fs::read(Path::new(&restored.safeguard_backup_path).join("saves/world.db")).unwrap(),
            b"current world",
            "retry after {stage} must preserve the pre-interruption world, not an empty or partially restored generation"
        );
        assert_eq!(
            fs::read(Path::new(&backup.backup_path).join("saves/world.db")).unwrap(),
            b"earlier world"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn completed_backup_restore_preserves_changed_cleanup_copies_and_allows_startup() {
    for mutation in ["changed", "partially-cleaned", "cleanup-depth"] {
        let root = test_root("restore-completed-cleanup");
        let record = test_record(&root);
        fs::create_dir_all(&record.saves_dir).unwrap();
        fs::write(record.saves_dir.join("world.db"), b"current world").unwrap();
        fs::write(record.saves_dir.join("players.db"), b"current players").unwrap();
        if mutation == "cleanup-depth" {
            let mut path = record.saves_dir.clone();
            for _ in 0..129 {
                path = path.join("d");
            }
            fs::create_dir_all(path).unwrap();
        }
        let parent = record.saves_dir.parent().unwrap();
        let staging = create_unique_directory(parent, ".restore-publish").unwrap();
        fs::write(staging.join("world.db"), b"restored world").unwrap();
        let rollback = unique_available_path(parent, ".restore-rollback");
        let mut journal =
            recovery::Journal::prepare(&record.saves_dir, &staging, &rollback).unwrap();
        journal.begin().unwrap();
        move_directory(&record.saves_dir, &rollback).unwrap();
        publish_new_directory(&staging, &record.saves_dir).unwrap();
        journal.commit().unwrap();
        // The process ended after commit, before or during old-copy cleanup.
        if mutation == "changed" {
            fs::write(rollback.join("world.db"), b"repaired recovery world").unwrap();
        } else if mutation == "partially-cleaned" {
            fs::remove_file(rollback.join("world.db")).unwrap();
        }
        let before = crate::test_file_snapshot::tree_snapshot(&rollback).unwrap();
        let error = recovery::recover(
            &record.saves_dir,
            &publication::Publication::uncoordinated(),
        )
        .expect_err("incomplete cleanup must report the retained recovery copy");
        assert!(error.to_string().contains("already completed"), "{error}");
        assert!(error.to_string().contains("cleanup"), "{error}");
        if mutation == "cleanup-depth" {
            assert!(
                error.to_string().contains("cleanup traversal limit"),
                "{error}"
            );
        }
        assert_eq!(
            fs::read(record.saves_dir.join("world.db")).unwrap(),
            b"restored world"
        );
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&rollback).unwrap(),
            before
        );
        recovery::ensure_record_ready(&record, &record.saves_dir)
            .expect("a verified completed restore must remain available despite leftover cleanup");
        recovery::recover(
            &record.saves_dir,
            &publication::Publication::uncoordinated(),
        )
        .unwrap();
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&rollback).unwrap(),
            before
        );
        fs::remove_dir_all(root).unwrap();
    }
}
