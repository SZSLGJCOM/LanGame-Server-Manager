use super::*;

#[path = "commands_creation_isolation_tests.rs"]
mod isolation;

#[path = "commands_creation_source_concurrency_tests.rs"]
mod source_concurrency;

tokio::task_local! {
    static CREATION_HOOKS: Arc<CreationHooks>;
    pub(in crate::commands) static CREATION_INSTALL_FORBIDDEN: bool;
}

pub(in crate::commands) fn program_install_forbidden() -> bool {
    CREATION_INSTALL_FORBIDDEN
        .try_with(|value| *value)
        .unwrap_or(false)
}

pub(in crate::commands) struct CreationHooks {
    pub(in crate::commands) before_lock: tokio::sync::Notify,
    worker_started: tokio::sync::Notify,
    release_worker: tokio::sync::Notify,
    pause: bool,
}

impl CreationHooks {
    fn new(pause: bool) -> Arc<Self> {
        Arc::new(Self {
            before_lock: tokio::sync::Notify::new(),
            worker_started: tokio::sync::Notify::new(),
            release_worker: tokio::sync::Notify::new(),
            pause,
        })
    }

    pub(in crate::commands) async fn pause_worker(&self) {
        self.worker_started.notify_one();
        if self.pause {
            self.release_worker.notified().await;
        }
    }
}

pub(in crate::commands) fn current_hooks() -> Option<Arc<CreationHooks>> {
    CREATION_HOOKS.try_with(Arc::clone).ok()
}

struct ReleaseWorkerOnDrop(Arc<CreationHooks>);

impl Drop for ReleaseWorkerOnDrop {
    fn drop(&mut self) {
        self.0.release_worker.notify_one();
    }
}

struct TestRoot(PathBuf);

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn prepare_fixture() -> Result<
    (TestRoot, ProgramDataEnvGuard, app_storage::StorageBootstrap),
    Box<dyn std::error::Error>,
