use super::*;

struct AutostartFixture {
    root: PathBuf,
    paths: StoragePaths,
    details: InstanceDetails,
}

impl AutostartFixture {
    async fn new() -> Self {
        let root = unique_test_root();
        let paths = test_paths(&root);
        let descriptor = test_descriptor(&root);
        prepare_environment(&root, &descriptor);
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let created = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Autostart fixture".to_owned(),
                module_id: descriptor.summary.id.clone(),
            },
        )
        .await
        .unwrap();
        let details = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        Self {
            root,
            paths,
            details,
        }
    }

    fn configuration(&self) -> Vec<u8> {
        fs::read(&self.details.config_file_path).unwrap()
    }

    async fn assert_unchanged(&self, original_config: &[u8]) {
        assert_eq!(self.configuration(), original_config);
        let actual = read_instance_details(&self.paths, &self.details.summary.id)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(&self.details).unwrap()
        );
    }
}

impl Drop for AutostartFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

#[tokio::test]
async fn instance_autostart_updates_only_policy_and_manager_mirror() {
    let fixture = AutostartFixture::new().await;
    let config_path = Path::new(&fixture.details.config_file_path);
    let native_path = config_path.parent().unwrap().join("cluster.ini");
    let native_content = b"[NETWORK]\ncustom_operator_value = preserve\n";
    fs::write(&native_path, native_content).unwrap();
    let mut original: Value = serde_json::from_slice(&fixture.configuration()).unwrap();
    original["operator_metadata"] = json!({ "preserve": [1, 2, 3] });
    fs::write(config_path, serde_json::to_vec(&original).unwrap()).unwrap();

    for autostart in [true, false] {
        let updated =
            update_instance_autostart(&fixture.paths, &fixture.details.summary.id, autostart)
                .await
                .unwrap();
        let mut expected = serde_json::to_value(&fixture.details).unwrap();
        expected["summary"]["autostart"] = json!(autostart);
        assert_eq!(serde_json::to_value(&updated).unwrap(), expected);
        original["autostart"] = json!(autostart);
        assert_eq!(
            serde_json::from_slice::<Value>(&fixture.configuration()).unwrap(),
            original
        );
        assert_eq!(fs::read(&native_path).unwrap(), native_content);
        let reloaded = read_instance_details(&fixture.paths, &fixture.details.summary.id)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(&reloaded).unwrap(), expected);
    }
}

#[tokio::test]
async fn instance_autostart_survives_settings_save_from_an_older_details_snapshot() {
    let fixture = AutostartFixture::new().await;
    let mut settings: Value = serde_json::from_str(&fixture.details.settings_json).unwrap();
    settings["cluster_name"] = json!("Saved from an older configuration view");
    let stale_input = UpdateInstanceInput {
        id: fixture.details.summary.id.clone(),
        bind_ip: fixture.details.summary.bind_ip.clone(),
        auto_backup_on_stop: fixture.details.auto_backup_on_stop,
        backup_retention_count: fixture.details.backup_retention_count,
        settings_json: serde_json::to_string(&settings).unwrap(),
        ports: fixture.details.ports.clone(),
    };
    update_instance_autostart(&fixture.paths, &fixture.details.summary.id, true)
        .await
        .unwrap();

    let saved = update_instance(&fixture.paths, stale_input).await.unwrap();
    assert!(saved.summary.autostart);
    assert_eq!(
        serde_json::from_str::<Value>(&saved.settings_json).unwrap()["cluster_name"],
        "Saved from an older configuration view"
    );
    let document: Value = serde_json::from_slice(&fixture.configuration()).unwrap();
    assert_eq!(document["autostart"], true);
}

#[tokio::test]
async fn instance_autostart_can_change_while_running_without_changing_the_active_run() {
    let fixture = AutostartFixture::new().await;
    record_started_test_instance(
        &fixture.paths,
        &fixture.details.summary.id,
        12345,
        "fixture.log",
    )
    .await
    .unwrap();
    let running = read_active_instance_run(&fixture.paths, &fixture.details.summary.id)
        .await
        .unwrap()
        .unwrap();

    let updated = update_instance_autostart(&fixture.paths, &fixture.details.summary.id, true)
        .await
        .unwrap();
    assert!(updated.summary.autostart);
    assert!(matches!(updated.summary.status, InstanceStatus::Running));
    assert_eq!(
        serde_json::to_value(updated.active_run.unwrap()).unwrap(),
        serde_json::to_value(running).unwrap()
    );
}

#[tokio::test]
async fn instance_autostart_write_failure_preserves_files_and_database() {
    let fixture = AutostartFixture::new().await;
    let original = fixture.configuration();
    crate::atomic_file::fail_next_atomic_write_for_test(Path::new(
        &fixture.details.config_file_path,
    ));

    let result = update_instance_autostart(&fixture.paths, &fixture.details.summary.id, true).await;
    assert!(matches!(result, Err(StorageError::WriteConfig { .. })));
    fixture.assert_unchanged(&original).await;
}

#[tokio::test]
async fn instance_autostart_commit_failure_restores_manager_mirror_and_database() {
    let fixture = AutostartFixture::new().await;
    let original = fixture.configuration();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    for statement in [
        "CREATE TABLE fixture_autostart_parent (id INTEGER PRIMARY KEY)",
        "CREATE TABLE fixture_autostart_child (parent_id INTEGER REFERENCES fixture_autostart_parent(id) DEFERRABLE INITIALLY DEFERRED)",
        "CREATE TRIGGER fixture_reject_autostart_commit AFTER UPDATE OF autostart ON instances BEGIN INSERT INTO fixture_autostart_child VALUES (1); END",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    pool.close().await;

    let result = update_instance_autostart(&fixture.paths, &fixture.details.summary.id, true).await;
    assert!(matches!(result, Err(StorageError::Sqlx(_))));
    fixture.assert_unchanged(&original).await;
}

#[tokio::test]
async fn instance_autostart_rejects_unknown_instances_and_contended_settings_locks() {
    let fixture = AutostartFixture::new().await;
    let unknown = update_instance_autostart(&fixture.paths, "missing-fixture-instance", true).await;
    assert!(matches!(unknown, Err(StorageError::MissingInstance { .. })));

    let original = fixture.configuration();
    let _lease = crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
        &fixture.paths,
        &fixture.details.summary.id,
    )
    .unwrap();
    let locked = update_instance_autostart(&fixture.paths, &fixture.details.summary.id, true).await;
    assert!(matches!(
        locked,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    fixture.assert_unchanged(&original).await;
}
