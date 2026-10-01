use super::*;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-file-set-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("data/plugins")).unwrap();
        for file in ["first.txt", "second.txt"] {
            fs::write(
                root.join("data/plugins").join(file),
                b"first = 1\r\nsecond = 2\r\n",
            )
            .unwrap();
        }
        Self { root }
    }

    fn edits(&self) -> Vec<InstanceFileEdits> {
        ["first.txt", "second.txt"]
            .into_iter()
            .map(|name| {
                let file = format!("data/plugins/{name}");
                InstanceFileEdits {
                    source_sha256: read_file(&self.root, &file).unwrap().source_sha256,
                    file,
                    edits: vec![InstanceTextEdit {
                        before: "first = 1".into(),
                        after: "first = 3".into(),
                    }],
                }
            })
            .collect()
    }

    fn prepare(&self) -> PreparedInstanceFilePatches {
        prepare_patches(&self.root, "target", self.edits()).unwrap()
    }

    fn read(&self, index: usize) -> Vec<u8> {
        fs::read(self.root.join(if index == 0 {
            "data/plugins/first.txt"
        } else {
            "data/plugins/second.txt"
        }))
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn multi_file_success_preserves_unedited_bytes_and_backups() {
    let fixture = Fixture::new();
    let mut edits = fixture.edits();
    edits[0].edits.push(InstanceTextEdit {
        before: "second = 2".into(),
        after: "second = 4".into(),
    });
    let prepared = prepare_patches(&fixture.root, "target", edits).unwrap();
    assert_eq!(prepared.previews()[0].edits.len(), 2);
    let result = apply_patches(&prepared).unwrap();
    assert_eq!(result.status, InstanceFilePatchesStatus::Applied);
    assert_eq!(fixture.read(0), b"first = 3\r\nsecond = 4\r\n");
    assert_eq!(fixture.read(1), b"first = 3\r\nsecond = 2\r\n");
    for outcome in result.files {
        assert_eq!(outcome.state, InstanceFilePatchState::Applied);
        assert!(outcome.read_back_verified);
        let backup = fixture
            .root
            .join("data/.langame/file-patches")
            .join(outcome.backup_id.unwrap());
        assert_eq!(
            fs::read(backup.join("original")).unwrap(),
            b"first = 1\r\nsecond = 2\r\n"
        );
    }
}

#[test]
fn stale_second_source_fails_before_first_write_or_backup() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    fs::write(
        fixture.root.join("data/plugins/second.txt"),
        b"external edit",
    )
    .unwrap();
    assert!(
        apply_patches(&prepared)
            .unwrap_err()
            .to_string()
            .contains("source changed")
    );
    assert_eq!(fixture.read(0), prepared.patches[0].original);
    assert_eq!(fixture.read(1), b"external edit");
    assert!(!fixture.root.join("data/.langame/file-patches").exists());
}

#[test]
fn overlapping_duplicate_missing_and_oversized_edits_fail_preparation() {
    let fixture = Fixture::new();
    let mut overlap = fixture.edits();
    overlap[0].edits.push(InstanceTextEdit {
        before: "first".into(),
        after: "changed".into(),
    });
    assert!(
        prepare_patches(&fixture.root, "target", overlap)
            .unwrap_err()
            .to_string()
            .contains("overlap")
    );
    let mut duplicate = fixture.edits();
    duplicate[1].file = duplicate[0].file.to_ascii_uppercase();
    assert!(
        validate_instance_file_edits(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("only once")
    );
    let mut missing = fixture.edits();
    missing[0].edits[0].before = "missing".into();
    assert!(prepare_patches(&fixture.root, "target", missing).is_err());
    let mut excessive = fixture.edits();
    excessive[0].edits[0].after = "x".repeat(MAX_PATCH_BYTES);
    assert!(validate_instance_file_edits(&excessive).is_err());
    assert!(validate_instance_file_edits(&[]).is_err());
    assert!(
        validate_instance_file_edits(&vec![fixture.edits()[0].clone(); MAX_FILES + 1]).is_err()
    );
    let mut protected = fixture.edits();
    protected[1].file = "config/server.json".into();
    assert!(prepare_patches(&fixture.root, "target", protected).is_err());
    assert_eq!(fixture.read(0), b"first = 1\r\nsecond = 2\r\n");
}

#[test]
fn edits_match_the_original_document_without_cascading() {
    let fixture = Fixture::new();
    let mut edits = fixture.edits();
    edits.truncate(1);
    edits[0].edits[0].after = "second = 2".into();
    edits[0].edits.push(InstanceTextEdit {
        before: "second = 2".into(),
        after: "second = 9".into(),
    });
    apply_patches(&prepare_patches(&fixture.root, "target", edits).unwrap()).unwrap();
    assert_eq!(fixture.read(0), b"second = 2\r\nsecond = 9\r\n");
}

#[test]
fn second_write_failure_rolls_back_first_and_retains_all_backups() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    crate::atomic_file::fail_next_atomic_write_for_test(
        &fixture.root.join("data/plugins/second.txt"),
    );
    let result = apply_patches(&prepared).unwrap();
    assert_eq!(result.status, InstanceFilePatchesStatus::RolledBack);
    assert_eq!(result.files[0].state, InstanceFilePatchState::RolledBack);
    assert_eq!(result.files[1].state, InstanceFilePatchState::NotApplied);
    assert!(
        result
            .files
            .iter()
            .all(|file| file.backup_id.is_some() && file.read_back_verified)
    );
    for index in 0..2 {
        assert_eq!(fixture.read(index), prepared.patches[index].original);
    }
}

