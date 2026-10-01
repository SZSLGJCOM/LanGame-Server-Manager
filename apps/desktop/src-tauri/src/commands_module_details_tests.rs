use super::*;
use app_storage::{StoragePaths, bootstrap_storage_with_paths};

struct ModuleDetailsFixtureRoot(PathBuf);

impl Drop for ModuleDetailsFixtureRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn installation_refresh_preserves_latest_task_state_and_unrequested_program_counts() {
    let requested = vec![ModuleSummary {
        id: "fixture".into(),
        name: "Fixture".into(),
        version: "1".into(),
        description: None,
        steam_app_id: None,
        install_state: InstallState::Updating,
        instance_program_count: 1,
        archived_program_count: 2,
        supported_platforms: vec!["windows".into()],
    }];
    let mut current = requested.clone();
    current[0].install_state = InstallState::Installed;
    current[0].instance_program_count = 3;
    current[0].archived_program_count = 4;
    let mut incoming = requested.clone();
    incoming[0].install_state = InstallState::Incomplete;
    incoming[0].instance_program_count = 0;
    incoming[0].archived_program_count = 0;
    merge_module_refresh(&mut incoming, &current, &requested, false);
    assert_eq!(incoming[0].install_state, InstallState::Installed);
    assert_eq!(incoming[0].instance_program_count, 3);
    assert_eq!(incoming[0].archived_program_count, 4);

    current[0].install_state = InstallState::Updating;
    incoming[0].install_state = InstallState::Incomplete;
    incoming[0].archived_program_count = 5;
    merge_module_refresh(&mut incoming, &current, &requested, true);
    assert_eq!(incoming[0].install_state, InstallState::Updating);
    assert_eq!(incoming[0].archived_program_count, 5);
}

#[tokio::test]
async fn restored_configuration_details_do_not_require_archive_inventory()
-> Result<(), Box<dyn std::error::Error>> {
    let root = ModuleDetailsFixtureRoot(temp_test_dir("module-details"));
    let storage = bootstrap_storage_with_paths(StoragePaths {
        app_data_root: root.0.join("app-data"),
        settings_path: root.0.join("app-data/settings.json"),
        database_path: root.0.join("app-data/db/lgs.db"),
        logs_root: root.0.join("app-data/logs"),
        modules_root: workspace_root().join("modules"),
        migrations_root: workspace_root().join("migrations"),
        steamcmd_root: root.0.join("steamcmd"),
        games_root: root.0.join("games"),
        instances_root: root.0.join("instances"),
        archives_root: root.0.join("instances/.trash"),
    })?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "astroneer")?;
    prepare_fake_registered_program(&storage.paths, descriptor).await?;
    let created = create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Archive configuration fixture".into(),
            module_id: "astroneer".into(),
        },
    )
    .await?;
    let original_config = fs::read(&created.config_file_path)?;
    let archived = app_storage::archive_instance(&storage.paths, &created.summary.id).await?;
    app_storage::restore_instance_archive(&storage.paths, &archived.archive_id).await?;
    let expected = load_module_details_with_install_state(&storage, descriptor, true).await?;

    // The archive list refresh owns this exclusive lease after restoration.
    // A configuration read must not depend on its unrelated program inventory.
    let inventory_path = storage
        .paths
        .instances_root
        .join(".langame/locks/instance-settings/archive-inventory.lock");
    let inventory = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&inventory_path)?;
    inventory.try_lock()?;
    let blocked = load_module_details_with_install_state(&storage, descriptor, true)
        .await
        .expect_err("program inventory must retain the archive writer boundary");
    assert!(
        blocked.contains("instance settings are locked"),
        "{blocked}"
    );
    assert!(blocked.contains("archive-inventory.lock"), "{blocked}");

    let details = load_module_details_with_install_state(&storage, descriptor, false).await?;
    let installations =
        load_module_installation_summaries(&storage, std::slice::from_ref(descriptor)).await?;
    assert_eq!(installations.len(), 1);
    assert_eq!(installations[0].id, descriptor.summary.id);
    assert_eq!(
        installations[0].install_state,
        expected.summary.install_state
    );
    assert_eq!(details.schema_json, descriptor.schema_json);
    let mut expected_configuration = expected.clone();
    expected_configuration.summary.instance_program_count = 0;
    expected_configuration.summary.archived_program_count = 0;
    assert_eq!(
        serde_json::to_value(&details)?,
        serde_json::to_value(&expected_configuration)?
    );
    assert_eq!(fs::read(&created.config_file_path)?, original_config);
    drop(inventory);
    let refreshed = load_module_details_with_install_state(&storage, descriptor, true).await?;
    assert_eq!(
        serde_json::to_value(&refreshed)?,
        serde_json::to_value(&expected)?
    );
    Ok(())
}
