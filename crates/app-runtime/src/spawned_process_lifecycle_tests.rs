use super::*;
use crate::*;
use std::sync::atomic::{AtomicU32, Ordering};

const PROBE: &str = "spawned_process_lifecycle::tests::graceful_spawn_probe";
const ROOT: &str = "LGSM_GRACEFUL_SPAWN_ROOT";
const ROLE: &str = "LGSM_GRACEFUL_SPAWN_ROLE";
static INTERRUPTS: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn record_interrupt(event: u32) -> i32 {
    if event == CTRL_C_EVENT {
        INTERRUPTS.fetch_add(1, Ordering::SeqCst);
        1
    } else {
        0
    }
}

#[test]
#[ignore = "owned native child fixture; launched by graceful_spawn regressions"]
fn graceful_spawn_probe() {
    let root = PathBuf::from(std::env::var_os(ROOT).expect("fixture root"));
    let role = std::env::var(ROLE).expect("fixture role");
    assert_ne!(unsafe { SetConsoleCtrlHandler(std::ptr::null_mut(), 0) }, 0);
    assert_ne!(
        unsafe { SetConsoleCtrlHandler(record_interrupt as *const () as *mut _, 1) },
        0
    );
    if role == "detached" {
        assert_ne!(unsafe { FreeConsole() }, 0);
    }
    if role == "parent" {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", PROBE, "--ignored", "--nocapture"])
            .env(ROLE, "leaf")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn native descendant inside the same owned Job");
        fs::write(root.join("leaf.pid"), child.id().to_string()).unwrap();
        assert!(wait_until(
            || root.join("leaf.ready").exists(),
            Duration::from_secs(5)
        ));
        // Dropping std::process::Child closes this handle without stopping its process.
        drop(child);
    }
    fs::write(root.join(format!("{role}.ready")), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut last_count = 0;
    loop {
        let count = INTERRUPTS.load(Ordering::SeqCst);
        if count != last_count {
            fs::write(root.join(format!("{role}.interrupts")), count.to_string()).unwrap();
            last_count = count;
        }
        if root.join("release").exists()
            || (count > 0 && matches!(role.as_str(), "cooperate" | "parent"))
        {
            println!("native-shutdown-tail-{role}");
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
            std::process::exit(if role == "natural" { 23 } else { 0 });
        }
        assert!(Instant::now() < deadline, "fixture release deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_until(mut ready: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if ready() {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(5)));
    }
}

struct Fixture {
    root: PathBuf,
    spawned: SpawnedProcess,
    root_handle: WindowsProcessHandle,
    leaf_handle: Option<WindowsProcessHandle>,
}

impl Fixture {
    fn new(role: &str) -> Self {
        let root = test_support::unique_test_root();
        fs::create_dir_all(&root).unwrap();
        let executable = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let plan = LaunchPlan {
            environment: BTreeMap::from([
                (ROOT.into(), root.to_string_lossy().into_owned()),
                (ROLE.into(), role.into()),
            ]),
            instance_id: "graceful-spawn".into(),
            instance_name: "Graceful spawn fixture".into(),
            module_id: "fixture".into(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: app_core::InstallState::Installed,
            uses_private_runtime: false,
            working_directory: root.to_string_lossy().into_owned(),
            executable_path: executable.clone(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: Vec::new(),
            args: ["--exact", PROBE, "--ignored", "--nocapture"]
                .map(String::from)
                .into(),
            command_line: executable,
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: RuntimePerformancePolicy::default(),
            performance_preview: Default::default(),
        };
        let spawned = spawn_launch_plan(&plan, root.join("console.log")).unwrap();
        let root_handle = WindowsProcessHandle::open(spawned.pid, PROCESS_TERMINATE)
            .unwrap()
            .unwrap();
        let mut fixture = Self {
            root,
            spawned,
            root_handle,
            leaf_handle: None,
        };
        assert!(
            wait_until(
                || fixture.root.join(format!("{role}.ready")).exists(),
                Duration::from_secs(10)
            ),
            "native fixture ready"
        );
        if role == "parent" {
            let pid = fs::read_to_string(fixture.root.join("leaf.pid"))
                .unwrap()
                .parse()
                .unwrap();
            fixture.leaf_handle = Some(
                WindowsProcessHandle::open(pid, PROCESS_TERMINATE)
                    .unwrap()
                    .unwrap(),
            );
        }
        assert!(fixture.spawned.hidden_desktop.is_some());
        fixture
    }

    fn assert_retained(&mut self) {
        let child = self
            .spawned
            .child
            .as_mut()
            .expect("failed graceful stop retains native child");
        assert!(child.has_owned_process_tree());
        assert_eq!(child.owned_process_tree_is_running().unwrap(), Some(true));
        assert!(
            self.spawned.hidden_desktop.is_some(),
            "failed graceful stop retains private desktop"
        );
    }

    fn release(&mut self) {
        fs::write(self.root.join("release"), b"release").unwrap();
        assert!(self.root_handle.wait_for_exit(5000).unwrap());
        if let Some(leaf) = &self.leaf_handle {
            assert!(leaf.wait_for_exit(5000).unwrap());
        }
        // A signaled process handle can precede the Job's active-count update.
        // Zero-budget observations below require the entire tree to be settled.
        assert!(wait_until(
            || self
                .spawned
                .child
                .as_mut()
                .unwrap()
                .owned_process_tree_is_running()
                .unwrap()
                == Some(false),
            Duration::from_secs(5),
        ));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(self.root.join("release"), b"release");
        for handle in std::iter::once(&self.root_handle).chain(self.leaf_handle.iter()) {
            if !handle.wait_for_exit(1500).unwrap_or(false) {
                // Failure recovery is confined to this disposable fixture's pinned handles.
                let _ = handle.terminate();
                let _ = handle.wait_for_exit(1500);
            }
        }
        self.spawned.child.take();
        self.spawned.hidden_desktop.take();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn graceful_spawn_cooperates_without_job_termination_and_drains_output() {
    let mut fixture = Fixture::new("cooperate");
    let RuntimeChild::Windows(native) = fixture.spawned.child.as_mut().unwrap() else {
        panic!("native owner");
    };
    // Even an empty Job cannot be terminated through this duplicate handle.
    let full_access =
        windows_process_job::restrict_job_to_query_for_test(native.job.as_mut().unwrap()).unwrap();
    let result = stop_spawned_process_gracefully(&mut fixture.spawned, Duration::from_secs(5));
    assert_eq!(result.unwrap(), Some(0));
    assert!(fixture.spawned.child.is_none() && fixture.spawned.hidden_desktop.is_none());
    assert_eq!(
        fs::read_to_string(fixture.root.join("cooperate.interrupts")).unwrap(),
        "1"
    );
    assert!(
        fs::read_to_string(fixture.root.join("console.log"))
            .unwrap()
            .contains("native-shutdown-tail-cooperate")
    );
    drop(full_access);
}

#[test]
fn graceful_spawn_refusing_process_times_out_alive_and_owned() {
    let mut fixture = Fixture::new("refuse");
    let result = stop_spawned_process_gracefully(&mut fixture.spawned, Duration::from_millis(150));
    assert!(
        matches!(result, Err(RuntimeProcessError::WaitTrackedProcess { source, .. }) if source.kind() == std::io::ErrorKind::TimedOut)
    );
    assert!(fixture.root_handle.is_running().unwrap());
    fixture.assert_retained();
    assert_eq!(
        fs::read_to_string(fixture.root.join("refuse.interrupts")).unwrap(),
        "1"
    );
}

#[test]
fn graceful_spawn_root_exit_does_not_kill_surviving_descendant() {
    let mut fixture = Fixture::new("parent");
    let result = stop_spawned_process_gracefully(&mut fixture.spawned, Duration::from_millis(150));
    assert!(
        matches!(result, Err(RuntimeProcessError::WaitTrackedProcess { source, .. }) if source.kind() == std::io::ErrorKind::TimedOut)
    );
    assert!(!fixture.root_handle.is_running().unwrap());
    assert!(fixture.leaf_handle.as_ref().unwrap().is_running().unwrap());
    fixture.assert_retained();
    fixture.release();
    // Preserve the root's original exit code without broadcasting again.
    assert_eq!(
        stop_spawned_process_gracefully(&mut fixture.spawned, Duration::ZERO).unwrap(),
        Some(0)
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("leaf.interrupts")).unwrap(),
        "1"
    );
    assert!(fixture.spawned.child.is_none() && fixture.spawned.hidden_desktop.is_none());
}

#[test]
fn graceful_spawn_natural_exit_uses_owned_handle_without_another_signal() {
    let mut fixture = Fixture::new("natural");
    fixture.release();
    assert_eq!(
        stop_spawned_process_gracefully(&mut fixture.spawned, Duration::ZERO).unwrap(),
        Some(23)
    );
    assert!(!fixture.root.join("natural.interrupts").exists());
    assert!(fixture.spawned.child.is_none() && fixture.spawned.hidden_desktop.is_none());
}

#[test]
fn graceful_spawn_native_control_error_preserves_all_ownership() {
    let mut fixture = Fixture::new("detached");
    assert!(matches!(
        stop_spawned_process_gracefully(&mut fixture.spawned, Duration::from_millis(150)),
        Err(RuntimeProcessError::ConsoleInterruptProcess { .. })
    ));
    assert!(fixture.root_handle.is_running().unwrap());
    fixture.assert_retained();
}

#[test]
fn graceful_spawn_failed_job_inspection_preserves_all_ownership() {
    let mut fixture = Fixture::new("cooperate");
    let RuntimeChild::Windows(native) = fixture.spawned.child.as_mut().unwrap() else {
        panic!("native owner");
    };
    let full_access =
        windows_process_job::restrict_job_to_terminate_for_test(native.job.as_mut().unwrap())
            .unwrap();
    let result = stop_spawned_process_gracefully(&mut fixture.spawned, Duration::from_millis(150));
    assert!(
        matches!(result, Err(RuntimeProcessError::WaitTrackedProcess { source, .. }) if source.raw_os_error() == Some(5))
    );
    assert!(fixture.root_handle.is_running().unwrap());
    assert!(!fixture.root.join("cooperate.interrupts").exists());
    let RuntimeChild::Windows(native) = fixture.spawned.child.as_mut().unwrap() else {
        panic!("native owner retained");
    };
    native.job = Some(full_access);
    fixture.assert_retained();
}

#[test]
fn graceful_spawn_missing_job_is_unknown_and_cannot_signal_or_finalize() {
    let mut fixture = Fixture::new("cooperate");
    let RuntimeChild::Windows(native) = fixture.spawned.child.as_mut().unwrap() else {
        panic!("native owner");
    };
    let job = native.job.take().unwrap();
    let result = stop_spawned_process_gracefully(&mut fixture.spawned, Duration::from_millis(150));
    assert!(
        result.is_err(),
        "a root handle alone cannot prove the complete owned tree"
    );
    assert!(fixture.root_handle.is_running().unwrap());
    assert!(!fixture.root.join("cooperate.interrupts").exists());
    let RuntimeChild::Windows(native) = fixture.spawned.child.as_mut().unwrap() else {
        panic!("native owner retained");
    };
    native.job = Some(job);
    fixture.assert_retained();
}
