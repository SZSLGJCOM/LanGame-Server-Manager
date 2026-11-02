use super::*;
use ark::removal::{RemoveInput, remove_ark_tools};

#[test]
fn ark_tools_remove_is_local_owned_work_and_rejects_unrelated_arguments() {
    use crate::command_registry::{Completion, Execution, LanAccess};
    let command = crate::command_registry::find("remove_ark_tools").unwrap();
    assert_eq!(command.execution, Execution::Runtime);
    assert_eq!(command.lan, LanAccess::Denied);
    assert_eq!(command.completion, Completion::Owned);
    assert!(serde_json::from_value::<RemoveInput>(json!({"instanceId": "ark"})).is_ok());
    assert!(
        serde_json::from_value::<RemoveInput>(json!({
            "instanceId": "ark", "root": "C:/unrelated"
        }))
        .is_err()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn ark_tools_remove_waits_for_instance_lock_and_rejects_pending_start()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::new("arksurvivalevolved").await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let held = app
        .state::<DesktopState>()
        .acquire_instance_mutation(&fixture.id)
        .await;
    let reservation = match app
        .state::<DesktopState>()
        .try_reserve_runtime_start(&fixture.id, "test")?
    {
        crate::state::RuntimeStartReservationAttempt::Reserved(lease) => lease,
        other => panic!("expected isolated start reservation: {other:?}"),
    };
    let handle = app.handle().clone();
    let id = fixture.id.clone();
    let mut operation = tokio::spawn(async move {
        remove_ark_tools(
            handle.state::<DesktopState>(),
            RemoveInput { instance_id: id },
        )
        .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(150), &mut operation)
            .await
            .is_err()
    );
    fixture.untouched();
    drop(held);
    let result = tokio::time::timeout(Duration::from_secs(15), operation).await??;
    assert!(
        result
            .err()
            .ok_or("pending start was accepted")?
            .contains("停止")
    );
    fixture.untouched();
    drop(reservation);
    drop(app);
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn ark_tools_remove_rejects_an_active_installation() -> Result<(), Box<dyn std::error::Error>>
{
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::new("arksurvivalevolved").await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let preparation = app
        .state::<DesktopState>()
        .ark_tools_progress
        .begin(fixture.id.clone(), "install-test".into())?;
    let result = remove_ark_tools(
        app.state::<DesktopState>(),
        RemoveInput {
            instance_id: fixture.id.clone(),
        },
    )
    .await;
    assert!(
        result
            .err()
            .ok_or("active installation was accepted")?
            .contains("正在安装")
    );
    fixture.untouched();
    drop(preparation);
    drop(app);
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn ark_tools_remove_rejects_a_live_runtime_before_touching_files()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::new("arksurvivalevolved").await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let details = read_instance_details(&fixture.storage.paths, &fixture.id).await?;
    let identity =
        inspect_process_identity(std::process::id())?.ok_or("test process disappeared")?;
    let log_path = fixture
        .root
        .join("running-guard.log")
        .to_string_lossy()
        .into_owned();
    let session_id = "ark-remove-running-guard";
    let run = mark_instance_process_started_with_identity(
        &fixture.storage.paths,
        &StartedInstanceProcess {
            instance_id: &fixture.id,
            session_id: Some(session_id),
            process_key: "main",
            display_name: "Server",
            pid: std::process::id(),
            log_path: &log_path,
            is_primary: true,
        },
        Some(&identity),
    )
    .await?;
    app.state::<DesktopState>()
        .runtime_supervisor
        .lock()
        .unwrap()
        .insert_running(
            details.summary,
            Some(session_id.into()),
            vec![ManagedProcess {
                run_id: run.run_id,
                process_key: "main".into(),
                display_name: "Server".into(),
                pid: std::process::id(),
                process_identity: identity.clone(),
                root_process_identity: identity,
                log_path,
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
    let result = remove_ark_tools(
        app.state::<DesktopState>(),
        RemoveInput {
            instance_id: fixture.id.clone(),
        },
    )
    .await;
    // This is only a synthetic registration of the test process, never a child
    // to terminate. Remove the registration before any assertions or cleanup.
    app.state::<DesktopState>()
        .runtime_supervisor
        .lock()
        .unwrap()
        .take_running_for_stop(&fixture.id);
    mark_instance_process_stopped(&fixture.storage.paths, &fixture.id, run.run_id, None, false)
        .await?;
    assert!(
        result
            .err()
            .ok_or("running instance was accepted")?
            .contains("停止")
    );
    fixture.untouched();
    drop(app);
    fixture.finish()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn ark_tools_remove_rejects_a_different_program_binding()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let fixture = ArkFixture::new("arksurvivalevolved").await?;
    let other = fixture.root.join("another-runtime");
    fs::create_dir(&other)?;
    let binding = app_storage::read_instance_program_install(&fixture.storage.paths, &fixture.id)
        .await?
        .ok_or("ARK fixture instance binding disappeared")?;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(&fixture.storage.paths.database_path),
    )
    .await?;
    let changed = sqlx::query("UPDATE game_installs SET install_root=?1 WHERE id=?2")
        .bind(other.to_string_lossy().as_ref())
        .bind(binding.install.id)
        .execute(&pool)
        .await;
    pool.close().await;
    assert_eq!(changed?.rows_affected(), 1);
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let result = remove_ark_tools(
        app.state::<DesktopState>(),
        RemoveInput {
            instance_id: fixture.id.clone(),
        },
    )
    .await;
    let error = result.err().ok_or("wrong program binding was accepted")?;
    assert!(
        error.contains("program path does not match its registered installation"),
        "{error}"
    );
    fixture.untouched();
    assert_eq!(fs::read_dir(other)?.count(), 0);
    drop(app);
    fixture.finish()?;
    Ok(())
}
