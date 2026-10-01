#[tokio::test]
async fn mark_instance_run_lifecycle_updates_status() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Runner"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let run = record_started_test_instance(
        &paths,
        &created.summary.id,
        4321,
        "D:/LanGame/instances/dst-runner/logs/run-1.log",
    )
    .await
    .unwrap();
    assert_eq!(run.pid, Some(4321));

    let active = read_active_instance_run(&paths, &created.summary.id)
        .await
        .unwrap()
        .expect("active run");
    assert_eq!(active.run_id, run.run_id);

    let running_summary = list_instances(&paths)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == created.summary.id)
        .unwrap();
    assert!(matches!(running_summary.status, InstanceStatus::Running));

    let running_details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        running_details.active_run.as_ref().map(|item| item.run_id),
        Some(run.run_id)
    );

    let stopped =
        mark_instance_process_stopped(&paths, &created.summary.id, run.run_id, Some(0), false)
            .await
            .unwrap();
    assert!(matches!(stopped.status, InstanceStatus::Stopped));

    let active_after_stop = read_active_instance_run(&paths, &created.summary.id)
        .await
        .unwrap();
    assert!(active_after_stop.is_none());

    let stopped_details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert!(stopped_details.active_run.is_none());

    cleanup_root(&root);
}

#[tokio::test]
async fn running_instance_rejects_bind_and_port_changes_atomically() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Frozen Network"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();
    let baseline = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let run = record_started_test_instance(
        &paths,
        &created.summary.id,
        4322,
        "D:/LanGame/instances/dst-frozen/logs/run-1.log",
    )
    .await
    .unwrap();

    let update = |bind_ip: String, ports: Vec<PortBinding>| UpdateInstanceInput {
        id: created.summary.id.clone(),
        bind_ip,
        auto_backup_on_stop: baseline.auto_backup_on_stop,
        backup_retention_count: baseline.backup_retention_count,
        settings_json: baseline.settings_json.clone(),
        ports,
    };
    let bind_error = update_instance(
        &paths,
        update(String::from("127.0.0.1"), baseline.ports.clone()),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        bind_error,
        StorageError::ActiveInstanceNetworkMutation { .. }
    ));

    let mut changed_ports = baseline.ports.clone();
    for port in &mut changed_ports {
        port.port += 1_000;
    }
    let port_error = update_instance(
        &paths,
        update(baseline.summary.bind_ip.clone(), changed_ports.clone()),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        port_error,
        StorageError::ActiveInstanceNetworkMutation { .. }
    ));

    let direct_port_error = update_instance_ports(&paths, &created.summary.id, &changed_ports)
        .await
        .unwrap_err();
    assert!(matches!(
        direct_port_error,
        StorageError::ActiveInstanceNetworkMutation { .. }
    ));
    let unchanged_direct = update_instance_ports(&paths, &created.summary.id, &baseline.ports)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&unchanged_direct).unwrap(),
        serde_json::to_value(&baseline.ports).unwrap()
    );

    let unchanged = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(unchanged.summary.bind_ip, baseline.summary.bind_ip);
    assert_eq!(
        serde_json::to_value(&unchanged.ports).unwrap(),
        serde_json::to_value(&baseline.ports).unwrap()
    );

    mark_instance_process_stopped(&paths, &created.summary.id, run.run_id, Some(0), false)
        .await
        .unwrap();
    cleanup_root(&root);
}

