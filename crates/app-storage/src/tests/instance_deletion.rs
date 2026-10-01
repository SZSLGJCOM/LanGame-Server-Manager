#[tokio::test]
async fn archive_then_delete_instance_preserves_the_archive_and_clears_the_restored_root() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = declared_save_path_test_descriptor(&root);
    prepare_declared_save_path_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Save Path Delete"),
            module_id: String::from("savepathtest"),
        },
    )
    .await
    .unwrap();

    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let instance_root = root.join("instances").join(&created.summary.id);
    let managed_saves_root = PathBuf::from(&details.saves_path);
    fs::create_dir_all(&managed_saves_root).unwrap();
    fs::write(
        instance_root.join("config").join("notes.txt"),
        "config-sentinel",
    )
    .unwrap();
    fs::write(managed_saves_root.join("world.db"), "managed-world").unwrap();
    let backup = create_instance_backup(&paths, &created.summary.id)
        .await
        .unwrap();
    let traversal_error = delete_instance_backup(&paths, &created.summary.id, "../config")
        .await
        .expect_err("backup traversal must be rejected");
    assert!(matches!(
        traversal_error,
        StorageError::InvalidBackupId { .. }
    ));
    assert!(instance_root.join("config").join("notes.txt").is_file());

    let deleted = archive_instance(&paths, &created.summary.id).await.unwrap();
    let archived_root = PathBuf::from(
        deleted
            .archived_instance_root
            .clone()
            .expect("instance root should be archived"),
    );

    assert_eq!(deleted.instance_id, created.summary.id);
    assert_eq!(deleted.instance_name, created.summary.name);
    assert!(deleted.saves_archived_with_instance_root);
    assert_eq!(deleted.preserved_external_saves_path, None);
    assert!(!instance_root.exists());
    assert!(archived_root.join("config").join("notes.txt").exists());
    assert_eq!(
        fs::read_to_string(
            archived_root
                .join("config")
                .join("savegame")
                .join("world.db")
        )
        .unwrap(),
        "managed-world"
    );
    assert!(
        archived_root
            .join("backups")
            .join(&backup.backup_id)
            .join("backup.json")
            .exists()
    );
    assert!(matches!(
        read_instance_details(&paths, &created.summary.id).await,
        Err(StorageError::MissingInstance { .. })
    ));
    assert!(
        list_instances(&paths)
            .await
            .unwrap()
            .iter()
            .all(|instance| instance.id != created.summary.id)
    );

    restore_instance_archive(&paths, &deleted.archive_id).await.unwrap();
    assert_eq!(fs::read_to_string(managed_saves_root.join("world.db")).unwrap(), "managed-world");
    assert!(instance_root.join("backups").join(&backup.backup_id).is_dir());
    let removed = delete_instance(&paths, &created.summary.id).await.unwrap();
    assert_eq!(Path::new(&removed.deleted_instance_root), instance_root);
    assert!(!instance_root.exists());
    assert!(!archived_root.exists());
    let archives = list_instance_archives(&paths).await.unwrap();
    assert!(archives.archives.is_empty());
    assert!(archives.pending_deletions.is_empty());
    cleanup_root(&root);
}
#[tokio::test]
async fn delete_instance_refuses_paths_outside_managed_instances_root() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = declared_save_path_test_descriptor(&root);
    prepare_declared_save_path_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Unsafe Delete"),
            module_id: String::from("savepathtest"),
        },
    )
    .await
    .unwrap();

    let escaped_config_root = root.join("escaped-root").join("config");
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("UPDATE instances SET config_path = ?2 WHERE id = ?1")
        .bind(&created.summary.id)
        .bind(escaped_config_root.to_string_lossy().into_owned())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let error = delete_instance(&paths, &created.summary.id)
        .await
        .expect_err("delete should refuse unmanaged paths");
    assert!(matches!(error, StorageError::UnsafeManagedPath { .. }));
    assert!(
        list_instances(&paths)
            .await
            .unwrap()
            .iter()
            .any(|instance| instance.id == created.summary.id)
    );

    cleanup_root(&root);
}
