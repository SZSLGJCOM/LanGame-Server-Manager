use super::*;

async fn create_unrelated_instance(fixture: &Fixture) -> String {
    let module = fixture.paths.modules_root.join("other");
    fs::create_dir_all(&module).unwrap();
    let definition = fs::read_to_string(fixture.paths.modules_root.join("fixture/module.toml"))
        .unwrap()
        .replace("fixture", "other");
    fs::write(module.join("module.toml"), definition).unwrap();
    let descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "other")
        .unwrap();
    crate::sync_modules(
        &fixture.paths,
        &[fixture.descriptor.clone(), descriptor.clone()],
    )
    .await
    .unwrap();
    let library = fixture.paths.games_root.join("other");
    fs::create_dir_all(&library).unwrap();
    fs::write(library.join("server.bin"), b"unrelated official program").unwrap();
    crate::record_library_program_baseline(&library, &descriptor, true, None).unwrap();
    crate::sync_game_installs(
        &fixture.paths,
        &[GameInstallSyncRecord {
            module_id: "other".into(),
            install_root: library.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("other-build".into()),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
    crate::create_instance_with_options(
        &fixture.paths,
        &descriptor,
        CreateInstanceInput {
            name: "Unrelated server".into(),
            module_id: "other".into(),
        },
        InstanceCreationOptions {
            require_clean_program: true,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .provisioning
    .summary
    .id
}

#[tokio::test]
async fn archived_seed_copy_blocks_its_source_but_allows_unrelated_archive_lifecycle() {
    let fixture = Fixture::new().await;
    let other = create_unrelated_instance(&fixture).await;
    let before = tree_snapshot(&fixture.archive).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut pause = pause_at(&cancellation, PausePoint::Copy);
    let paths = fixture.paths.clone();
    let descriptor = fixture.descriptor.clone();
    let worker = tokio::spawn(async move {
        crate::prepare_clean_library_seed(&paths, &descriptor, Some(cancellation)).await
    });
    pause.reached().await;
    assert!(matches!(
        crate::restore_instance_archive(&fixture.paths, &fixture.archive_id).await,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    assert!(matches!(
        crate::purge_instance_archive(&fixture.paths, &fixture.archive_id).await,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    let archived = crate::archive_instance(&fixture.paths, &other)
        .await
        .unwrap();
    crate::restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    let archived = crate::archive_instance(&fixture.paths, &other)
        .await
        .unwrap();
    crate::purge_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert!(
        !worker.is_finished(),
        "seed copy must remain paused during unrelated work"
    );
    assert_eq!(tree_snapshot(&fixture.archive).unwrap(), before);
    drop(pause);
    let seed = worker.await.unwrap().unwrap();
    assert!(!seed.requires_validation);
    assert_eq!(seed.current_version.as_deref(), Some("build-17"));
    assert_eq!(
        fs::read(seed.install_root.join("server.bin")).unwrap(),
        b"official executable"
    );
    crate::restore_instance_archive(&fixture.paths, &fixture.archive_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn archived_seed_reports_a_busy_source_without_publishing_an_empty_seed() {
    let fixture = Fixture::new().await;
    let _writer =
        crate::instance_archive::program_source_mutation_lock(&fixture.paths, &fixture.archive_id)
            .unwrap();
    assert!(matches!(
        crate::prepare_clean_library_seed(&fixture.paths, &fixture.descriptor, None).await,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    assert!(!fixture.paths.games_root.join("fixture").exists());
}

#[tokio::test]
async fn archived_seed_can_start_while_unrelated_archive_catalog_mutation_is_admitted() {
    let fixture = Fixture::new().await;
    let before = tree_snapshot(&fixture.archive).unwrap();
    let _inventory = inventory_lock(&fixture.paths).unwrap();
    let seed = crate::prepare_clean_library_seed(&fixture.paths, &fixture.descriptor, None)
        .await
        .unwrap();
    assert!(!seed.requires_validation);
    assert_eq!(seed.current_version.as_deref(), Some("build-17"));
    assert_eq!(
        fs::read(seed.install_root.join("server.bin")).unwrap(),
        b"official executable"
    );
    assert_eq!(tree_snapshot(&fixture.archive).unwrap(), before);
}

#[tokio::test]
async fn source_lease_revalidates_an_archive_restored_after_catalog_enumeration() {
    let fixture = Fixture::new().await;
    let candidates = read_archived_program_sources(&fixture.paths).await.unwrap();
    assert_eq!(candidates.len(), 1);
    crate::restore_instance_archive(&fixture.paths, &fixture.archive_id)
        .await
        .unwrap();
    assert!(
        lease_archived_program_source(&fixture.paths, &candidates[0].archive_id)
            .await
            .unwrap()
            .is_none()
    );
}
