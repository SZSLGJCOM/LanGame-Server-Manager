use super::*;
use std::future::Future;
use std::task::Poll;

#[tokio::test(flavor = "current_thread")]
async fn assistant_runtime_dispatch_rechecks_preview_after_waiting_for_instance_lock()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-runtime-lock");
    let _env = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let instance = create_fake_minecraft_instance(state.clone(), "Runtime lock fixture").await?;
    let storage = bootstrap_storage()?;
    let instance_id = &instance.summary.id;
    let process = StartedInstanceProcess {
        instance_id,
        session_id: Some("preview-run"),
        process_key: "main",
        display_name: "Fixture process record",
        pid: 1234,
        log_path: "isolated-runtime-fixture.log",
        is_primary: true,
    };
    let old_run =
        mark_instance_process_started_with_identity(&storage.paths, &process, None).await?;
    let expected = read_instance_details(&storage.paths, instance_id).await?;
    let held_lock = state.acquire_instance_mutation(instance_id).await;
    let mut cached = None;
    let pending = acquire_runtime_command_instance_mutation(
        &state,
        &storage,
        instance_id,
        Some(&expected),
        &mut cached,
    );
    tokio::pin!(pending);
    // Poll through the uncontended lock registry to the held instance lock.
    // This barrier uses no sleeps and keeps the attempted dispatch queued.
    std::future::poll_fn(|cx| {
        assert!(matches!(pending.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    mark_instance_process_stopped(&storage.paths, instance_id, old_run.run_id, Some(0), false)
        .await?;
    let replacement = StartedInstanceProcess {
        session_id: Some("replacement-run"),
        ..process
    };
    let new_run =
        mark_instance_process_started_with_identity(&storage.paths, &replacement, None).await?;
    assert_ne!(old_run.run_id, new_run.run_id);
    drop(held_lock);
    let error = pending
        .await
        .expect_err("a replacement run must invalidate the preview");
    assert!(error.contains("changed after the assistant preview"));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_runtime_dispatch_holds_instance_lock_after_validating_unchanged_state()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-runtime-unchanged");
    let _env = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let instance =
        create_fake_minecraft_instance(state.clone(), "Unchanged runtime fixture").await?;
    let storage = bootstrap_storage()?;
    let instance_id = &instance.summary.id;
    let expected = read_instance_details(&storage.paths, instance_id).await?;
    let mut cached = None;
    let dispatch_lock = acquire_runtime_command_instance_mutation(
        &state,
        &storage,
        instance_id,
        Some(&expected),
        &mut cached,
    )
    .await?;
    assert_eq!(cached.as_ref().unwrap().summary.id, *instance_id);
    let competing = state.acquire_instance_mutation(instance_id);
    tokio::pin!(competing);
    std::future::poll_fn(|cx| {
        assert!(matches!(competing.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    drop(dispatch_lock);
    let _released = competing.await;
    Ok(())
}
