use super::*;

#[tokio::test(flavor = "current_thread")]
async fn windrose_bootstrap_retained_world_waits_for_current_host_after_engine_ready() {
    let fixture = Fixture::new("held_ready").await;
    let state = DesktopState::default();
    let reservation = reserve(&state, &fixture.instance.summary.id);
    let group = state
        .runtime_resource_admission
        .reserve(
            &fixture.instance.summary.id,
            &fixture.plan.launch_plan.performance_policy.resource_limits,
        )
        .unwrap();
    let prepared =
        app_storage::prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .unwrap()
            .unwrap();
    // A previous interrupted bootstrap can leave a complete world and ready
    // output. Neither belongs to the process about to start.
    write_native_world(&fixture.runtime());
    assert!(prepared.inspect_native_state().unwrap().world_ready);
    std::fs::write(&fixture.plan.log_path, format!("{HOST_READY_LOG}\n")).unwrap();
    let plan = fixture.plan.clone();
    let mut worker =
        tokio::spawn(
            async move { run_bootstrap(prepared, permit(), plan, reservation, group).await },
        );
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if std::fs::read_to_string(&fixture.plan.log_path)
                .unwrap_or_default()
                .contains("LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the current engine must initialize before the held HostReady assertion");
    let premature = tokio::time::timeout(Duration::from_millis(1500), &mut worker).await;
    let completed_before_current_ready = premature.is_ok();
    let command_before_current_ready = fixture.runtime().join("bootstrap-command").exists();
    // Always release the synthetic child, even when demonstrating the defect.
    std::fs::write(fixture.runtime().join("release-ready"), b"release").unwrap();
    let outcome = match premature {
        Ok(result) => result.unwrap(),
        Err(_) => worker.await.unwrap(),
    };
    assert!(
        !completed_before_current_ready,
        "old world/HostReady plus current EngineInit must not stop before current HostReady"
    );
    assert!(
        !command_before_current_ready,
        "current EngineInit alone must not dispatch native quit before the current HostReady"
    );
    let pid = fixture.wait_for_pid().await;
    assert!(!app_runtime::process_is_running(pid).unwrap());
    assert!(matches!(outcome.unwrap(), BootstrapOutcome::Ready));
    assert!(fixture.runtime().join("bootstrap-flushed").is_file());
    assert_eq!(
        std::fs::read_to_string(fixture.runtime().join("bootstrap-command")).unwrap(),
        "quit"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn windrose_bootstrap_unavailable_console_retains_owned_process_and_journal() {
    let fixture = Fixture::new("detached_ready").await;
    let state = DesktopState::default();
    let reservation = reserve(&state, &fixture.instance.summary.id);
    let group = state
        .runtime_resource_admission
        .reserve(
            &fixture.instance.summary.id,
            &fixture.plan.launch_plan.performance_policy.resource_limits,
        )
        .unwrap();
    let prepared =
        app_storage::prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .unwrap()
            .unwrap();
    let outcome = run_bootstrap(prepared, permit(), fixture.plan.clone(), reservation, group)
        .await
        .unwrap();
    let BootstrapOutcome::CleanupPending(mut pending) = outcome else {
        panic!("unavailable console must retain the live owner, not complete bootstrap");
    };
    let was_owned = pending.spawned.child.is_some();
    let was_alive = app_runtime::process_matches_identity(
        pending.spawned.pid,
        &pending.spawned.process_identity,
    )
    .unwrap();
    let retained = fixture
        .paths
        .instances_root
        .join(&fixture.instance.summary.id)
        .join("config/.langame-windrose-bootstrap.json")
        .is_file();
    // Explicit cleanup of this disposable synthetic fixture only. Production
    // must return the live owner to the ordinary registered lifecycle.
    app_runtime::stop_spawned_process(&mut pending.spawned).unwrap();
    assert!(was_owned && was_alive && retained);
    assert!(!fixture.runtime().join("bootstrap-flushed").exists());
    assert!(
        pending
            .message
            .contains("process-tree shutdown is unconfirmed")
    );
}
