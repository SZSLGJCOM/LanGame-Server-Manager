use super::*;

fn assert_missing_runtime(error: StorageError) {
    assert!(
        matches!(&error, StorageError::PrivateRuntimeRefresh { .. })
            && error
                .to_string()
                .contains("required private runtime is missing"),
        "unexpected error: {error}"
    );
}

fn changed_mod_input(details: &InstanceDetails) -> UpdateInstanceInput {
    let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    settings["shared_workshop_mod_ids"] = json!("4999999999");
    UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: details.summary.bind_ip.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: serde_json::to_string(&settings).unwrap(),
        ports: details.ports.clone(),
    }
}

#[tokio::test]
async fn required_runtime_missing_blocks_read_write_start_and_backup_without_shared_fallback() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: "Private world".into(),
            module_id: "dontstarve".into(),
        },
    )
    .await
    .unwrap();
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let save = Path::new(&details.saves_path).join("world.sentinel");
    fs::write(&save, b"private save").unwrap();
    let backup = create_instance_backup(&paths, &created.summary.id)
        .await
        .unwrap();
    let config_before = fs::read(&created.config_file_path).unwrap();
    let shared_setup = paths
        .games_root
        .join("dontstarve/mods/dedicated_server_mods_setup.lua");
    fs::create_dir_all(shared_setup.parent().unwrap()).unwrap();
    fs::write(&shared_setup, b"shared setup must stay unchanged").unwrap();
    let private_runtime = instance_private_runtime_root(&created);
    fs::remove_dir_all(&private_runtime).unwrap();

    assert_missing_runtime(
        read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap_err(),
    );
    assert_missing_runtime(
        recover_interrupted_instance_runtime(&paths, &created.summary.id)
            .await
            .unwrap_err(),
    );
    assert_missing_runtime(
        update_instance(&paths, changed_mod_input(&details))
            .await
            .unwrap_err(),
    );
    assert_missing_runtime(
        materialize_instance_configuration_for_start(&paths, &created.summary.id)
            .await
            .unwrap_err(),
    );
    assert_missing_runtime(
        create_instance_backup(&paths, &created.summary.id)
            .await
            .unwrap_err(),
    );
    assert_missing_runtime(
        restore_instance_backup(&paths, &created.summary.id, &backup.backup_id)
            .await
            .unwrap_err(),
    );
    assert_missing_runtime(
        archive_instance(&paths, &created.summary.id)
            .await
            .unwrap_err(),
    );

    assert_eq!(fs::read(&created.config_file_path).unwrap(), config_before);
    assert_eq!(
        fs::read(&shared_setup).unwrap(),
        b"shared setup must stay unchanged"
    );
    assert_eq!(fs::read(&save).unwrap(), b"private save");
    assert!(!private_runtime.exists());
    assert_eq!(list_instances(&paths).await.unwrap().len(), 1);
    delete_instance(&paths, &created.summary.id).await.unwrap();
    assert!(list_instances(&paths).await.unwrap().is_empty());
    assert_eq!(
        fs::read(&shared_setup).unwrap(),
        b"shared setup must stay unchanged"
    );
    cleanup_root(&root);
}

#[tokio::test]
async fn damaged_peer_cannot_redirect_an_independent_dst_mod_setup() {
    let (root, paths, first) = runtime_recovery_fixture(false).await;
    let descriptor = test_descriptor(&root);
    replenish_test_library(&paths, &descriptor).await;
    let second = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: "Other private world".into(),
            module_id: "dontstarve".into(),
        },
    )
    .await
    .unwrap();
    let details = read_instance_details(&paths, &second.summary.id)
        .await
        .unwrap();
    let shared_setup = paths
        .games_root
        .join("dontstarve/mods/dedicated_server_mods_setup.lua");
    fs::create_dir_all(shared_setup.parent().unwrap()).unwrap();
    fs::write(&shared_setup, b"shared package sentinel").unwrap();
    let first_config = fs::read(&first.config_file_path).unwrap();
    fs::remove_dir_all(instance_private_runtime_root(&first)).unwrap();
    update_instance(&paths, changed_mod_input(&details))
        .await
        .unwrap();
    assert!(
        fs::read_to_string(
            instance_private_runtime_root(&second).join("mods/dedicated_server_mods_setup.lua")
        )
        .unwrap()
        .contains("4999999999")
    );
    assert_eq!(fs::read(&first.config_file_path).unwrap(), first_config);
    assert_eq!(fs::read(shared_setup).unwrap(), b"shared package sentinel");
    assert_missing_runtime(
        read_instance_details(&paths, &first.summary.id)
            .await
            .unwrap_err(),
    );
    cleanup_root(&root);
}

