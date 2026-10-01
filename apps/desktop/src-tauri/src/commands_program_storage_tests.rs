use super::*;
use crate::commands::commands_program_storage::*;
use app_storage::{StoragePaths, bootstrap_storage_with_paths};

struct ProgramFixtureRoot(PathBuf);
impl Drop for ProgramFixtureRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn exclusive_mod_mutation_blocks_archive_admission_and_rejects_frozen_source()
-> Result<(), Box<dyn std::error::Error>> {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    let root = ProgramFixtureRoot(temp_test_dir("exclusive-mods"));
    let storage = bootstrap_storage_with_paths(StoragePaths {
        app_data_root: root.0.join("app-data"),
        settings_path: root.0.join("app-data/settings.json"),
        database_path: root.0.join("app-data/db/lgs.db"),
        logs_root: root.0.join("app-data/logs"),
        modules_root: workspace_root().join("modules"),
        migrations_root: workspace_root().join("migrations"),
        steamcmd_root: root.0.join("steamcmd"),
        games_root: root.0.join("games"),
        instances_root: root.0.join("instances"),
        archives_root: root.0.join("instances/.trash"),
    })?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "astroneer")?;
    let library = storage.paths.games_root.join("astroneer");
    fs::create_dir_all(&library)?;
    fs::write(
        library.join("AstroServer.exe"),
        b"official program, never executed",
    )?;
    app_storage::record_library_program_baseline(&library, descriptor, true, None)?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: "astroneer".into(),
            install_root: library.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("fixture-build".into()),
            mark_verified: true,
        }],
    )
    .await?;
    let options = app_storage::InstanceCreationOptions {
        prefer_existing_install: true,
        require_clean_program: true,
        program_mode: Some(app_core::InstanceProgramMode::Independent),
        ..Default::default()
    };
    let first = app_storage::create_instance_with_options(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Exclusive original".into(),
            module_id: "astroneer".into(),
        },
        options.clone(),
    )
    .await?;
    let second = app_storage::create_instance_with_options(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Independent archive".into(),
            module_id: "astroneer".into(),
        },
        options,
    )
    .await?;
    let first_id = &first.provisioning.summary.id;
    let second_id = &second.provisioning.summary.id;
    assert!(app_storage::instance_uses_exclusive_program(
        &storage.paths.instances_root.join(first_id)
    )?);
    assert_ne!(
        fs::canonicalize(&first.effective_install_root)?,
        fs::canonicalize(&second.effective_install_root)?
    );
    let state = DesktopState::default();
    let operation = state.begin_storage_context_operation("exclusive Mod mutation regression")?;
    let instance_lock = state.acquire_instance_mutation(first_id).await;
    let program_guard =
        prepare_instance_mod_program(&storage, &operation, first_id, instance_lock).await?;
    let unrelated_roots = [root.0.join("other-game")];
    let unrelated = tokio::time::timeout(
        Duration::from_secs(5),
        app_steamcmd::acquire_game_install_lifecycle("other-game", &unrelated_roots),
    )
    .await
    .expect("an unrelated game must not wait for the Mod operation")?;
    drop(unrelated);
    let reached_install_lock = std::sync::atomic::AtomicBool::new(false);
    // Use the command's instance -> installation admission order. Poll once
    // deterministically: the Mod operation must retain the install lease even
    // though exclusive use needs no detach or program copy.
    let mut archive = Box::pin(async {
        let _instance = state.acquire_instance_mutation(second_id).await;
        reached_install_lock.store(true, Ordering::SeqCst);
        let _install = app_steamcmd::acquire_game_install_lifecycle(
            "astroneer",
            &[PathBuf::from(&second.effective_install_root)],
        )
        .await?;
        Ok::<_, Box<dyn std::error::Error>>(
            app_storage::archive_instance(&storage.paths, second_id).await?,
        )
    });
    assert!(matches!(
        archive
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    assert!(reached_install_lock.load(Ordering::SeqCst));
    assert!(storage.paths.instances_root.join(second_id).exists());
    drop(program_guard);
    let archived = archive.await?;
    let inventory = app_storage::list_instance_archives(&storage.paths).await?;
    let entry = inventory
        .archives
        .iter()
        .find(|entry| entry.archive_id == archived.archive_id)
        .unwrap();
    assert_eq!(entry.program_storage, "reconstructable");
    let instance_lock = state.acquire_instance_mutation(first_id).await;
    let error =
        match prepare_instance_mod_program(&storage, &operation, first_id, instance_lock).await {
            Ok(_) => panic!("a data archive must freeze its exclusive installation source"),
            Err(error) => error,
        };
    assert!(error.contains(&archived.archive_id), "{error}");
    assert_eq!(
        fs::read(library.join("AstroServer.exe"))?,
        b"official program, never executed"
    );
    app_storage::restore_instance_archive(&storage.paths, &archived.archive_id).await?;
    let instance_lock = state.acquire_instance_mutation(first_id).await;
    let _program_guard =
        prepare_instance_mod_program(&storage, &operation, first_id, instance_lock).await?;
    Ok(())
}

#[tokio::test]
async fn existing_program_is_verified_only_at_storage_admission_and_dirty_source_is_preserved()
-> Result<(), Box<dyn std::error::Error>> {
    let root = ProgramFixtureRoot(temp_test_dir("program-admission-verification"));
    let storage = bootstrap_storage_with_paths(StoragePaths {
        app_data_root: root.0.join("app-data"),
        settings_path: root.0.join("app-data/settings.json"),
        database_path: root.0.join("app-data/db/lgs.db"),
        logs_root: root.0.join("app-data/logs"),
        modules_root: workspace_root().join("modules"),
        migrations_root: workspace_root().join("migrations"),
        steamcmd_root: root.0.join("steamcmd"),
        games_root: root.0.join("games"),
        instances_root: root.0.join("instances"),
        archives_root: root.0.join("instances").join(".trash"),
    })?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let mut descriptor = find_descriptor(&descriptors, "astroneer")?.clone();
    descriptor.summary.steam_app_id = None;
    let install = descriptor.install.as_mut().unwrap();
    install.source = None;
    install.download_url_windows = None;
    let original = storage.paths.games_root.join(&install.shared_game_dir);
    let executable = original.join(&descriptor.process.as_ref().unwrap().executable);
    fs::create_dir_all(executable.parent().unwrap())?;
    fs::write(&executable, b"official fixture program")?;
    // Model a completed library installation whose manifest is subsequently
    // damaged; preparation must still defer content verification to admission.
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: original.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    fs::write(original.join(".langame-clean-package.json"), b"{broken")?;
    let state = DesktopState::default();
    let operation =
        state.begin_storage_context_operation("deferred official package verification")?;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &descriptor.summary.id,
        std::slice::from_ref(&original),
    )
    .await?;
    // Even parsing the manifest here would fail: candidate preparation must not
    // do its own SHA pass or claim the candidate's official files are verified.
    let CreationProgramPreparation::Ready(revision) = prepare_creation_program(
        &storage,
        &descriptor,
        &operation,
        &guard,
        &original,
        app_core::InstanceProgramSource::Verified,
    )
    .await?
    else {
        return Err("fixture program must have a completed revision".into());
    };
    let input = CreateInstanceInput {
        name: "Verify at admission".into(),
        module_id: descriptor.summary.id.clone(),
    };
    let options = app_storage::InstanceCreationOptions {
        source_generation: Some(revision),
        require_clean_program: true,
        cancellation: Some(operation.cancellation_token()),
        ..Default::default()
    };
    let corrupt = app_storage::create_instance_with_options(
        &storage.paths,
        &descriptor,
        input.clone(),
        options.clone(),
    )
    .await
    .unwrap_err();
    assert!(!matches!(
        corrupt,
        app_storage::StorageError::CleanLibraryProgramRequired { .. }
    ));
    assert!(
        corrupt
            .to_string()
            .contains("invalid clean package manifest"),
        "{corrupt}"
    );

    app_storage::record_library_program_baseline(&original, &descriptor, true, None)?;
    fs::write(&executable, b"operator-modified program")?;
    fs::write(
        original.join("unknown-user.dat"),
        b"preserve original bytes",
    )?;
    let dirty = app_storage::create_instance_with_options(
        &storage.paths,
        &descriptor,
        input.clone(),
        options,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            dirty,
            app_storage::StorageError::CleanLibraryProgramRequired { .. }
        ),
        "{dirty}"
    );
    assert!(
        app_storage::list_instances(&storage.paths)
            .await?
            .is_empty()
    );
    assert!(storage.paths.archives_root.is_dir());
    assert_eq!(fs::read_dir(&storage.paths.archives_root)?.count(), 0);
    assert!(fs::read_dir(&storage.paths.instances_root)?.all(|entry| {
        let entry = entry.unwrap();
        entry.file_name() == ".langame"
            || entry.path() == storage.paths.archives_root
            || !entry.path().is_dir()
    }));

    let retained =
        app_storage::read_library_program_install(&storage.paths, &descriptor.summary.id)
            .await?
            .unwrap();
    assert_eq!(
        fs::canonicalize(&retained.install_root)?,
        fs::canonicalize(&original)?
    );
    assert_eq!(fs::read(&executable)?, b"operator-modified program");
    assert_eq!(
        fs::read(original.join("unknown-user.dat"))?,
        b"preserve original bytes"
    );
    assert!(!storage.paths.games_root.join(".langame-clean").exists());
    let projection_error = app_storage::create_instance_with_options(
        &storage.paths,
        &descriptor,
        input.clone(),
        app_storage::InstanceCreationOptions {
            program_mode: Some(app_core::InstanceProgramMode::Independent),
            use_local_program: true,
            private_runtime: Some(app_storage::PrivateRuntimeProjection {
                private_directories: vec![PathBuf::from("mods")],
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(
        projection_error
            .to_string()
            .contains("local program import requires an independent instance")
    );
    assert_eq!(fs::read(&executable)?, b"operator-modified program");
    let imported = app_storage::create_instance_with_options(
        &storage.paths,
        &descriptor,
        input,
        app_storage::InstanceCreationOptions {
            program_mode: Some(app_core::InstanceProgramMode::Independent),
            use_local_program: true,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        fs::read(
            imported
                .effective_install_root
                .join(&descriptor.process.as_ref().unwrap().executable)
        )?,
        b"operator-modified program"
    );
    assert_eq!(
        fs::read(imported.effective_install_root.join("unknown-user.dat"))?,
        b"preserve original bytes"
    );
    assert!(!app_storage::library_program_is_pristine(
        &imported.effective_install_root,
        &descriptor,
        None
    )?);
    let summary = load_module_details_with_install_state(&storage, &descriptor, true)
        .await?
        .summary;
    assert_eq!(summary.install_state, InstallState::Installed);
    assert_eq!(summary.instance_program_count, 1);
    assert_eq!(summary.archived_program_count, 0);
    let archived =
        app_storage::archive_instance(&storage.paths, &imported.provisioning.summary.id).await?;
    let summary = load_module_details_with_install_state(&storage, &descriptor, true)
        .await?
        .summary;
    assert_eq!(summary.install_state, InstallState::Installed);
    assert_eq!(summary.instance_program_count, 0);
    assert_eq!(summary.archived_program_count, 1);
    app_storage::restore_instance_archive(&storage.paths, &archived.archive_id).await?;
    app_storage::delete_instance(&storage.paths, &imported.provisioning.summary.id).await?;
    let summary = load_module_details_with_install_state(&storage, &descriptor, true)
        .await?
        .summary;
    assert_eq!(summary.install_state, InstallState::Installed);
    assert_eq!(summary.instance_program_count, 0);
    assert_eq!(summary.archived_program_count, 0);
    Ok(())
}

#[tokio::test]
async fn creation_preparation_preserves_an_incomplete_local_program_for_repair()
-> Result<(), Box<dyn std::error::Error>> {
    let root = ProgramFixtureRoot(temp_test_dir("program-acquisition-retry"));
    let storage = bootstrap_storage_with_paths(StoragePaths {
        app_data_root: root.0.join("app-data"),
        settings_path: root.0.join("app-data/settings.json"),
        database_path: root.0.join("app-data/db/lgs.db"),
        logs_root: root.0.join("app-data/logs"),
        modules_root: workspace_root().join("modules"),
        migrations_root: workspace_root().join("migrations"),
        steamcmd_root: root.0.join("steamcmd"),
        games_root: root.0.join("games"),
        instances_root: root.0.join("instances"),
        archives_root: root.0.join("instances").join(".trash"),
    })?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let mut descriptor = find_descriptor(&descriptors, "astroneer")?.clone();
    // Stop at the provider contract boundary: no SteamCMD process or network is used.
    descriptor.summary.steam_app_id = None;
    let install = descriptor.install.as_mut().unwrap();
    install.source = None;
    install.download_url_windows = None;
    let original = storage.paths.games_root.join(&install.shared_game_dir);
    fs::create_dir_all(&original)?;
    fs::write(original.join("custom-loader.dll"), b"user loader, preserve")?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: original.to_string_lossy().into_owned(),
            install_state: InstallState::Incomplete,
            current_version: None,
            mark_verified: false,
        }],
    )
    .await?;
    let state = DesktopState::default();
    let operation = state.begin_storage_context_operation("failed clean acquisition fixture")?;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &descriptor.summary.id,
        std::slice::from_ref(&original),
    )
    .await?;
    let CreationProgramPreparation::NeedsRepair(first_error) = prepare_creation_program(
        &storage,
        &descriptor,
        &operation,
        &guard,
        &original,
        app_core::InstanceProgramSource::Verified,
    )
    .await?
    else {
        return Err("incomplete local program must require repair".into());
    };
    let first = app_storage::read_library_program_install(&storage.paths, &descriptor.summary.id)
        .await?
        .unwrap();
    assert_eq!(
        fs::canonicalize(&first.install_root)?,
        fs::canonicalize(&original)?
    );
    assert!(matches!(first.install_state, InstallState::Incomplete));
    assert!(!app_storage::library_program_acquisition_is_trusted(
        &first.install_root,
        &descriptor
    )?);
    fs::write(
        first.install_root.join("partial-download.bin"),
        b"partial fixture",
    )?;
    persist_descriptor_install_states(&storage, &[descriptor.clone()]).await?;
    let CreationProgramPreparation::NeedsRepair(second_error) = prepare_creation_program(
        &storage,
        &descriptor,
        &operation,
        &guard,
        &original,
        app_core::InstanceProgramSource::Verified,
    )
    .await?
    else {
        return Err("incomplete local program must still require repair".into());
    };
    let second = app_storage::read_library_program_install(&storage.paths, &descriptor.summary.id)
        .await?
        .unwrap();
    assert_eq!(first_error, second_error);
    assert_eq!(first.id, second.id);
    assert_eq!(first.install_root, second.install_root);
    assert_eq!(
        fs::read(second.install_root.join("partial-download.bin"))?,
        b"partial fixture"
    );
    assert_eq!(
        fs::read(original.join("custom-loader.dll"))?,
        b"user loader, preserve"
    );
    assert!(!storage.paths.games_root.join(".langame-clean").exists());
    Ok(())
}