#[test]
fn rollback_does_not_overwrite_external_edits_and_reports_partial_recovery() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let result = writes::apply_with_test_hook(&prepared, |index| {
        if index == 1 {
            fs::write(
                fixture.root.join("data/plugins/first.txt"),
                b"external change",
            )
            .unwrap();
            return Err(invalid(
                Path::new("second.txt"),
                "injected failure after external edit",
            ));
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(result.status, InstanceFilePatchesStatus::Partial);
    assert_eq!(
        result.files[0].state,
        InstanceFilePatchState::RecoveryRequired
    );
    assert!(!result.files[0].read_back_verified);
    assert!(result.files[0].backup_id.is_some());
    assert_eq!(fixture.read(0), b"external change");
    assert_eq!(fixture.read(1), prepared.patches[1].original);
}

#[test]
fn rollback_write_failure_reports_recovery_and_keeps_replacement() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let result = writes::apply_with_test_hook(&prepared, |index| {
        if index == 1 {
            crate::atomic_file::fail_next_atomic_write_for_test(
                &fixture.root.join("data/plugins/first.txt"),
            );
            return Err(invalid(
                Path::new("second.txt"),
                "injected second-file failure",
            ));
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(result.status, InstanceFilePatchesStatus::Partial);
    assert_eq!(
        result.files[0].state,
        InstanceFilePatchState::RecoveryRequired
    );
    assert_eq!(fixture.read(0), prepared.patches[0].replacement);
}

#[test]
fn final_set_verification_detects_a_changed_earlier_file() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let result = writes::apply_with_test_hook(&prepared, |index| {
        if index == 1 {
            fs::write(
                fixture.root.join("data/plugins/first.txt"),
                b"external change",
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(result.status, InstanceFilePatchesStatus::Partial);
    assert_eq!(
        result.files[0].state,
        InstanceFilePatchState::RecoveryRequired
    );
    assert_eq!(result.files[1].state, InstanceFilePatchState::RolledBack);
    assert_eq!(fixture.read(0), b"external change");
    assert_eq!(fixture.read(1), prepared.patches[1].original);
}

#[test]
fn backup_capacity_failure_happens_before_any_file_write() {
    let fixture = Fixture::new();
    let parent = fixture.root.join("data/.langame/file-patches");
    fs::create_dir_all(&parent).unwrap();
    for index in 0..63 {
        fs::write(parent.join(format!("retained-{index}")), b"retained").unwrap();
    }
    let prepared = fixture.prepare();
    let result = apply_patches(&prepared).unwrap();
    assert_eq!(result.status, InstanceFilePatchesStatus::NotApplied);
    assert!(result.files[0].backup_id.is_some());
    assert!(result.files[1].backup_id.is_none());
    for index in 0..2 {
        assert_eq!(fixture.read(index), prepared.patches[index].original);
    }
}

#[tokio::test]
async fn batch_public_api_rechecks_instance_binding_running_state_and_mutation_lease() {
    let fixture = Fixture::new();
    let paths = StoragePaths {
        app_data_root: fixture.root.join("appdata"),
        settings_path: fixture.root.join("appdata/settings.json"),
        database_path: fixture.root.join("appdata/db/lgs.db"),
        logs_root: fixture.root.join("logs"),
        modules_root: fixture.root.join("modules"),
        migrations_root: fixture.root.join("migrations"),
        steamcmd_root: fixture.root.join("steamcmd"),
        games_root: fixture.root.join("games"),
        instances_root: fixture.root.join("instances"),
        archives_root: fixture.root.join("instances").join(".trash"),
    };
    let root = paths.instances_root.join("target");
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::rename(fixture.root.join("data"), root.join("data")).unwrap();
    crate::initialize_database(&paths).await.unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("INSERT INTO modules (id,name,version) VALUES ('generic','Generic','1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO instances (id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES ('target','Target','generic',?1,?2,?3,?4)")
        .bind(root.join("data").to_string_lossy().as_ref()).bind(root.join("config").to_string_lossy().as_ref())
        .bind(root.join("logs").to_string_lossy().as_ref()).bind(root.join("saves").to_string_lossy().as_ref())
        .execute(&pool).await.unwrap();
    let files = ["first.txt", "second.txt"]
        .into_iter()
        .map(|name| {
            let file = format!("data/plugins/{name}");
            InstanceFileEdits {
                source_sha256: read_file(&root, &file).unwrap().source_sha256,
                file,
                edits: vec![InstanceTextEdit {
                    before: "first = 1".into(),
                    after: "first = 3".into(),
                }],
            }
        })
        .collect();
    let prepared = prepare_instance_file_patches(&paths, "target", files)
        .await
        .unwrap();
    sqlx::query("UPDATE instances SET status='running' WHERE id='target'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        apply_instance_file_patches(&paths, "target", prepared.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("Stop the instance")
    );
    sqlx::query("UPDATE instances SET status='stopped' WHERE id='target'")
        .execute(&pool)
        .await
        .unwrap();
    let lease = acquire_instance_settings_mutation_lock(&paths, "target").unwrap();
    assert!(matches!(
        apply_instance_file_patches(&paths, "target", prepared.clone()).await,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    drop(lease);
    let mut mismatched = prepared.clone();
    mismatched.patches[1].instance_id = "other".into();
    assert!(
        apply_instance_file_patches(&paths, "target", mismatched)
            .await
            .is_err()
    );
    assert_eq!(
        apply_instance_file_patches(&paths, "target", prepared)
            .await
            .unwrap()
            .status,
        InstanceFilePatchesStatus::Applied
    );
    pool.close().await;
}