async fn runtime_recovery_fixture(
    projection: bool,
) -> (PathBuf, StoragePaths, InstanceProvisioning) {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance_with_options(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: "Recoverable private world".into(),
            module_id: "dontstarve".into(),
        },
        InstanceCreationOptions {
            private_runtime: projection.then(|| PrivateRuntimeProjection {
                private_directories: vec![PathBuf::from("private-data")],
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .provisioning;
    (root, paths, created)
}

#[tokio::test]
async fn runtime_recovery_restores_owned_rollback_before_details_without_changing_instance_files() {
    let (root, paths, created) = runtime_recovery_fixture(true).await;
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let runtime = instance_private_runtime_root(&created);
    let instance_root = runtime.parent().unwrap();
    let rollback = instance_root.join("runtime.refresh-rollback");
    let staging = instance_root.join("runtime.refresh-staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("pending.sentinel"), b"preserve staging").unwrap();
    fs::write(runtime.join("local-mod.sentinel"), b"private mod bytes").unwrap();
    let save = Path::new(&details.saves_path).join("world.sentinel");
    fs::write(&save, b"private world bytes").unwrap();
    let config_before = fs::read(&created.config_file_path).unwrap();
    fs::rename(&runtime, &rollback).unwrap();
    assert_missing_runtime(
        read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap_err(),
    );
    recover_interrupted_instance_runtime(&paths, &created.summary.id)
        .await
        .unwrap();
    assert!(!rollback.exists());
    assert_eq!(
        fs::read(runtime.join("local-mod.sentinel")).unwrap(),
        b"private mod bytes"
    );
    assert_eq!(fs::read(&save).unwrap(), b"private world bytes");
    assert_eq!(fs::read(&created.config_file_path).unwrap(), config_before);
    assert_eq!(
        fs::read(staging.join("pending.sentinel")).unwrap(),
        b"preserve staging"
    );
    let recovered = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(recovered.settings_json, details.settings_json);
    assert_eq!(recovered.saves_path, details.saves_path);
    recover_interrupted_instance_runtime(&paths, &created.summary.id)
        .await
        .unwrap();
    cleanup_root(&root);
}

#[tokio::test]
async fn runtime_recovery_preserves_active_and_unrecognized_rollback_directories() {
    let (root, paths, created) = runtime_recovery_fixture(true).await;
    let runtime = instance_private_runtime_root(&created);
    let rollback = runtime.parent().unwrap().join("runtime.refresh-rollback");
    fs::rename(&runtime, &rollback).unwrap();
    let pool = crate::storage_db::connect_pool(&paths).await.unwrap();
    sqlx::query("UPDATE instances SET status = 'running' WHERE id = ?1")
        .bind(&created.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let error = recover_interrupted_instance_runtime(&paths, &created.summary.id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("stop the instance"), "{error}");
    assert!(rollback.exists());
    assert!(!runtime.exists());
    let pool = crate::storage_db::connect_pool(&paths).await.unwrap();
    sqlx::query("UPDATE instances SET status = 'stopped' WHERE id = ?1")
        .bind(&created.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let marker = rollback.join(crate::private_runtime::PRIVATE_RUNTIME_MARKER);
    fs::write(&marker, b"unrecognized").unwrap();
    assert!(
        recover_interrupted_instance_runtime(&paths, &created.summary.id)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&marker).unwrap(), b"unrecognized");
    fs::write(&marker, b"managed\n").unwrap();
    let baseline = rollback.join(".langame-package-baseline.json");
    fs::write(&baseline, b"invalid baseline").unwrap();
    assert!(
        recover_interrupted_instance_runtime(&paths, &created.summary.id)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&baseline).unwrap(), b"invalid baseline");
    assert!(!runtime.exists());
    cleanup_root(&root);
}

#[tokio::test]
async fn runtime_recovery_rejects_database_paths_outside_managed_instances() {
    let (root, paths, created) = runtime_recovery_fixture(true).await;
    let outside = root.join("outside");
    let rollback = outside.join("runtime.refresh-rollback");
    fs::create_dir_all(outside.join("config")).unwrap();
    fs::rename(instance_private_runtime_root(&created), &rollback).unwrap();
    let pool = crate::storage_db::connect_pool(&paths).await.unwrap();
    sqlx::query("UPDATE instances SET config_path = ?1 WHERE id = ?2")
        .bind(outside.join("config").to_string_lossy().into_owned())
        .bind(&created.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let error = recover_interrupted_instance_runtime(&paths, &created.summary.id)
        .await
        .unwrap_err();
    assert!(
        matches!(error, StorageError::UnsafeManagedPath { .. }),
        "{error}"
    );
    assert!(rollback.exists());
    assert!(!outside.join("runtime").exists());
    cleanup_root(&root);
}

#[cfg(windows)]
#[tokio::test]
async fn runtime_recovery_rejects_an_instance_root_junction() {
    use std::os::windows::process::CommandExt;
    let (root, paths, created) = runtime_recovery_fixture(true).await;
    let runtime = instance_private_runtime_root(&created);
    let instance_root = runtime.parent().unwrap();
    let outside = root.join("outside-instance");
    fs::rename(&runtime, instance_root.join("runtime.refresh-rollback")).unwrap();
    fs::rename(instance_root, &outside).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(instance_root)
        .arg(&outside)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(output.status.success(), "junction fixture: {output:?}");
    let result = recover_interrupted_instance_runtime(&paths, &created.summary.id).await;
    fs::remove_dir(instance_root).expect("unlink fixture junction without deleting its target");
    assert!(
        matches!(result, Err(StorageError::UnsafeManagedPath { .. })),
        "{result:?}"
    );
    assert!(outside.join("runtime.refresh-rollback").is_dir());
    assert!(!outside.join("runtime").exists());
    cleanup_root(&root);
}
