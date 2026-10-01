use super::super::*;
use super::ark_ascended_support::{ark_ascended_test_descriptor, prepare_ark_ascended_environment};
use super::ark_evolved::{ark_test_descriptor, prepare_ark_environment};

#[path = "tests_ark_maps_storage.rs"]
mod map_storage;

#[tokio::test]
async fn ark_ascended_runtime_overview_prefers_shootergame_log_when_newer() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = ark_ascended_test_descriptor(&root);
    prepare_ark_ascended_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("arksa");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("arksurvivalascended"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("arksa-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("ASA Runtime"),
            module_id: String::from("arksurvivalascended"),
        },
    )
    .await
    .unwrap();

    let entrypoint_log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-entrypoint.log");
    fs::write(&entrypoint_log_path, "script entrypoint boot\n").unwrap();

    std::thread::sleep(Duration::from_millis(20));

    let shared_log = install_root.join("ShooterGame/Saved/Logs/ShooterGame.log");
    fs::create_dir_all(shared_log.parent().unwrap()).unwrap();
    fs::write(&shared_log, b"shared package log must not be selected").unwrap();
    let shooter_log_path = instance_private_runtime_root(&created)
        .join("ShooterGame")
        .join("Saved")
        .join("Logs")
        .join("ShooterGame.log");
    fs::create_dir_all(shooter_log_path.parent().unwrap()).unwrap();
    fs::write(
        &shooter_log_path,
        "[2026.04.11-10.45.41:918][ 13]Server: \"ASA Runtime\" has successfully started!\n[2026.04.11-10.46.40:245][238]Server has completed startup and is now advertising for join. (10.10GB Mem)\n",
    )
    .unwrap();

    let run = record_started_test_instance(
        &paths,
        &created.summary.id,
        8123,
        &entrypoint_log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        overview.log_tail.source_path,
        Some(shooter_log_path.to_string_lossy().into_owned())
    );
    assert!(
        overview
            .log_tail
            .lines
            .iter()
            .any(|line| line.contains("advertising for join"))
    );

    let document = read_instance_log_document(&paths, &created.summary.id, 10, None)
        .await
        .unwrap();
    assert_eq!(
        document.source_path,
        Some(shooter_log_path.to_string_lossy().into_owned())
    );
    assert!(
        document
            .lines
            .iter()
            .any(|line| line.contains("has successfully started"))
    );

    let explicit_run_document =
        read_instance_log_document(&paths, &created.summary.id, 10, Some(run.run_id))
            .await
            .unwrap();
    assert_eq!(
        explicit_run_document.source_path,
        Some(entrypoint_log_path.to_string_lossy().into_owned())
    );
    assert!(
        explicit_run_document
            .lines
            .iter()
            .any(|line| line.contains("script entrypoint boot"))
    );

    assert_eq!(
        fs::read(shared_log).unwrap(),
        b"shared package log must not be selected"
    );
    cleanup_root(&root);
}

#[tokio::test]
async fn ark_evolved_runtime_overview_falls_back_to_entrypoint_log_when_shootergame_log_empty() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = ark_test_descriptor(&root);
    prepare_ark_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("arkse");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("arksurvivalevolved"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("arkse-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("ASE Runtime"),
            module_id: String::from("arksurvivalevolved"),
        },
    )
    .await
    .unwrap();

    let entrypoint_log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-entrypoint.log");
    fs::write(
        &entrypoint_log_path,
        "bootstrap launch\nwaiting for game runtime log\n",
    )
    .unwrap();

    let shooter_log_path = install_root
        .join("ShooterGame")
        .join("Saved")
        .join("Logs")
        .join("ShooterGame.log");
    fs::create_dir_all(shooter_log_path.parent().unwrap()).unwrap();
    fs::write(&shooter_log_path, "").unwrap();

    let run = record_started_test_instance(
        &paths,
        &created.summary.id,
        8124,
        &entrypoint_log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        overview.log_tail.source_path,
        Some(entrypoint_log_path.to_string_lossy().into_owned())
    );
    assert!(
        overview
            .log_tail
            .lines
            .iter()
            .any(|line| line.contains("bootstrap launch"))
    );

    let document = read_instance_log_document(&paths, &created.summary.id, 10, None)
        .await
        .unwrap();
    assert_eq!(
        document.source_path,
        Some(entrypoint_log_path.to_string_lossy().into_owned())
    );
    assert!(
        document
            .lines
            .iter()
            .any(|line| line.contains("waiting for game runtime log"))
    );

    let explicit_run_document =
        read_instance_log_document(&paths, &created.summary.id, 10, Some(run.run_id))
            .await
            .unwrap();
    assert_eq!(
        explicit_run_document.source_path,
        Some(entrypoint_log_path.to_string_lossy().into_owned())
    );

    cleanup_root(&root);
}

