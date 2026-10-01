use super::*;

struct PolicyFixtureRoot(PathBuf);

impl Drop for PolicyFixtureRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn check_policy_boundary(
    settings_json: &str,
    unsupported: bool,
) -> Result<Result<(), String>, Box<dyn std::error::Error>> {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let root = PolicyFixtureRoot(std::env::temp_dir().join(format!(
        "lgsm-prestart-policy-{}",
        uuid::Uuid::new_v4().simple()
    )));
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let paths = app_storage::StoragePaths {
        app_data_root: root.0.join("app-data"),
        settings_path: root.0.join("app-data/settings.json"),
        database_path: root.0.join("app-data/db/lgs.db"),
        logs_root: root.0.join("app-data/logs"),
        modules_root: workspace.join("modules"),
        migrations_root: workspace.join("migrations"),
        steamcmd_root: root.0.join("steamcmd"),
        games_root: root.0.join("games"),
        instances_root: root.0.join("instances"),
        archives_root: root.0.join("instances/.trash"),
    };
    let storage = StorageBootstrap {
        settings: paths.settings(),
        storage_status: paths.probe_status(),
        paths,
    };
    let descriptor = discover_modules(&storage.paths.modules_root)?
        .into_iter()
        .find(|module| module.summary.id == "minecraft")
        .unwrap();
    let program = storage.paths.games_root.join("minecraft");
    fs::create_dir_all(&program)?;
    fs::write(program.join("server.jar"), b"preserved fixture server")?;
    let mut module = map_module_details_with_install_state(
        &storage.settings,
        &descriptor,
        Some(&program.to_string_lossy()),
    );
    module.summary.install_state = InstallState::Installed;
    // An accidental network attempt stays local and is observable without
    // downloading or executing any server payload.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let source = format!("http://{}/should-not-be-requested", listener.local_addr()?);
    let install = module.install.as_mut().unwrap();
    install.minecraft.as_mut().unwrap().manifest_url = Some(source.clone());
    if unsupported {
        install.download_url_windows = Some(source);
    }
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: "policy-fixture".into(),
            name: "Policy fixture".into(),
            module_id: "minecraft".into(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: "0.0.0.0".into(),
            port_count: 0,
            autostart: false,
        },
        config_file_path: storage
            .paths
            .instances_root
            .join("policy-fixture/config/instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: storage
            .paths
            .instances_root
            .join("policy-fixture/saves")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 1,
        settings_json: settings_json.into(),
        ports: Vec::new(),
        active_run: None,
    };
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let reservation = match state.try_reserve_runtime_start(&instance.summary.id, "manual")? {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected start reservation: {other:?}"),
    };
    let guard =
        app_steamcmd::acquire_game_install_lifecycle("minecraft", std::slice::from_ref(&program))
            .await?;
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        run_prestart_update_if_needed(
            &state,
            &storage,
            PrestartUpdateRequest {
                descriptor: &descriptor,
                module: &module,
                instance: &instance,
                install_guard: guard,
                reservation: &reservation,
                context: PrestartUpdateLogContext {
                    source: "manual",
                    console_log_path: None,
                },
            },
        ),
    )
    .await
    .expect("policy-only startup paths must not contact an installer");
    let result = result.map(|guard| {
        guard.ensure_scope("minecraft", &program).unwrap();
    });
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    assert_eq!(
        fs::read(program.join("server.jar"))?,
        b"preserved fixture server"
    );
    assert!(!storage.paths.database_path.exists());
    assert!(state.app_state.read().unwrap().jobs.is_empty());
    Ok(result)
}