#[tokio::test]
async fn runtime_overview_reads_recent_runs_and_log_tail() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Logs"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-1.log");
    fs::write(
        &log_path,
        "booting\nloading world\nserver ready\nlistening on udp/10999\nheartbeat ok\n",
    )
    .unwrap();

    let run = record_started_test_instance(
        &paths,
        &created.summary.id,
        6789,
        &log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(overview.recent_runs.len(), 1);
    assert_eq!(overview.recent_runs[0].run_id, run.run_id);
    assert_eq!(overview.recent_runs[0].status, "running");
    assert_eq!(
        overview.log_tail.source_path,
        Some(log_path.to_string_lossy().into_owned())
    );
    assert_eq!(overview.log_tail.total_lines, 5);
    assert!(overview.log_tail.read_error.is_none());
    assert_eq!(overview.health.status, "ready");
    assert!(
        overview.health.matched_line.is_some(),
        "expected runtime health to capture a ready line"
    );
    assert!(
        overview
            .log_tail
            .lines
            .iter()
            .any(|line| line.contains("server ready"))
    );

    let document = read_instance_log_document(&paths, &created.summary.id, 3, None)
        .await
        .unwrap();
    assert_eq!(document.total_lines, 5);
    assert!(document.truncated);
    assert_eq!(document.lines.len(), 3);
    assert_eq!(document.lines[0], "server ready");

    mark_instance_process_stopped(&paths, &created.summary.id, run.run_id, Some(0), false)
        .await
        .unwrap();

    let second_log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-2.log");
    fs::write(
        &second_log_path,
        "booting again\nloading mods\nserver ready second\nheartbeat second\n",
    )
    .unwrap();

    let second_run = record_started_test_instance(
        &paths,
        &created.summary.id,
        6790,
        &second_log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    let latest_document = read_instance_log_document(&paths, &created.summary.id, 10, None)
        .await
        .unwrap();
    assert_eq!(
        latest_document.source_path,
        Some(second_log_path.to_string_lossy().into_owned())
    );
    assert!(
        latest_document
            .lines
            .iter()
            .any(|line| line.contains("server ready second"))
    );

    let first_run_document =
        read_instance_log_document(&paths, &created.summary.id, 10, Some(run.run_id))
            .await
            .unwrap();
    assert_eq!(
        first_run_document.source_path,
        Some(log_path.to_string_lossy().into_owned())
    );
    assert!(
        first_run_document
            .lines
            .iter()
            .any(|line| line.contains("server ready"))
    );

    mark_instance_process_stopped(
        &paths,
        &created.summary.id,
        second_run.run_id,
        Some(0),
        false,
    )
    .await
    .unwrap();

    let stopped_overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(stopped_overview.recent_runs[0].run_id, second_run.run_id);
    assert_eq!(stopped_overview.recent_runs[0].status, "stopped");

    cleanup_root(&root);
}

#[tokio::test]
async fn dragonwilds_runtime_overview_marks_ready_to_join_as_ready() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = runescape_dragonwilds_test_descriptor(&root);
    prepare_runescape_dragonwilds_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Dragonwilds Ready"),
            module_id: String::from("runescapedragonwilds"),
        },
    )
    .await
    .unwrap();

    let log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-dragonwilds.log");
    fs::write(
        &log_path,
        concat!(
            "LogInit: Command Line: -Port=17777 -QueryPort=27057 -log -stdout -FullStdOutLogOutput\n",
            "LogRedpointEOSFrameworkExtra: Verbose: Discovered 0 console commands using RedpointConsoleCommand class.\n",
            "LogNet: Name:GameNetDriver IpNetDriver listening on port 17777\n",
            "LogPersistence: [DedicatedServer] NewGame() WorldName[LanGameSmokeWorld] MapName[L_World]\n",
            "LogRedpointEOS: Verbose: CreateSession: Successfully created session 'GameSession'\n",
            "LogDomMatcherSession: START SESSION - Success\n",
            "LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]\n",
        ),
    )
    .unwrap();

    record_started_test_instance(
        &paths,
        &created.summary.id,
        4019830,
        &log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(overview.health.status, "ready");
    assert!(
        overview.health.summary.contains("ReadyToJoin"),
        "unexpected runtime health summary: {}",
        overview.health.summary
    );
    assert!(
        overview.health.summary.contains("L_World"),
        "Dragonwilds runtime health should include the loaded map: {}",
        overview.health.summary
    );
    assert!(
        overview
            .health
            .matched_line
            .as_deref()
            .unwrap_or_default()
            .contains("ReadyToJoin"),
        "expected Dragonwilds health to capture the ReadyToJoin line"
    );
    assert!(
        overview.diagnostics.iter().any(|signal| {
            signal.code == "dragonwilds_no_host_console_commands_discovered"
                && signal.severity == "info"
                && !signal.actionable
        }),
        "expected Dragonwilds diagnostics to record the missing host console command surface"
    );
    assert!(
        overview.diagnostics.iter().any(|signal| {
            signal.code == "dragonwilds_query_port_launch_arg_observed"
                && signal.severity == "info"
                && signal.summary.contains("27057")
                && signal.summary.contains("player query")
                && !signal.actionable
        }),
        "expected Dragonwilds diagnostics to record that QueryPort was launch-observed without enabling a player query surface: {:?}",
        overview.diagnostics
    );

    cleanup_root(&root);
}