#[cfg(windows)]
#[tokio::test]
async fn ark_evolved_runtime_overview_reports_ready_from_bound_udp_ports() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = ark_test_descriptor(&root);
    prepare_ark_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("arkse");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("arksurvivalevolved"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("arkse-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("ASE Ready"),
            module_id: String::from("arksurvivalevolved"),
        },
    )
    .await
    .unwrap();

    configure_test_instance_runtime(&paths, &created.summary.id, "127.0.0.1", false).await;

    let game_socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
    let query_socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
    replace_instance_ports_for_test(
        &paths,
        &created.summary.id,
        &[
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: game_socket.local_addr().unwrap().port(),
            },
            PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: query_socket.local_addr().unwrap().port(),
            },
        ],
    )
    .await;

    let entrypoint_log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-entrypoint.log");
    fs::write(&entrypoint_log_path, "bootstrap launch\n").unwrap();

    let shooter_log_path = install_root
        .join("ShooterGame")
        .join("Saved")
        .join("Logs")
        .join("ShooterGame.log");
    fs::create_dir_all(shooter_log_path.parent().unwrap()).unwrap();
    fs::write(&shooter_log_path, "").unwrap();

    record_started_test_instance(
        &paths,
        &created.summary.id,
        std::process::id(),
        &entrypoint_log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    std::thread::sleep(Duration::from_millis(150));

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(overview.health.status, "ready");
    assert!(
        overview
            .health
            .summary
            .to_ascii_lowercase()
            .contains("required udp ports"),
        "unexpected runtime health summary: {}",
        overview.health.summary
    );
    assert_eq!(
        overview.log_tail.source_path,
        Some(entrypoint_log_path.to_string_lossy().into_owned())
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn archive_then_delete_private_saves_when_module_uses_install_root_storage() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = ark_ascended_test_descriptor(&root);
    prepare_ark_ascended_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("arksa");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("arksurvivalascended"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("arksa-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("ASA Delete"),
            module_id: String::from("arksurvivalascended"),
        },
    )
    .await
    .unwrap();

    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let private_saves_root = PathBuf::from(&details.saves_path);
    let world_file = private_saves_root
        .join("TheIsland_WP")
        .join("SavedArks")
        .join("world.ark");
    fs::create_dir_all(world_file.parent().unwrap()).unwrap();
    fs::write(&world_file, "private-world").unwrap();
    let backup = create_instance_backup(&paths, &created.summary.id)
        .await
        .unwrap();

    let deleted = archive_instance(&paths, &created.summary.id).await.unwrap();
    let archived_root = PathBuf::from(
        deleted
            .archived_instance_root
            .clone()
            .expect("instance root should be archived"),
    );

    assert_eq!(deleted.instance_id, created.summary.id);
    assert!(deleted.saves_archived_with_instance_root);
    assert_eq!(deleted.preserved_external_saves_path, None);
    assert_eq!(
        fs::read_to_string(
            archived_root.join(
                world_file
                    .strip_prefix(paths.instances_root.join(&created.summary.id))
                    .unwrap()
            )
        )
        .unwrap(),
        "private-world"
    );
    assert!(!world_file.exists());
    assert!(
        !install_root
            .join("ShooterGame/Saved")
            .join(&created.summary.id)
            .exists()
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
    restore_instance_archive(&paths, &deleted.archive_id)
        .await
        .unwrap();
    assert_eq!(fs::read_to_string(&world_file).unwrap(), "private-world");
    let removed = delete_instance(&paths, &created.summary.id).await.unwrap();
    assert!(!Path::new(&removed.deleted_instance_root).exists());
    assert!(!world_file.exists());
    assert!(!archived_root.exists());
    assert!(install_root.is_dir());
    let archives = list_instance_archives(&paths).await.unwrap();
    assert!(archives.archives.is_empty());
    assert!(archives.pending_deletions.is_empty());
    cleanup_root(&root);
}
