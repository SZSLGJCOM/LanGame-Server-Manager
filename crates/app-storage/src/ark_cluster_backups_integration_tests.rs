use super::super::*;
use super::ark_evolved::{ark_test_descriptor, prepare_ark_environment};

async fn update_gameplay(
    paths: &StoragePaths,
    id: &str,
    cluster: &Path,
    map: &str,
    max_players: u32,
) -> InstanceDetails {
    let current = read_instance_details(paths, id).await.unwrap();
    let mut settings: Value = serde_json::from_str(&current.settings_json).unwrap();
    settings["cluster_id"] = json!("public-api-fixture");
    settings["cluster_directory"] = json!(cluster);
    settings["map_name"] = json!(map);
    settings["max_players"] = json!(max_players);
    update_instance(
        paths,
        UpdateInstanceInput {
            id: id.to_owned(),
            bind_ip: current.summary.bind_ip,
            ports: current.ports,
            auto_backup_on_stop: current.auto_backup_on_stop,
            backup_retention_count: current.backup_retention_count,
            settings_json: serde_json::to_string(&settings).unwrap(),
        },
    )
    .await
    .unwrap()
}

fn port_values(ports: &[PortBinding]) -> Value {
    serde_json::to_value(ports).unwrap()
}

#[tokio::test]
async fn ark_cluster_backup_public_api_restores_games_and_preserves_database_registration() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = ark_test_descriptor(&root);
    prepare_ark_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let transfer = root.join("exclusive-transfer");
    fs::create_dir(&transfer).unwrap();
    fs::write(transfer.join("upload"), b"original-transfer").unwrap();
    let mut original = Vec::new();
    for (name, map) in [("Island", "TheIsland"), ("Center", "TheCenter")] {
        replenish_test_library(&paths, &descriptor).await;
        let created = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: name.to_owned(),
                module_id: "arksurvivalevolved".to_owned(),
            },
        )
        .await
        .unwrap();
        let details = update_gameplay(&paths, &created.summary.id, &transfer, map, 20).await;
        fs::create_dir_all(&details.saves_path).unwrap();
        fs::write(
            Path::new(&details.saves_path).join("world.ark"),
            format!("original-{name}"),
        )
        .unwrap();
        original.push(details);
    }
    let anchor = &original[0].summary.id;
    let report = read_ark_cluster_report(&paths, anchor).await.unwrap();
    assert!(!report.start_blocked, "{:?}", report.issues);
    let identity = report.identity.unwrap();
    assert_eq!(identity.member_ids.len(), 2);

    // Public admission must reject contention, stale membership and active state.
    let held = crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
        &paths,
        &original[1].summary.id,
    )
    .unwrap();
    assert!(
        create_ark_cluster_backup(&paths, anchor, &identity, true)
            .await
            .is_err()
    );
    drop(held);
    let mut stale = identity.clone();
    stale.member_ids.pop();
    assert!(
        create_ark_cluster_backup(&paths, anchor, &stale, true)
            .await
            .is_err()
    );
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("UPDATE instances SET status = 'running' WHERE id = ?1")
        .bind(anchor)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        create_ark_cluster_backup(&paths, anchor, &identity, true)
            .await
            .is_err()
    );
    sqlx::query("UPDATE instances SET status = 'stopped' WHERE id = ?1")
        .bind(anchor)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let backup = create_ark_cluster_backup(&paths, anchor, &identity, true)
        .await
        .unwrap();
    assert_eq!(
        list_ark_cluster_backups(&paths, anchor)
            .await
            .unwrap()
            .len(),
        1
    );

    for (details, map) in original.iter().zip(["TheIsland", "TheCenter"]) {
        update_gameplay(&paths, &details.summary.id, &transfer, map, 31).await;
        fs::write(
            Path::new(&details.saves_path).join("world.ark"),
            b"modified-world",
        )
        .unwrap();
    }
    fs::write(transfer.join("upload"), b"modified-transfer").unwrap();

    // Restoring whole configuration must never silently reintroduce old network
    // or registration mirrors while the database continues to claim new values.
    update_instance_autostart(&paths, anchor, true)
        .await
        .unwrap();
    let error = restore_ark_cluster_backup(&paths, anchor, &identity, &backup.backup_id, true)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Snapshot registration differs"));
    assert_eq!(
        fs::read(transfer.join("upload")).unwrap(),
        b"modified-transfer"
    );
    assert_eq!(
        list_ark_cluster_backups(&paths, anchor)
            .await
            .unwrap()
            .len(),
        1
    );
    update_instance_autostart(&paths, anchor, false)
        .await
        .unwrap();

    let original_ports = original[0].ports.clone();
    let mut changed_ports = original_ports.clone();
    changed_ports.iter_mut().for_each(|port| port.port += 1000);
    update_instance_ports(&paths, anchor, &changed_ports)
        .await
        .unwrap();
    assert!(
        restore_ark_cluster_backup(&paths, anchor, &identity, &backup.backup_id, true)
            .await
            .unwrap_err()
            .to_string()
            .contains("Snapshot registration differs")
    );
    update_instance_ports(&paths, anchor, &original_ports)
        .await
        .unwrap();

    let result = restore_ark_cluster_backup(&paths, anchor, &identity, &backup.backup_id, true)
        .await
        .unwrap();
    assert_eq!(result.safeguard_backup.backup_kind, "pre_restore");
    assert!(result.cleanup_warnings.is_empty());
    for before in &original {
        let after = read_instance_details(&paths, &before.summary.id)
            .await
            .unwrap();
        assert_eq!(after.summary.name, before.summary.name);
        assert_eq!(after.summary.bind_ip, before.summary.bind_ip);
        assert_eq!(after.summary.autostart, before.summary.autostart);
        assert_eq!(port_values(&after.ports), port_values(&before.ports));
        let settings: Value = serde_json::from_str(&after.settings_json).unwrap();
        assert_eq!(settings["max_players"], json!(20));
        let mirror: Value =
            serde_json::from_slice(&fs::read(&after.config_file_path).unwrap()).unwrap();
        assert_eq!(mirror["instance_name"], json!(after.summary.name));
        assert_eq!(mirror["autostart"], json!(after.summary.autostart));
        assert_eq!(mirror["ports"], port_values(&after.ports));
        assert_eq!(mirror["settings"]["bind_ip"], json!(after.summary.bind_ip));
        assert_eq!(
            fs::read(Path::new(&after.saves_path).join("world.ark")).unwrap(),
            format!("original-{}", before.summary.name).as_bytes()
        );
        assert!(
            read_pending_ark_cluster_restore(&paths, &before.summary.id)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        fs::read(transfer.join("upload")).unwrap(),
        b"original-transfer"
    );
    assert_eq!(
        list_ark_cluster_backups(&paths, anchor)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        fs::canonicalize(&root).unwrap().parent(),
        Some(fs::canonicalize(std::env::temp_dir()).unwrap().as_path())
    );
    cleanup_root(&root);
}