#[test]
fn dragonwilds_query_port_binding_signal_distinguishes_launch_arg_from_udp_bind() {
    let missing_signal = crate::runtime::runescape_dragonwilds_query_port_binding_signal(
        27057,
        &std::collections::HashSet::from([17777_u16]),
        Some(String::from(
            "LogInit: Command Line: -Port=17777 -QueryPort=27057",
        )),
    );
    assert_eq!(missing_signal.code, "dragonwilds_query_port_not_bound");
    assert_eq!(missing_signal.severity, "warning");
    assert!(missing_signal.actionable);
    assert!(missing_signal.summary.contains("27057"));

    let bound_signal = crate::runtime::runescape_dragonwilds_query_port_binding_signal(
        27057,
        &std::collections::HashSet::from([17777_u16, 27057_u16]),
        Some(String::from(
            "LogInit: Command Line: -Port=17777 -QueryPort=27057",
        )),
    );
    assert_eq!(bound_signal.code, "dragonwilds_query_port_bound");
    assert_eq!(bound_signal.severity, "info");
    assert!(!bound_signal.actionable);
    assert!(bound_signal.summary.contains("27057"));
}

#[tokio::test]
async fn dragonwilds_runtime_overview_reads_official_saved_log_when_process_log_is_empty() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let mut descriptor = runescape_dragonwilds_test_descriptor(&root);
    // Match the real module: existing native saved data stays in the library
    // and must never become this instance's current log.
    descriptor.storage.runtime_copy_exclusions = vec!["RSDragonwilds/Saved".into()];
    prepare_runescape_dragonwilds_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("runescapedragonwilds");
    let shared_log = install_root.join("RSDragonwilds/Saved/Logs/RSDragonwilds.log");
    fs::create_dir_all(shared_log.parent().unwrap()).unwrap();
    fs::write(&shared_log, b"shared package log must not be selected").unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("runescapedragonwilds"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("smoke")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let library_before = crate::read_library_program_install(&paths, "runescapedragonwilds")
        .await.unwrap().unwrap();
    let library_files = crate::test_file_snapshot::tree_snapshot(&install_root).unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Dragonwilds Saved Log"),
            module_id: String::from("runescapedragonwilds"),
        },
    )
    .await
    .unwrap();

    let official_log_path = instance_private_runtime_root(&created)
        .join("RSDragonwilds")
        .join("Saved")
        .join("Logs")
        .join("RSDragonwilds.log");
    assert!(!official_log_path.exists(), "library logs must not be copied into a new instance");
    fs::create_dir_all(official_log_path.parent().unwrap()).unwrap();
    let process_log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-dragonwilds-empty.log");
    fs::write(&process_log_path, "").unwrap();
    mark_instance_process_started_with_identity(
        &paths,
        &StartedInstanceProcess {
            instance_id: &created.summary.id,
            session_id: Some("dragonwilds-current-session"),
            process_key: "main",
            display_name: "Server",
            pid: 4019830,
            log_path: &process_log_path.to_string_lossy(),
            is_primary: true,
        },
        Some(&app_core::ProcessIdentity {
            // 2026-09-30 01:02:03.000 UTC in Windows FILETIME units.
            creation_time: 134_352_037_230_000_000,
            image_path: instance_private_runtime_root(&created)
                .join("RSDragonwildsServer.exe")
                .to_string_lossy()
                .into_owned(),
        }),
    )
    .await
    .unwrap();

    for prefix in ["", "[2026.09.30-01.02.02:999][ 1]"] {
        fs::write(
            &official_log_path,
            format!(
                "{prefix}LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]\n"
            ),
        )
        .unwrap();
        let overview = read_instance_runtime_overview(&paths, &created.summary.id)
            .await
            .unwrap();
        assert_eq!(
            overview.log_tail.source_path.as_deref(),
            Some(official_log_path.to_string_lossy().as_ref())
        );
        assert_eq!(overview.health.status, "starting");
    }
    fs::write(
        &official_log_path,
        concat!(
            "[2026.09.30-01.02.03:001][ 1]LogPersistence: [DedicatedServer] PostLoadWorldState() WorldName[SmokeWorld] MapName[L_World]\n",
            "[2026.09.30-01.02.03:001][ 1]LogRedpointEOS: Verbose: CreateSession: Successfully created session 'GameSession'\n",
            "[2026.09.30-01.02.03:001][ 1]LogDomMatcherSession: START SESSION - Success\n",
            "[2026.09.30-01.02.03:001][ 1]LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]\n",
        ),
    )
    .unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        overview.log_tail.source_path.as_deref(),
        Some(official_log_path.to_string_lossy().as_ref())
    );
    assert_eq!(overview.health.status, "ready");
    assert!(
        overview.health.summary.contains("L_World"),
        "Dragonwilds runtime health should come from the official saved log: {}",
        overview.health.summary
    );

    let retained_log = instance_private_runtime_root(&created).parent().unwrap()
        .join("installation-retained/RSDragonwilds/Saved/Logs/RSDragonwilds.log");
    assert!(!retained_log.exists(), "excluded library data must remain at its original path");
    assert_eq!(
        fs::read(&shared_log).unwrap(),
        b"shared package log must not be selected"
    );
    assert_eq!(crate::test_file_snapshot::tree_snapshot(&install_root).unwrap(), library_files);
    let library_after = crate::read_library_program_install(&paths, "runescapedragonwilds")
        .await.unwrap().unwrap();
    assert_eq!(library_after.id, library_before.id);
    assert_eq!(library_after.current_version, library_before.current_version);
    assert_eq!(library_after.install_state, InstallState::Installed);
    assert_eq!(library_after.install_root, install_root);
    assert_eq!(library_after.scope, crate::ProgramInstallScope::Library);
    assert_eq!(library_after.owner_instance_id, None);
    cleanup_root(&root);
}

