use super::*;

async fn completed_instance_acquisition(
    fixture: &Fixture,
) -> TestResult<app_storage::ProgramInstallRecord> {
    let root = app_storage::new_instance_program_acquisition(
        &fixture.storage.paths,
        &fixture.descriptor.summary.id,
    )?;
    let root_text = root.to_string_lossy();
    let module = map_module_details_with_install_state(
        &fixture.storage.settings,
        &fixture.descriptor,
        Some(&root_text),
    );
    install_fixture(
        module,
        root.clone(),
        app_steamcmd::InstallCancellation::new(),
    )
    .await?;
    app_storage::record_library_program_baseline(&root, &fixture.descriptor, true, None)?;
    sync_game_installs(
        &fixture.storage.paths,
        &[GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("fixture-build".into()),
            mark_verified: true,
        }],
    )
    .await?;
    assert!(app_storage::is_instance_program_acquisition(
        &fixture.storage.paths,
        &fixture.descriptor.summary.id,
        &root,
    )?);
    Ok(
        app_storage::read_program_install_owner(&fixture.storage.paths, &root)
            .await?
            .unwrap(),
    )
}

#[tokio::test]
async fn completed_instance_acquisition_recovers_through_a_retained_library_without_downloading()
-> TestResult {
    for mode in [
        Some(app_core::InstanceProgramMode::Independent),
        Some(app_core::InstanceProgramMode::Shared),
        None,
    ] {
        let fixture = Fixture::for_module("minecraft").await?;
        let original = OriginalState::new(&fixture).await?;
        let completed = completed_instance_acquisition(&fixture).await?;
        let acquisition = &completed.install_root;
        let completed_files = snapshot(acquisition)?;
        let target = &fixture.repair;
        let created = {
            let guard = app_steamcmd::acquire_game_install_lifecycle(
                &fixture.descriptor.summary.id,
                &[
                    fixture.original.clone(),
                    acquisition.clone(),
                    target.clone(),
                ],
            )
            .await?;
            let (operation, job) = fixture.operation()?;
            let mut request = fixture.request(&operation, &guard, &job);
            request.program_root = acquisition;
            request.mode = mode;
            create_with_program_repair(request, |_, _, _| async {
                Err(
                    "a complete acquisition must seed the retained library without downloading"
                        .into(),
                )
            })
            .await?
        };
        let library = app_storage::read_program_install_owner(&fixture.storage.paths, target)
            .await?
            .unwrap();
        assert_eq!(library.scope, app_storage::ProgramInstallScope::Library);
        assert_eq!(library.owner_instance_id, None);
        assert_eq!(library.install_state, InstallState::Installed);
        assert_ne!(library.id, completed.id);
        assert!(app_storage::library_program_is_pristine(
            target,
            &fixture.descriptor,
            None
        )?);
        let binding =
            app_storage::read_instance_program_install(&fixture.storage.paths, &created.summary.id)
                .await?
                .unwrap();
        assert_ne!(binding.install.id, completed.id);
        if mode == Some(app_core::InstanceProgramMode::Independent) {
            assert_eq!(
                binding.install.scope,
                app_storage::ProgramInstallScope::Instance
            );
            assert_eq!(
                binding.install.owner_instance_id.as_deref(),
                Some(created.summary.id.as_str())
            );
            assert_ne!(binding.install.id, library.id);
            assert_ne!(
                fs::canonicalize(&binding.install.install_root)?,
                fs::canonicalize(target)?
            );
        } else {
            assert_eq!(binding.install.id, library.id);
            assert_eq!(
                binding.install.scope,
                app_storage::ProgramInstallScope::Library
            );
            assert_eq!(binding.install.owner_instance_id, None);
            assert_eq!(binding.runtime_mode, "shared");
            assert_eq!(
                fs::canonicalize(&binding.install.install_root)?,
                fs::canonicalize(target)?
            );
        }
        assert_eq!(snapshot(acquisition)?, completed_files);
        let retained = app_storage::read_program_install_owner(&fixture.storage.paths, acquisition)
            .await?
            .unwrap();
        assert_eq!(retained.id, completed.id);
        assert_eq!(retained.scope, app_storage::ProgramInstallScope::Library);
        assert_eq!(retained.owner_instance_id, None);
        original.assert_unchanged(&fixture).await?;
    }
    Ok(())
}

#[tokio::test]
async fn missing_instance_acquisition_rebuilds_into_shared_library_instead_of_resuming_private_root()
-> TestResult {
    let fixture = Fixture::for_module("minecraft").await?;
    let original = OriginalState::new(&fixture).await?;
    let completed = completed_instance_acquisition(&fixture).await?;
    let acquisition = &completed.install_root;
    // Retain the persisted registration while removing only this fixture's acquisition.
    let fixture_root = fs::canonicalize(&fixture.root)?;
    let missing_root = fs::canonicalize(acquisition)?;
    assert!(fixture_root.starts_with(fs::canonicalize(std::env::temp_dir())?));
    assert!(missing_root.starts_with(&fixture_root));
    assert_ne!(missing_root, fixture_root);
    assert_ne!(missing_root, fs::canonicalize(&fixture.original)?);
    assert!(app_storage::is_instance_program_acquisition(
        &fixture.storage.paths,
        &fixture.descriptor.summary.id,
        &missing_root,
    )?);
    fs::remove_dir_all(&missing_root)?;
    assert!(!acquisition.exists());
    let retained = app_storage::read_program_install_owner(&fixture.storage.paths, acquisition)
        .await?
        .unwrap();
    assert_eq!(retained.id, completed.id);
    assert_eq!(retained.install_state, InstallState::Installed);

    let target = &fixture.repair;
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let created = {
        let guard = app_steamcmd::acquire_game_install_lifecycle(
            &fixture.descriptor.summary.id,
            &[
                fixture.original.clone(),
                acquisition.clone(),
                target.clone(),
            ],
        )
        .await?;
        let (operation, job) = fixture.operation()?;
        let mut request = fixture.request(&operation, &guard, &job);
        request.program_root = acquisition;
        request.mode = Some(app_core::InstanceProgramMode::Shared);
        create_with_program_repair(request, |module, root, token| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                fs::canonicalize(&root).unwrap(),
                fs::canonicalize(target).unwrap(),
                "shared repair must publish outside the temporary instance namespace"
            );
            assert!(
                acquisition.is_dir(),
                "preparation must rebuild the missing source seed"
            );
            install_fixture(module, root, token)
        })
        .await?
    };
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let binding =
        app_storage::read_instance_program_install(&fixture.storage.paths, &created.summary.id)
            .await?
            .unwrap();
    assert_eq!(
        binding.install.scope,
        app_storage::ProgramInstallScope::Library
    );
    assert_eq!(binding.install.owner_instance_id, None);
    assert_eq!(binding.runtime_mode, "shared");
    assert_ne!(binding.install.id, completed.id);
    assert_eq!(
        fs::canonicalize(&binding.install.install_root)?,
        fs::canonicalize(target)?
    );
    let source = app_storage::read_program_install_owner(&fixture.storage.paths, acquisition)
        .await?
        .unwrap();
    assert_eq!(source.id, completed.id);
    assert_eq!(source.scope, app_storage::ProgramInstallScope::Library);
    assert_eq!(source.owner_instance_id, None);
    assert_eq!(source.install_state, InstallState::Incomplete);
    original.assert_unchanged(&fixture).await
}