#[tokio::test]
async fn shared_program_pending_start_blocks_update_but_detached_instance_does_not()
-> Result<(), Box<dyn std::error::Error>> {
    let root = ProgramFixtureRoot(temp_test_dir("programsharing"));
    let paths = StoragePaths {
        app_data_root: root.0.join("app-data"),
        settings_path: root.0.join("app-data/settings.json"),
        database_path: root.0.join("app-data/db/lgs.db"),
        logs_root: root.0.join("app-data/logs"),
        modules_root: workspace_root().join("modules"),
        migrations_root: workspace_root().join("migrations"),
        steamcmd_root: root.0.join("steamcmd"),
        games_root: root.0.join("games"),
        instances_root: root.0.join("instances"),
        archives_root: root.0.join("instances").join(".trash"),
    };
    let storage = bootstrap_storage_with_paths(paths)?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "minecraft")?;
    let library = storage.paths.games_root.join("minecraft");
    fs::create_dir_all(&library)?;
    fs::write(
        library.join("server.jar"),
        b"fixture server, never executed",
    )?;
    fs::create_dir_all(library.join("jre/bin"))?;
    fs::write(library.join("jre/bin/java.exe"), b"fixture, never executed")?;
    app_storage::record_library_program_baseline(&library, descriptor, true, None)?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: "minecraft".into(),
            install_root: library.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    let first = app_storage::create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Shared first".into(),
            module_id: "minecraft".into(),
        },
    )
    .await?;
    let second = app_storage::create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Shared second".into(),
            module_id: "minecraft".into(),
        },
    )
    .await?;
    let first_id = &first.summary.id;
    let second_id = &second.summary.id;
    let first_details = read_instance_details(&storage.paths, first_id).await?;
    let second_details = read_instance_details(&storage.paths, second_id).await?;
    assert_eq!(
        program_mode(&first_details)?,
        app_storage::InstanceProgramMode::Shared
    );
    assert!(
        !build_instance_launch_preview(&storage.settings, descriptor, &first_details)?
            .uses_private_runtime
    );
    assert_eq!(
        shared_program_references(&storage.paths, "minecraft", &library)
            .await?
            .len(),
        2
    );

    let state = DesktopState::default();
    let reservation = match state.try_reserve_runtime_start(first_id, "manual")? {
        crate::state::RuntimeStartReservationAttempt::Reserved(lease) => lease,
        other => panic!("expected start reservation, got {other:?}"),
    };
    // Both starts use the same install lease through publication. The start
    // holding it may update while the other still waits for admission.
    ensure_shared_program_unused(&state, &storage, "minecraft", &library, Some(second_id)).await?;
    assert!(
        ensure_shared_program_unused(&state, &storage, "minecraft", &library, None)
            .await
            .is_err()
    );
    assert!(
        acquire_library_program_mutation(&state, &storage, "minecraft", &library)
            .await
            .is_err()
    );
    drop(reservation);

    let active = app_storage::mark_instance_process_started_with_identity(
        &storage.paths,
        &app_storage::StartedInstanceProcess {
            instance_id: first_id,
            session_id: Some("shared-admission-fixture"),
            process_key: "main",
            display_name: "Synthetic active run",
            pid: u32::MAX,
            log_path: "fixture-not-executed.log",
            is_primary: true,
        },
        None,
    )
    .await?;
    for starting in [None, Some(second_id.as_str())] {
        assert!(
            ensure_shared_program_unused(&state, &storage, "minecraft", &library, starting)
                .await
                .is_err()
        );
    }
    app_storage::mark_instance_process_stopped(
        &storage.paths,
        first_id,
        active.run_id,
        Some(0),
        false,
    )
    .await?;

    let original_settings = fs::read(&first_details.config_file_path)?;
    let mut pinned_settings: serde_json::Value = serde_json::from_slice(&original_settings)?;
    pinned_settings["settings"]["program_update"] = serde_json::json!({ "policy": "pinned" });
    fs::write(
        &first_details.config_file_path,
        serde_json::to_vec(&pinned_settings)?,
    )?;
    let pinned = read_instance_details(&storage.paths, first_id).await?;
    assert_eq!(
        app_core::InstanceProgramUpdatePolicy::from_settings_json(&pinned.settings_json)?,
        app_core::InstanceProgramUpdatePolicy::Pinned,
    );
    for starting in [None, Some(first_id.as_str()), Some(second_id.as_str())] {
        let error = ensure_shared_program_unused(&state, &storage, "minecraft", &library, starting)
            .await
            .unwrap_err();
        assert!(error.contains("当前版本已固定"), "{error}");
    }
    assert!(
        acquire_library_program_mutation(&state, &storage, "minecraft", &library)
            .await
            .is_err()
    );
    fs::write(&first_details.config_file_path, original_settings)?;

    let operation = state.begin_storage_context_operation("program detach regression")?;
    let instance_lock = state.acquire_instance_mutation(first_id).await;
    let instance_lock =
        prepare_instance_mod_program(&storage, &operation, first_id, instance_lock).await?;
    instance_lock.install.ensure_scope("minecraft", &library)?;
    instance_lock
        .install
        .ensure_scope("minecraft", &instance_root(&first_details)?.join("runtime"))?;
    drop(instance_lock);
    let detached = read_instance_details(&storage.paths, first_id).await?;
    assert_eq!(
        program_mode(&detached)?,
        app_storage::InstanceProgramMode::Independent
    );
    let private = app_storage::resolve_instance_runtime_root(instance_root(&detached)?)?;
    assert_ne!(fs::canonicalize(&private)?, fs::canonicalize(&library)?);
    assert!(
        build_instance_launch_preview(&storage.settings, descriptor, &detached)?
            .uses_private_runtime
    );
    fs::write(private.join("server.jar"), b"instance loader")?;
    assert_ne!(fs::read(library.join("server.jar"))?, b"instance loader");
    assert_eq!(
        program_mode(&second_details)?,
        app_storage::InstanceProgramMode::Shared
    );
    assert_eq!(
        shared_program_references(&storage.paths, "minecraft", &library)
            .await?
            .len(),
        1
    );
    let reservation = match state.try_reserve_runtime_start(first_id, "manual")? {
        crate::state::RuntimeStartReservationAttempt::Reserved(lease) => lease,
        other => panic!("expected start reservation, got {other:?}"),
    };
    ensure_shared_program_unused(&state, &storage, "minecraft", &library, None).await?;
    let _guard = acquire_library_program_mutation(&state, &storage, "minecraft", &library).await?;
    drop(reservation);
    Ok(())
}
