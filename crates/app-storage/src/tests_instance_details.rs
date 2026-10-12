use super::*;
use std::os::windows::fs::OpenOptionsExt;

pub(super) fn assert_database_closed(paths: &StoragePaths) {
    let exclusive = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&paths.database_path)
        .expect("instance reads must release all SQLite handles before returning");
    drop(exclusive);
}

#[tokio::test]
async fn instance_details_release_database_after_missing_instance() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();

    let error = read_instance_details(&paths, "missing-details-instance")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::MissingInstance { ref id } if id == "missing-details-instance"
    ));
    assert_database_closed(&paths);
    fs::remove_dir_all(&root).expect("remove closed instance-details fixture");
}

#[tokio::test]
async fn runtime_overview_releases_database_after_missing_instance() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();

    let error = read_instance_runtime_overview(&paths, "missing-overview-instance")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::MissingInstance { ref id } if id == "missing-overview-instance"
    ));
    assert_database_closed(&paths);
    fs::remove_dir_all(&root).expect("remove closed runtime-overview fixture");
}

#[tokio::test]
async fn instance_details_release_database_after_success_and_invalid_configuration() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let instance = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Details connection lifetime"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();

    let details = read_instance_details(&paths, &instance.summary.id)
        .await
        .unwrap();
    assert_eq!(details.summary.id, instance.summary.id);
    assert_database_closed(&paths);

    fs::write(&instance.config_file_path, "{invalid configuration").unwrap();
    let error = read_instance_details(&paths, &instance.summary.id)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::InvalidConfigJson { ref path, .. }
            if path == Path::new(&instance.config_file_path)
    ));
    assert_database_closed(&paths);
    fs::remove_dir_all(&root).expect("remove closed instance-details fixture");
}

#[tokio::test]
async fn combined_runtime_read_preserves_details_and_standalone_overview_errors() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let instance = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Runtime read connection lifetime"),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();

    let expected_details = read_instance_details(&paths, &instance.summary.id)
        .await
        .unwrap();
    let expected_overview = read_instance_runtime_overview(&paths, &instance.summary.id)
        .await
        .unwrap();
    assert_database_closed(&paths);
    let (details, overview) =
        read_instance_details_and_runtime_overview(&paths, &instance.summary.id)
            .await
            .unwrap();
    assert_eq!(
        serde_json::to_value(details).unwrap(),
        serde_json::to_value(expected_details).unwrap()
    );
    assert_eq!(
        serde_json::to_value(overview).unwrap(),
        serde_json::to_value(expected_overview).unwrap()
    );
    assert_database_closed(&paths);

    fs::write(&instance.config_file_path, "{invalid configuration").unwrap();
    // A standalone DST overview does not need the complete settings projection.
    read_instance_runtime_overview(&paths, &instance.summary.id)
        .await
        .unwrap();
    assert_database_closed(&paths);
    let error = read_instance_details_and_runtime_overview(&paths, &instance.summary.id)
        .await
        .unwrap_err();
    assert!(
        matches!(error, StorageError::InvalidConfigJson { ref path, .. }
        if path == Path::new(&instance.config_file_path))
    );
    assert_database_closed(&paths);
    fs::remove_dir_all(&root).expect("remove closed combined-runtime fixture");
}
