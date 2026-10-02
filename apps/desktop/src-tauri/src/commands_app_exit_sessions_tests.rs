use super::*;

#[tokio::test]
pub(super) async fn app_exit_drains_multiple_active_sessions_for_one_instance()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("app-exit-multi-session");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_terraria_install(&settings)?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "terraria")?;
    let install = descriptor
        .install
        .as_ref()
        .expect("Terraria fixture install contract");
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: String::from("terraria"),
            install_root: storage
                .paths
                .games_root
                .join(&install.shared_game_dir)
                .to_string_lossy()
                .into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let created = create_instance_record_inner(
        app.state::<DesktopState>(),
        CreateInstanceInput {
            name: String::from("App Exit Multi Session"),
            module_id: String::from("terraria"),
        },
    )
    .await?;
    let executable = app_runtime::windows_system_directory()?.join("cmd.exe");
    let plan = LaunchPlan {
        environment: Default::default(),
        instance_id: created.summary.id.clone(),
        instance_name: created.summary.name.clone(),
        module_id: created.summary.module_id.clone(),
        install_root: root.to_string_lossy().into_owned(),
        install_state: InstallState::Installed,
        uses_private_runtime: false,
        working_directory: root.to_string_lossy().into_owned(),
        executable_path: executable.to_string_lossy().into_owned(),
        executable_exists: executable.is_file(),
        ready_to_launch: true,
        validation_issues: vec![],
        // Retain the native child until its identity and session are registered.
        args: vec!["/d".into(), "/q".into()],
        command_line: String::from("owned app-exit session fixture"),
        window_policy: ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
        performance_policy: RuntimePerformancePolicy::default(),
        performance_preview: Default::default(),
    };
    let mut processes = Vec::new();
    for (session_id, process_key) in [("old-session", "old"), ("new-session", "new")] {
        let spawned = spawn_launch_plan(&plan, root.join(format!("{session_id}.log")))?;
        let process = mark_instance_process_started_with_identity(
            &storage.paths,
            &StartedInstanceProcess {
                instance_id: &created.summary.id,
                session_id: Some(session_id),
                process_key,
                display_name: process_key,
                pid: spawned.pid,
                log_path: &spawned.log_path,
                is_primary: true,
            },
            Some(&spawned.process_identity),
        )
        .await?;
        processes.push(ManagedProcess {
            run_id: process.run_id,
            process_key: process_key.into(),
            display_name: process_key.into(),
            pid: spawned.pid,
            process_identity: spawned.process_identity,
            root_process_identity: spawned.root_process_identity,
            log_path: spawned.log_path,
            is_primary: session_id == "new-session",
            uses_script_entrypoint: spawned.uses_script_entrypoint,
            performance_policy: RuntimePerformancePolicy::default(),
            last_performance_refresh: None,
            last_performance_target_count: None,
            last_performance_application: None,
            child: spawned.child,
            hidden_desktop: spawned.hidden_desktop,
        });
    }
    assert_eq!(
        list_active_instance_runs(&storage.paths)
            .await?
            .into_iter()
            .filter(|run| run.instance_id == created.summary.id)
            .count(),
        2
    );

    let state = app.state::<DesktopState>();
    let active_run = read_active_instance_run(&storage.paths, &created.summary.id)
        .await?
        .ok_or("both fixture sessions must remain active")?;
    {
        let mut supervisor = state.runtime_supervisor.lock().unwrap();
        supervisor.insert_running(
            created.summary.clone(),
            Some("new-session".into()),
            processes,
        );
        assert_eq!(supervisor.tracked_instances()[0].run_id, active_run.run_id);
        assert_eq!(
            supervisor.instance_process_tree_is_running(&created.summary.id)?,
            Some(true),
        );
        for process_key in ["old", "new"] {
            supervisor.dispatch_command(&created.summary.id, Some(process_key), "exit 0")?;
        }
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if state
                .runtime_supervisor
                .lock()
                .unwrap()
                .instance_process_tree_is_running(&created.summary.id)?
                == Some(false)
            {
                break Ok::<(), app_runtime::RuntimeProcessError>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    let storage_shutdown = state.begin_storage_shutdown_exclusive()?;
    let stopped = shutdown_one_running_instance_for_app_exit(
        &state,
        &storage,
        &storage_shutdown,
        &created.summary.id,
        &HashMap::new(),
    )
    .await?;
    assert_eq!(
        stopped
            .expect("stopped instance is available for the backup phase")
            .summary
            .id,
        created.summary.id,
    );

    assert!(
        list_active_instance_runs(&storage.paths)
            .await?
            .into_iter()
            .all(|run| run.instance_id != created.summary.id),
        "all sessions for the instance must be drained before app exit completes"
    );
    for instance_id in [created.summary.id.as_str(), "reserved-start-without-a-run"] {
        assert!(
            shutdown_one_running_instance_for_app_exit(
                &state,
                &storage,
                &storage_shutdown,
                instance_id,
                &HashMap::new(),
            )
            .await?
            .is_none(),
            "an already stopped or cancelled start must not schedule a backup"
        );
    }
    drop(storage_shutdown);
    state.shutdown_in_progress.store(false, Ordering::SeqCst);
    drop(app);
    let _ = fs::remove_dir_all(root);
    Ok::<(), Box<dyn std::error::Error>>(())
}
