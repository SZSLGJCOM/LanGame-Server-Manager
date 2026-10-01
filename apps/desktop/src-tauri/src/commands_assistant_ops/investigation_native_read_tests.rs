use super::*;

#[tokio::test(flavor = "current_thread")]
async fn native_investigation_reads_settings_and_launch_preview_without_starting() {
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

    // The assessment fixture's fourth value deliberately points at an unopened
    // store. Production launch preview bootstraps storage, so use that same
    // isolated store for creation and both native reads.
    let state = app.state::<DesktopState>();
    let storage = bootstrap_storage().unwrap();
    initialize_database(&storage.paths).await.unwrap();
    let descriptors = discover_modules(&storage.paths.modules_root).unwrap();
    let descriptor = find_descriptor(&descriptors, "dontstarve").unwrap();
    sync_modules(&storage.paths, std::slice::from_ref(descriptor))
        .await
        .unwrap();
    // This inert package substitutes only the installed game boundary. Its fake
    // executable is never run; these reads require no model, UAC or firewall work.
    crate::commands::tests::prepare_fake_registered_program(&storage.paths, descriptor)
        .await
        .unwrap();
    let original_program = storage
        .paths
        .games_root
        .join(&descriptor.install.as_ref().unwrap().shared_game_dir)
        .join(&descriptor.process.as_ref().unwrap().executable);
    let original_bytes = fs::read(&original_program).unwrap();
    let created = create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Native investigation read fixture".into(),
            module_id: "dontstarve".into(),
        },
    )
    .await
    .unwrap();
    let created_details = read_instance_details(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    let mut settings: Value = serde_json::from_str(&created_details.settings_json).unwrap();
    settings["cluster_name"] = json!("Native investigation read fixture");
    settings["offline_cluster"] = json!(true);
    settings["enable_caves"] = json!(false);
    update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: created_details.summary.bind_ip.clone(),
            auto_backup_on_stop: created_details.auto_backup_on_stop,
            backup_retention_count: created_details.backup_retention_count,
            settings_json: settings.to_string(),
            ports: created_details.ports.clone(),
        },
    )
    .await
    .unwrap();
    let before = read_instance_details(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    let module = map_module_details_with_install_state(&storage.settings, descriptor, None);
    assert!(matches!(before.summary.status, InstanceStatus::Stopped));
    assert_eq!(before.summary.active_process_count, 0);
    assert!(before.active_run.is_none());

    let reconciliation = reconcile_runtime_state(&state);
    assert!(std::mem::size_of_val(&reconciliation) <= 1024);
    drop(reconciliation);
    let preview = preview_instance_launch(state.clone(), before.summary.id.clone());
    assert!(
        std::mem::size_of_val(&preview) <= 8 * 1024,
        "launch preview must not inline the shared reconciliation state machine"
    );
    drop(preview);

    let mut evidence = Vec::new();
    for tool in [
        AssistantReadTool::ReadSettings {
            keys: vec![
                "cluster_name".into(),
                "offline_cluster".into(),
                "enable_caves".into(),
            ],
            offset: 0,
        },
        AssistantReadTool::InspectLaunch {},
    ] {
        let read =
            read_assistant_investigation_tool(&state, &storage, Some(&before), Some(&module), tool);
        // Bound the actual production factory, not a box added by the test.
        // This guards orchestration stack size; it does not prove engine launch.
        assert!(
            std::mem::size_of_val(&read) <= 1024,
            "native read factory must not inline its full state machine"
        );
        let read = tokio::time::timeout(Duration::from_secs(30), read);
        assert!(std::mem::size_of_val(&read) <= 1024);
        evidence.push(read.await.unwrap().unwrap());
    }
    assert_eq!(evidence[0]["unknownKeys"], json!([]));
    let entries = evidence[0]["entries"].as_array().unwrap();
    for (key, expected) in [
        ("cluster_name", json!("Native investigation read fixture")),
        ("offline_cluster", json!(true)),
        ("enable_caves", json!(false)),
    ] {
        let entry = entries.iter().find(|entry| entry["key"] == key).unwrap();
        assert_eq!(entry["exists"], true);
        assert_eq!(entry["schemaExists"], true);
        assert_eq!(entry["value"], expected);
    }
    let plan: LaunchPlan = serde_json::from_value(evidence.pop().unwrap()).unwrap();
    assert_eq!(plan.instance_id, created.summary.id);
    assert_eq!(plan.module_id, "dontstarve");
    assert!(plan.executable_exists);
    assert!(plan.ready_to_launch, "{:?}", plan.validation_issues);
    assert!(Path::new(&plan.executable_path).is_file());

    let after = read_instance_details(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    assert!(matches!(after.summary.status, InstanceStatus::Stopped));
    assert_eq!(after.summary.active_process_count, 0);
    assert!(after.active_run.is_none());
    assert_eq!(after.settings_json, before.settings_json);
    assert_eq!(
        serde_json::to_value(&after.ports).unwrap(),
        serde_json::to_value(&before.ports).unwrap()
    );
    assert!(
        state
            .runtime_supervisor
            .lock()
            .unwrap()
            .tracked_instances()
            .is_empty()
    );
    assert_eq!(fs::read(&original_program).unwrap(), original_bytes);
    drop(app);
    fs::remove_dir_all(root).unwrap();
}
