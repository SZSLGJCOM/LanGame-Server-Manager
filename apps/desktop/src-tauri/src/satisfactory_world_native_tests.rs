use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_core::{
    ActiveInstanceRun, InstanceProcessState, InstanceStatus, InstanceSummary, PortBinding,
    RuntimePerformancePolicy,
};
use app_runtime::{ManagedProcess, RuntimeChild};
use serde_json::json;

use super::*;

struct NativeCleanup {
    supervisor: Arc<Mutex<app_runtime::RuntimeSupervisor>>,
    instance_id: String,
}

struct UnregisteredChild(Option<std::process::Child>);

impl Drop for UnregisteredChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            if let Err(error) = child.kill() {
                eprintln!("Native probe unpublished-process stop failed: {error}");
            }
            if let Err(error) = child.wait() {
                eprintln!("Native probe unpublished-process reap failed: {error}");
            }
        }
    }
}

impl Drop for NativeCleanup {
    fn drop(&mut self) {
        let mut supervisor = self
            .supervisor
            .lock()
            .expect("Native probe supervisor lock");
        if let Some(mut instance) = supervisor.take_running_for_stop(&self.instance_id) {
            for process in &mut instance.processes {
                if let Some(RuntimeChild::Standard(mut child)) = process.child.take() {
                    if child.try_wait().expect("Native probe exit check").is_none() {
                        child
                            .kill()
                            .expect("Stop only the native probe's owned process handle");
                    }
                    child.wait().expect("Reap the native probe process");
                }
            }
        }
    }
}

fn probe_root() -> PathBuf {
    let root = PathBuf::from(
        std::env::var_os("LGSM_SATISFACTORY_PROBE_ROOT")
            .expect("Set LGSM_SATISFACTORY_PROBE_ROOT to the authorized isolated package root"),
    );
    let root = dunce::canonicalize(root).expect("Canonical native probe root");
    let task_work = PathBuf::from(
        std::env::var_os("LGSM_SATISFACTORY_PROBE_TASK_WORK").expect(
            "Set LGSM_SATISFACTORY_PROBE_TASK_WORK to the authorized isolated task work directory",
        ),
    );
    let task_work = dunce::canonicalize(task_work).expect("Canonical native probe task work");
    let repository = dunce::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
        .expect("Canonical source repository");
    assert_eq!(
        task_work.file_name().and_then(|name| name.to_str()),
        Some("work"),
        "Native probe output belongs in the authorized task work directory"
    );
    assert!(
        !task_work.starts_with(&repository),
        "Native probe output must remain outside the source repository"
    );
    assert_eq!(
        root,
        task_work.join("satisfactory-api-probe"),
        "Native tests use only the authorized task-owned probe root"
    );
    assert!(
        root.join("probe.py").is_file(),
        "Existing task-owned native probe marker"
    );
    root
}

