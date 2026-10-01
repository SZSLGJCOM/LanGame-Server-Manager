use super::*;

struct SharedSaveFixture {
    root: PathBuf,
    paths: StoragePaths,
    original: InstanceDetails,
    files: [PathBuf; 3],
}

impl SharedSaveFixture {
    async fn new() -> Self {
        let root = unique_test_root();
        let mut paths = test_paths(&root);
        paths.modules_root = repo_root().join("modules");
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|descriptor| descriptor.summary.id == "arksurvivalascended")
            .unwrap();
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let install_root = paths
            .games_root
            .join(&descriptor.install.as_ref().unwrap().shared_game_dir);
        fs::create_dir_all(&install_root).unwrap();
        let created = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Shared save fixture".to_owned(),
                module_id: "arksurvivalascended".to_owned(),
            },
        )
        .await
        .unwrap();
        let original = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let config_path = PathBuf::from(&original.config_file_path);
        let config_root = config_path.parent().unwrap();
        let private_root = config_root.parent().unwrap().join("runtime");
        assert_eq!(
            crate::resolve_instance_private_runtime_root(config_root.parent().unwrap())
                .expect("created instance must have a valid private runtime marker"),
            private_root
        );
        let source_path = config_root.join("GameUserSettings.ini");
        let native_path =
            private_root.join("ShooterGame/Saved/Config/WindowsServer/GameUserSettings.ini");
        assert!(native_path.is_file());
        Self {
            root,
            paths,
            original,
            files: [config_path, source_path, native_path],
        }
    }

    fn update_input(&self) -> UpdateInstanceInput {
        let mut settings: Value = serde_json::from_str(&self.original.settings_json).unwrap();
        settings["server_name"] = json!("Changed fixture server");
        UpdateInstanceInput {
            id: self.original.summary.id.clone(),
            bind_ip: "192.0.2.10".to_owned(),
            auto_backup_on_stop: !self.original.auto_backup_on_stop,
            backup_retention_count: self.original.backup_retention_count + 1,
            settings_json: serde_json::to_string(&settings).unwrap(),
            ports: self.original.ports.clone(),
        }
    }

    fn snapshot(&self) -> [Vec<u8>; 3] {
        self.files.each_ref().map(|path| fs::read(path).unwrap())
    }

    async fn assert_unchanged(&self, before: &[Vec<u8>; 3]) {
        let actual = read_instance_details(&self.paths, &self.original.summary.id)
            .await
            .unwrap();
        assert_eq!(actual.summary.bind_ip, self.original.summary.bind_ip);
        assert_eq!(actual.summary.autostart, self.original.summary.autostart);
        assert_eq!(
            actual.auto_backup_on_stop,
            self.original.auto_backup_on_stop
        );
        assert_eq!(
            actual.backup_retention_count,
            self.original.backup_retention_count
        );
        for (path, expected) in self.files.iter().zip(before) {
            assert!(
                fs::read(path).unwrap() == *expected,
                "failed save changed {}",
                path.strip_prefix(&self.root).unwrap().display()
            );
        }
    }
}

impl Drop for SharedSaveFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

#[tokio::test]
async fn shared_save_instance_config_failure_keeps_files_and_database_unchanged() {
    let fixture = SharedSaveFixture::new().await;
    let before = fixture.snapshot();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.files[0]);
    let error = update_instance(&fixture.paths, fixture.update_input())
        .await
        .expect_err("injected instance config failure must reject the save");
    assert!(matches!(error, StorageError::WriteConfig { .. }));
    fixture.assert_unchanged(&before).await;
}