#[tokio::test]
async fn pinned_prestart_preserves_program_without_contacting_the_installer()
-> Result<(), Box<dyn std::error::Error>> {
    check_policy_boundary(r#"{"program_update":{"policy":"pinned"}}"#, false).await??;
    Ok(())
}

#[tokio::test]
async fn unsupported_prestart_source_preserves_program_without_claiming_an_update()
-> Result<(), Box<dyn std::error::Error>> {
    check_policy_boundary("{}", true).await??;
    Ok(())
}

#[tokio::test]
async fn invalid_prestart_policy_stops_before_program_or_install_record_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let error = check_policy_boundary(r#"{"program_update":{"policy":"unknown"}}"#, false)
        .await?
        .unwrap_err();
    assert!(error.contains("invalid program_update"), "{error}");
    Ok(())
}

struct AutomaticPolicyFixture {
    _root: PolicyFixtureRoot,
    storage: StorageBootstrap,
    descriptor: ModuleDescriptor,
    module: ModuleDetails,
    instance: InstanceDetails,
    program: PathBuf,
    library: PathBuf,
}

impl AutomaticPolicyFixture {
    async fn new(manifest_url: String) -> Result<Self, Box<dyn std::error::Error>> {
        let root = PolicyFixtureRoot(std::env::temp_dir().join(format!(
            "lgsm-automatic-policy-{}",
            uuid::Uuid::new_v4().simple()
        )));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let paths = app_storage::StoragePaths {
            app_data_root: root.0.join("app-data"),
            settings_path: root.0.join("app-data/settings.json"),
            database_path: root.0.join("app-data/db/lgs.db"),
            logs_root: root.0.join("app-data/logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.0.join("steamcmd"),
            games_root: root.0.join("games"),
            instances_root: root.0.join("instances"),
            archives_root: root.0.join("instances/.trash"),
        };
        let storage = app_storage::bootstrap_storage_with_paths(paths)?;
        initialize_database(&storage.paths).await?;
        let descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|module| module.summary.id == "minecraft")
            .unwrap();
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        let library = storage.paths.games_root.join("minecraft");
        fs::create_dir_all(library.join("jre/bin"))?;
        fs::write(library.join("server.jar"), b"preserved fixture server")?;
        fs::write(library.join("jre/bin/java.exe"), b"inert java fixture")?;
        app_storage::record_library_program_baseline(&library, &descriptor, true, None)?;
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
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            &descriptor,
            CreateInstanceInput {
                name: "Automatic policy fixture".into(),
                module_id: "minecraft".into(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                prefer_existing_install: false,
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await?;
        let instance =
            read_instance_details(&storage.paths, &created.provisioning.summary.id).await?;
        let program = PathBuf::from(
            super::super::commands_runtime_lifecycle::private_runtime_install_root(&instance)?,
        );
        assert_ne!(fs::canonicalize(&program)?, fs::canonicalize(&library)?);
        let mut module = map_module_details_with_install_state(
            &storage.settings,
            &descriptor,
            Some(&program.to_string_lossy()),
        );
        assert!(matches!(
            module.summary.install_state,
            InstallState::Installed
        ));
        module
            .install
            .as_mut()
            .unwrap()
            .minecraft
            .as_mut()
            .unwrap()
            .manifest_url = Some(manifest_url);
        let settings: Value = serde_json::from_str(&instance.settings_json)?;
        assert!(settings.get("program_update").is_none());
        Ok(Self {
            _root: root,
            storage,
            descriptor,
            module,
            instance,
            program,
            library,
        })
    }
}

struct LocalManifestSource {
    url: String,
    requested: tokio::sync::oneshot::Receiver<()>,
    release: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), String>>,
}

impl Drop for LocalManifestSource {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn local_manifest_source(hold_response: bool) -> Result<LocalManifestSource, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let url = format!(
        "http://{}/manifest.json",
        listener.local_addr().map_err(|error| error.to_string())?
    );
    let (requested, observed) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut buffer = [0; 1024];
            let read = stream
                .read(&mut buffer)
                .await
                .map_err(|error| error.to_string())?;
            if read == 0 || request.len() + read > 8192 {
                return Err("invalid local metadata request".into());
            }
            request.extend_from_slice(&buffer[..read]);
        }
        if !request.starts_with(b"GET /manifest.json ") {
            return Err("unexpected local metadata route".into());
        }
        requested
            .send(())
            .map_err(|_| "metadata observer dropped".to_owned())?;
        if hold_response {
            // The test cancels only after a real request arrives, then releases
            // this server after the client has acknowledged cleanup.
            released
                .await
                .map_err(|_| "metadata release dropped".to_owned())?;
        } else {
            let body = "invalid manifest JSON";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .map_err(|error| error.to_string())?;
            stream.shutdown().await.map_err(|error| error.to_string())?;
        }
        Ok(())
    });
    Ok(LocalManifestSource {
        url,
        requested: observed,
        release,
        task,
    })
}