fn launch(root: &Path, port: u16, reliable: u16) -> (DesktopState, InstanceDetails, NativeCleanup) {
    use std::os::windows::process::CommandExt;
    let data = root.join("data");
    let profile = data.join("profile");
    std::fs::create_dir_all(profile.join("AppData/Local")).unwrap();
    std::fs::create_dir_all(profile.join("AppData/Roaming")).unwrap();
    let package = root.parent().unwrap().join("server");
    let executable = package.join("Engine/Binaries/Win64/FactoryServer-Win64-Shipping-Cmd.exe");
    let log_path = root.join("native-production-service.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let child = std::process::Command::new(&executable)
        .args([
            "FactoryGame",
            "-unattended",
            "-stdout",
            "-FullStdOutLogOutput",
            "-log",
        ])
        .arg(format!("-UserDir={}", data.display()))
        .arg(format!("-Port={port}"))
        .arg(format!("-ReliablePort={reliable}"))
        .current_dir(&package)
        .env("USERPROFILE", &profile)
        .env("LOCALAPPDATA", profile.join("AppData/Local"))
        .env("APPDATA", profile.join("AppData/Roaming"))
        .creation_flags(0x0800_0000)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let pid = child.id();
    let mut child = UnregisteredChild(Some(child));
    let state = DesktopState::default();
    let instance_id = uuid::Uuid::new_v4().to_string();
    let summary = InstanceSummary {
        id: instance_id.clone(),
        name: "Satisfactory native service probe".into(),
        module_id: "satisfactory".into(),
        status: InstanceStatus::Running,
        active_process_count: 1,
        bind_ip: "0.0.0.0".into(),
        port_count: 3,
        autostart: false,
    };
    // Publish the handle before any fallible follow-up so cleanup owns it even
    // if inspection or an API assertion fails.
    let initial_identity = app_runtime::inspect_process_identity(pid).unwrap().unwrap();
    state.runtime_supervisor.lock().unwrap().insert_running(
        summary.clone(),
        Some("native-probe".into()),
        vec![ManagedProcess {
            run_id: 1,
            process_key: "server".into(),
            display_name: "Satisfactory".into(),
            pid,
            process_identity: initial_identity.clone(),
            root_process_identity: initial_identity.clone(),
            log_path: log_path.to_string_lossy().into_owned(),
            is_primary: true,
            uses_script_entrypoint: false,
            performance_policy: RuntimePerformancePolicy::default(),
            last_performance_refresh: None,
            last_performance_target_count: None,
            last_performance_application: None,
            child: Some(RuntimeChild::Standard(child.0.take().unwrap())),
            hidden_desktop: None,
        }],
    );
    let cleanup = NativeCleanup {
        supervisor: state.runtime_supervisor.clone(),
        instance_id: instance_id.clone(),
    };
    let details = InstanceDetails {
        summary,
        config_file_path: root
            .join("settings/instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: profile
            .join("AppData/Local/FactoryGame/Saved/SaveGames")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 1,
        settings_json: "{}".into(),
        ports: vec![
            PortBinding {
                name: "game".into(),
                protocol: "udp".into(),
                port,
            },
            PortBinding {
                name: "game_tcp".into(),
                protocol: "tcp".into(),
                port,
            },
            PortBinding {
                name: "reliable".into(),
                protocol: "tcp".into(),
                port: reliable,
            },
        ],
        active_run: Some(ActiveInstanceRun {
            run_id: 1,
            session_id: Some("native-probe".into()),
            pid: Some(pid),
            log_path: Some(log_path.to_string_lossy().into_owned()),
            process_count: 1,
            processes: vec![InstanceProcessState {
                run_id: 1,
                session_id: Some("native-probe".into()),
                process_key: "server".into(),
                display_name: "Satisfactory".into(),
                pid: Some(pid),
                process_identity: Some(initial_identity),
                status: "running".into(),
                started_at: None,
                stopped_at: None,
                exit_code: None,
                crash_flag: false,
                log_path: Some(log_path.to_string_lossy().into_owned()),
                is_primary: true,
            }],
        }),
    };
    (state, details, cleanup)
}

fn api_with_store(
    endpoint: &context::Endpoint,
    store: &Arc<Mutex<BTreeMap<String, String>>>,
) -> api::Api {
    let mut api = api::Api::new(endpoint.clone()).unwrap();
    api.test_credentials = Some(store.clone());
    api
}

async fn loaded(
    endpoint: &context::Endpoint,
    store: &Arc<Mutex<BTreeMap<String, String>>>,
    session: &str,
) -> SatisfactoryWorldSnapshot {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(60) {
        if let Ok(snapshot) = service::read(&api_with_store(endpoint, store)).await
            && snapshot.is_game_running
            && snapshot.active_session_name == session
        {
            return snapshot;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("The native production-service world did not finish loading");
}

#[tokio::test]
#[ignore = "requires LGSM_SATISFACTORY_PROBE_ROOT and LGSM_SATISFACTORY_PROBE_TASK_WORK: authorized task-owned copied server; starts a fresh isolated profile, touches no original saves or system keyring"]
async fn production_service_claim_rules_room_create_and_load_against_isolated_native_server() {
    let probe = probe_root();
    let root = probe.join(format!(
        "production-service-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&root).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let reliable = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    assert_ne!(port, reliable);
    let (state, details, cleanup) = launch(&root, port, reliable);
    let endpoint = context::resolve(&state, &details).unwrap().unwrap();
    let store = Arc::new(Mutex::new(BTreeMap::new()));
    let started = Instant::now();
    loop {
        if service::read(&api_with_store(&endpoint, &store))
            .await
            .is_ok()
        {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(90),
            "Native API readiness timeout"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let claimed = service::setup(
        &api_with_store(&endpoint, &store),
        SetupSatisfactoryServerInput {
            instance_id: details.summary.id.clone(),
            server_name: "LGSM Production Service".into(),
            admin_password: None,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        claimed.connection_status,
        SatisfactoryConnectionStatus::Ready
    ));
    assert!(store.lock().unwrap().contains_key("admin-password"));
    assert!(store.lock().unwrap().contains_key("api-token"));
    let first = "LGSM_Native_Service_World";
    let result = service::create(
        &api_with_store(&endpoint, &store),
        CreateSatisfactoryWorldInput {
            instance_id: details.summary.id.clone(),
            expected_revision: claimed.revision,
            session_name: first.into(),
            starting_location: "Grass Fields".into(),
            skip_onboarding: true,
            acknowledge_enable_advanced_settings: false,
            game_mode_settings: BTreeMap::from([
                ("FG.GameMode.PartsCostMultiplier".into(), "125".into()),
                ("FG.GameMode.NodeRandomizationSeed".into(), "42".into()),
            ]),
            advanced_game_settings: BTreeMap::new(),
        },
    )
    .await
    .unwrap();
    assert!(result.accepted);
    let current = loaded(&endpoint, &store, first).await;
    let current = service::write_rules(
        &api_with_store(&endpoint, &store),
        WriteSatisfactoryWorldRulesInput {
            instance_id: details.summary.id.clone(),
            expected_revision: current.revision,
            acknowledge_enable_advanced_settings: true,
            advanced_game_settings: BTreeMap::from([(
                "FG.GameRules.NoPower".into(),
                "True".into(),
            )]),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        current.advanced_game_settings["FG.GameRules.NoPower"],
        "True"
    );
    let current = service::room(
        &api_with_store(&endpoint, &store),
        WriteSatisfactoryRoomInput {
            instance_id: details.summary.id.clone(),
            expected_revision: current.revision,
            server_name: Some("LGSM Native Room Readback".into()),
            client_password: Some(uuid::Uuid::new_v4().simple().to_string()),
            auto_load_session_name: Some(first.into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        current.server_name.as_deref(),
        Some("LGSM Native Room Readback")
    );
    let selected = current
        .sessions
        .iter()
        .find(|session| session.session_name == first)
        .unwrap()
        .saves
        .iter()
        .find(|save| save.save_name.starts_with("LGSM_world_settings_"))
        .unwrap()
        .save_name
        .clone();
    let result = service::load(
        &api_with_store(&endpoint, &store),
        LoadSatisfactorySaveInput {
            instance_id: details.summary.id.clone(),
            expected_revision: current.revision,
            save_name: selected,
        },
    )
    .await
    .unwrap();
    assert!(result.accepted);
    let current = loaded(&endpoint, &store, first).await;
    assert_eq!(
        current.advanced_game_settings["FG.GameRules.NoPower"],
        "True"
    );
    let second = "LGSM_Native_Service_Second";
    let result = service::create(
        &api_with_store(&endpoint, &store),
        CreateSatisfactoryWorldInput {
            instance_id: details.summary.id.clone(),
            expected_revision: current.revision,
            session_name: second.into(),
            starting_location: String::new(),
            skip_onboarding: true,
            acknowledge_enable_advanced_settings: false,
            game_mode_settings: BTreeMap::new(),
            advanced_game_settings: BTreeMap::new(),
        },
    )
    .await
    .unwrap();
    assert!(result.accepted);
    let current = loaded(&endpoint, &store, second).await;
    assert!(current.sessions.iter().any(|session| {
        session.session_name == first
            && session
                .saves
                .iter()
                .any(|save| save.save_name.starts_with("LGSM_before_world_change_"))
    }));
    std::fs::write(root.join("production-service-readback.json"), serde_json::to_vec_pretty(&json!({"instance_id":details.summary.id,"active_session":current.active_session_name,"old_world_preserved":true,"native_service_verified":true,"credential_store":"isolated memory boundary; system keyring untouched"})).unwrap()).unwrap();
    drop(cleanup);
}
