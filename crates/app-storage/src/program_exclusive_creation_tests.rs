use super::*;

#[path = "program_creation_availability_tests.rs"]
mod availability;

#[path = "program_exclusive_reuse_tests.rs"]
mod reuse;

async fn register(fixture: &CleanCreationFixture) {
    sync_game_installs(
        &fixture.paths,
        &[GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: fixture.source.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("original-build".into()),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn existing_library_inventory_does_not_wait_for_unrelated_archive_mutation() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    register(&fixture).await;
    let _archive = crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
        &fixture.paths,
        "archive-inventory",
    )
    .unwrap();
    let inventory = crate::inspect_module_programs(
        &fixture.paths,
        &fixture.descriptor,
        None,
        app_core::InstanceProgramSource::Verified,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        false,
    )
    .await
    .unwrap();
    assert!(!inventory.requires_archive_inventory);
    assert!(inventory.creation.can_create);
    assert_eq!(inventory.creation.action, "existing_install");
}

#[tokio::test]
async fn missing_library_requests_archive_inventory_without_bypassing_its_lock() {
    let fixture = CleanCreationFixture::new().await;
    let archive = crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
        &fixture.paths,
        "archive-inventory",
    )
    .unwrap();
    let inspect = |include_archived_sources| {
        crate::inspect_module_programs(
            &fixture.paths,
            &fixture.descriptor,
            None,
            app_core::InstanceProgramSource::Verified,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            include_archived_sources,
        )
    };
    let pending = inspect(false).await.unwrap();
    assert!(pending.requires_archive_inventory);
    assert!(!pending.creation.can_create);
    assert!(
        inspect(true).await.is_err(),
        "archive inspection must retain the shared inventory lock"
    );
    drop(archive);
    let complete = inspect(true).await.unwrap();
    assert!(!complete.requires_archive_inventory);
    assert!(!complete.creation.can_create);
}

async fn create(
    fixture: &CleanCreationFixture,
    name: &str,
) -> Result<CreateInstanceResult, StorageError> {
    create_instance_with_options(
        &fixture.paths,
        &fixture.descriptor,
        CreateInstanceInput {
            name: name.into(),
            module_id: fixture.descriptor.summary.id.clone(),
        },
        InstanceCreationOptions {
            prefer_existing_install: true,
            require_clean_program: true,
            program_mode: Some(InstanceProgramMode::Independent),
            ..Default::default()
        },
    )
    .await
}

