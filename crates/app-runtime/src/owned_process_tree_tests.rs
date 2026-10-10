use super::*;
use std::time::{Duration, Instant};

const PROBE_TEST: &str = "owned_process_tree_tests::process_tree_probe";
const PROBE_DIRECTORY: &str = "LGSM_PROCESS_TREE_PROBE_DIRECTORY";
const PROBE_ROLE: &str = "LGSM_PROCESS_TREE_PROBE_ROLE";

#[path = "owned_process_tree_inspection_tests.rs"]
mod inspection_tests;

#[test]
#[ignore = "internal child-process probe; exercised by owned process tree tests"]
fn process_tree_probe() {
    let root = PathBuf::from(std::env::var_os(PROBE_DIRECTORY).expect("probe directory"));
    let role = std::env::var(PROBE_ROLE).expect("probe role");
    if role == "launcher" {
        let mut children = Vec::new();
        for index in 0..2 {
            children.push(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--ignored", "--exact", PROBE_TEST, "--nocapture"])
                    .env(PROBE_ROLE, format!("leaf-{index}"))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn leaf"),
            );
        }
        fs::write(
            root.join("children"),
            format!("{}\n{}\n", children[0].id(), children[1].id()),
        )
        .unwrap();
        wait_until(
            || root.join("exit-launcher").exists(),
            Duration::from_secs(20),
        );
        // Deliberately hand off to two surviving children. The launch owner,
        // not this short-lived launcher, is responsible for both of them.
    } else {
        let grandchild = if role == "leaf-0" {
            Some(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--ignored", "--exact", PROBE_TEST, "--nocapture"])
                    .env(PROBE_ROLE, "grandchild")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn grandchild"),
            )
        } else {
            None
        };
        fs::write(root.join(&role), std::process::id().to_string()).unwrap();
        wait_until(
            || root.join("release-leaves").exists(),
            Duration::from_secs(25),
        );
        if let Some(mut grandchild) = grandchild {
            grandchild.wait().expect("wait for released grandchild");
        }
    }
}