> {
    let root = TestRoot(temp_test_dir("create-lifecycle"));
    let environment = ProgramDataEnvGuard::set(&root.0.join("programdata"));
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: root.0.join("instances").to_string_lossy().into_owned(),
        games_root: root.0.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: root.0.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let install_root = storage.paths.games_root.join("astroneer");
    fs::create_dir_all(&install_root)?;
    fs::write(
        install_root.join("AstroServer.exe"),
        b"synthetic package fixture",
    )?;
    app_storage::record_library_program_baseline(
        &install_root,
        find_descriptor(&descriptors, "astroneer")?,
        true,
        None,
    )?;
    // Model both parts of a completed installation: package evidence and the
    // persisted owner. A bare directory is intentionally not a creation source.
    app_storage::sync_game_installs(
        &storage.paths,
        &[app_storage::GameInstallSyncRecord {
            module_id: String::from("astroneer"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    Ok((root, environment, storage))
}

fn input() -> CreateInstanceInput {
    CreateInstanceInput {
        name: "Lifecycle creation".into(),
        module_id: "astroneer".into(),
    }
}

fn assert_no_instance_residue(paths: &app_storage::StoragePaths, allow_lock_metadata: bool) {
    assert!(paths.archives_root.is_dir());
    assert_eq!(fs::read_dir(&paths.archives_root).unwrap().count(), 0);
    for entry in fs::read_dir(&paths.instances_root).unwrap() {
        let path = entry.unwrap().path();
        assert!(
            path == paths.archives_root
                || (allow_lock_metadata && path == paths.instances_root.join(".langame")),
            "unexpected creation residue: {}",
            path.display()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn creation_lifecycle_reuses_the_archived_only_program_through_the_desktop_coordinator()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let (_root, _environment, storage) = prepare_fixture().await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let owner = create_instance_record_inner(app.state::<DesktopState>(), input())
        .await
        .map_err(|error| format!("first exclusive creation: {error}"))?;
    let owner_install =
        app_storage::read_instance_program_install(&storage.paths, &owner.summary.id)
            .await?
            .unwrap()
            .install;
    let owner_program = owner_install.install_root;
    assert_eq!(owner_program, storage.paths.games_root.join("astroneer"));
    let original_program = fs::read(owner_program.join("AstroServer.exe"))?;
    let first = create_instance_record_inner(app.state::<DesktopState>(), input())
        .await
        .map_err(|error| format!("second independent creation: {error}"))?;
    let independent = app_storage::read_instance_program_install(&storage.paths, &first.summary.id)
        .await?
        .unwrap();
    assert_eq!(
        independent.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_ne!(independent.install.install_root, owner_program);
    assert_eq!(
        fs::read(independent.install.install_root.join("AstroServer.exe"))?,
        original_program
    );
    app_storage::delete_instance(&storage.paths, &owner.summary.id).await?;
    assert_eq!(
        fs::read(owner_program.join("AstroServer.exe"))?,
        original_program
    );
    let first_details = read_instance_details(&storage.paths, &first.summary.id).await?;
    fs::create_dir_all(&first_details.saves_path)?;
    fs::write(
        Path::new(&first_details.saves_path).join("original-world.dat"),
        b"preserved world",
    )?;
    uninstall_module_game(app.handle().clone(), "astroneer".into()).await?;
    let deletion = app_storage::archive_instance(&storage.paths, &first.summary.id).await?;
    let archived = PathBuf::from(deletion.archived_instance_root.unwrap());
    let program = archived.join("runtime/AstroServer.exe");
    let before = fs::read(&program)?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    let summaries = load_module_summaries_with_install_state(&storage, &descriptors).await?;
    let summary = summaries
        .iter()
        .find(|module| module.id == "astroneer")
        .unwrap();
    assert_eq!(summary.install_state, InstallState::NotInstalled);
    assert_eq!(summary.archived_program_count, 1);
    let second = create_instance_record_inner(app.state::<DesktopState>(), input())
        .await
        .map_err(|error| format!("archived-only creation: {error}"))?;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&storage.paths.database_path)
            .read_only(true),
    )
    .await?;
    let library_records = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, install_root FROM game_installs WHERE module_id = 'astroneer' AND scope = 'library'",
    )
    .fetch_all(&pool)
    .await?;
    pool.close().await;
    assert_eq!(library_records.len(), 1);
    assert_eq!(library_records[0].0, owner_install.id);
    assert_eq!(
        fs::canonicalize(&library_records[0].1)?,
        fs::canonicalize(&owner_program)?
    );
    assert_ne!(first.summary.id, second.summary.id);
    let second_details = read_instance_details(&storage.paths, &second.summary.id).await?;
    assert!(
        !Path::new(&second_details.saves_path)
            .join("original-world.dat")
            .exists()
    );
    assert_eq!(fs::read(&program)?, before);
    let second_program =
        app_storage::read_instance_program_install(&storage.paths, &second.summary.id)
            .await?
            .unwrap()
            .install
            .install_root
            .join("AstroServer.exe");
    assert_eq!(fs::read(second_program)?, before);
    assert!(!storage.paths.steamcmd_root.join("steamcmd.exe").exists());
    assert_eq!(
        app_storage::list_instance_archives(&storage.paths)
            .await?
            .archives
            .len(),
        1
    );
    let removed = delete_instance_record(app.handle().clone(), second.summary.id.clone()).await?;
    assert!(!Path::new(&removed.deleted_instance_root).exists());
    assert_eq!(fs::read(&program)?, before);
    let library = app_storage::read_library_program_install(&storage.paths, "astroneer")
        .await?
        .unwrap();
    assert_eq!(library.install_state, InstallState::Installed);
    assert!(library.install_root.join("AstroServer.exe").is_file());
    assert_eq!(
        app_storage::list_instance_archives(&storage.paths)
            .await?
            .archives
            .len(),
        1
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn creation_lifecycle_never_imports_modified_programs_when_official_repair_is_unavailable()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let (_root, _environment, storage) = prepare_fixture().await?;
    let library = storage.paths.games_root.join("astroneer");
    fs::write(library.join("AstroServer.exe"), b"modified local program")?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let error = create_instance_record(
        app.state::<DesktopState>(),
        input(),
        Some(app_core::InstanceProgramMode::Shared),
    )
    .await
    .unwrap_err();
    assert!(error.contains("不支持共享"), "{error}");
    assert_eq!(
        fs::read(library.join("AstroServer.exe"))?,
        b"modified local program"
    );
    let error = create_instance_record(
        app.state::<DesktopState>(),
        input(),
        Some(app_core::InstanceProgramMode::Independent),
    )
    .await
    .unwrap_err();
    assert!(error.to_lowercase().contains("steamcmd"), "{error}");
    assert!(list_instances(&storage.paths).await?.is_empty());
    assert_eq!(
        fs::read(library.join("AstroServer.exe"))?,
        b"modified local program"
    );
    assert!(!storage.paths.steamcmd_root.join("steamcmd.exe").exists());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn creation_lifecycle_shutdown_cancels_a_queued_create_without_waiting_for_install()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let (_root, _environment, storage) = prepare_fixture().await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let program_roots = [storage.paths.games_root.join("astroneer")];
    let lifecycle =
        app_steamcmd::acquire_game_install_lifecycle("astroneer", &program_roots).await?;
    let hooks = CreationHooks::new(false);
    let handle = app.handle().clone();
    let creation = tokio::spawn(CREATION_HOOKS.scope(hooks.clone(), async move {
        create_instance_record_inner(handle.state::<DesktopState>(), input()).await
    }));
    tokio::time::timeout(Duration::from_secs(10), hooks.before_lock.notified()).await?;
    let state = app.state::<DesktopState>();
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    let shutdown = state
        .drain_storage_operations_for_shutdown(Duration::from_secs(2))
        .await?;
    let error = creation
        .await?
        .expect_err("queued create must observe shutdown");
    assert!(error.contains("cancelled"), "{error}");
    assert!(list_instances(&storage.paths).await?.is_empty());
    assert_no_instance_residue(&storage.paths, false);
    drop(shutdown);
    drop(lifecycle);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn creation_lifecycle_shutdown_cancels_the_owned_worker_before_preparation()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let (_root, _environment, storage) = prepare_fixture().await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let hooks = CreationHooks::new(true);
    let release = ReleaseWorkerOnDrop(hooks.clone());
    let handle = app.handle().clone();
    let creation = tokio::spawn(CREATION_HOOKS.scope(hooks.clone(), async move {
        create_instance_record_inner(handle.state::<DesktopState>(), input()).await
    }));
    tokio::time::timeout(Duration::from_secs(10), hooks.worker_started.notified()).await?;
    let state = app.state::<DesktopState>();
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    assert!(
        state
            .drain_storage_operations_for_shutdown(Duration::ZERO)
            .await
            .is_err()
    );
    drop(release);
    let error = tokio::time::timeout(Duration::from_secs(10), creation)
        .await??
        .expect_err("owned preparation must observe shutdown cancellation");
    assert!(error.to_lowercase().contains("cancel"), "{error}");
    state
        .drain_storage_operations_for_shutdown(Duration::from_secs(2))
        .await?;
    assert!(list_instances(&storage.paths).await?.is_empty());
    assert_no_instance_residue(&storage.paths, true);
    let _next = tokio::time::timeout(
        Duration::from_secs(2),
        app_steamcmd::acquire_game_install_lifecycle(
            "astroneer",
            &[storage.paths.games_root.join("astroneer")],
        ),
    )
    .await??;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn creation_lifecycle_waits_for_uninstall_then_reprobes_missing_program()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let (_root, _environment, storage) = prepare_fixture().await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let program_roots = [storage.paths.games_root.join("astroneer")];
    let lifecycle =
        app_steamcmd::acquire_game_install_lifecycle("astroneer", &program_roots).await?;
    let hooks = CreationHooks::new(false);
    let handle = app.handle().clone();
    let creation = tokio::spawn(CREATION_HOOKS.scope(hooks.clone(), async move {
        create_instance_record_inner(handle.state::<DesktopState>(), input()).await
    }));
    tokio::time::timeout(Duration::from_secs(10), hooks.before_lock.notified()).await?;
    assert!(list_instances(&storage.paths).await?.is_empty());
    assert_no_instance_residue(&storage.paths, false);
    assert!(!creation.is_finished());

    // Exercise the uninstall critical section on this synthetic package while
    // creation is waiting on the same real lifecycle coordinator.
    let install_root = storage.paths.games_root.join("astroneer");
    let staged = storage
        .paths
        .games_root
        .join(".astroneer.uninstall-fixture");
    fs::rename(&install_root, &staged)?;
    lifecycle
        .remove_staged_directory(install_root.clone(), staged)
        .await?;
    let error = tokio::time::timeout(Duration::from_secs(10), creation)
        .await??
        .expect_err("creation must recheck after uninstall releases the lock");
    // The current creator retries missing programs through official acquisition.
    // This fixture has no SteamCMD, so re-probing after uninstall must reach that
    // prerequisite check without publishing an instance. Only a registered,
    // empty acquisition may remain for a later creation attempt.
    assert!(error.contains("\"code\":\"steamcmd_not_ready\""), "{error}");
    assert!(list_instances(&storage.paths).await?.is_empty());
    assert_no_instance_residue(&storage.paths, true);
    let pending = app_storage::read_program_install_owner(&storage.paths, &install_root)
        .await?
        .expect("failed acquisition must retain its installation registration");
    assert_eq!(pending.install_root, install_root);
    assert_eq!(pending.module_id, "astroneer");
    assert_eq!(pending.scope, app_storage::ProgramInstallScope::Library);
    assert_eq!(pending.install_state, InstallState::Incomplete);
    assert!(pending.owner_instance_id.is_none());
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    assert!(app_storage::library_program_acquisition_is_trusted(
        &install_root,
        find_descriptor(&descriptors, "astroneer")?,
    )?);
    assert!(!install_root.join("AstroServer.exe").exists());
    let game_paths = fs::read_dir(&storage.paths.games_root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(game_paths, vec![install_root.clone()]);
    let mut acquisition_files = fs::read_dir(&install_root)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    acquisition_files.sort();
    assert_eq!(
        acquisition_files,
        vec![
            std::ffi::OsString::from(".langame-program-acquisition.json"),
            std::ffi::OsString::from(".langame-retained-library.json"),
        ],
    );
    let retained: Value = serde_json::from_slice(&fs::read(
        install_root.join(".langame-retained-library.json"),
    )?)?;
    assert_eq!(retained["version"], 1);
    assert_eq!(retained["module_id"], "astroneer");
    assert_eq!(
        fs::canonicalize(retained["program_root"].as_str().unwrap())?,
        fs::canonicalize(&install_root)?,
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn creation_lifecycle_worker_keeps_lock_after_request_cancellation()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let (_root, _environment, storage) = prepare_fixture().await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let hooks = CreationHooks::new(true);
    let release = ReleaseWorkerOnDrop(hooks.clone());
    let handle = app.handle().clone();
    let creation = tokio::spawn(CREATION_HOOKS.scope(hooks.clone(), async move {
        create_instance_record_inner(handle.state::<DesktopState>(), input()).await
    }));
    tokio::time::timeout(Duration::from_secs(10), hooks.worker_started.notified()).await?;
    creation.abort();
    assert!(creation.await.unwrap_err().is_cancelled());
    assert!(list_instances(&storage.paths).await?.is_empty());

    let program_roots = [storage.paths.games_root.join("astroneer")];
    let next_operation = app_steamcmd::acquire_game_install_lifecycle("astroneer", &program_roots);
    tokio::pin!(next_operation);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut next_operation)
            .await
            .is_err(),
        "request cancellation must not release the paused worker's lifecycle lock"
    );
    drop(release);
    let _next_guard = tokio::time::timeout(Duration::from_secs(10), &mut next_operation).await??;
    let instances = list_instances(&storage.paths).await?;
    assert_eq!(
        instances.len(),
        1,
        "the owned worker completes after its caller stops waiting"
    );
    let instance = read_instance_details(&storage.paths, &instances[0].id).await?;
    assert!(Path::new(&instance.config_file_path).is_file());
    let instance_root = super::super::commands_program_storage::instance_root(&instance)?;
    assert_eq!(
        app_storage::instance_program_mode(instance_root)?,
        app_core::InstanceProgramMode::Independent
    );
    let program = app_storage::resolve_instance_runtime_root(instance_root)?;
    assert_eq!(
        fs::read(program.join("AstroServer.exe"))?,
        b"synthetic package fixture"
    );
    let library = app_storage::read_library_program_install(&storage.paths, "astroneer")
        .await?
        .expect("creation must retain its program library");
    assert_eq!(library.install_state, InstallState::Installed);
    assert_eq!(
        library.install_root,
        storage.paths.games_root.join("astroneer")
    );
    assert_eq!(
        fs::read(library.install_root.join("AstroServer.exe"))?,
        b"synthetic package fixture"
    );
    assert_ne!(library.install_root, program);
    Ok(())
}
