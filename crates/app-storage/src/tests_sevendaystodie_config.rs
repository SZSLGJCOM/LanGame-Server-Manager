use super::*;

#[tokio::test]
async fn running_instances_stage_settings_without_rewriting_live_native_files() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .expect("discover repo modules")
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "sevendaystodie")
        .expect("missing 7 Days to Die module");

    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    prepare_shared_install(&paths, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Navezgane Live Writer Boundary"),
            module_id: String::from("sevendaystodie"),
        },
    )
    .await
    .unwrap();
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let config_root = Path::new(&details.config_file_path).parent().unwrap();
    let native_path = config_root.join("serverconfig.xml");
    let game_owned_bytes = b"game-owned-live-config\n";
    fs::write(&native_path, game_owned_bytes).unwrap();

    let run = record_started_test_instance(
        &paths,
        &created.summary.id,
        43_210,
        config_root.join("running.log").to_string_lossy().as_ref(),
    )
    .await
    .unwrap();
    let mut settings = serde_json::from_str::<Value>(&details.settings_json)
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
    settings.insert(
        String::from("server_name"),
        Value::String(String::from("Staged Until Restart")),
    );

    let updated = update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: details.summary.bind_ip,
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: Value::Object(settings).to_string(),
            ports: details.ports,
        },
    )
    .await
    .unwrap();

    assert!(updated.settings_json.contains("Staged Until Restart"));
    assert_eq!(fs::read(&native_path).unwrap(), game_owned_bytes);

    mark_instance_process_stopped(&paths, &created.summary.id, run.run_id, Some(0), false)
        .await
        .unwrap();
    materialize_instance_configuration(&paths, &created.summary.id)
        .await
        .unwrap();
    let stopped_native = fs::read_to_string(native_path).unwrap();
    assert!(stopped_native.contains("Staged Until Restart"));
    assert!(!stopped_native.contains("game-owned-live-config"));

    cleanup_root(&root);
}
