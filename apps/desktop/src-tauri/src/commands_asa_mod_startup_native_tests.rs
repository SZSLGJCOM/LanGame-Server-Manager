use super::*;

struct NativeListenerFixture {
    root: PathBuf,
    storage: StorageBootstrap,
    state: DesktopState,
    details: InstanceDetails,
    processes: Vec<(ProcessLaunchPlan, app_runtime::SpawnedProcess)>,
}

impl NativeListenerFixture {
    fn new(port: u16, bind: bool) -> Self {
        let scratch_root = std::env::var_os("LANGAME_RUNTIME_STARTUP_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let root = scratch_root.join(format!("lg-asa-mod-startup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script = root.join("listener.ps1");
        fs::write(&script, format!(r#"
$ErrorActionPreference = 'Stop'
if ({bind}) {{
    $cache = [IO.File]::Open((Join-Path $PSScriptRoot 'shared-mod-cache'), 'OpenOrCreate', 'ReadWrite', 'None')
    $cache.WriteByte(1)
    $cache.Dispose()
    $listener = [Net.Sockets.UdpClient]::new([Net.IPEndPoint]::new([Net.IPAddress]::Loopback, {port}))
    [IO.File]::WriteAllText((Join-Path $PSScriptRoot 'cache-initialized'), 'complete')
}}
Start-Sleep -Seconds 30
"#, bind = if bind { "$true" } else { "$false" })).unwrap();
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
            archives_root: root.join("instances/.trash"),
        };
        let storage = StorageBootstrap {
            settings: paths.settings(),
            storage_status: paths.probe_status(),
            paths,
        };
        let mut details = instance();
        details
            .ports
            .iter_mut()
            .find(|port| port.name == "game")
            .unwrap()
            .port = port;
        let launch = LaunchPlan {
            environment: Default::default(),
            instance_id: details.summary.id.clone(),
            instance_name: details.summary.name.clone(),
            module_id: "arksurvivalascended".into(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            uses_private_runtime: false,
            working_directory: root.to_string_lossy().into_owned(),
            executable_path: "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe".into(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: Vec::new(),
            args: vec![
                "-NoProfile".into(),
                "-File".into(),
                script.to_string_lossy().into_owned(),
            ],
            command_line: "synthetic ASA Mod listener boundary".into(),
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: RuntimePerformancePolicy::default(),
            performance_preview: Default::default(),
        };
        let log_path = root.join("managed-console/run-main.log");
        // Production owns segmented console output. A bare runtime File sink
        // would create an unowned file that the managed store must reject.
        let writer = app_storage::managed_console_log::ManagedConsoleLog::open(&log_path).unwrap();
        let spawned =
            app_runtime::spawn_launch_plan_with_log_writer(&launch, &log_path, Box::new(writer))
                .unwrap();
        let plan = ProcessLaunchPlan {
            process_key: "main".into(),
            display_name: "Island".into(),
            log_path: spawned.log_path.clone(),
            launch_plan: launch,
        };
        Self {
            root,
            storage,
            state: DesktopState::default(),
            details,
            processes: vec![(plan, spawned)],
        }
    }

    async fn wait(&mut self, timeout_ms: u64) -> Result<(), String> {
        let reservation = reserve(&self.state);
        let policy = ModuleBindAddressSpec {
            mode: ModuleBindAddressMode::Strict,
            port_names: vec!["game".into()],
            required_setting_key: None,
            startup_timeout_ms: timeout_ms,
        };
        wait_until_previous_map_ready(
            &self.state,
            &self.storage,
            &self.details,
            &policy,
            &mut self.processes,
            "map-scorched",
            "manual",
            &reservation,
        )
        .await
    }
}

impl Drop for NativeListenerFixture {
    fn drop(&mut self) {
        let cleanup = stop_spawned_processes(&mut self.processes);
        if cleanup.has_survivors() {
            eprintln!("native listener fixture cleanup retained a process: {cleanup:?}");
        } else if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("native listener fixture directory cleanup failed: {error}");
        }
    }
}

#[tokio::test]
async fn asa_mod_startup_native_gate_reads_owned_udp_listener_and_primary_pending_console() {
    let held = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = held.local_addr().unwrap().port();
    drop(held);
    let mut fixture = NativeListenerFixture::new(port, true);
    fixture.wait(15000).await.unwrap();
    assert_eq!(
        fs::read(fixture.root.join("shared-mod-cache")).unwrap(),
        [1]
    );
    assert!(fixture.root.join("cache-initialized").is_file());
    let snapshot = read_log_path_snapshot(fixture.processes[0].0.log_path.clone(), 50);
    assert!(snapshot.read_error.is_none(), "{:?}", snapshot.read_error);
    let text = snapshot.lines.join("\n");
    assert!(
        text.contains("waiting for map `main` game listener"),
        "{text}"
    );
    assert!(text.contains("starting `map-scorched`"), "{text}");
    let cleanup = stop_spawned_processes(&mut fixture.processes);
    assert!(!cleanup.has_survivors(), "{cleanup:?}");
}

#[tokio::test]
async fn asa_mod_startup_native_gate_rejects_an_unrelated_process_owning_same_port() {
    let unrelated = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut fixture = NativeListenerFixture::new(unrelated.local_addr().unwrap().port(), false);
    let error = fixture.wait(3000).await.unwrap_err();
    assert!(
        error.contains("owned game listener was not verified"),
        "{error}"
    );
    assert!(!fixture.root.join("cache-initialized").exists());
    let cleanup = stop_spawned_processes(&mut fixture.processes);
    assert!(!cleanup.has_survivors(), "{cleanup:?}");
    drop(unrelated);
}
