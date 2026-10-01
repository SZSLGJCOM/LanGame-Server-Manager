use super::*;
#[path = "commands_windrose_console_fixture.rs"]
mod native_console_fixture;
use std::net::TcpListener;

const CHILD_TEST: &str = "commands::commands_runtime_lifecycle::windrose_lifecycle::tests::windrose_initialization_child";
const HOST_READY_LOG: &str = "R5LogCoopProxy: [000566] ...opProxyServer::SetIsReadyForHostOwnerConnect Host server is ready for owner to connect. Semaphore null";
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(handler: *mut std::ffi::c_void, add: i32) -> i32;
    fn FreeConsole() -> i32;
}

unsafe extern "system" fn record_interrupt(event: u32) -> i32 {
    if event == 0 {
        INTERRUPTED.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}

fn permit() -> tokio::sync::OwnedSemaphorePermit {
    Arc::new(tokio::sync::Semaphore::new(1))
        .try_acquire_owned()
        .unwrap()
}

#[test]
#[ignore = "owned synthetic native-process boundary, invoked only by Windrose bootstrap tests"]
fn windrose_initialization_child() {
    let root = PathBuf::from(std::env::var("LANGAME_WINDROSE_BOOTSTRAP_FIXTURE").unwrap());
    assert!(
        root.canonicalize()
            .unwrap()
            .starts_with(std::env::temp_dir().canonicalize().unwrap())
    );
    assert_ne!(unsafe { SetConsoleCtrlHandler(std::ptr::null_mut(), 0) }, 0);
    assert_ne!(
        unsafe { SetConsoleCtrlHandler(record_interrupt as *const () as *mut _, 1) },
        0
    );
    std::fs::write(root.join("bootstrap-pid"), std::process::id().to_string()).unwrap();
    let mode = std::env::var("LANGAME_WINDROSE_BOOTSTRAP_MODE").unwrap();
    if mode == "exit" {
        return;
    }
    if mode != "detached_ready" {
        native_console_fixture::create();
    }
    if matches!(mode.as_str(), "ready" | "unclean") {
        write_native_world(&root);
        println!("LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()");
        println!("{HOST_READY_LOG}");
    }
    if mode == "detached_ready" {
        write_native_world(&root);
        assert_ne!(unsafe { FreeConsole() }, 0);
        let path = std::env::var("LANGAME_WINDROSE_BOOTSTRAP_LOG").unwrap();
        let mut log = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        std::io::Write::write_all(&mut log, format!("{HOST_READY_LOG}\n").as_bytes()).unwrap();
    }
    if mode == "held_ready" {
        println!("LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !root.join("release-ready").is_file() && !INTERRUPTED.load(Ordering::SeqCst) {
            native_console_fixture::pump();
            assert!(
                Instant::now() < deadline,
                "readiness fixture gate was not released"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        if !INTERRUPTED.load(Ordering::SeqCst) {
            println!("{HOST_READY_LOG}");
        }
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while !INTERRUPTED.load(Ordering::SeqCst) {
        native_console_fixture::pump();
        assert!(
            Instant::now() < deadline,
            "synthetic bootstrap must receive owned native quit"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    if mode == "unclean" {
        std::process::exit(23);
    }
    std::fs::write(root.join("bootstrap-flushed"), b"clean native shutdown").unwrap();
}

fn write_native_world(root: &Path) {
    let world = root.join("R5/Saved/SaveProfiles/Default/RocksDB_v2/fixture/Worlds/NativeWorld");
    std::fs::create_dir_all(&world).unwrap();
    let description = json!({ "Version": 1, "WorldDescription": {
        "islandId": "NativeWorld", "WorldName": "The Archipelago", "WorldPresetType": "Medium",
        "WorldSettings": {
            "BoolParameters": {
                "{\"TagName\": \"WDS.Parameter.Coop.SharedQuests\"}": true,
                "{\"TagName\": \"WDS.Parameter.EasyExplore\"}": false
            },
            "FloatParameters": {
                "{\"TagName\": \"WDS.Parameter.MobHealthMultiplier\"}": 1.0,
                "{\"TagName\": \"WDS.Parameter.MobDamageMultiplier\"}": 1.0,
                "{\"TagName\": \"WDS.Parameter.ShipsHealthMultiplier\"}": 1.0,
                "{\"TagName\": \"WDS.Parameter.ShipsDamageMultiplier\"}": 1.0,
                "{\"TagName\": \"WDS.Parameter.BoardingDifficultyMultiplier\"}": 1.0,
                "{\"TagName\": \"WDS.Parameter.Coop.StatsCorrectionModifier\"}": 1.0,
                "{\"TagName\": \"WDS.Parameter.Coop.ShipStatsCorrectionModifier\"}": 0.0
            },
            "TagParameters": {
                "{\"TagName\": \"WDS.Parameter.CombatDifficulty\"}": {
                    "TagName": "WDS.Parameter.CombatDifficulty.Normal"
                }
            }
        }
    }});
    std::fs::write(
        world.join("WorldDescription.json"),
        serde_json::to_vec(&description).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("R5/ServerDescription.json"),
        serde_json::to_vec(&json!({
            "Version": 1,
            "ServerDescription_Persistent": {
                "PersistentServerId": "NATIVE-IDENTITY",
                "InviteCode": "NativeInvite",
                "WorldIslandId": "NativeWorld",
                "UseDirectConnection": false,
                "DirectConnectionServerPort": 32017,
                "ServerName": "Native defaults"
            }
        }))
        .unwrap(),
    )
    .unwrap();
}

struct Fixture {
    root: PathBuf,
    paths: app_storage::StoragePaths,
    instance: InstanceDetails,
    plan: ProcessLaunchPlan,
}

impl Fixture {
    async fn new(mode: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lg-windrose-start-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let paths = app_storage::StoragePaths {
            app_data_root: root.clone(),
            settings_path: root.join("settings.json"),
            database_path: root.join("database/lgsm.db"),
            logs_root: root.join("logs"),
            modules_root: repository.join("modules"),
            migrations_root: repository.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        let descriptor = discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == "windrose")
            .unwrap();
        let install = paths.games_root.join("windrose");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        std::fs::write(
            install.join("WindroseServer.exe"),
            b"synthetic official program boundary",
        )
        .unwrap();
        app_storage::record_library_program_baseline(&install, &descriptor, true, None).unwrap();
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        sync_game_installs(
            &paths,
            &[GameInstallSyncRecord {
                module_id: "windrose".into(),
                install_root: install.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("fixture".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        let instance = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                module_id: "windrose".into(),
                name: "Windrose fixture".into(),
            },
        )
        .await
        .unwrap();
        let instance = read_instance_details(&paths, &instance.summary.id)
            .await
            .unwrap();
        let mut launch_plan =
            build_instance_launch_preview(&paths.settings(), &descriptor, &instance).unwrap();
        assert!(launch_plan.args.iter().any(|arg| arg == "-NewConsole"));
        let shutdown = descriptor.runtime.shutdown.as_ref().unwrap();
        assert_eq!(shutdown.commands[0].transport, "unreal_console");
        assert_eq!(shutdown.commands[0].command, "quit");
        assert!(shutdown.commands[0].fallback_transport.is_none());
        launch_plan.executable_path = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        launch_plan.executable_exists = true;
        launch_plan.ready_to_launch = true;
        launch_plan.validation_issues.clear();
        launch_plan.args = vec![
            "--exact".into(),
            CHILD_TEST.into(),
            "--ignored".into(),
            "--nocapture".into(),
        ];
        launch_plan.environment.insert(
            "LANGAME_WINDROSE_BOOTSTRAP_FIXTURE".into(),
            launch_plan.install_root.clone(),
        );
        launch_plan
            .environment
            .insert("LANGAME_WINDROSE_BOOTSTRAP_MODE".into(), mode.into());
        launch_plan.environment.insert(
            "LANGAME_WINDROSE_BOOTSTRAP_LOG".into(),
            root.join("bootstrap.log").to_string_lossy().into_owned(),
        );
        let plan = ProcessLaunchPlan {
            process_key: "windrose-bootstrap".into(),
            display_name: "Synthetic native bootstrap".into(),
            log_path: root.join("bootstrap.log").to_string_lossy().into_owned(),
            launch_plan,
        };
        Self {
            root,
            paths,
            instance,
            plan,
        }
    }

    fn runtime(&self) -> PathBuf {
        PathBuf::from(&self.plan.launch_plan.install_root)
    }

    async fn wait_for_pid(&self) -> u32 {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if let Ok(pid) = std::fs::read_to_string(self.runtime().join("bootstrap-pid"))
                    && let Ok(pid) = pid.parse::<u32>()
                {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("synthetic native process must start")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn reserve(state: &DesktopState, id: &str) -> RuntimeStartReservationLease {
    match state.try_reserve_runtime_start(id, "manual").unwrap() {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected start reservation: {other:?}"),
    }
}

#[test]
fn windrose_bootstrap_without_native_direct_mode_ignores_occupied_example_port() {
    // If another process already owns 7777, keep it untouched; otherwise own
    // the listener for this check. Either case supplies a real occupied port.
    let listener = match TcpListener::bind(("0.0.0.0", 7777)) {
        Ok(listener) => Some(listener),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => None,
        Err(error) => panic!("cannot establish an occupied example port: {error}"),
    };
    assert!(TcpListener::bind(("0.0.0.0", 7777)).is_err());
    for direct in [None, Some(false)] {
        for port in [None, Some(7777)] {
            check_initialization_ports(&WindroseBootstrapObservation {
                world_ready: false,
                use_direct_connection: direct,
                direct_connection_server_port: port,
            })
            .unwrap();
        }
    }
    assert!(TcpListener::bind(("0.0.0.0", 7777)).is_err());
    drop(listener);
}

#[test]
fn windrose_bootstrap_port_conflict_keeps_the_existing_listener() {
    let listener = TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let observed = WindroseBootstrapObservation {
        world_ready: false,
        use_direct_connection: Some(true),
        direct_connection_server_port: Some(port),
    };
    assert!(
        check_initialization_ports(&observed)
            .unwrap_err()
            .contains(&port.to_string())
    );
    assert!(TcpListener::bind(("0.0.0.0", port)).is_err());
}

#[test]
fn windrose_bootstrap_direct_mode_requires_a_valid_native_port() {
    for port in [None, Some(0)] {
        assert!(
            check_initialization_ports(&WindroseBootstrapObservation {
                world_ready: false,
                use_direct_connection: Some(true),
                direct_connection_server_port: port,
            })
            .is_err()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn windrose_bootstrap_stops_native_process_before_applying_instance_settings() {
    let fixture = Fixture::new("ready").await;
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
    assert!(matches!(outcome, BootstrapOutcome::Ready));
    let pid = fixture.wait_for_pid().await;
    assert!(!app_runtime::process_is_running(pid).unwrap());
    assert!(fixture.runtime().join("bootstrap-flushed").is_file());
    assert_eq!(
        std::fs::read_to_string(fixture.runtime().join("bootstrap-command")).unwrap(),
        "quit"
    );
    let server: Value = serde_json::from_slice(
        &std::fs::read(fixture.runtime().join("R5/ServerDescription.json")).unwrap(),
    )
    .unwrap();
    let persistent = &server["ServerDescription_Persistent"];
    assert_eq!(persistent["WorldIslandId"], "NativeWorld");
    assert_eq!(persistent["PersistentServerId"], "NATIVE-IDENTITY");
    assert_eq!(persistent["UseDirectConnection"], true);
    assert_eq!(persistent["ServerName"], "Windrose fixture");
    let port = fixture
        .instance
        .ports
        .iter()
        .find(|port| port.name == "direct")
        .unwrap()
        .port;
    assert_eq!(persistent["DirectConnectionServerPort"], port);
    // The native phase precedes ordinary start materialization. A remap held
    // back during bootstrap must reach both the native file and firewall specs.
    let formal_port = if port == 28037 { 28038 } else { 28037 };
    let mut formal_ports = fixture.instance.ports.clone();
    for binding in &mut formal_ports {
        binding.port = formal_port;
    }
    let formal = materialize_runtime_start_configuration(
        fixture.paths.clone(),
        fixture.instance.clone(),
        Some(formal_ports),
        fixture.instance.summary.id.clone(),
    )
    .await
    .unwrap();
    assert!(
        formal
            .ports
            .iter()
            .all(|binding| binding.port == formal_port)
    );
    let formal_server: Value = serde_json::from_slice(
        &std::fs::read(fixture.runtime().join("R5/ServerDescription.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        formal_server["ServerDescription_Persistent"]["DirectConnectionServerPort"],
        formal_port
    );
    let rules = app_platform_win::build_instance_firewall_rule_specs(
        &formal.summary.id,
        &formal.summary.name,
        &formal.ports,
        None,
    );
    assert_eq!(rules.len(), 2);
    assert!(rules.iter().all(|rule| rule.local_port == formal_port));
    assert!(rules.iter().any(|rule| rule.protocol == "TCP"));
    assert!(rules.iter().any(|rule| rule.protocol == "UDP"));
    assert!(
        app_storage::prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn windrose_bootstrap_cancelled_waiter_keeps_job_and_lease_until_shutdown_cleanup() {
    let fixture = Fixture::new("waiting").await;
    let original = std::fs::read(fixture.runtime().join("R5/ServerDescription.json")).unwrap();
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
    let plan = fixture.plan.clone();
    let waiter = tokio::spawn(async move {
        let worker_lease = reservation.clone();
        spawn_storage_context_task(&worker_lease, async move {
            run_bootstrap(prepared, permit(), plan, reservation, group).await
        })
        .await
        .unwrap()
    });
    let pid = fixture.wait_for_pid().await;
    waiter.abort();
    let _ = waiter.await;
    assert!(state.begin_storage_shutdown_exclusive().is_err());
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    state
        .drain_storage_operations_for_shutdown(Duration::from_secs(30))
        .await
        .unwrap();
    assert!(!app_runtime::process_is_running(pid).unwrap());
    assert_eq!(
        std::fs::read(fixture.runtime().join("R5/ServerDescription.json")).unwrap(),
        original
    );
}

#[tokio::test(flavor = "current_thread")]
async fn windrose_bootstrap_abnormal_exit_keeps_native_world_and_recovery_record() {
    let fixture = Fixture::new("unclean").await;
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
    let result = run_bootstrap(prepared, permit(), fixture.plan.clone(), reservation, group).await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("abnormal native exit must not complete bootstrap"),
    };
    assert!(error.contains("Some(23)"), "{error}");
    assert!(!app_runtime::process_is_running(fixture.wait_for_pid().await).unwrap());
    let bytes = std::fs::read(fixture.runtime().join("R5/ServerDescription.json")).unwrap();
    let server: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        server["ServerDescription_Persistent"]["ServerName"],
        "Native defaults"
    );
    assert!(fixture.runtime().join("R5/Saved/SaveProfiles/Default/RocksDB_v2/fixture/Worlds/NativeWorld/WorldDescription.json").is_file());
    let retry =
        app_storage::prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .unwrap()
            .unwrap();
    assert!(retry.inspect_native_state().unwrap().world_ready);
    assert_eq!(
        std::fs::read(fixture.runtime().join("R5/ServerDescription.json")).unwrap(),
        bytes
    );
}

#[path = "commands_windrose_bootstrap_tests.rs"]
mod bootstrap_tests;
