struct RestoreIsolationFixture {
    root: PathBuf,
    paths: StoragePaths,
    instances: Vec<InstanceProvisioning>,
    shared: PathBuf,
}

impl RestoreIsolationFixture {
    async fn new() -> Self {
        let root = test_root("restore-isolation");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/lgs.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: root.join("modules"),
            migrations_root: repository.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        crate::initialize_database(&paths).await.unwrap();
        let module_root = paths.modules_root.join("conanexiles");
        fs::create_dir_all(&module_root).unwrap();
        copy_directory_contents_with_hook(
            &repository.join("modules/conanexiles"),
            &module_root,
            &mut |_, _| Ok(()),
        )
        .unwrap();
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|descriptor| descriptor.summary.id == "conanexiles")
            .unwrap();
        crate::sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let shared = paths.games_root.join("conanexiles");
        let mut instances = Vec::new();
        for name in ["Restore Alpha", "Restore Beta"] {
            // The installer supplies a fresh package after the previous instance
            // takes ownership of its download. Never seed from its mutable data.
            prepare_restore_fixture_download(&shared);
            instances.push(
                crate::create_instance(
                    &paths,
                    &descriptor,
                    CreateInstanceInput {
                        name: name.to_owned(),
                        module_id: "conanexiles".to_owned(),
                    },
                )
                .await
                .unwrap(),
            );
        }
        // An unrelated subsequent download must also survive a backup restore.
        prepare_restore_fixture_download(&shared);
        Self {
            root,
            paths,
            instances,
            shared,
        }
    }

    fn point_saves_at_same_directory(&self) -> PathBuf {
        let saves = self.root.join("conflicting-saves");
        let manifest = self.paths.modules_root.join("conanexiles/module.toml");
        let before = fs::read_to_string(&manifest).unwrap();
        let after = before.replace(
            "saves_path_template = \"{{paths.install_root}}/ConanSandbox/Saved\"",
            &format!(
                "saves_path_template = {:?}",
                saves.to_string_lossy().replace('\\', "/")
            ),
        );
        assert_ne!(
            before, after,
            "fixture must redirect the declared save boundary"
        );
        fs::write(manifest, after).unwrap();
        saves
    }
}

fn prepare_restore_fixture_download(root: &Path) {
    if !root.exists() {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join("package.fixture"), b"synthetic package").unwrap();
    }
}

#[tokio::test]
async fn restore_rejects_overlapping_save_ownership_before_changing_files() {
    let fixture = RestoreIsolationFixture::new().await;
    let saves = fixture.point_saves_at_same_directory();
    let first_id = &fixture.instances[0].summary.id;
    let second_id = &fixture.instances[1].summary.id;
    fs::create_dir_all(&saves).unwrap();
    let world = saves.join("game.db");
    fs::write(&world, b"backed-up shared world").unwrap();
    let backup = create_instance_backup(&fixture.paths, first_id)
        .await
        .unwrap();
    fs::write(&world, b"latest shared world must survive\0\x01").unwrap();
    let before = crate::test_file_snapshot::tree_snapshot(&saves).unwrap();

    let error = restore_instance_backup(&fixture.paths, first_id, &backup.backup_id)
        .await
        .expect_err("restoring shared saves must not replace the peer's world");
    assert!(
        matches!(&error, StorageError::InstancePathConflict {
            kind, other_instance_id, ..
        } if kind == "saves" && other_instance_id == second_id),
        "unexpected restore error: {error}"
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&saves).unwrap(),
        before
    );
    assert_eq!(
        fs::read(&world).unwrap(),
        b"latest shared world must survive\0\x01"
    );
    assert_eq!(
        list_instance_backups(&fixture.paths, first_id)
            .await
            .unwrap()
            .len(),
        1
    );
    let lease = acquire_instance_settings_mutation_lock(&fixture.paths, first_id)
        .expect("rejected restore must release its mutation lease");
    drop(lease);
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    tx.rollback().await.unwrap();
    pool.close().await;
    fs::remove_dir_all(fixture.root).unwrap();
}

#[tokio::test]
async fn private_restore_preserves_peer_and_retains_pre_restore_world() {
    let fixture = RestoreIsolationFixture::new().await;
    let first = crate::read_instance_details(&fixture.paths, &fixture.instances[0].summary.id)
        .await
        .unwrap();
    let second = crate::read_instance_details(&fixture.paths, &fixture.instances[1].summary.id)
        .await
        .unwrap();
    assert_ne!(first.saves_path, second.saves_path);
    let world = Path::new(&first.saves_path).join("game.db");
    let peer_world = Path::new(&second.saves_path).join("game.db");
    fs::write(&world, b"alpha backup\0\x01").unwrap();
    fs::write(&peer_world, b"beta world\0\x02").unwrap();
    let peer_root = Path::new(&second.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let peer_before = crate::test_file_snapshot::tree_snapshot(peer_root).unwrap();
    let shared_before = crate::test_file_snapshot::tree_snapshot(&fixture.shared).unwrap();
    let backup = create_instance_backup(&fixture.paths, &first.summary.id)
        .await
        .unwrap();
    fs::write(&world, b"alpha before restore").unwrap();

    let restored = restore_instance_backup(&fixture.paths, &first.summary.id, &backup.backup_id)
        .await
        .unwrap();
    assert_eq!(fs::read(&world).unwrap(), b"alpha backup\0\x01");
    assert_eq!(
        fs::read(Path::new(&restored.safeguard_backup_path).join("saves/game.db")).unwrap(),
        b"alpha before restore"
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(peer_root).unwrap(),
        peer_before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.shared).unwrap(),
        shared_before
    );
    fs::remove_dir_all(fixture.root).unwrap();
}
