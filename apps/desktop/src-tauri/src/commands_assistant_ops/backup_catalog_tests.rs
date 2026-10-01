use super::*;

#[tokio::test(flavor = "current_thread")]
async fn assistant_backup_catalog_does_not_reconcile_stale_running_records() {
    let (_lock, _environment, app, isolated) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let root = isolated.paths.app_data_root.parent().unwrap().to_path_buf();
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("lg-test-assessment-")
    );
    assert!(
        fs::canonicalize(&root)
            .unwrap()
            .starts_with(fs::canonicalize(std::env::temp_dir()).unwrap())
    );
    let state = app.state::<DesktopState>();
    let storage = bootstrap_storage().unwrap();
    initialize_database(&storage.paths).await.unwrap();
    let descriptors = discover_modules(&storage.paths.modules_root).unwrap();
    let descriptor = find_descriptor(&descriptors, "dontstarve").unwrap();
    sync_modules(&storage.paths, std::slice::from_ref(descriptor))
        .await
        .unwrap();
    crate::commands::tests::prepare_fake_registered_program(&storage.paths, descriptor)
        .await
        .unwrap();
    let created = create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Read-only backup catalog fixture".into(),
            module_id: "dontstarve".into(),
        },
    )
    .await
    .unwrap();
    let stopped = read_instance_details(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    fs::create_dir_all(&stopped.saves_path).unwrap();
    fs::write(
        Path::new(&stopped.saves_path).join("world.db"),
        b"isolated save",
    )
    .unwrap();
    let backup = app_storage::create_instance_backup(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    // No process is launched. The deliberately missing process identity makes
    // this a stale record that runtime reconciliation would mark as stopped.
    mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &created.summary.id,
            session_id: Some("catalog-read-fixture"),
            process_key: "master",
            display_name: "Master",
            pid: std::process::id(),
            log_path: "isolated-catalog.log",
            is_primary: true,
        },
        None,
    )
    .await
    .unwrap();
    let before = read_instance_details(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    assert!(matches!(before.summary.status, InstanceStatus::Running));
    assert!(before.active_run.is_some());
    let active_before = serde_json::to_value(&before.active_run).unwrap();

    let evidence = read_assistant_investigation_tool(
        &state,
        &storage,
        Some(&before),
        None,
        AssistantReadTool::ListBackups { offset: 0 },
    )
    .await
    .unwrap();
    assert_eq!(evidence["instanceId"], created.summary.id);
    assert_eq!(evidence["backups"].as_array().unwrap().len(), 1);
    assert_eq!(evidence["backups"][0]["backupId"], backup.backup_id);
    assert!(evidence["backups"][0].get("backupPath").is_none());
    let after = read_instance_details(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    assert!(matches!(after.summary.status, InstanceStatus::Running));
    assert_eq!(
        serde_json::to_value(&after.active_run).unwrap(),
        active_before,
        "reading backup metadata must not reconcile or replace running records"
    );

    let transition = state.begin_storage_context_transition().unwrap();
    assert!(
        read_assistant_investigation_tool(
            &state,
            &storage,
            Some(&before),
            None,
            AssistantReadTool::ListBackups { offset: 0 }
        )
        .await
        .unwrap_err()
        .contains("paths are being updated")
    );
    drop(transition);
    drop(app);
    fs::remove_dir_all(root).unwrap();
}