#[tokio::test]
async fn dragonwilds_runtime_overview_reports_latest_world_save_file() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = runescape_dragonwilds_test_descriptor(&root);
    prepare_runescape_dragonwilds_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("runescapedragonwilds");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("runescapedragonwilds"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("smoke")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Dragonwilds Latest Save"),
            module_id: String::from("runescapedragonwilds"),
        },
    )
    .await
    .unwrap();
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let saves_root = PathBuf::from(&details.saves_path);
    fs::create_dir_all(&saves_root).unwrap();
    fs::write(saves_root.join("OldWorld.sav"), "old").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    fs::write(saves_root.join("SmokeWorld.sav"), "new").unwrap();
    fs::write(saves_root.join("SmokeWorld.tmp"), "ignore").unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert!(
        overview.diagnostics.iter().any(|signal| {
            signal.code == "dragonwilds_latest_world_save_detected"
                && signal.severity == "info"
                && signal.summary.contains("SmokeWorld.sav")
                && signal.summary.contains("2 .sav")
                && !signal.actionable
        }),
        "expected Dragonwilds diagnostics to expose the latest .sav world file: {:?}",
        overview.diagnostics
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn dragonwilds_runtime_overview_marks_missing_owner_id_as_error() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = runescape_dragonwilds_test_descriptor(&root);
    prepare_runescape_dragonwilds_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Dragonwilds Missing Owner"),
            module_id: String::from("runescapedragonwilds"),
        },
    )
    .await
    .unwrap();

    let log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-dragonwilds-owner.log");
    fs::write(
        &log_path,
        "Dedicated Server cannot continue due to DedicatedServer.ini configuration. The [OwnerId] for this server is empty\n",
    )
    .unwrap();

    record_started_test_instance(
        &paths,
        &created.summary.id,
        4019831,
        &log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(overview.health.status, "error");
    assert!(
        overview.health.summary.contains("OwnerId"),
        "unexpected runtime health summary: {}",
        overview.health.summary
    );
    assert!(
        overview
            .health
            .matched_line
            .as_deref()
            .unwrap_or_default()
            .contains("OwnerId"),
        "expected Dragonwilds health to capture the OwnerId validation line"
    );

    cleanup_root(&root);
}
