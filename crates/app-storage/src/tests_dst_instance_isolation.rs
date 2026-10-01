use super::*;

async fn configure_isolated_dst_mod(
    paths: &StoragePaths,
    instance_id: &str,
    mod_id: &str,
    marker: &str,
) -> InstanceDetails {
    let details = read_instance_details(paths, instance_id).await.unwrap();
    let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    settings["offline_cluster"] = json!(true);
    settings["lan_only_cluster"] = json!(true);
    settings["enable_caves"] = json!(true);
    settings["shared_workshop_mod_ids"] = json!(mod_id);
    settings["master_enabled_workshop_mod_ids"] = json!(mod_id);
    settings["caves_enabled_workshop_mod_ids"] = json!(mod_id);
    settings["master_mod_configuration_options"] = json!({
        (mod_id): { "isolation_marker": format!("{marker}-master") }
    });
    settings["caves_mod_configuration_options"] = json!({
        (mod_id): { "isolation_marker": format!("{marker}-caves") }
    });
    update_instance(
        paths,
        UpdateInstanceInput {
            id: details.summary.id,
            bind_ip: details.summary.bind_ip,
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: serde_json::to_string(&settings).unwrap(),
            ports: details.ports,
        },
    )
    .await
    .unwrap()
}

fn snapshot_dst_files(files: impl IntoIterator<Item = PathBuf>) -> Vec<(PathBuf, Vec<u8>)> {
    files
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}

fn assert_dst_files_unchanged(snapshot: &[(PathBuf, Vec<u8>)]) {
    for (path, expected) in snapshot {
        assert_eq!(
            fs::read(path).unwrap(),
            *expected,
            "another instance changed {}",
            path.display()
        );
    }
}

#[tokio::test]
async fn dst_instances_keep_mods_and_saves_isolated_through_materialization_and_deletion() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = fs::canonicalize(repo_root().join("modules")).unwrap();
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "dontstarve")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let shared_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(shared_root.join("bin64")).unwrap();
    fs::create_dir_all(shared_root.join("mods")).unwrap();
    let shared_package = shared_root.join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe");
    let shared_setup = shared_root.join("mods/dedicated_server_mods_setup.lua");
    fs::write(
        &shared_package,
        b"synthetic package fixture; never executed",
    )
    .unwrap();
    fs::write(&shared_setup, b"-- retained shared installation settings\n").unwrap();
    let shared_before = snapshot_dst_files([shared_package, shared_setup]);

    let create = |name: &str| CreateInstanceInput {
        name: name.to_owned(),
        module_id: "dontstarve".to_owned(),
    };
    let first = create_instance(&paths, &descriptor, create("DST Alpha"))
        .await
        .unwrap();
    fs::create_dir_all(shared_root.join("bin64")).unwrap();
    fs::write(
        shared_root.join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe"),
        b"synthetic package fixture; never executed",
    )
    .unwrap();
    let second = create_instance(&paths, &descriptor, create("DST Beta"))
        .await
        .unwrap();
    // A third download remains a separate library source while both owned
    // runtimes are configured, backed up and deleted below.
    for (path, bytes) in &shared_before {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let first_details =
        configure_isolated_dst_mod(&paths, &first.summary.id, "4111111111", "alpha").await;
    let second_details =
        configure_isolated_dst_mod(&paths, &second.summary.id, "4222222222", "beta").await;
    let first_runtime = instance_private_runtime_root(&first);
    let second_runtime = instance_private_runtime_root(&second);
    assert_ne!(first_runtime, second_runtime);
    assert_ne!(first_details.saves_path, second_details.saves_path);
    assert_ne!(first.config_file_path, second.config_file_path);

    let mut first_saves = Vec::new();
    let mut second_files = Vec::new();
    for (instance, details, runtime, mod_id, other_mod_id, marker) in [
        (
            &first,
            &first_details,
            &first_runtime,
            "4111111111",
            "4222222222",
            "alpha",
        ),
        (
            &second,
            &second_details,
            &second_runtime,
            "4222222222",
            "4111111111",
            "beta",
        ),
    ] {
        let config_dir = Path::new(&instance.config_file_path).parent().unwrap();
        let cluster_dir = Path::new(&details.saves_path);
        assert_eq!(cluster_dir, config_dir.join("clusters/main"));
        let setup_path = runtime.join("mods/dedicated_server_mods_setup.lua");
        let setup = fs::read_to_string(&setup_path).unwrap();
        assert!(setup.contains(&format!("ServerModSetup(\"{mod_id}\")")));
        assert!(!setup.contains(other_mod_id));
        if instance.summary.id == second.summary.id {
            second_files.extend([cluster_dir.join("cluster.ini"), setup_path]);
        }
        for shard in ["Master", "Caves"] {
            let shard_dir = cluster_dir.join(shard);
            let overrides_path = shard_dir.join("modoverrides.lua");
            let overrides = fs::read_to_string(&overrides_path).unwrap();
            assert!(overrides.contains(&format!("workshop-{mod_id}")));
            assert!(!overrides.contains(other_mod_id));
            assert!(overrides.contains(&format!("{marker}-{}", shard.to_lowercase())));
            let save_path = shard_dir.join("save/session/world.sentinel");
            fs::create_dir_all(save_path.parent().unwrap()).unwrap();
            fs::write(
                &save_path,
                format!("{marker}-{shard}-saved-world\0\x01\x02"),
            )
            .unwrap();
            if instance.summary.id == second.summary.id {
                second_files.extend([
                    overrides_path,
                    shard_dir.join("server.ini"),
                    shard_dir.join("worldgenoverride.lua"),
                    save_path,
                ]);
            } else {
                first_saves.push(save_path);
            }
        }
    }
    let first_saves_before = snapshot_dst_files(first_saves);
    let second_before = snapshot_dst_files(second_files);
    let second_document_before = fs::read(&second.config_file_path).unwrap();
    assert_dst_files_unchanged(&shared_before);

    // Editing and preparing Alpha must leave Beta's persisted/native state intact.
    configure_isolated_dst_mod(&paths, &first.summary.id, "4333333333", "alpha-edited").await;
    materialize_instance_configuration_for_start(&paths, &first.summary.id)
        .await
        .unwrap();
    assert_eq!(
        fs::read(&second.config_file_path).unwrap(),
        second_document_before
    );
    assert_dst_files_unchanged(&first_saves_before);
    assert_dst_files_unchanged(&second_before);
    assert_dst_files_unchanged(&shared_before);

    // Beta may refresh its generated timestamp, but must retain its native files.
    materialize_instance_configuration_for_start(&paths, &second.summary.id)
        .await
        .unwrap();
    assert_dst_files_unchanged(&second_before);
    let second_document_before_delete = fs::read(&second.config_file_path).unwrap();
    delete_instance(&paths, &first.summary.id).await.unwrap();
    assert!(
        read_instance_details(&paths, &first.summary.id)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(&second.config_file_path).unwrap(),
        second_document_before_delete
    );
    materialize_instance_configuration_for_start(&paths, &second.summary.id)
        .await
        .unwrap();
    assert_dst_files_unchanged(&second_before);
    assert_dst_files_unchanged(&shared_before);
    let retained = read_instance_details(&paths, &second.summary.id)
        .await
        .unwrap();
    assert_eq!(retained.settings_json, second_details.settings_json);
    assert_eq!(list_instances(&paths).await.unwrap().len(), 1);

    cleanup_root(&root);
}
