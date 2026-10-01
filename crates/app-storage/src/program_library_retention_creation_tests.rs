use super::*;
use crate::program_library_retention::{RETAINED_LIBRARY, retained_library_program_source};

async fn register_library(fixture: &CleanCreationFixture, root: &Path, state: InstallState) -> i64 {
    sync_game_installs(
        &fixture.paths,
        &[GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: state,
            current_version: Some("official-build".into()),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
    crate::read_program_install_owner(&fixture.paths, root)
        .await
        .unwrap()
        .unwrap()
        .id
}

async fn create(fixture: &CleanCreationFixture, name: &str) -> CreateInstanceResult {
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
    .unwrap()
}

#[tokio::test]
async fn retained_library_survives_real_instance_deletions_and_seeds_further_copies() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    let original_id = register_library(&fixture, &fixture.source, InstallState::Installed).await;
    let first = create(&fixture, "Original first").await;
    let second = create(&fixture, "Healthy second").await;
    fs::write(
        fixture.source.join("payload.bin"),
        b"modified first program",
    )
    .unwrap();
    fs::write(fixture.source.join("personal.cfg"), b"first instance data").unwrap();
    let original = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();

    let library = fixture
        .paths
        .games_root
        .join("dontstarve-original-retained");
    let seed =
        crate::prepare_clean_library_seed_at(&fixture.paths, &fixture.descriptor, &library, None)
            .await
            .unwrap();
    assert!(
        !seed.requires_validation,
        "healthy second instance must supply the complete package"
    );
    assert_eq!(seed.current_version.as_deref(), Some("official-build"));
    crate::retain_library_program_source(&fixture.paths, &library, &fixture.descriptor).unwrap();
    let library_id = register_library(&fixture, &library, InstallState::Installed).await;
    let library_files = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    assert!(!library.join("personal.cfg").exists());
    let third = create(&fixture, "Copy from repaired library").await;
    assert_ne!(third.effective_install_root, library);
    let binding = read_instance_program_install(&fixture.paths, &third.provisioning.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(binding.install.scope, ProgramInstallScope::Instance);
    assert_eq!(
        binding.install.owner_instance_id.as_deref(),
        Some(third.provisioning.summary.id.as_str())
    );

    for instance in [&third, &second, &first] {
        let id = &instance.provisioning.summary.id;
        crate::delete_instance(&fixture.paths, id).await.unwrap();
        assert!(!fixture.paths.instances_root.join(id).exists());
        if id != &first.provisioning.summary.id {
            assert!(!instance.effective_install_root.exists());
        }
    }
    assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        original
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        library_files
    );
    let retained = crate::read_program_install_owner(&fixture.paths, &library)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retained.id, library_id);
    assert_eq!(retained.scope, ProgramInstallScope::Library);
    assert_eq!(retained.owner_instance_id, None);
    let cleanup = crate::plan_module_library_cleanup(&fixture.paths, &fixture.descriptor, true)
        .await
        .unwrap();
    assert_eq!(cleanup.keep_install_id, Some(library_id));
    assert!(
        cleanup
            .installations
            .iter()
            .any(|record| record.id == original_id)
    );
    let protected = crate::library_cleanup_retained_paths(&fixture.source, &fixture.descriptor)
        .unwrap()
        .unwrap();
    assert!(protected.contains(&fs::canonicalize(fixture.source.join("payload.bin")).unwrap()));
    assert!(protected.contains(&fs::canonicalize(fixture.source.join("personal.cfg")).unwrap()));

    for name in ["Recreated unique instance", "Recreated second instance"] {
        let created = create(&fixture, name).await;
        assert_ne!(created.effective_install_root, library);
        assert_eq!(
            fs::read(created.effective_install_root.join("payload.bin")).unwrap(),
            vec![7; 512 * 1024 + 1]
        );
        assert!(!created.effective_install_root.join("personal.cfg").exists());
    }
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        library_files
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        original
    );
    let uninstall = crate::plan_module_library_cleanup(&fixture.paths, &fixture.descriptor, false)
        .await
        .unwrap();
    assert_eq!(uninstall.keep_install_id, None);
    assert_eq!(
        uninstall
            .installations
            .iter()
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        vec![original_id, library_id]
    );
}

