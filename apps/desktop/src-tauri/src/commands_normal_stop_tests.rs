use super::*;

struct Fixture {
    root: PathBuf,
    state: DesktopState,
    storage: StorageBootstrap,
    details: InstanceDetails,
    active: ActiveInstanceRun,
    pid: u32,
    identity: ProcessIdentity,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-normal-stop-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = app_storage::StoragePaths {
            app_data_root: root.clone(),
            settings_path: root.join("settings.json"),
            database_path: root.join("db/lgs.db"),
            logs_root: root.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("archives"),
        };
        let storage = StorageBootstrap {
            settings: paths.settings(),
            storage_status: paths.probe_status(),
            paths,
        };
        let script = root.join("server.ps1");
        fs::write(
            &script,
            r#"
[Console]::WriteLine('fixture-ready')
[Console]::Out.Flush()
while ($null -ne ($line = [Console]::In.ReadLine())) {
    [Console]::WriteLine('received:' + $line)
    [Console]::Out.Flush()
    if ($line -eq 'release') { exit 0 }
    if ($line.StartsWith('exit:')) { [Environment]::Exit([int]$line.Substring(5)) }
}
"#,
        )
        .unwrap();
        let summary = InstanceSummary {
            id: "normal-stop-fixture".into(),
            name: "Normal stop fixture".into(),
            module_id: "fixture".into(),
            status: InstanceStatus::Running,
            active_process_count: 1,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        };
        let executable = app_runtime::windows_system_directory()
            .unwrap()
            .join("WindowsPowerShell/v1.0/powershell.exe");
        let plan = LaunchPlan {
            environment: Default::default(),
            instance_id: summary.id.clone(),
            instance_name: summary.name.clone(),
            module_id: summary.module_id.clone(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            uses_private_runtime: false,
            working_directory: root.to_string_lossy().into_owned(),
            executable_path: executable.to_string_lossy().into_owned(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: vec![],
            args: vec![
                "-NoProfile".into(),
                "-File".into(),
                script.to_string_lossy().into_owned(),
            ],
            command_line: "synthetic refusing server".into(),
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: RuntimePerformancePolicy::default(),
            performance_preview: Default::default(),
        };
        let mut spawned = spawn_launch_plan(&plan, root.join("server.log")).unwrap();
        let pid = spawned.pid;
        let identity = spawned.process_identity.clone();
        let active: ActiveInstanceRun = serde_json::from_value(json!({
            "run_id": 1, "session_id": "fixture-session", "pid": pid, "log_path": spawned.log_path,
            "process_count": 1, "processes": [{ "run_id": 1, "session_id": "fixture-session", "pid": pid,
                "process_identity": identity, "process_key": "main", "display_name": "Fixture",
                "status": "running", "is_primary": true, "crash_flag": false, "log_path": spawned.log_path }]
        })).unwrap();
        let details = InstanceDetails {
            summary: summary.clone(),
            config_file_path: String::new(),
            saves_path: String::new(),
            backup_uses_declared_saves_path: false,
            auto_backup_on_stop: false,
            backup_retention_count: 1,
            settings_json: "{}".into(),
            ports: vec![],
            active_run: Some(active.clone()),
        };
        let state = DesktopState::default();
        state.runtime_supervisor.lock().unwrap().insert_running(
            summary,
            Some("fixture-session".into()),
            vec![ManagedProcess {
                run_id: 1,
                process_key: "main".into(),
                display_name: "Fixture".into(),
                pid,
                process_identity: identity.clone(),
                root_process_identity: spawned.root_process_identity.clone(),
                log_path: spawned.log_path.clone(),
                is_primary: true,
                uses_script_entrypoint: false,
                performance_policy: RuntimePerformancePolicy::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: spawned.child.take(),
                hidden_desktop: spawned.hidden_desktop.take(),
            }],
        );
        let fixture = Self {
            root,
            state,
            storage,
            details,
            active,
            pid,
            identity,
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            while !fs::read_to_string(fixture.root.join("server.log"))
                .unwrap_or_default()
                .contains("fixture-ready")
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture starts before stop request");
        fixture
    }

    fn assert_retained(&self) {
        assert!(
            process_matches_identity(self.pid, &self.identity).unwrap(),
            "normal stop must not kill a refusing server"
        );
        let mut supervisor = self.state.runtime_supervisor.lock().unwrap();
        assert!(supervisor.is_tracked(&self.details.summary.id));
        assert!(supervisor.reap_exited().unwrap().is_empty());
        assert_eq!(
            supervisor
                .instance_process_tree_is_running(&self.details.summary.id)
                .unwrap(),
            Some(true)
        );
    }

    async fn wait_for_log_line(&self, expected: &str) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !fs::read_to_string(self.root.join("server.log"))
                .unwrap()
                .contains(expected)
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture output must be drained into its log");
    }

    async fn persist_running(&mut self) {
        self.storage.paths.migrations_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../migrations");
        fs::create_dir_all(self.root.join("db")).unwrap();
        initialize_database(&self.storage.paths).await.unwrap();
        let options =
            sqlx::sqlite::SqliteConnectOptions::new().filename(&self.storage.paths.database_path);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("INSERT INTO modules (id, name, version) VALUES ('fixture', 'Fixture', '1')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO instances (id, name, module_id, data_path, config_path, logs_path, saves_path) VALUES (?1, 'Normal stop fixture', 'fixture', ?2, ?2, ?2, ?2)")
            .bind(&self.details.summary.id).bind(self.root.to_string_lossy().as_ref())
            .execute(&pool).await.unwrap();
        pool.close().await;
        let process = &self.active.processes[0];
        let recorded = mark_instance_process_started_with_identity(
            &self.storage.paths,
            &StartedInstanceProcess {
                instance_id: &self.details.summary.id,
                session_id: self.active.session_id.as_deref(),
                process_key: "main",
                display_name: "Fixture",
                pid: self.pid,
                log_path: process.log_path.as_deref().unwrap(),
                is_primary: true,
            },
            Some(&self.identity),
        )
        .await
        .unwrap();
        assert_eq!(recorded.run_id, self.active.run_id);
        update_desktop_state_instances(
            &self.state,
            list_instances(&self.storage.paths).await.unwrap(),
        )
        .unwrap();
    }

    async fn assert_finalized(&self, code: i32, crash: bool) {
        assert!(!process_matches_identity(self.pid, &self.identity).unwrap());
        assert!(
            !self
                .state
                .runtime_supervisor
                .lock()
                .unwrap()
                .is_tracked(&self.details.summary.id)
        );
        assert!(
            read_active_instance_run(&self.storage.paths, &self.details.summary.id)
                .await
                .unwrap()
                .is_none()
        );
        let summary = list_instances(&self.storage.paths)
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.id == self.details.summary.id)
            .unwrap();
        assert_eq!(summary.active_process_count, 0);
        assert!(matches!(
            (&summary.status, crash),
            (InstanceStatus::Error, true) | (InstanceStatus::Stopped, false)
        ));
        if crash {
            let state = self.state.app_state.read().unwrap();
            let summary = state
                .instances
                .iter()
                .find(|item| item.id == self.details.summary.id)
                .unwrap();
            assert!(matches!(summary.status, InstanceStatus::Error));
            assert_eq!(summary.active_process_count, 0);
            assert_eq!(state.snapshot.running_instances, 0);
        }
        let options =
            sqlx::sqlite::SqliteConnectOptions::new().filename(&self.storage.paths.database_path);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        let stored: (String, Option<i64>, i64) =
            sqlx::query_as("SELECT status, exit_code, crash_flag FROM instance_runs WHERE id = ?1")
                .bind(self.active.run_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        pool.close().await;
        assert_eq!(
            stored,
            (
                if crash { "error" } else { "stopped" }.into(),
                Some(i64::from(code)),
                i64::from(crash)
            )
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let mut supervisor = self.state.runtime_supervisor.lock().unwrap();
        if supervisor
            .dispatch_command(&self.details.summary.id, Some("main"), "release")
            .is_ok()
        {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            // Observe the cooperative fixture exit, without paying the generic
            // Ctrl+C grace period after every successful negative assertion.
            while supervisor
                .instance_process_tree_is_running(&self.details.summary.id)
                .ok()
                != Some(Some(false))
            {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
            }
        }
        if let Some(mut managed) = supervisor.take_running_for_stop(&self.details.summary.id) {
            // Bounded fallback cleanup owns only this disposable synthetic process.
            let _ = stop_managed_instance(&mut managed);
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn normal_stop_refusing_stdin_server_remains_alive_and_owned() {
    let fixture = Fixture::new().await;
    let shutdown: ModuleShutdownSpec = serde_json::from_value(json!({
        "grace_period_ms": 25, "commands": [{"transport": "stdin", "command": "quit", "wait_after_ms": 0}]
    })).unwrap();
    let lease = fixture
        .state
        .begin_storage_context_operation("normal stop regression")
        .unwrap();
    let error = super::super::commands_instance_stop::stop_run(
        &fixture.state,
        &fixture.storage,
        &lease,
        &fixture.details,
        &fixture.active,
        Some(&shutdown),
        InstanceShutdownSource::Manual,
    )
    .await
    .unwrap_err();
    fixture.assert_retained();
    assert!(error.contains("did not exit"), "{error}");
    fixture.wait_for_log_line("received:quit").await;
}

#[tokio::test]
async fn normal_stop_without_declared_strategy_preserves_running_server() {
    let fixture = Fixture::new().await;
    let lease = fixture
        .state
        .begin_storage_context_operation("normal stop missing strategy")
        .unwrap();
    let error = super::super::commands_instance_stop::stop_run(
        &fixture.state,
        &fixture.storage,
        &lease,
        &fixture.details,
        &fixture.active,
        None,
        InstanceShutdownSource::Manual,
    )
    .await
    .unwrap_err();
    fixture.assert_retained();
    assert!(error.contains("no declared shutdown strategy"), "{error}");
}

#[tokio::test]
async fn normal_stop_finalization_rejects_live_tree_before_taking_ownership() {
    let fixture = Fixture::new().await;
    let lease = fixture
        .state
        .begin_storage_context_operation("normal stop finalization")
        .unwrap();
    let error = stop_active_instance_processes(
        &fixture.state,
        &fixture.storage,
        &lease,
        &fixture.details.summary.id,
        &fixture.active,
        InstanceShutdownSource::Manual,
    )
    .await
    .unwrap_err();
    assert!(error.contains("not been confirmed exited"), "{error}");
    fixture.assert_retained();
}

#[tokio::test]
async fn normal_stop_rejects_untracked_process_with_valid_identity_without_killing_it() {
    let fixture = Fixture::new().await;
    let managed = fixture
        .state
        .runtime_supervisor
        .lock()
        .unwrap()
        .take_running_for_stop(&fixture.details.summary.id)
        .unwrap();
    let lease = fixture
        .state
        .begin_storage_context_operation("untracked normal stop")
        .unwrap();
    let result = stop_active_instance_processes(
        &fixture.state,
        &fixture.storage,
        &lease,
        &fixture.details.summary.id,
        &fixture.active,
        InstanceShutdownSource::Manual,
    )
    .await;
    let survived = process_matches_identity(fixture.pid, &fixture.identity).unwrap();
    assert!(
        fixture
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .restore_running_after_failed_stop(managed)
    );
    assert!(result.unwrap_err().contains("not been confirmed exited"));
    assert!(
        survived,
        "normal stop must not kill a valid but untracked process"
    );
    assert_eq!(fixture.active.processes[0].status, "running");
    assert!(
        !fixture.storage.paths.database_path.exists(),
        "rejection must not change runtime storage"
    );
}

#[tokio::test]
async fn normal_stop_rejects_already_exited_untracked_record_without_tree_evidence() {
    // Replaces the retired untracked-kill helper's success test: a missing PID
    // proves neither complete tree exit nor permission to finalize its record.
    let mut fixture = Fixture::new().await;
    let managed = fixture
        .state
        .runtime_supervisor
        .lock()
        .unwrap()
        .take_running_for_stop(&fixture.details.summary.id)
        .unwrap();
    fixture.active.pid = Some(u32::MAX);
    fixture.active.processes[0].pid = Some(u32::MAX);
    fixture.active.processes[0].process_identity = Some(ProcessIdentity {
        creation_time: 1,
        image_path: "missing.exe".into(),
    });
    let lease = fixture
        .state
        .begin_storage_context_operation("exited untracked normal stop")
        .unwrap();
    let result = stop_active_instance_processes(
        &fixture.state,
        &fixture.storage,
        &lease,
        &fixture.details.summary.id,
        &fixture.active,
        InstanceShutdownSource::Manual,
    )
    .await;
    assert!(
        fixture
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .restore_running_after_failed_stop(managed)
    );
    assert!(result.unwrap_err().contains("not been confirmed exited"));
    assert_eq!(fixture.active.processes[0].status, "running");
    assert!(
        !fixture.storage.paths.database_path.exists(),
        "an incomplete exit proof must preserve storage"
    );
}

#[tokio::test]
async fn normal_stop_confirms_tree_exit_when_rcon_completion_connection_closes() {
    use crate::runtime_transport::{source_rcon_read_packet, source_rcon_write_packet};
    use std::net::TcpListener;

    let mut fixture = Fixture::new().await;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    fixture.details.ports = vec![app_core::PortBinding {
        name: "rcon".into(),
        protocol: "tcp".into(),
        port: listener.local_addr().unwrap().port(),
    }];
    fixture.details.settings_json =
        r#"{"rcon_enabled":true,"rcon_password":"fixture-password"}"#.into();
    let runtime = Arc::clone(&fixture.state.runtime_supervisor);
    let id = fixture.details.summary.id.clone();
    let server = std::thread::spawn(move || -> Result<(), String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        stream
            .set_nonblocking(false)
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|error| error.to_string())?;
        let auth = source_rcon_read_packet(&mut stream)?;
        source_rcon_write_packet(&mut stream, auth.id, 2, "")?;
        let command = source_rcon_read_packet(&mut stream)?;
        if command.body != b"quit" {
            return Err("unexpected synthetic shutdown command".into());
        }
        source_rcon_write_packet(&mut stream, command.id, 0, "")?;
        let _completion_marker = source_rcon_read_packet(&mut stream)?;
        // The server executed shutdown, then exits before answering the RCON
        // completion marker. Never make a generic EOF count as command success.
        runtime
            .lock()
            .unwrap()
            .dispatch_command(&id, Some("main"), "release")
            .map_err(|error| error.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while runtime
            .lock()
            .unwrap()
            .instance_process_tree_is_running(&id)
            .map_err(|error| error.to_string())?
            != Some(false)
        {
            if std::time::Instant::now() >= deadline {
                return Err("fixture did not exit".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(()) // Closing the stream deliberately omits the marker response.
    });
    let shutdown: ModuleShutdownSpec = serde_json::from_value(json!({
        "grace_period_ms": 1000, "commands": [{"transport": "source_rcon", "fallback_transport": "stdin",
            "command": "quit", "enabled_setting_key": "rcon_enabled", "wait_after_ms": 100}]
    })).unwrap();
    let result = request_instance_graceful_shutdown(
        &fixture.state,
        &fixture.storage,
        &fixture.details,
        Some(&shutdown),
        InstanceShutdownSource::Manual,
    )
    .await;
    server.join().unwrap().unwrap();
    result.expect("an observed complete tree exit confirms shutdown despite a lost RCON reply");
    assert_eq!(
        fixture
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .instance_process_tree_is_running(&fixture.details.summary.id)
            .unwrap(),
        Some(false)
    );
    fixture.wait_for_log_line("received:release").await;
    let log = fs::read_to_string(fixture.root.join("server.log")).unwrap();
    assert!(log.contains("received:release"));
    assert!(
        !log.contains("received:quit"),
        "a completed shutdown must not be replayed through stdin"
    );
}

#[tokio::test]
async fn normal_stop_preserves_declared_stdin_fallback_when_rcon_is_disabled() {
    let mut fixture = Fixture::new().await;
    fixture.details.settings_json = r#"{"rcon_enabled":false}"#.into();
    let shutdown: ModuleShutdownSpec = serde_json::from_value(json!({
        "grace_period_ms": 1000, "commands": [{"transport": "source_rcon", "fallback_transport": "stdin",
            "command": "release", "enabled_setting_key": "rcon_enabled", "wait_after_ms": 25}]
    })).unwrap();
    request_instance_graceful_shutdown(
        &fixture.state,
        &fixture.storage,
        &fixture.details,
        Some(&shutdown),
        InstanceShutdownSource::Manual,
    )
    .await
    .unwrap();
    assert_eq!(
        fixture
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .instance_process_tree_is_running(&fixture.details.summary.id)
            .unwrap(),
        Some(false)
    );
    fixture.wait_for_log_line("received:release").await;
}

#[tokio::test]
async fn normal_stop_transport_failure_does_not_confirm_a_running_tree() {
    let mut fixture = Fixture::new().await;
    fixture.details.settings_json = r#"{"rcon_enabled":false}"#.into();
    let shutdown: ModuleShutdownSpec = serde_json::from_value(json!({
        "grace_period_ms": 25, "commands": [{"transport": "source_rcon",
            "command": "quit", "enabled_setting_key": "rcon_enabled", "wait_after_ms": 25}]
    }))
    .unwrap();
    let error = request_instance_graceful_shutdown(
        &fixture.state,
        &fixture.storage,
        &fixture.details,
        Some(&shutdown),
        InstanceShutdownSource::Manual,
    )
    .await
    .unwrap_err();
    assert!(error.contains("not enabled"), "{error}");
    fixture.assert_retained();
}

// The local peer substitutes only the wire protocol. The process, its owned
// tree, stdin fallback, and shutdown lifecycle remain the production paths.
fn rcon_disconnect_peer(
    fixture: &mut Fixture,
    reject_auth: bool,
) -> (
    std::thread::JoinHandle<Result<(), String>>,
    tokio::sync::oneshot::Receiver<()>,
) {
    use crate::runtime_transport::{source_rcon_read_packet, source_rcon_write_packet};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    fixture.details.ports = vec![app_core::PortBinding {
        name: "rcon".into(),
        protocol: "tcp".into(),
        port: listener.local_addr().unwrap().port(),
    }];
    fixture.details.settings_json =
        r#"{"rcon_enabled":true,"rcon_password":"fixture-password"}"#.into();
    let (closed, receiver) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        stream
            .set_nonblocking(false)
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|error| error.to_string())?;
        let auth = source_rcon_read_packet(&mut stream)?;
        source_rcon_write_packet(&mut stream, if reject_auth { -1 } else { auth.id }, 2, "")?;
        if !reject_auth {
            let command = source_rcon_read_packet(&mut stream)?;
            if command.body != b"release" {
                return Err("unexpected fixture command".into());
            }
        }
        drop(stream);
        let _ = closed.send(());
        Ok(())
    });
    (thread, receiver)
}

fn rcon_disconnect_shutdown(grace_period_ms: u64) -> ModuleShutdownSpec {
    serde_json::from_value(json!({
        "grace_period_ms": grace_period_ms, "commands": [{"transport": "source_rcon", "fallback_transport": "stdin",
            "command": "release", "enabled_setting_key": "rcon_enabled", "wait_after_ms": 0}]
    })).unwrap()
}

#[tokio::test]
async fn normal_stop_rcon_auth_failure_preserves_declared_fallback() {
    let mut fixture = Fixture::new().await;
    let (peer, _) = rcon_disconnect_peer(&mut fixture, true);
    let shutdown = rcon_disconnect_shutdown(1000);
    let result = request_instance_graceful_shutdown(
        &fixture.state,
        &fixture.storage,
        &fixture.details,
        Some(&shutdown),
        InstanceShutdownSource::Manual,
    )
    .await;
    peer.join().unwrap().unwrap();
    result.expect(
        "an authentication rejection did not deliver shutdown and permits declared fallback",
    );
    fixture.wait_for_log_line("received:release").await;
}

#[tokio::test]
async fn normal_stop_rcon_response_failure_retains_live_tree_without_replaying() {
    let mut fixture = Fixture::new().await;
    let (peer, _) = rcon_disconnect_peer(&mut fixture, false);
    let shutdown = rcon_disconnect_shutdown(50);
    let result = request_instance_graceful_shutdown(
        &fixture.state,
        &fixture.storage,
        &fixture.details,
        Some(&shutdown),
        InstanceShutdownSource::Manual,
    )
    .await;
    peer.join().unwrap().unwrap();
    let error = result.expect_err("a missing response does not authorize the fallback release");
    assert!(
        error.contains("no fallback or forced stop was attempted"),
        "{error}"
    );
    fixture.assert_retained();
    assert!(
        !fs::read_to_string(fixture.root.join("server.log"))
            .unwrap()
            .contains("received:release")
    );
}

#[tokio::test]
async fn normal_stop_rcon_response_failure_waits_full_grace_for_cooperative_exit() {
    let mut fixture = Fixture::new().await;
    let (peer, closed) = rcon_disconnect_peer(&mut fixture, false);
    let shutdown = rcon_disconnect_shutdown(2000);
    let stop = request_instance_graceful_shutdown(
        &fixture.state,
        &fixture.storage,
        &fixture.details,
        Some(&shutdown),
        InstanceShutdownSource::Manual,
    );
    tokio::pin!(stop);
    tokio::select! {
        result = &mut stop => panic!("shutdown returned before the cooperative process exited: {result:?}"),
        signal = closed => signal.expect("peer closes after receiving the shutdown command"),
    }
    // Exercise a response loss while teardown still runs beyond wait_after_ms=0.
    // This deadline asserts waiting behavior; the process exits only on release.
    let early = tokio::time::timeout(Duration::from_millis(100), &mut stop).await;
    assert!(
        early.is_err(),
        "shutdown must await the complete tree, not execute the release fallback"
    );
    fixture.assert_retained();
    fixture
        .state
        .runtime_supervisor
        .lock()
        .unwrap()
        .dispatch_command(&fixture.details.summary.id, Some("main"), "release")
        .unwrap();
    stop.await
        .expect("tree exit during grace confirms shutdown despite the missing response");
    peer.join().unwrap().unwrap();
}

#[tokio::test]
async fn normal_stop_native_exception_is_persisted_as_crash_after_owned_tree_exit() {
    let mut fixture = Fixture::new().await;
    fixture.persist_running().await;
    // Reproduce the native status from Sons of the Forest without crashing a
    // test runner or touching a game. This owned fixture exits on its stdin.
    let shutdown: ModuleShutdownSpec = serde_json::from_value(json!({
        "grace_period_ms": 3000, "commands": [{"transport": "stdin", "command": "exit:-2147483645", "wait_after_ms": 0}]
    })).unwrap();
    let lease = fixture
        .state
        .begin_storage_context_operation("native exception stop regression")
        .unwrap();
    let result = super::super::commands_instance_stop::stop_run(
        &fixture.state,
        &fixture.storage,
        &lease,
        &fixture.details,
        &fixture.active,
        Some(&shutdown),
        InstanceShutdownSource::Manual,
    )
    .await;
    let error =
        result.expect_err("an exited tree with STATUS_BREAKPOINT is not a successful normal stop");
    assert!(
        error.contains("native_shutdown_crash") && error.contains("0x80000003"),
        "{error}"
    );
    fixture.assert_finalized(-2147483645, true).await;
}

#[tokio::test]
async fn normal_stop_preserves_native_zero_ctrl_c_and_ark_exit_codes() {
    for code in [0, -1073741510, -1] {
        let mut fixture = Fixture::new().await;
        fixture.persist_running().await;
        let shutdown: ModuleShutdownSpec = serde_json::from_value(json!({
            "grace_period_ms": 3000, "commands": [{"transport": "stdin", "command": format!("exit:{code}"), "wait_after_ms": 0}]
        })).unwrap();
        let lease = fixture
            .state
            .begin_storage_context_operation("permitted native stop exit")
            .unwrap();
        let stopped = super::super::commands_instance_stop::stop_run(
            &fixture.state,
            &fixture.storage,
            &lease,
            &fixture.details,
            &fixture.active,
            Some(&shutdown),
            InstanceShutdownSource::Manual,
        )
        .await
        .unwrap();
        assert_eq!(stopped[0].exit_code, Some(code));
        fixture.assert_finalized(code, false).await;
    }
}