fn wait_until(mut ready: impl FnMut() -> bool, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct LaunchFixture {
    root: PathBuf,
    spawned: Option<SpawnedProcess>,
    handles: Vec<WindowsProcessHandle>,
}

impl LaunchFixture {
    fn new(surface: ProcessHostSurface) -> Self {
        let root = test_support::unique_test_root();
        fs::create_dir(&root).unwrap();
        let executable = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let plan = LaunchPlan {
            environment: BTreeMap::from([
                (PROBE_DIRECTORY.into(), root.to_string_lossy().into_owned()),
                (PROBE_ROLE.into(), "launcher".into()),
            ]),
            instance_id: "owned-tree-probe".into(),
            instance_name: "Owned tree probe".into(),
            module_id: "demo".into(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: app_core::InstallState::Installed,
            uses_private_runtime: false,
            working_directory: root.to_string_lossy().into_owned(),
            executable_path: executable.clone(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: vec![],
            args: vec![
                "--ignored".into(),
                "--exact".into(),
                PROBE_TEST.into(),
                "--nocapture".into(),
            ],
            command_line: executable,
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: surface,
            host_notes: None,
            performance_policy: RuntimePerformancePolicy::default(),
            performance_preview: RuntimePerformancePolicyPreview::default(),
        };
        let spawned = spawn_launch_plan(&plan, root.join("console.log")).unwrap();
        let root_handle = WindowsProcessHandle::open(spawned.pid, PROCESS_TERMINATE)
            .unwrap()
            .expect("root is alive");
        let mut fixture = Self {
            root,
            spawned: Some(spawned),
            handles: vec![root_handle],
        };
        assert!(
            wait_until(
                || fixture.root.join("leaf-0").exists()
                    && fixture.root.join("leaf-1").exists()
                    && fs::read_to_string(fixture.root.join("grandchild"))
                        .ok()
                        .is_some_and(|pid| pid.parse::<u32>().is_ok())
                    && fs::read_to_string(fixture.root.join("children"))
                        .ok()
                        .is_some_and(|pids| pids.lines().count() == 2
                            && pids.lines().all(|pid| pid.parse::<u32>().is_ok())),
                Duration::from_secs(10),
            ),
            "both child probes must execute"
        );
        for pid in fs::read_to_string(fixture.root.join("children"))
            .unwrap()
            .lines()
        {
            fixture.handles.push(
                WindowsProcessHandle::open(pid.parse().unwrap(), PROCESS_TERMINATE)
                    .unwrap()
                    .expect("leaf is alive"),
            );
        }
        let grandchild = fs::read_to_string(fixture.root.join("grandchild"))
            .unwrap()
            .parse()
            .unwrap();
        fixture.handles.push(
            WindowsProcessHandle::open(grandchild, PROCESS_TERMINATE)
                .unwrap()
                .expect("grandchild is alive"),
        );
        fixture
    }

    fn hand_off(&mut self) {
        let spawned = self.spawned.as_mut().unwrap();
        spawned.pid = self.handles[1].pid;
        spawned.process_identity = self.handles[1].identity().unwrap();
        fs::write(self.root.join("exit-launcher"), b"exit").unwrap();
        assert!(self.handles[0].wait_for_exit(5000).unwrap());
        assert_eq!(
            stabilize_spawned_process("probe.exe", spawned, Duration::ZERO).unwrap(),
            None,
        );
        assert!(
            spawned
                .child
                .as_ref()
                .is_some_and(RuntimeChild::has_owned_process_tree),
            "handoff must retain the kernel owner while its workload runs"
        );
        assert!(
            self.handles[1..]
                .iter()
                .all(|handle| handle.is_running().unwrap()),
            "handoff must retain the launch owner without killing live workloads",
        );
    }

    fn register(&mut self) -> RuntimeSupervisor {
        let spawned = self.spawned.take().unwrap();
        let mut supervisor = RuntimeSupervisor::default();
        supervisor.insert_running(
            app_core::InstanceSummary {
                id: "owned-tree-probe".into(),
                name: "Owned tree probe".into(),
                module_id: "demo".into(),
                status: app_core::InstanceStatus::Running,
                active_process_count: 1,
                bind_ip: "127.0.0.1".into(),
                port_count: 0,
                autostart: false,
            },
            None,
            vec![ManagedProcess {
                run_id: 1,
                process_key: "main".into(),
                display_name: "Probe".into(),
                pid: spawned.pid,
                process_identity: spawned.process_identity,
                root_process_identity: spawned.root_process_identity,
                log_path: spawned.log_path,
                is_primary: true,
                uses_script_entrypoint: false,
                performance_policy: RuntimePerformancePolicy::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: spawned.child,
                hidden_desktop: spawned.hidden_desktop,
            }],
        );
        supervisor
    }

    fn leaves_stopped(&self) -> bool {
        self.handles[1..]
            .iter()
            .all(|handle| handle.wait_for_exit(1500).unwrap())
    }
}

impl Drop for LaunchFixture {
    fn drop(&mut self) {
        let _ = fs::write(self.root.join("release-leaves"), b"exit");
        // Retained handles make failure cleanup independent of PID reuse and
        // ensure this regression does not rely on the outer Cargo Job cleanup.
        for handle in &self.handles {
            if handle.is_running().unwrap_or(true) {
                let _ = handle.terminate();
            }
            let _ = handle.wait_for_exit(1500);
        }
        self.spawned.take();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn completed_tree_cleanup_deadline_confirms_already_exited_owned_handles() {
    let mut fixture = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    assert_eq!(
        fixture
            .spawned
            .as_mut()
            .unwrap()
            .child
            .as_mut()
            .unwrap()
            .owned_process_tree_is_running()
            .unwrap(),
        Some(true),
        "capture the owned root and descendants before they exit"
    );
    fs::write(fixture.root.join("exit-launcher"), b"exit").unwrap();
    fs::write(fixture.root.join("release-leaves"), b"exit").unwrap();
    assert!(fixture.handles[0].wait_for_exit(5000).unwrap());
    assert!(fixture.leaves_stopped());
    let spawned = fixture.spawned.as_mut().unwrap();
    let RuntimeChild::Windows(child) = spawned.child.as_mut().unwrap() else {
        panic!("native launch owner required");
    };
    let job = child.job.as_ref().unwrap();
    assert!(wait_until(
        || job.active_process_count().unwrap() == 0,
        Duration::from_secs(5),
    ));
    // Model a worker resuming after its deadline without depending on scheduler
    // timing. Completion is a kernel fact; an expired wait budget cannot undo it.
    job.terminate_and_wait_until(Instant::now() - Duration::from_secs(1))
        .unwrap();
}

#[test]
fn live_tree_cleanup_deadline_preserves_every_owned_process() {
    let mut fixture = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    let spawned = fixture.spawned.as_mut().unwrap();
    let RuntimeChild::Windows(child) = spawned.child.as_mut().unwrap() else {
        panic!("native launch owner required");
    };
    let error = child
        .job
        .as_ref()
        .unwrap()
        .terminate_and_wait_until(Instant::now() - Duration::from_secs(1))
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("phase=cleanup.initial_capture"));
    assert!(
        fixture
            .handles
            .iter()
            .all(|handle| handle.is_running().unwrap()),
        "a timed-out unfinished operation retains its owner and never reports success"
    );
}

#[test]
fn stopping_handed_off_launch_reaps_siblings_without_stopping_another_launch() {
    let mut first = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    let second = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    first.hand_off();
    stop_spawned_process(first.spawned.as_mut().unwrap()).unwrap();
    let all_stopped = first.leaves_stopped();
    let other_alive = second
        .handles
        .iter()
        .all(|handle| handle.is_running().unwrap());
    drop(first);
    drop(second);
    assert!(
        all_stopped,
        "stopping a workload must also stop its surviving sibling"
    );
    assert!(
        other_alive,
        "a launch must never stop another launch or the outer Job"
    );
}

#[test]
fn dropping_launch_owner_reaps_ordinary_descendants() {
    let mut fixture = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    fixture.spawned.take();
    let root_stopped = fixture.handles[0].wait_for_exit(1500).unwrap();
    let leaves_stopped = fixture.leaves_stopped();
    drop(fixture);
    assert!(
        root_stopped && leaves_stopped,
        "launch Drop must release its complete owned tree"
    );
}

#[test]
fn stopping_handed_off_pseudo_console_reaps_surviving_siblings() {
    let mut fixture = LaunchFixture::new(ProcessHostSurface::ManagedPseudoConsole);
    fixture.hand_off();
    stop_spawned_process(fixture.spawned.as_mut().unwrap()).unwrap();
    let leaves_stopped = fixture.leaves_stopped();
    drop(fixture);
    assert!(
        leaves_stopped,
        "ConPTY handoff must preserve ownership of every descendant"
    );
}

#[test]
fn reaping_handoff_retains_siblings_until_the_entire_tree_exits() {
    let mut fixture = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    fixture.hand_off();
    let mut supervisor = fixture.register();
    assert!(supervisor.reap_exited().unwrap().is_empty());
    assert!(
        supervisor
            .matches_running_process("owned-tree-probe", 1, "main", fixture.handles[1].pid)
            .unwrap(),
        "workload liveness must not be inferred from its exited launcher",
    );
    assert!(fixture.handles[2].is_running().unwrap());
    fixture.handles[1].terminate().unwrap();
    assert!(fixture.handles[1].wait_for_exit(1500).unwrap());
    assert!(supervisor.reap_exited().unwrap().is_empty());
    assert!(supervisor.is_tracked("owned-tree-probe"));
    assert!(
        fixture.handles[2..]
            .iter()
            .all(|handle| handle.is_running().unwrap())
    );
    fs::write(fixture.root.join("release-leaves"), b"exit").unwrap();
    assert!(fixture.leaves_stopped());
    assert!(wait_until(
        || supervisor
            .instance_process_tree_is_running("owned-tree-probe")
            .unwrap()
            == Some(false),
        Duration::from_secs(5),
    ));
    let exited = supervisor.reap_exited().unwrap();
    assert_eq!(exited.len(), 1);
    assert!(!supervisor.is_tracked("owned-tree-probe"));
    assert!(
        fixture.leaves_stopped(),
        "reaping must report exit only after its siblings exit themselves"
    );
}

#[test]
fn reaping_direct_launcher_exit_waits_for_its_remaining_children() {
    let mut fixture = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    fs::write(fixture.root.join("exit-launcher"), b"exit").unwrap();
    assert!(fixture.handles[0].wait_for_exit(5000).unwrap());
    let mut supervisor = fixture.register();
    assert!(
        supervisor
            .reap_exited_for(&std::collections::HashSet::new())
            .unwrap()
            .is_empty()
    );
    assert!(
        supervisor.is_tracked("owned-tree-probe"),
        "a stop-owned instance must remain available to its lifecycle owner"
    );
    assert!(supervisor.reap_exited().unwrap().is_empty());
    assert!(supervisor.is_tracked("owned-tree-probe"));
    assert!(
        fixture.handles[1..]
            .iter()
            .all(|handle| handle.is_running().unwrap())
    );
    fs::write(fixture.root.join("release-leaves"), b"exit").unwrap();
    assert!(fixture.leaves_stopped());
    assert!(wait_until(
        || supervisor
            .instance_process_tree_is_running("owned-tree-probe")
            .unwrap()
            == Some(false),
        Duration::from_secs(5),
    ));
    let exited = supervisor.reap_exited().unwrap();
    assert_eq!(exited.len(), 1);
    assert_eq!(exited[0].exit_code, Some(0));
    assert!(
        fixture.handles[1..]
            .iter()
            .all(|handle| handle.wait_for_exit(0).unwrap()),
        "reap must confirm every member exited before releasing its owner"
    );
}

#[test]
fn failed_tree_cleanup_keeps_the_launch_owned_until_a_successful_retry() {
    for reap in [false, true] {
        let mut fixture = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
        fs::write(fixture.root.join("exit-launcher"), b"exit").unwrap();
        assert!(fixture.handles[0].wait_for_exit(5000).unwrap());
        let mut supervisor = fixture.register();
        let mut managed = supervisor
            .take_running_for_stop("owned-tree-probe")
            .unwrap();
        let RuntimeChild::Windows(child) = managed.processes[0].child.as_mut().unwrap() else {
            panic!("native launch owner required");
        };
        let full_access = if reap {
            // Natural reaping only queries the tree; deny that query rather
            // than expecting it to attempt an unauthorized termination.
            windows_process_job::restrict_job_to_terminate_for_test(child.job.as_mut().unwrap())
                .unwrap()
        } else {
            windows_process_job::restrict_job_to_query_for_test(child.job.as_mut().unwrap())
                .unwrap()
        };
        let error = if reap {
            assert!(supervisor.restore_running_after_failed_stop(managed));
            let error = supervisor.reap_exited().unwrap_err();
            assert!(supervisor.is_tracked("owned-tree-probe"));
            managed = supervisor
                .take_running_for_stop("owned-tree-probe")
                .unwrap();
            error
        } else {
            stop_managed_instance(&mut managed).unwrap_err()
        };
        assert!(
            matches!(error, RuntimeProcessError::WaitTrackedProcess { source, .. }
            if source.raw_os_error() == Some(5))
        );
        assert!(
            fixture.handles[1..]
                .iter()
                .all(|handle| handle.is_running().unwrap())
        );
        let RuntimeChild::Windows(child) = managed.processes[0]
            .child
            .as_mut()
            .expect("cleanup failure must retain the launch owner")
        else {
            panic!("native launch owner required");
        };
        child.job.replace(full_access);
        if reap {
            assert!(supervisor.restore_running_after_failed_stop(managed));
            assert!(supervisor.reap_exited().unwrap().is_empty());
            assert!(
                fixture.handles[1..]
                    .iter()
                    .all(|handle| handle.is_running().unwrap())
            );
            fs::write(fixture.root.join("release-leaves"), b"exit").unwrap();
            assert!(fixture.leaves_stopped());
            assert!(wait_until(
                || supervisor
                    .instance_process_tree_is_running("owned-tree-probe")
                    .unwrap()
                    == Some(false),
                Duration::from_secs(5),
            ));
            assert_eq!(supervisor.reap_exited().unwrap().len(), 1);
        } else {
            assert_eq!(stop_managed_instance(&mut managed).unwrap().len(), 1);
        }
        assert!(
            fixture.handles[1..]
                .iter()
                .all(|handle| handle.wait_for_exit(0).unwrap())
        );
    }
}

#[test]
fn a_later_cleanup_failure_does_not_lose_earlier_exit_events() {
    let mut first = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    let mut second = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    for fixture in [&first, &second] {
        fs::write(fixture.root.join("exit-launcher"), b"exit").unwrap();
        assert!(fixture.handles[0].wait_for_exit(5000).unwrap());
        fs::write(fixture.root.join("release-leaves"), b"exit").unwrap();
        assert!(fixture.leaves_stopped());
    }
    let mut supervisor = first.register();
    let mut other = second
        .register()
        .take_running_for_stop("owned-tree-probe")
        .unwrap();
    other.summary.id = "second-owned-tree-probe".into();
    assert!(supervisor.restore_running_after_failed_stop(other));
    for id in ["owned-tree-probe", "second-owned-tree-probe"] {
        assert!(wait_until(
            || supervisor.instance_process_tree_is_running(id).unwrap() == Some(false),
            Duration::from_secs(5),
        ));
    }
    let failed_id = supervisor
        .tracked_instances()
        .last()
        .unwrap()
        .summary
        .id
        .clone();
    let mut failed = supervisor.take_running_for_stop(&failed_id).unwrap();
    let RuntimeChild::Windows(child) = failed.processes[0].child.as_mut().unwrap() else {
        panic!("native launch owner required");
    };
    let full_access =
        windows_process_job::restrict_job_to_terminate_for_test(child.job.as_mut().unwrap())
            .unwrap();
    assert!(supervisor.restore_running_after_failed_stop(failed));
    assert_eq!(
        supervisor.tracked_instances().last().unwrap().summary.id,
        failed_id
    );
    assert!(supervisor.reap_exited().is_err());
    assert_eq!(
        supervisor.tracked_instances().len(),
        2,
        "both exit events remain owned for retry"
    );
    let mut failed = supervisor.take_running_for_stop(&failed_id).unwrap();
    let RuntimeChild::Windows(child) = failed.processes[0].child.as_mut().unwrap() else {
        panic!("native launch owner required");
    };
    child.job = Some(full_access);
    assert!(supervisor.restore_running_after_failed_stop(failed));
    assert_eq!(supervisor.reap_exited().unwrap().len(), 2);
    assert!(supervisor.tracked_instances().is_empty());
}