#[cfg(windows)]
#[tokio::test]
async fn first_instance_reuses_installer_verification_without_reading_payload_again() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    register(&fixture).await;
    let created = create_instance_with_options(
        &fixture.paths,
        &fixture.descriptor,
        CreateInstanceInput {
            name: "First instance from verified download".into(),
            module_id: fixture.descriptor.summary.id.clone(),
        },
        InstanceCreationOptions {
            prefer_existing_install: true,
            require_clean_program: true,
            program_mode: Some(InstanceProgramMode::Independent),
            cancellation: Some(Arc::clone(&cancellation)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        reads.chunks(),
        0,
        "a fresh first instance must reuse installer content verification"
    );
    assert_eq!(
        fs::canonicalize(&created.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    let instance_root = fixture
        .paths
        .instances_root
        .join(&created.provisioning.summary.id);
    assert!(!instance_root.join("runtime/payload.bin").exists());
    assert!(instance_uses_exclusive_program(&instance_root).unwrap());
    assert_eq!(
        fs::read(fixture.source.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
}

#[tokio::test]
async fn first_instance_uses_downloaded_files_and_deletion_retains_the_installation() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    register(&fixture).await;
    let plan = inspect_instance_program_creation(
        &fixture.paths,
        &fixture.descriptor,
        None,
        app_core::InstanceProgramSource::Verified,
        None,
    )
    .await
    .unwrap();
    assert_eq!(plan.action, "existing_install");
    let first = create(&fixture, "First exclusive server").await.unwrap();
    let id = &first.provisioning.summary.id;
    let root = fixture.paths.instances_root.join(id);
    assert_eq!(
        fs::canonicalize(&first.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    assert!(!root.join("runtime/payload.bin").exists());
    assert!(instance_uses_exclusive_program(&root).unwrap());
    assert!(instance_uses_library_program(&root).unwrap());
    assert_eq!(
        instance_program_mode(&root).unwrap(),
        InstanceProgramMode::Independent
    );
    let registered = read_instance_program_install(&fixture.paths, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(registered.install.scope, ProgramInstallScope::Library);
    assert_eq!(registered.install.owner_instance_id, None);
    // Exercise the actual persisted relation removal. Filesystem archive and
    // deletion have separate recovery tests and never own this library row.
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("DELETE FROM instances WHERE id=?1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        read_instance_program_install(&fixture.paths, id)
            .await
            .unwrap()
            .is_none()
    );
    let retained = read_library_program_install(&fixture.paths, &fixture.descriptor.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retained.id, registered.install.id);
    assert_eq!(
        fs::read(fixture.source.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
    let next = create(&fixture, "Fresh after deletion").await.unwrap();
    assert_ne!(
        fs::canonicalize(next.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
}

#[tokio::test]
async fn second_independent_instance_never_inherits_first_instance_unknown_files_or_mods() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    register(&fixture).await;
    let first = create(&fixture, "First exclusive server").await.unwrap();
    fs::create_dir_all(fixture.source.join("mods/workshop-1234")).unwrap();
    fs::write(
        fixture.source.join("mods/workshop-1234/modmain.lua"),
        b"personal mod",
    )
    .unwrap();
    fs::write(fixture.source.join("world.sav"), b"private first world").unwrap();
    let local_plan = inspect_instance_program_creation(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Local,
        None,
    )
    .await
    .unwrap();
    assert!(!local_plan.can_create);
    let local_error = create_instance_with_options(
        &fixture.paths,
        &fixture.descriptor,
        CreateInstanceInput {
            name: "No implicit clone".into(),
            module_id: fixture.descriptor.summary.id.clone(),
        },
        InstanceCreationOptions {
            prefer_existing_install: true,
            use_local_program: true,
            program_mode: Some(InstanceProgramMode::Independent),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(local_error.to_string().contains("managed instance data"));
    let second = create(&fixture, "Second independent server").await.unwrap();
    assert_ne!(second.effective_install_root, first.effective_install_root);
    assert!(!second.effective_install_root.join("world.sav").exists());
    assert!(
        !second
            .effective_install_root
            .join("mods/workshop-1234")
            .exists()
    );
    assert_eq!(
        fs::read(second.effective_install_root.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
    fs::write(
        second.effective_install_root.join("payload.bin"),
        b"second changed only",
    )
    .unwrap();
    assert_eq!(
        fs::read(fixture.source.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
    let owned = read_instance_program_install(&fixture.paths, &second.provisioning.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owned.install.scope, ProgramInstallScope::Instance);
    assert_eq!(
        owned.install.owner_instance_id.as_deref(),
        Some(second.provisioning.summary.id.as_str())
    );
}

#[tokio::test]
async fn untrusted_added_files_force_a_clean_copy_before_the_first_instance() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    register(&fixture).await;
    fs::write(
        fixture.source.join("unknown-loader.dll"),
        b"untrusted extension",
    )
    .unwrap();
    let created = create(&fixture, "No implicit import").await.unwrap();
    assert_ne!(created.effective_install_root, fixture.source);
    assert!(
        !created
            .effective_install_root
            .join("unknown-loader.dll")
            .exists()
    );
    assert_eq!(
        fs::read(fixture.source.join("unknown-loader.dll")).unwrap(),
        b"untrusted extension"
    );
}

#[tokio::test]
async fn official_excluded_defaults_allow_direct_use_but_modified_defaults_do_not() {
    let mut fixture = CleanCreationFixture::new().await;
    fixture
        .descriptor
        .storage
        .runtime_copy_exclusions
        .push("official-settings.ini".into());
    fs::write(
        fixture.source.join("official-settings.ini"),
        b"shipped defaults",
    )
    .unwrap();
    fixture.record();
    register(&fixture).await;
    let plan = inspect_instance_program_creation(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Verified,
        None,
    )
    .await
    .unwrap();
    assert_eq!(plan.action, "existing_install");
    fs::write(
        fixture.source.join("official-settings.ini"),
        b"personal settings",
    )
    .unwrap();
    assert!(matches!(
        create(&fixture, "No hidden settings import")
            .await
            .unwrap_err(),
        StorageError::CleanLibraryProgramRequired { .. }
    ));
    fixture.assert_no_instance().await;
    fs::write(
        fixture.source.join("official-settings.ini"),
        b"shipped defaults",
    )
    .unwrap();
    let created = create(&fixture, "Official original settings")
        .await
        .unwrap();
    assert_eq!(
        fs::canonicalize(created.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
}

#[tokio::test]
async fn cancelled_installation_preview_stops_before_inspecting_the_library() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    register(&fixture).await;
    let cancellation = Arc::new(AtomicBool::new(true));
    let error = inspect_instance_program_creation(
        &fixture.paths,
        &fixture.descriptor,
        None,
        app_core::InstanceProgramSource::Verified,
        Some(cancellation),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, StorageError::InstanceCreationCancelled));
}
