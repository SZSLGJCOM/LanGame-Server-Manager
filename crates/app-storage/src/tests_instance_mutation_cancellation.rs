use super::*;
use crate::instance_settings_lock::{
    acquire_instance_settings_mutation_lock, acquire_module_instance_creation_lock_blocking,
};

#[path = "tests_instance_creation_cancellation.rs"]
mod explicit_creation_cancellation_tests;

async fn prepare_test() -> (PathBuf, StoragePaths, ModuleDescriptor) {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    (root, paths, descriptor)
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the controlled transaction did not reach its expected boundary");
}

async fn wait_for_instance_completion(paths: &StoragePaths, instance_id: &str) {
    wait_until(|| acquire_instance_settings_mutation_lock(paths, instance_id).is_ok()).await;
}

async fn create_test_instance(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
) -> InstanceProvisioning {
    create_instance(
        paths,
        descriptor,
        CreateInstanceInput {
            name: String::from("Cancellation boundary"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn cancelled_create_waiter_preserves_the_directory_until_its_transaction_finishes() {
    let (root, paths, descriptor) = prepare_test().await;
    let pool = connect_pool(&paths).await.unwrap();
    let write_reservation = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let creation_paths = paths.clone();
    let caller =
        tokio::spawn(async move { create_test_instance(&creation_paths, &descriptor).await });

    // Runtime preparation precedes the database write reservation. Hold that
    // real boundary so cancellation cannot race a private create that finished.
    wait_until(|| {
        managed_instance_directories(&paths)
            .iter()
            .any(|path| path.join("runtime/.langame-private-runtime").is_file())
    })
    .await;
    let pending_root = managed_instance_directories(&paths).pop().unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert!(
        pending_root
            .join("runtime/.langame-private-runtime")
            .is_file()
    );
    write_reservation.rollback().await.unwrap();
    pool.close().await;

    let completion_paths = paths.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            acquire_module_instance_creation_lock_blocking(&completion_paths, "dontstarve")
        }),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    let instances = list_instances(&paths).await.unwrap();
    assert_eq!(instances.len(), 1);
    assert!(pending_root.join("config/instance.json").is_file());
    assert_eq!(instances[0].name, "Cancellation boundary");
    cleanup_root(&root);
}

#[tokio::test]
async fn cancelled_update_waiter_finishes_native_files_and_database_together() {
    let (root, paths, descriptor) = prepare_test().await;
    let created = create_test_instance(&paths, &descriptor).await;
    update_instance_autostart(&paths, &created.summary.id, true)
        .await
        .unwrap();
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    settings["cluster_name"] = Value::String(String::from("Committed after cancellation"));
    settings["shared_workshop_mod_ids"] = Value::String(String::from("4111111111"));
    let config_path = PathBuf::from(&details.config_file_path);
    let native_path = config_path.parent().unwrap().join("cluster.ini");
    let input = UpdateInstanceInput {
        id: created.summary.id.clone(),
        bind_ip: details.summary.bind_ip,
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string(&settings).unwrap(),
        ports: details.ports,
    };
    let pool = connect_pool(&paths).await.unwrap();
    let write_reservation = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let mutation_paths = paths.clone();
    let caller = tokio::spawn(async move { update_instance(&mutation_paths, input).await });
    // The real SQLite write reservation pauses the admitted mutation before
    // native files are written; cancelling only its waiter must not cancel it.
    wait_until(|| {
        matches!(
            acquire_instance_settings_mutation_lock(&paths, &created.summary.id),
            Err(StorageError::InstanceSettingsLocked { .. })
        )
    })
    .await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert!(matches!(
        acquire_instance_settings_mutation_lock(&paths, &created.summary.id),
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    write_reservation.rollback().await.unwrap();
    pool.close().await;
    wait_for_instance_completion(&paths, &created.summary.id).await;

    let saved = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert!(saved.summary.autostart);
    assert!(
        fs::read_to_string(&native_path)
            .unwrap()
            .contains("Committed after cancellation")
    );
    let saved_settings: Value = serde_json::from_str(&saved.settings_json).unwrap();
    assert_eq!(
        saved_settings["cluster_name"],
        "Committed after cancellation"
    );
    assert!(
        fs::read_to_string(
            instance_private_runtime_root(&created).join("mods/dedicated_server_mods_setup.lua")
        )
        .unwrap()
        .contains("ServerModSetup(\"4111111111\")")
    );
    cleanup_root(&root);
}

#[tokio::test]
async fn cancelled_delete_waiter_finishes_owned_files_and_database_removal_together() {
    let (root, paths, descriptor) = prepare_test().await;
    let created = create_test_instance(&paths, &descriptor).await;
    let instance_root = paths.instances_root.join(&created.summary.id);
    let library_root = paths.games_root.join(&descriptor.summary.id);
    let library_before = crate::test_file_snapshot::tree_snapshot(&library_root).unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    let write_reservation = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let mutation_paths = paths.clone();
    let instance_id = created.summary.id.clone();
    let caller = tokio::spawn(async move { delete_instance(&mutation_paths, &instance_id).await });
    wait_until(|| {
        matches!(
            acquire_instance_settings_mutation_lock(&paths, &created.summary.id),
            Err(StorageError::InstanceSettingsLocked { .. })
        )
    })
    .await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    write_reservation.rollback().await.unwrap();
    pool.close().await;
    wait_for_instance_completion(&paths, &created.summary.id).await;

    assert!(list_instances(&paths).await.unwrap().is_empty());
    assert!(!instance_root.exists());
    let archives = list_instance_archives(&paths).await.unwrap();
    assert!(archives.archives.is_empty());
    assert!(archives.pending_deletions.is_empty());
    assert_eq!(
        fs::read_dir(paths.instances_root.join(".trash"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library_root).unwrap(),
        library_before
    );
    cleanup_root(&root);
}
