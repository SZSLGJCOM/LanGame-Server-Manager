//! Exercise the actual detached service creator without a service database or UI.
//! The libtest logfile argument carries each isolated fixture's output directory.
use super::*;
use app_core::{
    InstanceStatus, InstanceSummary, LaunchPlan, ProcessHostSurface, ProcessWindowPolicy,
};
use app_runtime::{ManagedProcess, ProcessExitTarget, RuntimeSupervisor};
use std::ffi::{OsStr, OsString, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

const FIXTURE: &str = "runtime_service::client::ctrl_c_tests::console_inheritance_process_fixture";
const MARKER: &str = "langame-console-inheritance-fixture";
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

fn fixture_arguments(log: &Path) -> Vec<OsString> {
    [
        "--exact",
        FIXTURE,
        "--ignored",
        "--nocapture",
        "--test-threads=1",
        "--logfile",
    ]
    .map(OsString::from)
    .into_iter()
    .chain([log.as_os_str().to_owned()])
    .collect()
}

fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "isolated Ctrl+C fixture timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct ServiceOwner(Arc<ProcessExitTarget>);
impl Drop for ServiceOwner {
    fn drop(&mut self) {
        // Recovery is restricted to this test-created, identity-pinned service.
        // Its game owns a kill-on-close Job and has an independent deadline.
        if self.0.is_running().unwrap_or(false) {
            let _ = self.0.terminate();
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.0.is_running().unwrap_or(false) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn run_case(normalize_controller: bool) -> Value {
    let nonce = uuid::Uuid::new_v4().to_string();
    let root = std::env::temp_dir().join(format!("lgsm-console-inheritance-{nonce}"));
    fs::create_dir(&root).unwrap();
    fs::write(root.join(MARKER), &nonce).unwrap();
    let log = root.join(if normalize_controller {
        "baseline.log"
    } else {
        "service.log"
    });
    let endpoint = security::Endpoint::isolated(&nonce).unwrap();
    // The production function supplies the service's creation flags. A fix to
    // those flags must change this test's observed child behavior.
    let client = connect_or_start_with_arguments(endpoint, &fixture_arguments(&log)).unwrap();
    let service = ServiceOwner(client.service_target().unwrap());
    fs::write(root.join("connected"), b"connected").unwrap();
    wait_until(Duration::from_secs(12), || !service.0.is_running().unwrap());
    let receipt: Value =
        serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(receipt["interrupt_sent"], true, "{receipt}");
    assert_eq!(receipt["game_exit_code"], 0, "{receipt}");
    drop(client);
    drop(service);
    // Only the exact newly created direct child of the managed TEMP is removed.
    let canonical = fs::canonicalize(&root).unwrap();
    let canonical_temp = fs::canonicalize(std::env::temp_dir()).unwrap();
    assert_eq!(canonical.parent(), Some(canonical_temp.as_path()));
    fs::remove_dir_all(&canonical).unwrap();
    receipt
}

#[test]
fn detached_service_first_managed_game_receives_ctrl_c() {
    let baseline = run_case(true);
    assert_eq!(
        baseline["observed_ctrl_c"], true,
        "baseline must establish a working receiver: {baseline}"
    );
    let service = run_case(false);
    assert_eq!(
        service["observed_ctrl_c"], true,
        "the production detached service must not pass an ignored Ctrl+C state to its first managed game: {service}"
    );
}

#[test]
#[ignore = "child entry: isolated named pipe and synthetic game only; no real storage or game"]
fn console_inheritance_process_fixture() {
    let arguments: Vec<_> = std::env::args_os().collect();
    let Some(index) = arguments
        .iter()
        .position(|arg| arg == OsStr::new("--logfile"))
    else {
        return;
    };
    let log = PathBuf::from(arguments.get(index + 1).expect("fixture logfile"));
    let root = log.parent().unwrap();
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    let nonce = fs::read_to_string(root.join(MARKER)).expect("explicit fixture marker");
    assert_eq!(
        root.file_name().unwrap().to_string_lossy(),
        format!("lgsm-console-inheritance-{nonce}")
    );
    match log.file_stem().and_then(OsStr::to_str) {
        Some("game") => run_game(root),
        Some(role @ ("baseline" | "service")) => run_controller(root, &nonce, role == "baseline"),
        _ => panic!("unknown isolated console role"),
    }
}

unsafe extern "system" fn record_interrupt(event: u32) -> i32 {
    if event == 0 {
        INTERRUPTED.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}

fn run_game(root: &Path) {
    // Register a real handler WITHOUT NULL/FALSE: clearing the inherited ignore
    // bit here would hide exactly the production startup defect under test.
    assert_ne!(
        unsafe { SetConsoleCtrlHandler(record_interrupt as *const () as *mut c_void, 1) },
        0
    );
    fs::write(root.join("game.ready"), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !INTERRUPTED.load(Ordering::SeqCst) && !root.join("game.release").exists() {
        assert!(
            Instant::now() < deadline,
            "synthetic game owner disappeared"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(
        root.join("game.result.json"),
        serde_json::to_vec(
            &serde_json::json!({"observed_ctrl_c": INTERRUPTED.load(Ordering::SeqCst)}),
        )
        .unwrap(),
    )
    .unwrap();
}

fn run_controller(root: &Path, nonce: &str, normalize: bool) {
    let endpoint = security::Endpoint::isolated(nonce).unwrap();
    // Client::connect verifies this process through the real secured control
    // pipe. No command dispatcher, storage bootstrap, or real endpoint is used.
    let _pipe = tauri::async_runtime::block_on(async {
        security::create_pipe(&endpoint.control(), true).unwrap()
    });
    wait_until(Duration::from_secs(5), || root.join("connected").exists());
    if normalize {
        // Positive control belongs to the disposable controller, never the game
        // or parent test process. The production-service arm does not normalize.
        assert_ne!(unsafe { SetConsoleCtrlHandler(std::ptr::null_mut(), 0) }, 0);
    }
    let executable = std::env::current_exe().unwrap();
    let plan = LaunchPlan {
        instance_id: nonce.into(),
        instance_name: "Synthetic console inheritance".into(),
        module_id: "console-inheritance-fixture".into(),
        install_root: root.to_string_lossy().into_owned(),
        install_state: app_core::InstallState::Installed,
        uses_private_runtime: true,
        working_directory: root.to_string_lossy().into_owned(),
        executable_path: executable.to_string_lossy().into_owned(),
        executable_exists: true,
        ready_to_launch: true,
        validation_issues: Vec::new(),
        args: fixture_arguments(&root.join("game.log"))
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect(),
        environment: Default::default(),
        command_line: String::new(),
        window_policy: ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: ProcessHostSurface::ManagedTerminal,
        host_notes: None,
        performance_policy: Default::default(),
        performance_preview: Default::default(),
    };
    let mut spawned = app_runtime::spawn_launch_plan(&plan, root.join("game-output.log")).unwrap();
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(
        InstanceSummary {
            id: nonce.into(),
            name: plan.instance_name.clone(),
            module_id: plan.module_id.clone(),
            status: InstanceStatus::Running,
            active_process_count: 1,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        None,
        vec![ManagedProcess {
            run_id: 1,
            process_key: "main".into(),
            display_name: "Synthetic game".into(),
            pid: spawned.pid,
            process_identity: spawned.process_identity.clone(),
            root_process_identity: spawned.root_process_identity.clone(),
            log_path: spawned.log_path.clone(),
            is_primary: true,
            uses_script_entrypoint: false,
            performance_policy: Default::default(),
            last_performance_refresh: None,
            last_performance_target_count: None,
            last_performance_application: None,
            child: spawned.child.take(),
            hidden_desktop: spawned.hidden_desktop.take(),
        }],
    );
    wait_until(Duration::from_secs(3), || root.join("game.ready").exists());
    let interrupt = supervisor.request_console_interrupt(nonce, Some("main"));
    let deadline = Instant::now() + Duration::from_secs(2);
    while !root.join("game.result.json").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    // Even a failed signal exits cooperatively, so the red regression neither
    // leaves a process behind nor treats forced cleanup as successful delivery.
    fs::write(root.join("game.release"), b"release").unwrap();
    let mut managed = supervisor.take_running_for_stop(nonce).unwrap();
    let child = managed.processes[0].child.as_mut().unwrap();
    let mut code = None;
    wait_until(Duration::from_secs(3), || {
        if let Some(status) = child.try_wait().unwrap() {
            code = status.code();
            true
        } else {
            false
        }
    });
    let game: Value =
        serde_json::from_slice(&fs::read(root.join("game.result.json")).unwrap()).unwrap();
    fs::write(root.join("result.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "interrupt_sent": interrupt.is_ok(), "interrupt_error": interrupt.err().map(|error| error.to_string()),
        "observed_ctrl_c": game["observed_ctrl_c"], "game_exit_code": code,
    })).unwrap()).unwrap();
    drop(managed);
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(handler: *mut c_void, add: i32) -> i32;
}
