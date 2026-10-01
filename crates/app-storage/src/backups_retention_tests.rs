mod retention {
    use super::*;

    struct BackupFixture {
        root: PathBuf,
        record: StoredInstanceRecord,
    }

    impl BackupFixture {
        fn new(retention_count: u32) -> Self {
            let root = test_root("retention");
            let mut record = test_record(&root);
            record.backup_retention_count = retention_count;
            fs::create_dir_all(&record.saves_dir).unwrap();
            fs::write(record.saves_dir.join("world.db"), b"original").unwrap();
            Self { root, record }
        }

        fn create(&self, kind: InstanceBackupKind, timestamp: u128) -> InstanceBackupResult {
            let prefix = match kind {
                InstanceBackupKind::Manual => "saves",
                InstanceBackupKind::AutoStop => "auto-stop",
                InstanceBackupKind::PreRestore => "pre-restore",
            };
            let mut backup = create_instance_backup_with_prefix(
                &self.record,
                &self.record.saves_dir,
                &self.record.summary.id,
                prefix,
                kind,
            )
            .unwrap();
            // Make newest-first assertions independent of filesystem speed and clock granularity.
            backup.created_at_unix_ms = timestamp;
            write_instance_backup_manifest(Path::new(&backup.backup_path), &backup).unwrap();
            backup
        }

        fn prune(&self, preserved_ids: &[&str]) {
            prune_instance_backups(&self.record, &self.record.summary.id, preserved_ids).unwrap();
        }

        fn ids(&self) -> Vec<String> {
            collect_instance_backups(&self.record, &self.record.summary.id)
                .unwrap()
                .into_iter()
                .map(|backup| backup.backup_id)
                .collect()
        }

        fn cleanup(self) {
            fs::remove_dir_all(self.root).unwrap();
        }
    }

    fn check_created_backup_counts_toward_limit(kind: InstanceBackupKind) {
        for retention_count in [1, 3] {
            let fixture = BackupFixture::new(retention_count);
            let mut created_ids = Vec::new();
            for timestamp in 1..=5 {
                let backup = fixture.create(kind.clone(), timestamp);
                fixture.prune(&[&backup.backup_id]);
                created_ids.push(backup.backup_id);
                let expected_ids = created_ids
                    .iter()
                    .rev()
                    .take(retention_count as usize)
                    .cloned()
                    .collect::<Vec<_>>();
                assert_eq!(fixture.ids(), expected_ids);
                for removed_id in created_ids.iter().rev().skip(retention_count as usize) {
                    assert!(
                        !instance_backup_root(&fixture.record)
                            .join(removed_id)
                            .exists()
                    );
                }
            }
            fixture.cleanup();
        }
    }

    #[test]
    fn backup_retention_counts_new_manual_backup_toward_limit() {
        check_created_backup_counts_toward_limit(InstanceBackupKind::Manual);
    }

    #[test]
    fn backup_retention_counts_new_auto_stop_backup_toward_limit() {
        check_created_backup_counts_toward_limit(InstanceBackupKind::AutoStop);
    }

    fn check_restore_protects_source_and_safeguard(retention_count: u32) {
        let fixture = BackupFixture::new(retention_count);
        let source = fixture.create(InstanceBackupKind::Manual, 1);
        fixture.create(InstanceBackupKind::PreRestore, 2);
        fixture.create(InstanceBackupKind::AutoStop, 3);
        let latest = fixture.create(InstanceBackupKind::Manual, 4);
        fs::write(fixture.record.saves_dir.join("world.db"), b"before restore").unwrap();

        let restored = restore_instance_backup_transaction(
            &fixture.record,
            &fixture.record.saves_dir,
            &fixture.record.summary.id,
            &source.backup_id,
        )
        .unwrap();

        let mut expected_ids = vec![restored.safeguard_backup_id];
        if retention_count > 2 {
            expected_ids.push(latest.backup_id);
        }
        expected_ids.push(source.backup_id);
        assert_eq!(fixture.ids(), expected_ids);
        assert_eq!(
            fs::read(fixture.record.saves_dir.join("world.db")).unwrap(),
            b"original"
        );
        assert_eq!(
            fs::read(Path::new(&restored.safeguard_backup_path).join("saves/world.db")).unwrap(),
            b"before restore"
        );
        fixture.cleanup();
    }

    #[test]
    fn backup_retention_counts_restore_source_and_safeguard_toward_limit() {
        check_restore_protects_source_and_safeguard(3);
    }

    #[test]
    fn backup_retention_never_removes_restore_source_or_safeguard_for_small_limit() {
        for retention_count in [1, 2] {
            check_restore_protects_source_and_safeguard(retention_count);
        }
    }

    #[test]
    fn backup_retention_counts_only_existing_unique_protected_backups() {
        let fixture = BackupFixture::new(3);
        let source = fixture.create(InstanceBackupKind::Manual, 1);
        fixture.create(InstanceBackupKind::AutoStop, 2);
        let previous = fixture.create(InstanceBackupKind::PreRestore, 3);
        let latest = fixture.create(InstanceBackupKind::Manual, 4);

        fixture.prune(&[&source.backup_id, &source.backup_id, "missing-backup"]);

        assert_eq!(
            fixture.ids(),
            vec![latest.backup_id, previous.backup_id, source.backup_id]
        );
        fixture.cleanup();
    }

    #[test]
    fn backup_retention_without_protection_keeps_newest_backups_across_kinds() {
        let fixture = BackupFixture::new(3);
        fixture.create(InstanceBackupKind::Manual, 1);
        let safeguard = fixture.create(InstanceBackupKind::PreRestore, 2);
        let automatic = fixture.create(InstanceBackupKind::AutoStop, 3);
        let manual = fixture.create(InstanceBackupKind::Manual, 4);

        fixture.prune(&[]);

        assert_eq!(
            fixture.ids(),
            vec![manual.backup_id, automatic.backup_id, safeguard.backup_id]
        );
        fixture.cleanup();
    }
}
