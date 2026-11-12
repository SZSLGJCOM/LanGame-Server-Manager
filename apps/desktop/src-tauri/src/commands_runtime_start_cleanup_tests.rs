use super::*;

#[tokio::test]
async fn startup_cleanup_missing_owner_cannot_turn_an_exited_pid_into_cancellation_success() {
    let root = crate::commands::tests::temp_test_dir("cancel-missing-owner");
    fs::create_dir_all(&root).unwrap();
    let executable = app_runtime::windows_system_directory()
        .unwrap()
        .join("WindowsPowerShell/v1.0/powershell.exe");
    let launch_plan = LaunchPlan {
        instance_id: "cleanup-fixture".into(),
        instance_name: "Cleanup fixture".into(),
        module_id: "fixture".into(),
        install_root: root.to_string_lossy().into_owned(),
        install_state: InstallState::Installed,
        uses_private_runtime: false,
        can_update_before_start: false,
        working_directory: root.to_string_lossy().into_owned(),
        executable_path: executable.to_string_lossy().into_owned(),
        executable_exists: true,
        ready_to_launch: true,
        validation_issues: vec![],
        args: vec!["-NoProfile".into(), "-Command".into(), "exit 0".into()],
        environment: Default::default(),
        command_line: "isolated cleanup fixture".into(),
        window_policy: ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
        performance_policy: Default::default(),
        performance_preview: Default::default(),
    };
    let log_path = root.join("server.log");
    let mut process = app_runtime::spawn_launch_plan(&launch_plan, &log_path).unwrap();
    // Make the fixture safe before simulating a caller that lost its owner.
    app_runtime::force_stop_spawned_process(&mut process).unwrap();
    assert!(process.child.is_none());
    assert!(!process_matches_identity(process.pid, &process.process_identity).unwrap());
    let mut processes = vec![(
        ProcessLaunchPlan {
            process_key: "main".into(),
            display_name: "Fixture".into(),
            log_path: log_path.to_string_lossy().into_owned(),
            launch_plan,
        },
        process,
    )];
    let state = DesktopState::default();
    let reservation = match state
        .try_reserve_runtime_start("cleanup-fixture", "manual")
        .unwrap()
    {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected reservation: {other:?}"),
    };
    let receipt = state
        .request_runtime_start_cancellation("cleanup-fixture")
        .unwrap()
        .unwrap();
    let report = cleanup(&mut processes, &reservation).await.unwrap();
    assert!(
        report.has_survivors(),
        "missing tree ownership is unconfirmed cleanup"
    );
    drop(processes);
    drop(reservation);
    assert!(
        receipt
            .wait_until_settled(Duration::ZERO)
            .await
            .unwrap_err()
            .contains("original launch owner")
    );
    fs::remove_dir_all(root).unwrap();
}