#[tokio::test]
async fn retained_library_cleanup_prefers_the_newest_installed_marked_source() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    let default = register_library(&fixture, &fixture.source, InstallState::Installed).await;
    let mut expected = None;
    for name in ["first", "second"] {
        let root = fixture
            .paths
            .games_root
            .join(format!("dontstarve-original-{name}"));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("server.bin"), b"official program").unwrap();
        crate::record_library_program_baseline(&root, &fixture.descriptor, true, None).unwrap();
        crate::retain_library_program_source(&fixture.paths, &root, &fixture.descriptor).unwrap();
        expected = Some(register_library(&fixture, &root, InstallState::Installed).await);
    }
    crate::retain_library_program_source(&fixture.paths, &fixture.source, &fixture.descriptor)
        .unwrap();
    let pending = fixture.paths.games_root.join("dontstarve-original-pending");
    fs::create_dir(&pending).unwrap();
    crate::retain_library_program_source(&fixture.paths, &pending, &fixture.descriptor).unwrap();
    register_library(&fixture, &pending, InstallState::Incomplete).await;
    let plan = crate::plan_module_library_cleanup(&fixture.paths, &fixture.descriptor, true)
        .await
        .unwrap();
    assert_ne!(expected, Some(default));
    assert_eq!(plan.keep_install_id, expected);
    assert_eq!(plan.installations.len(), 4);
    let uninstall = crate::plan_module_library_cleanup(&fixture.paths, &fixture.descriptor, false)
        .await
        .unwrap();
    assert_eq!(uninstall.keep_install_id, None);
    assert_eq!(uninstall.installations.len(), 4);

    let damaged = fixture.paths.games_root.join("dontstarve-original-damaged");
    fs::create_dir(&damaged).unwrap();
    fs::write(damaged.join("server.bin"), b"official program").unwrap();
    crate::record_library_program_baseline(&damaged, &fixture.descriptor, true, None).unwrap();
    crate::retain_library_program_source(&fixture.paths, &damaged, &fixture.descriptor).unwrap();
    register_library(&fixture, &damaged, InstallState::Installed).await;
    fs::write(damaged.join("server.bin"), b"modified program").unwrap();
    assert_eq!(
        crate::plan_module_library_cleanup(&fixture.paths, &fixture.descriptor, true)
            .await
            .unwrap()
            .keep_install_id,
        expected
    );

    for name in ["first", "second"] {
        fs::write(
            fixture
                .paths
                .games_root
                .join(format!("dontstarve-original-{name}/server.bin")),
            b"modified program",
        )
        .unwrap();
    }
    fs::write(
        fixture.source.join("payload.bin"),
        b"modified default program",
    )
    .unwrap();
    let unmarked = fixture.paths.games_root.join("dontstarve-unmarked-healthy");
    fs::create_dir(&unmarked).unwrap();
    fs::write(unmarked.join("server.bin"), b"official program").unwrap();
    crate::record_library_program_baseline(&unmarked, &fixture.descriptor, true, None).unwrap();
    let unmarked_id = register_library(&fixture, &unmarked, InstallState::Installed).await;
    assert_eq!(
        crate::plan_module_library_cleanup(&fixture.paths, &fixture.descriptor, true)
            .await
            .unwrap()
            .keep_install_id,
        Some(unmarked_id)
    );
}

#[tokio::test]
async fn retained_library_marker_is_bounded_root_bound_and_never_a_health_assertion() {
    let fixture = CleanCreationFixture::new().await;
    let custom = fixture.root.join("custom-library");
    fs::create_dir(&custom).unwrap();
    crate::retain_library_program_source(&fixture.paths, &custom, &fixture.descriptor).unwrap();
    crate::retain_library_program_source(&fixture.paths, &custom, &fixture.descriptor).unwrap();
    assert!(retained_library_program_source(&custom, &fixture.descriptor.summary.id).unwrap());
    assert!(!crate::library_program_is_pristine(&custom, &fixture.descriptor, None).unwrap());
    assert!(!retained_library_program_source(&custom, "other-module").unwrap());
    fs::copy(
        custom.join(RETAINED_LIBRARY),
        fixture.source.join(RETAINED_LIBRARY),
    )
    .unwrap();
    assert!(
        !retained_library_program_source(&fixture.source, &fixture.descriptor.summary.id).unwrap()
    );
    assert!(
        crate::retain_library_program_source(&fixture.paths, &fixture.source, &fixture.descriptor)
            .is_err()
    );
    for bytes in [
        b"{\"version\":1,\"module_id\":\"dontstarve\",\"program_root\":\"invalid\",\"extra\":true}"
            .to_vec(),
        vec![b' '; 16_385],
    ] {
        fs::write(fixture.source.join(RETAINED_LIBRARY), &bytes).unwrap();
        assert!(
            !retained_library_program_source(&fixture.source, &fixture.descriptor.summary.id)
                .unwrap()
        );
        assert!(
            crate::retain_library_program_source(
                &fixture.paths,
                &fixture.source,
                &fixture.descriptor
            )
            .is_err()
        );
        assert_eq!(
            fs::read(fixture.source.join(RETAINED_LIBRARY)).unwrap(),
            bytes
        );
    }
    for target in [
        crate::new_instance_program_acquisition(&fixture.paths, &fixture.descriptor.summary.id)
            .unwrap(),
        fixture.paths.archives_root.join("archive"),
        fixture.paths.steamcmd_root.join("program"),
        fixture.paths.games_root.clone(),
    ] {
        assert!(
            crate::retain_library_program_source(&fixture.paths, &target, &fixture.descriptor)
                .is_err()
        );
        assert!(!target.join(RETAINED_LIBRARY).exists());
    }
}

