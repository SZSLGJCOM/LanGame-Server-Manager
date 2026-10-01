use super::super::*;
use super::ark_evolved::{ark_test_descriptor, prepare_ark_environment};

#[tokio::test]
async fn ark_settings_precondition_requires_acknowledged_defaults_after_clearing_a_number() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = ark_test_descriptor(&root);
    prepare_ark_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("ARK settings acknowledgement"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();
    let baseline = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let schema: Value = serde_json::from_str(descriptor.schema_json.as_deref().unwrap()).unwrap();
    let default_xp = &schema["properties"]["xp_multiplier"]["default"];
    assert!(default_xp.is_number());
    let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
    assert!(
        draft
            .as_object_mut()
            .unwrap()
            .remove("xp_multiplier")
            .is_some()
    );
    let submitted_json = draft.to_string();
    let input = |settings_json: String| UpdateInstanceInput {
        id: created.summary.id.clone(),
        bind_ip: baseline.summary.bind_ip.clone(),
        auto_backup_on_stop: baseline.auto_backup_on_stop,
        backup_retention_count: baseline.backup_retention_count,
        settings_json,
        ports: baseline.ports.clone(),
    };

    // Clearing a number removes its key in the editor. Saving restores the
    // schema default, so the submitted draft is no longer the persisted baseline.
    let acknowledged = update_instance_if_current(
        &paths,
        input(submitted_json.clone()),
        &baseline.settings_json,
    )
    .await
    .unwrap();
    let saved: Value = serde_json::from_str(&acknowledged.settings_json).unwrap();
    assert_eq!(&saved["xp_multiplier"], default_xp);
    let persisted = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&persisted.settings_json).unwrap(),
        saved
    );
    let config_before_stale = fs::read(&acknowledged.config_file_path).unwrap();

    draft["server_name"] = json!("ARK subsequent edit");
    let next_json = draft.to_string();
    let stale = update_instance_if_current(&paths, input(next_json.clone()), &submitted_json)
        .await
        .expect_err("the submitted draft omitted a default now present in storage");
    assert!(matches!(
        stale,
        StorageError::InstanceSettingsPreconditionFailed { .. }
    ));
    assert_eq!(
        fs::read(&acknowledged.config_file_path).unwrap(),
        config_before_stale,
        "a stale precondition must not change the persisted configuration"
    );

    let next = update_instance_if_current(&paths, input(next_json), &acknowledged.settings_json)
        .await
        .expect("the returned acknowledgement must support the next edit");
    let next_saved: Value = serde_json::from_str(&next.settings_json).unwrap();
    assert_eq!(next_saved["server_name"], json!("ARK subsequent edit"));
    assert_eq!(&next_saved["xp_multiplier"], default_xp);
    cleanup_root(&root);
}
