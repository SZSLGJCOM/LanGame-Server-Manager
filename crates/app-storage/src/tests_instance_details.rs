use super::*;
use std::os::windows::fs::OpenOptionsExt;

fn assert_database_closed(paths: &StoragePaths) {
    let exclusive = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&paths.database_path)
        .expect("instance details must release all SQLite handles before returning");
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