#[tokio::test]
async fn retained_library_metadata_survives_baselines_and_is_removable_only_when_valid() {
    let fixture = CleanCreationFixture::new().await;
    crate::retain_library_program_source(&fixture.paths, &fixture.source, &fixture.descriptor)
        .unwrap();
    fixture.record();
    assert!(
        retained_library_program_source(&fixture.source, &fixture.descriptor.summary.id).unwrap()
    );
    assert!(
        !crate::program_exclusive::unused_library_is_fresh(
            &fixture.source,
            &fixture.descriptor.summary.id,
            None
        )
        .unwrap()
    );
    let marker = fixture.source.join(RETAINED_LIBRARY);
    assert!(
        !crate::library_cleanup_retained_paths(&fixture.source, &fixture.descriptor)
            .unwrap()
            .unwrap()
            .contains(&fs::canonicalize(&marker).unwrap())
    );
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    record["personal_note"] = serde_json::json!("keep this");
    fs::write(&marker, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(
        crate::library_cleanup_retained_paths(&fixture.source, &fixture.descriptor)
            .unwrap()
            .unwrap()
            .contains(&fs::canonicalize(&marker).unwrap())
    );
}

#[tokio::test]
async fn retained_library_still_supports_explicit_shared_program_instances() {
    let mut fixture = CleanCreationFixture::new().await;
    fixture.paths.modules_root = fs::canonicalize(repo_root().join("modules")).unwrap();
    fixture.descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "minecraft")
        .unwrap();
    fixture.source = fixture.paths.games_root.join("minecraft");
    fs::create_dir_all(fixture.source.join("jre/bin")).unwrap();
    fs::write(
        fixture.source.join("jre/bin/java.exe"),
        b"fixture Java runtime",
    )
    .unwrap();
    fs::write(
        fixture.source.join("server.jar"),
        b"fixture Minecraft server",
    )
    .unwrap();
    sync_modules(&fixture.paths, std::slice::from_ref(&fixture.descriptor))
        .await
        .unwrap();
    fixture.record();
    crate::retain_library_program_source(&fixture.paths, &fixture.source, &fixture.descriptor)
        .unwrap();
    let library_id = register_library(&fixture, &fixture.source, InstallState::Installed).await;
    let created = create_instance_with_options(
        &fixture.paths,
        &fixture.descriptor,
        CreateInstanceInput {
            name: "Shared retained program".into(),
            module_id: fixture.descriptor.summary.id.clone(),
        },
        InstanceCreationOptions {
            prefer_existing_install: true,
            require_clean_program: true,
            program_mode: Some(InstanceProgramMode::Shared),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let binding = read_instance_program_install(&fixture.paths, &created.provisioning.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(binding.install.id, library_id);
    assert_eq!(binding.install.scope, ProgramInstallScope::Library);
    assert_eq!(binding.runtime_mode, "shared");
    crate::delete_instance(&fixture.paths, &created.provisioning.summary.id)
        .await
        .unwrap();
    assert!(
        crate::library_program_is_pristine(&fixture.source, &fixture.descriptor, None).unwrap()
    );
}

#[tokio::test]
async fn retained_library_seed_publishes_its_role_before_installation_completes() {
    let fixture = CleanCreationFixture::new().await;
    let root = fixture.paths.games_root.join("dontstarve-original-pending");
    let seed =
        crate::prepare_clean_library_seed_at(&fixture.paths, &fixture.descriptor, &root, None)
            .await
            .unwrap();
    assert!(seed.requires_validation);
    assert!(retained_library_program_source(&root, &fixture.descriptor.summary.id).unwrap());
    assert!(crate::library_program_acquisition_is_trusted(&root, &fixture.descriptor).unwrap());
    assert!(!crate::library_program_is_pristine(&root, &fixture.descriptor, None).unwrap());
    let acquisition =
        crate::new_instance_program_acquisition(&fixture.paths, &fixture.descriptor.summary.id)
            .unwrap();
    crate::prepare_instance_program_seed_at(
        &fixture.paths,
        &fixture.descriptor,
        &acquisition,
        None,
    )
    .await
    .unwrap();
    assert!(!acquisition.join(RETAINED_LIBRARY).exists());
}