async fn check_automatic_failure_or_cancellation(
    cancel: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let mut source = local_manifest_source(cancel).await?;
    let fixture = AutomaticPolicyFixture::new(source.url.clone()).await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    state.app_state.write().unwrap().settings = fixture.storage.settings.clone();
    let id = &fixture.instance.summary.id;
    let reservation = match state.try_reserve_runtime_start(id, "manual")? {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected start reservation: {other:?}"),
    };
    let _instance_lock = state.acquire_instance_mutation(id).await;
    // Scope only the independently owned runtime. A regression that attempts
    // to check the game's library instead must fail before the HTTP request.
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        "minecraft",
        std::slice::from_ref(&fixture.program),
    )
    .await?;
    let mut update = Box::pin(run_prestart_update_if_needed(
        &state,
        &fixture.storage,
        PrestartUpdateRequest {
            descriptor: &fixture.descriptor,
            module: &fixture.module,
            instance: &fixture.instance,
            install_guard: guard,
            reservation: &reservation,
            context: PrestartUpdateLogContext {
                source: "manual",
                console_log_path: None,
            },
        },
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! {
            biased;
            observed = &mut source.requested => observed.expect("the real metadata request must arrive"),
            result = &mut update => panic!("startup update finished before checking its actual program: {:?}", result.err()),
        }
    }).await.expect("the default automatic policy must query the local metadata endpoint");
    if cancel {
        let job_id = state.app_state.read().unwrap().jobs[0].id.clone();
        super::super::commands_install_progress::cancel_installation_job(state.clone(), job_id)?;
    }
    let result = tokio::time::timeout(Duration::from_secs(5), &mut update)
        .await
        .expect("failed or cancelled metadata checks must settle");
    assert!(
        result.is_err(),
        "metadata failure or cancellation must prevent startup"
    );
    drop(update);
    let jobs = state.app_state.read().unwrap().jobs.clone();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].target_id.as_deref(), Some(id.as_str()));
    assert!(matches!(
        (&jobs[0].status, cancel),
        (JobStatus::Failed, false) | (JobStatus::Cancelled, true)
    ));
    assert!(
        read_active_instance_run(&fixture.storage.paths, id)
            .await?
            .is_none()
    );
    assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(id));
    for root in [&fixture.program, &fixture.library] {
        assert_eq!(
            fs::read(root.join("server.jar"))?,
            b"preserved fixture server"
        );
    }
    let _reacquired = tokio::time::timeout(
        Duration::from_secs(3),
        app_steamcmd::acquire_game_install_lifecycle(
            "minecraft",
            std::slice::from_ref(&fixture.program),
        ),
    )
    .await
    .expect("terminal startup update must release its install lock")?;
    // oneshot::Sender::send consumes the sender; retain the abort-on-drop owner.
    let (unused, _) = tokio::sync::oneshot::channel();
    let release = std::mem::replace(&mut source.release, unused);
    let _ = release.send(());
    tokio::time::timeout(Duration::from_secs(3), &mut source.task).await???;
    Ok(())
}

#[tokio::test]
async fn automatic_prestart_metadata_failure_preserves_program_and_does_not_publish_a_run()
-> Result<(), Box<dyn std::error::Error>> {
    check_automatic_failure_or_cancellation(false).await
}

#[tokio::test]
async fn automatic_prestart_cancellation_settles_job_and_releases_program_lease()
-> Result<(), Box<dyn std::error::Error>> {
    check_automatic_failure_or_cancellation(true).await
}

#[tokio::test]
async fn cancelled_prestart_lock_wait_does_not_wait_for_the_current_program_owner()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let root = PolicyFixtureRoot(std::env::temp_dir().join(format!(
        "lgsm-prestart-lock-{}",
        uuid::Uuid::new_v4().simple()
    )));
    fs::create_dir_all(&root.0)?;
    let current_owner =
        app_steamcmd::acquire_game_install_lifecycle("minecraft", std::slice::from_ref(&root.0))
            .await?;
    let state = DesktopState::default();
    let reservation = match state.try_reserve_runtime_start("cancelled-lock-wait", "manual")? {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected start reservation: {other:?}"),
    };
    let mut wait = Box::pin(acquire_prestart_program_guard(
        "minecraft",
        &root.0,
        &reservation,
    ));
    std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(wait.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    assert!(
        state
            .drain_storage_operations_for_shutdown(Duration::ZERO)
            .await
            .is_err()
    );
    let result = tokio::time::timeout(Duration::from_secs(1), &mut wait)
        .await
        .expect("cancel must settle before another operation releases the program");
    assert!(matches!(result, Err(ref error) if error.contains("启动已取消")));
    drop(wait);
    current_owner.ensure_scope("minecraft", &root.0)?;
    drop(current_owner);
    let _next_owner = tokio::time::timeout(
        Duration::from_secs(3),
        app_steamcmd::acquire_game_install_lifecycle("minecraft", std::slice::from_ref(&root.0)),
    )
    .await
    .expect("a cancelled waiter must not retain the install lock")?;
    Ok(())
}
