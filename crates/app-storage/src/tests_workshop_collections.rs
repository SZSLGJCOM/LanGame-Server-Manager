use super::*;
use crate::test_file_snapshot::tree_snapshot;

#[tokio::test]
async fn workshop_collections_roundtrip_without_changing_native_files_and_reject_invalid_writes() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let mut descriptor = test_descriptor(&root);
    descriptor.workshop = Some(app_core::WorkshopSpec {
        provider: "steam".into(),
        consumer_app_id: Some(322330),
        supports_collections: true,
    });
    descriptor.manifest_toml.push_str("\n[workshop]\nprovider = \"steam\"\nconsumer_app_id = 322330\nsupports_collections = true\n");
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let details = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: "Collection provenance".into(),
            module_id: "dontstarve".into(),
        },
    )
    .await
    .unwrap();
    let details = read_instance_details(&paths, &details.summary.id)
        .await
        .unwrap();
    let config_path = Path::new(&details.config_file_path);
    let instance_root = config_path.parent().unwrap().parent().unwrap();
    let config_relative = config_path.strip_prefix(instance_root).unwrap();
    let mut before = tree_snapshot(instance_root).unwrap();
    before.remove(config_relative);
    let records = json!([{
        "id": "3495871201", "title": "Offline collection title",
        "member_ids": ["2039181790", "1909182187"]
    }]);
    let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    settings["steam_workshop_collections"] = records.clone();
    let input = |settings: &Value| UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: details.summary.bind_ip.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: settings.to_string(),
        ports: details.ports.clone(),
    };
    let saved = update_instance_if_current(&paths, input(&settings), &details.settings_json)
        .await
        .unwrap();
    let reloaded = read_instance_details(&paths, &details.summary.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&saved.settings_json).unwrap()["steam_workshop_collections"],
        records
    );
    assert_eq!(
        serde_json::from_str::<Value>(&reloaded.settings_json).unwrap()["steam_workshop_collections"],
        records
    );
    let mut after = tree_snapshot(instance_root).unwrap();
    after.remove(config_relative);
    assert_eq!(
        after, before,
        "collection provenance alone must not modify any native config or Mod file"
    );

    let stable = tree_snapshot(instance_root).unwrap();
    settings["steam_workshop_collections"][0]["member_ids"] = json!(["../outside"]);
    assert!(matches!(
        update_instance_if_current(&paths, input(&settings), &saved.settings_json).await,
        Err(StorageError::InvalidModuleSetting { .. })
    ));
    assert_eq!(
        tree_snapshot(instance_root).unwrap(),
        stable,
        "an invalid record must not change settings or native files"
    );

    // A normal subsequent settings save must retain provenance without a separate API.
    let mut settings: Value = serde_json::from_str(&reloaded.settings_json).unwrap();
    settings["cluster_name"] = json!("Renamed instance configuration");
    let renamed = update_instance_if_current(&paths, input(&settings), &reloaded.settings_json)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&renamed.settings_json).unwrap()["steam_workshop_collections"],
        records
    );
    cleanup_root(&root);
}
