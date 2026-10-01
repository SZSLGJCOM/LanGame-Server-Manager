use super::*;

async fn connection_fixture() -> (PathBuf, StoragePaths) {
    let root = unique_test_root();
    let paths = test_paths(&root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query(
        "INSERT INTO modules (id, name, version) VALUES ('network-fixture', 'Network', '1')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO instances (id, name, module_id, bind_ip, data_path, config_path, logs_path, saves_path) \
         VALUES ('network-one', 'Network One', 'network-fixture', '127.0.0.1', 'data', 'config', 'logs', 'saves')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    (root, paths)
}

#[tokio::test]
async fn instance_port_projection_distinguishes_empty_ports_from_a_missing_instance() {
    let (root, paths) = connection_fixture().await;
    let projections = read_instance_port_projections(
        &paths,
        &[String::from("network-one"), String::from("network-one")],
    )
    .await
    .unwrap();
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].instance_id, "network-one");
    assert_eq!(projections[0].module_id, "network-fixture");
    assert_eq!(projections[0].bind_ip, "127.0.0.1");
    assert!(projections[0].ports.is_empty());

    let error = read_instance_port_projections(
        &paths,
        &[String::from("network-one"), String::from("missing")],
    )
    .await
    .unwrap_err();
    assert!(matches!(error, StorageError::MissingInstance { id } if id == "missing"));
    cleanup_root(&root);
}

#[tokio::test]
async fn instance_port_projection_reads_persisted_address_and_ports_after_changes() {
    let (root, paths) = connection_fixture().await;
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("UPDATE instances SET bind_ip = '::1' WHERE id = 'network-one'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO instance_ports (instance_id, name, protocol, port) \
         VALUES ('network-one', 'game', 'tcp', 31015), ('network-one', 'query', 'udp', 31015)",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    let projections = read_instance_port_projections(&paths, &[String::from("network-one")])
        .await
        .unwrap();
    assert_eq!(projections[0].bind_ip, "::1");
    assert_eq!(projections[0].ports.len(), 2);
    assert_eq!(projections[0].ports[0].name, "game");
    assert_eq!(projections[0].ports[0].protocol, "tcp");
    assert_eq!(projections[0].ports[0].port, 31015);
    assert_eq!(projections[0].ports[1].protocol, "udp");
    assert_eq!(projections[0].ports[1].port, 31015);
    cleanup_root(&root);
}

#[tokio::test]
async fn instance_port_projection_rejects_excessive_rows_without_returning_partial_ports() {
    let (root, paths) = connection_fixture().await;
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query(
        "WITH RECURSIVE ports(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM ports WHERE value < 4097) \
         INSERT INTO instance_ports (instance_id, name, protocol, port) \
         SELECT 'network-one', 'port-' || value, 'udp', value FROM ports",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    let error = read_instance_port_projections(&paths, &[String::from("network-one")])
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::InstancePortProjectionRowLimitExceeded { max: 4096 }
    ));
    cleanup_root(&root);
}
