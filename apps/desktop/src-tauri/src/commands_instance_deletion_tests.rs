use super::*;

#[tokio::test(flavor = "current_thread")]
async fn instance_retirement_commands_block_active_run_then_archive_restore_and_delete()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("delete-command");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    assert!(storage.paths.archives_root.is_dir());
    assert_eq!(fs::read_dir(&storage.paths.archives_root)?.count(), 0);
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "astroneer")?;
    prepare_fake_registered_program(&storage.paths, descriptor).await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let library = app_storage::read_library_program_install(&storage.paths, "astroneer")
        .await?
        .unwrap();
    let library_before = retirement_tree_snapshot(&library.install_root)?;
    let created = create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Delete command fixture".into(),
            module_id: "astroneer".into(),
        },
    )
    .await?;
    let kept = create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Kept command fixture".into(),
            module_id: "astroneer".into(),
        },
    )
    .await?;
    let kept_root = storage.paths.instances_root.join(&kept.summary.id);
    let kept_before = retirement_tree_snapshot(&kept_root)?;
    let kept_config = fs::read(&kept.config_file_path)?;
    let details = read_instance_details(&storage.paths, &created.summary.id).await?;
    let world = PathBuf::from(&details.saves_path).join("world.fixture");
    fs::create_dir_all(world.parent().unwrap())?;
    fs::write(&world, b"command-world")?;
    let instance_root = storage.paths.instances_root.join(&created.summary.id);
    let archived_world_relative = fs::canonicalize(&world)?
        .strip_prefix(fs::canonicalize(&instance_root)?)?
        .to_path_buf();
    let backup = app_storage::create_instance_backup(&storage.paths, &created.summary.id).await?;
    let config_before = fs::read(&created.config_file_path)?;
    let log_path = root
        .join("active.fixture.log")
        .to_string_lossy()
        .into_owned();
    let run = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &created.summary.id,
            session_id: Some("delete-command-session"),
            process_key: "main",
            display_name: "Fixture",
            pid: std::process::id(),
            log_path: &log_path,
            is_primary: true,
        },
        None,
    )
    .await?;
    let identity = inspect_process_identity(std::process::id())?.expect("test process is running");
    {
        let state = app.state::<DesktopState>();
        state.runtime_supervisor.lock().unwrap().insert_running(
            details.summary.clone(),
            Some("delete-command-session".into()),
            vec![ManagedProcess {
                run_id: run.run_id,
                process_key: "main".into(),
                display_name: "Fixture".into(),
                pid: std::process::id(),
                process_identity: identity.clone(),
                root_process_identity: identity,
                log_path: log_path.clone(),
                is_primary: true,
                uses_script_entrypoint: false,
                performance_policy: RuntimePerformancePolicy::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: None,
                hidden_desktop: None,
            }],
        );
    }
    let error = delete_instance_record(app.handle().clone(), created.summary.id.clone())
        .await
        .expect_err("tracked running instance must not be deleted");
    assert!(
        error.contains("Stop the server before deleting the instance."),
        "{error}"
    );
    let archive_error = archive_instance_record(app.handle().clone(), created.summary.id.clone())
        .await
        .expect_err("tracked running instance must not be archived");
    assert!(
        archive_error.contains("Stop the server before archiving the instance."),
        "{archive_error}"
    );
    assert_eq!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .len(),
        2
    );
    assert_eq!(
        retirement_tree_snapshot(&library.install_root)?,
        library_before
    );
    let uninstall_error = uninstall_module_game(app.handle().clone(), "astroneer".into())
        .await
        .expect_err("active instances must also block package uninstallation");
    assert!(
        uninstall_error.contains("module_in_use"),
        "{uninstall_error}"
    );
    assert_eq!(fs::read(&created.config_file_path)?, config_before);
    assert_eq!(fs::read_dir(&storage.paths.archives_root)?.count(), 0);
    let rejected_archives = app_storage::list_instance_archives(&storage.paths).await?;
    assert!(rejected_archives.archives.is_empty());
    assert!(rejected_archives.pending_deletions.is_empty());
    {
        let state = app.state::<DesktopState>();
        state
            .runtime_supervisor
            .lock()
            .unwrap()
            .take_running_for_stop(&created.summary.id)
            .unwrap();
    }
    mark_instance_process_stopped(
        &storage.paths,
        &created.summary.id,
        run.run_id,
        Some(0),
        false,
    )
    .await?;
    let result = archive_instance_record(app.handle().clone(), created.summary.id.clone()).await?;
    let archive = PathBuf::from(result.archived_instance_root.unwrap());
    assert_eq!(
        fs::read(archive.join("config/instance.json"))?,
        config_before
    );
    assert_eq!(
        fs::read(
            archive
                .join("backups")
                .join(backup.backup_id)
                .join("saves/world.fixture")
        )?,
        b"command-world"
    );
    assert!(!world.exists());
    assert_eq!(
        fs::read(archive.join(archived_world_relative))?,
        b"command-world"
    );
    assert_eq!(fs::read(&kept.config_file_path)?, kept_config);
    assert_eq!(list_instances(&storage.paths).await?.len(), 1);
    assert_eq!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .len(),
        1
    );
    assert!(matches!(
        read_instance_details(&storage.paths, &created.summary.id).await,
        Err(app_storage::StorageError::MissingInstance { .. })
    ));
    use super::commands_storage_management as management;
    let listed = management::list_instance_archives(app.state::<DesktopState>()).await?;
    let archived = listed
        .archives
        .iter()
        .find(|item| item.instance_id.as_ref() == Some(&created.summary.id))
        .unwrap();
    assert!(archived.can_restore, "{:?}", archived.issues);
    let restored = management::restore_instance_archive(
        app.handle().clone(),
        management::InstanceArchiveInput {
            archive_id: archived.archive_id.clone(),
        },
    )
    .await?;
    assert_eq!(restored.instance_id, created.summary.id);
    assert_eq!(fs::read(&created.config_file_path)?, config_before);
    assert_eq!(fs::read(&world)?, b"command-world");
    assert_eq!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .len(),
        2
    );
    let scan = management::scan_storage_usage(
        app.state::<DesktopState>(),
        management::StorageScanInput {
            scan_id: uuid::Uuid::new_v4().to_string(),
        },
    )
    .await?;
    assert_eq!(scan.status, "complete", "{:?}", scan.entries);
    assert!(
        scan.entries
            .iter()
            .any(|entry| entry.category == "instance_program" && entry.file_count > 0)
    );
    let before_delete = management::list_instance_archives(app.state::<DesktopState>()).await?;
    let archive_ids = before_delete
        .archives
        .iter()
        .map(|entry| entry.archive_id.clone())
        .collect::<Vec<_>>();
    assert!(before_delete.pending_deletions.is_empty());
    let deleted = delete_instance_record(app.handle().clone(), created.summary.id.clone()).await?;
    assert_eq!(deleted.instance_id, created.summary.id);
    assert_eq!(PathBuf::from(deleted.deleted_instance_root), instance_root);
    assert_eq!(deleted.preserved_external_saves_path, None);
    assert!(!instance_root.exists());
    let listed = management::list_instance_archives(app.state::<DesktopState>()).await?;
    assert_eq!(
        listed
            .archives
            .iter()
            .map(|entry| entry.archive_id.clone())
            .collect::<Vec<_>>(),
        archive_ids
    );
    assert!(listed.pending_deletions.is_empty());
    let purge_candidate = create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Archive purge fixture".into(),
            module_id: "astroneer".into(),
        },
    )
    .await?;
    let purge_archive =
        archive_instance_record(app.handle().clone(), purge_candidate.summary.id).await?;
    let purged = management::purge_instance_archive(
        app.state::<DesktopState>(),
        management::InstanceArchiveInput {
            archive_id: purge_archive.archive_id,
        },
    )
    .await?;
    assert!(purged.purged);
    assert!(!Path::new(purge_archive.archived_instance_root.as_ref().unwrap()).exists());
    assert_eq!(retirement_tree_snapshot(&kept_root)?, kept_before);
    assert_eq!(
        retirement_tree_snapshot(&library.install_root)?,
        library_before
    );
    let library_after = app_storage::read_library_program_install(&storage.paths, "astroneer")
        .await?
        .unwrap();
    assert_eq!(library_after.id, library.id);
    assert_eq!(library_after.install_root, library.install_root);
    assert_eq!(library_after.install_state, InstallState::Installed);
    assert_eq!(library_after.current_version, library.current_version);
    assert_eq!(
        library_after.scope,
        app_storage::ProgramInstallScope::Library
    );
    assert_eq!(library_after.owner_instance_id, None);
    let purge_fixture = create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Archive purge fixture".into(),
            module_id: "astroneer".into(),
        },
    )
    .await?;
    let purge_archive =
        archive_instance_record(app.handle().clone(), purge_fixture.summary.id).await?;
    let purge_root = PathBuf::from(purge_archive.archived_instance_root.unwrap());
    assert!(purge_root.is_dir());
    let purged = management::purge_instance_archive(
        app.state::<DesktopState>(),
        management::InstanceArchiveInput {
            archive_id: purge_archive.archive_id,
        },
    )
    .await?;
    assert!(purged.purged);
    assert!(!purge_root.exists());
    assert_eq!(
        retirement_tree_snapshot(&library.install_root)?,
        library_before
    );
    assert_eq!(retirement_tree_snapshot(&kept_root)?, kept_before);
    assert_eq!(list_instances(&storage.paths).await?.len(), 1);
    let state = app.state::<DesktopState>();
    let cached = state.app_state.read().unwrap();
    assert_eq!(cached.instances.len(), 1);
    assert_eq!(cached.instances[0].id, kept.summary.id);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn instance_deletion_preserves_probed_library_install_state_for_all_repo_modules()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("delete-probe");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    assert_eq!(descriptors.len(), 32, "update the catalog fixture matrix");
    sync_modules(&storage.paths, &descriptors).await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;

    for descriptor in &descriptors {
        let module_id = &descriptor.summary.id;
        let install = descriptor
            .install
            .as_ref()
            .expect("catalog install contract");
        let install_root = storage.paths.games_root.join(&install.shared_game_dir);
        let verification = install_root.join(
            install
                .verification_path
                .as_deref()
                .or_else(|| {
                    descriptor
                        .process
                        .as_ref()
                        .map(|process| process.executable.as_str())
                        .filter(|path| !path.contains("{{"))
                })
                .expect("catalog verification path or static executable"),
        );
        fs::create_dir_all(verification.parent().expect("verification parent"))?;
        // Acquisition contains the verified payload. Runtime launch wrappers
        // such as start-romestead.bat are generated instance configuration.
        fs::write(&verification, b"synthetic program fixture, never executed")?;
        app_storage::record_library_program_baseline(&install_root, descriptor, true, None)?;
        sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: module_id.clone(),
                install_root: install_root.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: None,
                mark_verified: true,
            }],
        )
        .await?;
        let library = app_storage::read_library_program_install(&storage.paths, module_id)
            .await?
            .expect("registered fixture library");
        let mut instances = Vec::new();
        for name in ["Exclusive library instance", "Private program instance"] {
            let created = app_storage::create_instance_with_options(
                &storage.paths,
                descriptor,
                CreateInstanceInput {
                    name: name.into(),
                    module_id: module_id.clone(),
                },
                app_storage::InstanceCreationOptions {
                    prefer_existing_install: true,
                    require_clean_program: true,
                    program_mode: Some(app_storage::InstanceProgramMode::Independent),
                    ..Default::default()
                },
            )
            .await?;
            instances.push(created);
        }
        let exclusive_root = storage
            .paths
            .instances_root
            .join(&instances[0].provisioning.summary.id);
        assert!(app_storage::instance_uses_exclusive_program(
            &exclusive_root
        )?);
        assert_eq!(
            fs::canonicalize(&instances[0].effective_install_root)?,
            fs::canonicalize(&library.install_root)?,
            "{module_id}: first instance must exercise cleanup in the retained library"
        );
        let private_root = PathBuf::from(&instances[1].effective_install_root);
        assert_ne!(
            fs::canonicalize(&private_root)?,
            fs::canonicalize(&library.install_root)?,
            "{module_id}: second instance must exercise private program deletion"
        );
        let before =
            load_module_summaries_with_install_state(&storage, std::slice::from_ref(descriptor))
                .await?;
        assert_eq!(
            before[0].install_state,
            InstallState::Installed,
            "{module_id}"
        );
        assert_eq!(before[0].instance_program_count, 1, "{module_id}");

        for (index, instance) in instances.into_iter().enumerate() {
            delete_instance_record(app.handle().clone(), instance.provisioning.summary.id).await?;
            // Exercise the same filesystem probe and persisted-root lookup as
            // library refresh, rather than trusting the unchanged database flag.
            let summaries = load_module_summaries_with_install_state(
                &storage,
                std::slice::from_ref(descriptor),
            )
            .await?;
            assert_eq!(
                summaries[0].install_state,
                InstallState::Installed,
                "{module_id}: library probe after deleting instance {index}"
            );
            assert_eq!(
                summaries[0].instance_program_count,
                u32::from(index == 0),
                "{module_id}: private program count after deleting instance {index}"
            );
            assert_eq!(summaries[0].archived_program_count, 0, "{module_id}");
            assert!(
                library.install_root.is_dir(),
                "{module_id}: library retained"
            );
            assert_eq!(
                private_root.is_dir(),
                index == 0,
                "{module_id}: private root"
            );
            persist_descriptor_install_states(&storage, std::slice::from_ref(descriptor)).await?;
            let retained = app_storage::read_library_program_install(&storage.paths, module_id)
                .await?
                .expect("deletion must retain the registered library");
            assert_eq!(retained.id, library.id, "{module_id}");
            assert_eq!(
                retained.install_state,
                InstallState::Installed,
                "{module_id}"
            );
        }
    }
    Ok(())
}

fn retirement_tree_snapshot(
    root: &Path,
) -> std::io::Result<std::collections::BTreeMap<PathBuf, Option<Vec<u8>>>> {
    let mut snapshot = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("fixture child path")
                .to_path_buf();
            if entry.file_type()?.is_dir() {
                snapshot.insert(relative, None);
                pending.push(path);
            } else {
                snapshot.insert(relative, Some(fs::read(path)?));
            }
        }
    }
    Ok(snapshot)
}