#[tokio::test]
async fn shared_save_commit_failure_keeps_files_and_database_unchanged() {
    let fixture = SharedSaveFixture::new().await;
    let before = fixture.snapshot();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    for statement in [
        "CREATE TABLE fixture_commit_parent (id INTEGER PRIMARY KEY)",
        "CREATE TABLE fixture_commit_child (parent_id INTEGER REFERENCES fixture_commit_parent(id) DEFERRABLE INITIALLY DEFERRED)",
        "CREATE TRIGGER fixture_reject_save_commit AFTER UPDATE OF bind_ip ON instances BEGIN INSERT INTO fixture_commit_child VALUES (1); END",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    pool.close().await;
    let error = update_instance(&fixture.paths, fixture.update_input())
        .await
        .expect_err("deferred foreign key violation must reject the commit");
    assert!(matches!(error, StorageError::Sqlx(_)));
    fixture.assert_unchanged(&before).await;
}

#[tokio::test]
async fn shared_save_second_native_write_failure_restores_earlier_configuration() {
    let fixture = SharedSaveFixture::new().await;
    let before = fixture.snapshot();
    let second_native = fixture.files[2].parent().unwrap().join("Game.ini");
    crate::atomic_file::fail_next_atomic_write_for_test(&second_native);
    let mut input = fixture.update_input();
    let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
    settings["use_singleplayer_settings"] = json!(true);
    // Render a different managed value in the second native INI as well.
    settings["harvest_amount_multiplier"] = json!(2.0);
    input.settings_json = serde_json::to_string(&settings).unwrap();
    let error = update_instance(&fixture.paths, input).await.unwrap_err();
    assert!(matches!(error, StorageError::WriteConfig { .. }));
    fixture.assert_unchanged(&before).await;
}

#[tokio::test]
async fn shared_save_materialize_write_failure_restores_source_and_runtime() {
    let fixture = SharedSaveFixture::new().await;
    fs::write(
        &fixture.files[1],
        b"[SessionSettings]\nSessionName=previous rendered source\n",
    )
    .unwrap();
    let before = fixture.snapshot();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.files[2]);
    // Make runtime publication necessary even when stored settings are unchanged.
    fs::write(
        &fixture.files[2],
        b"[SessionSettings]\nSessionName=operator runtime value\n",
    )
    .unwrap();
    let mut before = before;
    before[2] = fs::read(&fixture.files[2]).unwrap();
    let error = materialize_instance_configuration(&fixture.paths, &fixture.original.summary.id)
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::WriteConfig { .. }));
    fixture.assert_unchanged(&before).await;
}

fn configuration_fixture_snapshot(
    root: &Path,
    paths: &[PathBuf],
) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else if entry.file_type().unwrap().is_file() {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    for path in paths {
        visit(root, path, &mut files);
    }
    files
}

#[tokio::test]
async fn shared_save_repository_modules_keep_configuration_on_commit_failure() {
    let descriptors = app_modules::discover_modules(repo_root().join("modules")).unwrap();
    assert_eq!(descriptors.len(), 32);
    for descriptor in descriptors {
        let root = unique_test_root();
        let mut paths = test_paths(&root);
        paths.modules_root = repo_root().join("modules");
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let install = paths
            .games_root
            .join(&descriptor.install.as_ref().unwrap().shared_game_dir);
        replenish_test_library(&paths, &descriptor).await;
        if descriptor.summary.id == "palworld" {
            fs::create_dir_all(install.join("Pal")).unwrap();
        }
        let created = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Module transaction fixture".to_owned(),
                module_id: descriptor.summary.id.clone(),
            },
        )
        .await
        .unwrap_or_else(|error| {
            panic!("{} fixture creation failed: {error}", descriptor.summary.id)
        });
        let original = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let instance_root = PathBuf::from(&original.config_file_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        let library_remains = install.exists();
        let mut snapshot_roots = vec![instance_root];
        if library_remains {
            snapshot_roots.push(install.clone());
        }
        let before = configuration_fixture_snapshot(&root, &snapshot_roots);
        let pool = connect_pool(&paths).await.unwrap();
        for statement in [
            "CREATE TABLE fixture_commit_parent (id INTEGER PRIMARY KEY)",
            "CREATE TABLE fixture_commit_child (parent_id INTEGER REFERENCES fixture_commit_parent(id) DEFERRABLE INITIALLY DEFERRED)",
            "CREATE TRIGGER fixture_reject_save_commit AFTER UPDATE OF bind_ip ON instances BEGIN INSERT INTO fixture_commit_child VALUES (1); END",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        pool.close().await;
        let mut proposed_settings: Value = serde_json::from_str(&original.settings_json).unwrap();
        if descriptor.summary.id == "runescapedragonwilds" {
            proposed_settings["owner_id"] = json!("76561198000000001");
        }
        let error = update_instance(
            &paths,
            UpdateInstanceInput {
                id: original.summary.id.clone(),
                bind_ip: "192.0.2.10".to_owned(),
                auto_backup_on_stop: !original.auto_backup_on_stop,
                backup_retention_count: original.backup_retention_count + 1,
                settings_json: serde_json::to_string(&proposed_settings).unwrap(),
                ports: original.ports.clone(),
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, StorageError::Sqlx(_)),
            "{}: {error}",
            descriptor.summary.id
        );
        let after = configuration_fixture_snapshot(&root, &snapshot_roots);
        assert_eq!(
            install.exists(),
            library_remains,
            "{} recreated or removed its library during a failed save",
            descriptor.summary.id
        );
        assert!(
            before == after,
            "{} changed configuration after a failed commit",
            descriptor.summary.id
        );
        let actual = read_instance_details(&paths, &original.summary.id)
            .await
            .unwrap();
        assert_eq!(actual.summary.bind_ip, original.summary.bind_ip);
        assert_eq!(actual.summary.autostart, original.summary.autostart);
        assert_eq!(actual.auto_backup_on_stop, original.auto_backup_on_stop);
        assert_eq!(
            actual.backup_retention_count,
            original.backup_retention_count
        );
        cleanup_root(&root);
    }
}
