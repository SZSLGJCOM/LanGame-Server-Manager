use super::*;
use std::future::Future;
use std::os::windows::fs::OpenOptionsExt;
use std::task::Poll;

async fn wait_for_uninstall_status(state: &DesktopState, expected: JobStatus) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        loop {
            if state.app_state.read().unwrap().jobs.iter().any(|job| {
                matches!(job.kind, JobKind::UninstallGame)
                    && std::mem::discriminant(&job.status) == std::mem::discriminant(&expected)
            }) {
                return;
            }
            tick.tick().await;
        }
    })
    .await
    .expect("owned uninstall must reach its observed job state");
}

async fn cancelled_uninstall_finishes_with(
    release_lock: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("uninst-cancel");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    let (storage, descriptor) = installed_astroneer_fixture(&root).await?;
    let instance = create_instance(
        &storage.paths,
        &descriptor,
        CreateInstanceInput {
            name: "Cancelled request".into(),
            module_id: "astroneer".into(),
        },
    )
    .await?;
    let instance = read_instance_details(&storage.paths, &instance.summary.id).await?;
    let private_save = PathBuf::from(&instance.saves_path).join("retained.savegame");
    fs::create_dir_all(private_save.parent().unwrap())?;
    fs::write(&private_save, b"private world")?;
    let instance_config = fs::read(&instance.config_file_path)?;
    let private_program = app_storage::resolve_instance_runtime_root(
        &storage.paths.instances_root.join(&instance.summary.id),
    )?
    .join("AstroServer.exe");
    let private_program_bytes = fs::read(&private_program)?;
    seed_astroneer_library(&storage, &descriptor).await?;
    let install_root = storage.paths.games_root.join("astroneer");
    let native_config = install_root.join("Astro/Saved/Config/WindowsServer/Engine.ini");
    let native_config_bytes = fs::read(&native_config)?;
    let program = install_root.join("AstroServer.exe");
    let mut lock = Some(
        fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&program)?,
    );
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let requester = tokio::spawn(uninstall_module_game(
        app.handle().clone(),
        "astroneer".into(),
    ));

    // Running is published only after both lifecycle/mutation locks are held.
    // Observe that signal; do not assume a sleep means staging has begun.
    wait_for_uninstall_status(&state, JobStatus::Running).await;
    requester.abort();
    assert!(requester.await.unwrap_err().is_cancelled());
    let lifecycle_roots = [install_root.clone()];
    let mut lifecycle = Box::pin(app_steamcmd::acquire_game_install_lifecycle(
        "astroneer",
        &lifecycle_roots,
    ));
    assert!(
        std::future::poll_fn(|cx| Poll::Ready(lifecycle.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    let mut mutation = Box::pin(state.acquire_instance_mutation(&instance.summary.id));
    assert!(
        std::future::poll_fn(|cx| Poll::Ready(mutation.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(state.begin_storage_context_transition().is_err());
    if release_lock {
        drop(lock.take());
    }
    let expected_job = if release_lock {
        JobStatus::Completed
    } else {
        JobStatus::Failed
    };
    wait_for_uninstall_status(&state, expected_job).await;
    // Completion must release the locks even though nobody awaited the command.
    let _lifecycle = tokio::time::timeout(Duration::from_secs(5), lifecycle).await??;
    let _mutation = tokio::time::timeout(Duration::from_secs(5), mutation).await?;
    assert!(state.begin_storage_context_transition().is_ok());

    let expected_install = if release_lock {
        InstallState::NotInstalled
    } else {
        InstallState::Installed
    };
    assert!(
        state
            .app_state
            .read()
            .unwrap()
            .modules
            .iter()
            .any(|module| { module.id == "astroneer" && module.install_state == expected_install })
    );
    let library = app_storage::read_library_program_install(&storage.paths, "astroneer")
        .await?
        .expect("library registration survives uninstall");
    assert_eq!(library.install_state, expected_install);
    // The stored module summary includes the still-installed independent instance.
    assert!(
        sync_modules(&storage.paths, &[])
            .await?
            .iter()
            .any(|module| {
                module.id == "astroneer" && module.install_state == InstallState::Installed
            })
    );
    assert_eq!(program.exists(), !release_lock);
    assert_eq!(fs::read(&native_config)?, native_config_bytes);
    assert_eq!(fs::read(&instance.config_file_path)?, instance_config);
    assert_eq!(fs::read(&private_program)?, private_program_bytes);
    assert_eq!(fs::read(&private_save)?, b"private world");
    assert_eq!(
        fs::read(storage.paths.games_root.join("other-game.sentinel"))?,
        b"keep unrelated files"
    );
    if !release_lock {
        assert_eq!(fs::read(&program)?, b"fixture server binary");
        assert!(state.app_state.read().unwrap().jobs.iter().any(|job| {
            job.detail
                .as_deref()
                .is_some_and(|detail| detail.contains("os error 5"))
        }));
    }
    drop(lock);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_uninstall_request_finishes_job_and_database_after_lock_release()
-> Result<(), Box<dyn std::error::Error>> {
    cancelled_uninstall_finishes_with(true).await
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_uninstall_request_reports_permanent_lock_without_losing_data()
-> Result<(), Box<dyn std::error::Error>> {
    cancelled_uninstall_finishes_with(false).await
}

#[tokio::test(flavor = "current_thread")]
async fn uninstall_uncertain_database_commit_preserves_recovery_until_command_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("uninst-db");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    let (storage, descriptor) = installed_astroneer_fixture(&root).await?;
    let install_root = storage.paths.games_root.join("astroneer");
    let program = install_root.join("AstroServer.exe");
    let native_config = install_root.join("Astro/Saved/Config/WindowsServer/Engine.ini");
    let config_before = fs::read(&native_config)?;
    let program_lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&program)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let mut unrelated = descriptor.summary.clone();
    unrelated.id = "unrelated-module".into();
    unrelated.install_state = InstallState::Updating;
    state.app_state.write().unwrap().modules.push(unrelated);
    let requester = tokio::spawn(uninstall_module_game(
        app.handle().clone(),
        "astroneer".into(),
    ));
    wait_for_uninstall_status(&state, JobStatus::Running).await;
    // The deny-delete handle holds the bounded stage retry. A durable prepared
    // record proves database preparation is done; list closes its pool before
    // returning. The earlier Running UI state does not provide that guarantee.
    let prepared = tokio::time::timeout(Duration::from_secs(15), async {
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        loop {
            let records = app_storage::program_removals_db::list(&storage.paths, "astroneer")
                .await
                .expect("read prepared removal checkpoint");
            if let Some(record) = records
                .into_iter()
                .find(|record| record.phase == "prepared")
            {
                return record;
            }
            tick.tick().await;
        }
    })
    .await?;
    let journal: serde_json::Value = serde_json::from_str(&prepared.journal_json)?;
    let staged: PathBuf = serde_json::from_value(journal["staged"].clone())?;
    assert_eq!(prepared.module_id, "astroneer");
    assert_eq!(prepared.phase, "prepared");
    assert_eq!(fs::read(&program)?, b"fixture server binary");
    assert!(!staged.exists());
    let database_lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&storage.paths.database_path)?;
    drop(program_lock);
    let error = tokio::time::timeout(Duration::from_secs(15), requester)
        .await??
        .unwrap_err();
    assert!(error.contains("无法确认提交结果"), "{error}");
    assert!(error.contains("恢复记录与文件已保留"), "{error}");
    assert!(error.contains("os error 32"), "{error}");
    wait_for_uninstall_status(&state, JobStatus::Failed).await;
    {
        let app_state = state.app_state.read().unwrap();
        assert!(app_state.jobs.iter().any(|job| {
            matches!(job.status, JobStatus::Failed) && job.detail.as_deref() == Some(error.as_str())
        }));
        assert!(app_state.modules.iter().any(|module| {
            module.id == "unrelated-module" && module.install_state == InstallState::Updating
        }));
    }
    // An unreadable commit outcome cannot authorize rollback or payload purge.
    assert!(!program.exists());
    assert_eq!(
        fs::read(staged.join("AstroServer.exe"))?,
        b"fixture server binary"
    );
    assert_eq!(fs::read(&native_config)?, config_before);
    assert_eq!(
        fs::read(storage.paths.games_root.join("other-game.sentinel"))?,
        b"keep unrelated files"
    );
    drop(database_lock);
    let pending = app_storage::program_removals_db::list(&storage.paths, "astroneer").await?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].operation_id, prepared.operation_id);
    assert_eq!(pending[0].phase, "prepared");
    assert_eq!(pending[0].journal_json, prepared.journal_json);
    let library = app_storage::read_library_program_install(&storage.paths, "astroneer")
        .await?
        .expect("pending removal retains its registration");
    assert_eq!(library.id, prepared.install_id);
    assert_eq!(library.install_state, InstallState::Installed);

    // Re-enter the public command to recover, then finish a fresh removal.
    let retried = tokio::time::timeout(
        Duration::from_secs(15),
        uninstall_module_game(app.handle().clone(), "astroneer".into()),
    )
    .await??;
    assert_eq!(retried.install_state, InstallState::NotInstalled);
    assert!(!retried.executable_exists);
    assert!(!program.exists());
    assert!(!staged.exists());
    assert_eq!(fs::read(&native_config)?, config_before);
    assert!(
        app_storage::program_removals_db::list(&storage.paths, "astroneer")
            .await?
            .is_empty()
    );
    let library = app_storage::read_library_program_install(&storage.paths, "astroneer")
        .await?
        .expect("uninstalled library registration remains");
    assert_eq!(library.install_state, InstallState::NotInstalled);
    assert!(state.app_state.read().unwrap().jobs.iter().any(|job| {
        matches!(job.kind, JobKind::UninstallGame) && matches!(job.status, JobStatus::Completed)
    }));
    assert_eq!(
        fs::read(storage.paths.games_root.join("other-game.sentinel"))?,
        b"keep unrelated files"
    );
    Ok(())
}
