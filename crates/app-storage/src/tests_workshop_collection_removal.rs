use super::*;

#[tokio::test]
async fn workshop_collection_removal_persists_only_selection_and_preserves_instance_cache_and_metadata()
 {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let mut descriptor = declared_save_path_test_descriptor(&root);
    descriptor.summary.id = "squad".into();
    descriptor.summary.name = "Squad collection fixture".into();
    descriptor.root = root.join("modules/squad");
    descriptor.manifest_toml = descriptor.manifest_toml.replace("savepathtest", "squad");
    descriptor.manifest_toml.push_str(
        "\n[workshop]\nprovider=\"steam\"\nconsumer_app_id=393380\nsupports_collections=false\n",
    );
    descriptor.workshop = Some(app_core::WorkshopSpec {
        provider: "steam".into(),
        consumer_app_id: Some(393380),
        supports_collections: false,
    });
    prepare_declared_save_path_environment(&root, &descriptor);
    let templates = descriptor.root.join("templates");
    fs::create_dir_all(&templates).unwrap();
    for entry in fs::read_dir(repo_root().join("modules/squad/templates")).unwrap() {
        fs::write(
            templates.join(entry.unwrap().file_name()),
            b"// synthetic configuration\n",
        )
        .unwrap();
    }
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: "Squad collection".into(),
            module_id: "squad".into(),
        },
    )
    .await
    .unwrap();
    let mut current = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let input_for = |current: &InstanceDetails, settings: &Value| UpdateInstanceInput {
        id: current.summary.id.clone(),
        bind_ip: current.summary.bind_ip.clone(),
        auto_backup_on_stop: current.auto_backup_on_stop,
        backup_retention_count: current.backup_retention_count,
        settings_json: settings.to_string(),
        ports: current.ports.clone(),
    };
    let mut settings: Value = serde_json::from_str(&current.settings_json).unwrap();
    settings["steam_workshop_collections"] = json!([
        {"id":"111111", "title":"Remove", "member_ids":["222222", "333333", "666666"]},
        {"id":"444444", "title":"Keep", "member_ids":["333333"]}
    ]);
    current = update_instance_if_current(
        &paths,
        input_for(&current, &settings),
        &current.settings_json,
    )
    .await
    .unwrap();
    let instance_root = Path::new(&current.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let mods = instance_root.join("runtime/SquadGame/Plugins/Mods");
    for id in ["222222", "333333", "666666", "CanadianArmedForces"] {
        fs::create_dir_all(mods.join(id)).unwrap();
        fs::write(mods.join(id).join("data.pak"), id).unwrap();
    }
    let shared_cache = paths
        .steamcmd_root
        .join("steamapps/workshop/content/393380/222222");
    fs::create_dir_all(&shared_cache).unwrap();
    fs::write(shared_cache.join("data.pak"), b"cache").unwrap();
    settings["steam_workshop_collections"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    let input = input_for(&current, &settings);
    // Exercise the public recovery path used by both desktop readback and start,
    // including refusal by an ordinary settings update while a journal is pending.
    let config_path = Path::new(&current.config_file_path);
    let original_bytes = fs::read(config_path).unwrap();
    let mut next_document: Value = serde_json::from_slice(&original_bytes).unwrap();
    next_document["settings"] = settings.clone();
    let next_bytes = serde_json::to_vec_pretty(&next_document).unwrap();
    let hash = |bytes: &[u8]| {
        use sha2::Digest;
        sha2::Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let operation_id = uuid::Uuid::new_v4().to_string();
    let retained = instance_root
        .join(".lgsm-workshop-retained")
        .join(&operation_id);
    fs::create_dir_all(&retained).unwrap();
    let journal_path = instance_root.join(".lgsm-workshop-removal.json");
    let journal = serde_json::to_vec(&json!({
        "version": 1, "instance_id": current.summary.id, "operation_id": operation_id,
        "original_sha256": hash(&original_bytes), "replacement_sha256": hash(&next_bytes), "members": ["222222"]
    })).unwrap();
    fs::write(retained.join("owner.json"), &journal).unwrap();
    fs::write(&journal_path, &journal).unwrap();
    fs::rename(mods.join("222222"), retained.join("222222")).unwrap();
    assert!(matches!(
        read_instance_details(&paths, &current.summary.id).await,
        Err(StorageError::PrivateRuntimeRefresh { .. })
    ));
    assert!(
        update_instance_if_current(&paths, input.clone(), &current.settings_json)
            .await
            .is_err()
    );
    assert_eq!(fs::read(config_path).unwrap(), original_bytes);
    recover_interrupted_instance_runtime(&paths, &current.summary.id)
        .await
        .unwrap();
    assert!(mods.join("222222/data.pak").exists());
    assert!(!journal_path.exists());
    read_instance_details(&paths, &current.summary.id)
        .await
        .unwrap();
    for altered in [
        UpdateInstanceInput {
            bind_ip: "127.0.0.2".into(),
            ..input.clone()
        },
        UpdateInstanceInput {
            backup_retention_count: input.backup_retention_count + 1,
            ..input.clone()
        },
        UpdateInstanceInput {
            auto_backup_on_stop: !input.auto_backup_on_stop,
            ..input.clone()
        },
        UpdateInstanceInput {
            ports: vec![PortBinding {
                name: "new".into(),
                protocol: "tcp".into(),
                port: 33333,
            }],
            ..input.clone()
        },
    ] {
        assert!(
            remove_instance_workshop_collection(
                &paths,
                altered,
                current.settings_json.clone(),
                "111111".into(),
                vec!["222222".into()],
                false
            )
            .await
            .is_err()
        );
        assert!(mods.join("222222/data.pak").exists());
    }
    assert!(
        remove_instance_workshop_collection(
            &paths,
            input.clone(),
            "{}".into(),
            "111111".into(),
            vec!["222222".into()],
            false
        )
        .await
        .is_err()
    );
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("UPDATE instances SET status = 'starting' WHERE id = ?")
        .bind(&current.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        remove_instance_workshop_collection(
            &paths,
            input.clone(),
            current.settings_json.clone(),
            "111111".into(),
            vec!["222222".into()],
            false
        )
        .await
        .is_err()
    );
    sqlx::query("UPDATE instances SET status = 'stopped' WHERE id = ?")
        .bind(&current.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let saved = remove_instance_workshop_collection(
        &paths,
        input,
        current.settings_json.clone(),
        "111111".into(),
        vec!["222222".into()],
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&saved.settings_json).unwrap(),
        settings
    );
    assert_eq!(
        serde_json::to_value(&saved.ports).unwrap(),
        serde_json::to_value(&current.ports).unwrap()
    );
    assert!(!mods.join("222222").exists());
    for id in ["333333", "666666", "CanadianArmedForces"] {
        assert_eq!(
            fs::read(mods.join(id).join("data.pak")).unwrap(),
            id.as_bytes()
        );
    }
    assert_eq!(fs::read(shared_cache.join("data.pak")).unwrap(), b"cache");
    let reloaded = read_instance_details(&paths, &saved.summary.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&reloaded.settings_json).unwrap(),
        settings
    );
    let committed_owner = fs::read_dir(instance_root.join(".lgsm-workshop-retained"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.join("222222/data.pak").exists())
        .unwrap();
    fs::write(
        &journal_path,
        fs::read(committed_owner.join("owner.json")).unwrap(),
    )
    .unwrap();
    assert!(
        read_instance_details(&paths, &saved.summary.id)
            .await
            .is_err()
    );
    recover_interrupted_instance_runtime(&paths, &saved.summary.id)
        .await
        .unwrap();
    assert!(!journal_path.exists());
    assert!(!mods.join("222222").exists());
    assert!(committed_owner.join("222222/data.pak").exists());
    read_instance_details(&paths, &saved.summary.id)
        .await
        .unwrap();
    let before_member = fs::read(config_path).unwrap();
    let saved_member = remove_instance_workshop_collection(
        &paths,
        input_for(&saved, &settings),
        saved.settings_json.clone(),
        "444444".into(),
        vec!["333333".into()],
        true,
    )
    .await
    .unwrap();
    assert!(!mods.join("333333").exists());
    assert_eq!(fs::read(config_path).unwrap(), before_member);
    assert_eq!(
        serde_json::from_str::<Value>(&saved_member.settings_json).unwrap(),
        settings
    );
    assert!(mods.join("666666/data.pak").exists());
    assert_eq!(fs::read(shared_cache.join("data.pak")).unwrap(), b"cache");
    cleanup_root(&root);
}
