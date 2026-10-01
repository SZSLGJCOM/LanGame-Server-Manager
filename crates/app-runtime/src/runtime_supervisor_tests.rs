use super::*;
use crate::test_support::unique_test_root;
use app_core::{InstanceStatus, InstanceSummary};

#[cfg(windows)]
#[test]
fn script_entrypoint_tracks_real_child_pid_and_stops_process_tree() {
    let temp_root = unique_test_root();
    std::fs::create_dir_all(&temp_root).expect("create temp root");

    let script_entrypoint_path = temp_root.join("launch-script-entrypoint.bat");
    std::fs::write(
        &script_entrypoint_path,
        "@echo off\r\nstart \"\" /B powershell -NoProfile -Command \"Start-Sleep -Seconds 20\"\r\npowershell -NoProfile -Command \"Start-Sleep -Seconds 20\"\r\n",
    )
    .expect("write script entrypoint");

    let plan = LaunchPlan {
        environment: Default::default(),
        instance_id: String::from("script-entrypoint-test"),
        instance_name: String::from("Script Entrypoint Test"),
        module_id: String::from("demo"),
        install_root: temp_root.to_string_lossy().into_owned(),
        install_state: app_core::InstallState::Installed,
        uses_private_runtime: false,
        working_directory: temp_root.to_string_lossy().into_owned(),
        executable_path: script_entrypoint_path.to_string_lossy().into_owned(),
        executable_exists: true,
        ready_to_launch: true,
        validation_issues: vec![],
        args: vec![],
        command_line: script_entrypoint_path.to_string_lossy().into_owned(),
        window_policy: ProcessWindowPolicy::Background,
        uses_script_entrypoint: true,
        requires_admin: false,
        host_surface: ProcessHostSurface::ManagedTerminal,
        host_notes: None,
        performance_policy: RuntimePerformancePolicy::default(),
        performance_preview: RuntimePerformancePolicyPreview::default(),
    };
    let log_path = temp_root.join("script-entrypoint.log");

    let mut spawned = spawn_launch_plan(&plan, &log_path).expect("spawn script entrypoint");
    let root_pid = spawned.pid;

    let startup_exit =
        stabilize_spawned_process(&plan.executable_path, &mut spawned, Duration::from_secs(3))
            .expect("stabilize spawn");
    assert_eq!(startup_exit, None);
    assert_ne!(spawned.pid, root_pid);
    assert!(process_is_running(spawned.pid).expect("tracked pid running"));

    let stop_exit = stop_spawned_process(&mut spawned).expect("stop script entrypoint tree");
    assert_eq!(stop_exit, None);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!process_is_running(spawned.pid).expect("tracked pid stopped"));

    let _ = std::fs::remove_dir_all(temp_root);
}

#[cfg(windows)]
#[test]
fn process_tree_stop_accepts_access_denied_when_root_exits_during_settle_wait() {
    let error = RuntimeProcessError::KillByPid {
        pid: 42,
        source: std::io::Error::from_raw_os_error(ERROR_ACCESS_DENIED),
    };
    let waited = std::cell::Cell::new(false);

    assert!(
        resolve_windows_process_tree_stop_result(error, || {
            waited.set(true);
            Ok(true)
        })
        .is_ok()
    );
    assert!(waited.get());
}

#[cfg(windows)]
#[test]
fn process_tree_stop_preserves_errors_while_the_root_is_still_running() {
    let error = RuntimeProcessError::KillByPid {
        pid: 42,
        source: std::io::Error::from_raw_os_error(ERROR_ACCESS_DENIED),
    };

    assert!(matches!(
        resolve_windows_process_tree_stop_result(error, || Ok(false)),
        Err(RuntimeProcessError::KillByPid { pid: 42, .. })
    ));
}

#[cfg(windows)]
#[test]
fn verified_process_tree_rejects_children_older_than_a_reused_root_pid() {
    let snapshot = vec![
        windows_process_record(20, 10),
        windows_process_record(30, 20),
    ];
    let mut inspected = Vec::new();

    let verified = collect_verified_windows_process_descendants_from_snapshot(
        10,
        &process_identity_at(100),
        &snapshot,
        |pid| {
            inspected.push(pid);
            Ok(Some(process_identity_at(match pid {
                20 => 90,
                30 => 110,
                _ => unreachable!(),
            })))
        },
    );

    assert!(verified.processes.is_empty());
    assert!(verified.inspection_errors.is_empty());
    assert_eq!(inspected, [20]);
}

#[cfg(windows)]
#[test]
fn verified_process_tree_does_not_cross_an_old_intermediate_process_edge() {
    let snapshot = vec![
        windows_process_record(20, 10),
        windows_process_record(30, 20),
        windows_process_record(40, 30),
    ];
    let mut inspected = Vec::new();

    let verified = collect_verified_windows_process_descendants_from_snapshot(
        10,
        &process_identity_at(100),
        &snapshot,
        |pid| {
            inspected.push(pid);
            Ok(Some(process_identity_at(match pid {
                20 => 120,
                30 => 110,
                40 => 130,
                _ => unreachable!(),
            })))
        },
    );

    assert_eq!(
        verified
            .processes
            .iter()
            .map(|process| process.process_id)
            .collect::<Vec<_>>(),
        [20]
    );
    assert!(verified.inspection_errors.is_empty());
    assert_eq!(inspected, [20, 30]);
}

#[cfg(windows)]
#[test]
fn verified_process_tree_stops_at_an_uninspectable_descendant() {
    let snapshot = vec![
        windows_process_record(20, 10),
        windows_process_record(30, 20),
    ];
    let mut inspected = Vec::new();

    let verified = collect_verified_windows_process_descendants_from_snapshot(
        10,
        &process_identity_at(100),
        &snapshot,
        |pid| {
            inspected.push(pid);
            Err(RuntimeProcessError::InspectProcessOutput {
                pid,
                message: String::from("injected inspection failure"),
            })
        },
    );

    assert!(verified.processes.is_empty());
    assert_eq!(verified.inspection_errors.len(), 1);
    assert_eq!(inspected, [20]);
}

#[cfg(windows)]
fn windows_process_record(process_id: u32, parent_process_id: u32) -> WindowsProcessRecord {
    WindowsProcessRecord {
        process_id,
        parent_process_id,
        name: format!("process-{process_id}.exe"),
        identity: None,
    }
}

#[cfg(windows)]
fn process_identity_at(creation_time: u64) -> ProcessIdentity {
    ProcessIdentity {
        creation_time,
        image_path: format!("C:/process-{creation_time}.exe"),
    }
}

fn spawn_test_stdin_process(run_id: i64, process_key: &str) -> ManagedProcess {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/Q", "/C", "more"]);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sh");
        command.args(["-c", "cat >/dev/null"]);
        command
    };
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn stdin sink");
    let pid = child.id();
    let process_identity = inspect_process_identity(pid)
        .expect("inspect stdin sink")
        .expect("stdin sink is running");

    ManagedProcess {
        run_id,
        process_key: process_key.to_owned(),
        display_name: String::from("Stdin sink"),
        pid,
        process_identity: process_identity.clone(),
        root_process_identity: process_identity,
        log_path: String::from("stdin-sink.log"),
        is_primary: true,
        uses_script_entrypoint: false,
        performance_policy: RuntimePerformancePolicy::default(),
        last_performance_refresh: None,
        last_performance_target_count: None,
        last_performance_application: None,
        child: Some(RuntimeChild::Standard(child)),
        hidden_desktop: None,
    }
}

fn test_running_summary(instance_id: &str) -> InstanceSummary {
    InstanceSummary {
        id: instance_id.to_owned(),
        name: String::from("Stdin Lease Test"),
        module_id: String::from("demo"),
        status: InstanceStatus::Running,
        active_process_count: 1,
        bind_ip: String::from("127.0.0.1"),
        port_count: 0,
        autostart: false,
    }
}

#[test]
fn workload_liveness_does_not_accept_a_live_launcher_for_a_stale_workload() {
    let mut root = spawn_test_stdin_process(1, "main");
    let mut workload = spawn_test_stdin_process(2, "workload");
    root.pid = workload.pid;
    root.process_identity = workload.process_identity.clone();
    root.process_identity.creation_time = root.process_identity.creation_time.saturating_sub(1);
    let pid = root.pid;
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(test_running_summary("stale-workload"), None, vec![root]);
    let result = supervisor.matches_running_process("stale-workload", 1, "main", pid);
    let mut instance = supervisor.take_running_for_stop("stale-workload").unwrap();
    terminate_test_managed_instance(&mut instance);
    let workload = workload.child.as_mut().unwrap();
    workload.terminate_owned().unwrap();
    workload.wait_code().unwrap();
    assert!(
        !result.unwrap(),
        "a live launch handle cannot prove another PID generation is running"
    );
}

#[cfg(windows)]
#[test]
fn graceful_root_exit_wait_uses_the_owned_child_handle() {
    let mut process = spawn_test_stdin_process(1, "main");
    let mut stale_identity = process.process_identity.clone();
    stale_identity.creation_time = stale_identity.creation_time.saturating_sub(1);
    let child = process.child.as_mut().expect("owned child");

    let status = runtime_supervisor::wait_for_graceful_tracked_exit(
        process.pid,
        &stale_identity,
        process.pid,
        child,
        Duration::ZERO,
    );
    child.terminate_owned().expect("clean up child");
    child.wait_code().expect("reap child");

    assert_eq!(status.expect("poll owned handle"), None);
}

#[cfg(windows)]
#[test]
fn graceful_descendant_exit_wait_still_checks_recorded_identity() {
    let mut process = spawn_test_stdin_process(1, "main");
    let mut stale_identity = process.process_identity.clone();
    stale_identity.creation_time = stale_identity.creation_time.saturating_sub(1);
    let child = process.child.as_mut().expect("owned child");

    let status = runtime_supervisor::wait_for_graceful_tracked_exit(
        process.pid,
        &stale_identity,
        std::process::id(),
        child,
        Duration::ZERO,
    );
    child.terminate_owned().expect("clean up child");
    child.wait_code().expect("reap child");

    assert_eq!(status.expect("poll recorded descendant"), Some(None));
}

#[cfg(windows)]
#[test]
fn console_interrupt_follows_the_workload_after_its_launcher_exits() {
    let mut root = spawn_test_stdin_process(1, "root");
    let mut workload = spawn_test_stdin_process(2, "workload");
    let child = root.child.as_mut().unwrap();
    let alive_target = runtime_supervisor::console_interrupt_target(
        workload.pid,
        &workload.process_identity,
        &root.process_identity,
        Some(child),
    )
    .unwrap();
    assert_eq!(alive_target, (root.pid, &root.process_identity));
    child.terminate_owned().unwrap();
    child.wait_code().unwrap();
    let exited_target = runtime_supervisor::console_interrupt_target(
        workload.pid,
        &workload.process_identity,
        &root.process_identity,
        Some(child),
    )
    .unwrap();
    let workload_child = workload.child.as_mut().unwrap();
    workload_child.terminate_owned().unwrap();
    workload_child.wait_code().unwrap();
    assert_eq!(exited_target, (workload.pid, &workload.process_identity));
}

fn terminate_test_managed_instance(instance: &mut ManagedInstance) {
    for process in &mut instance.processes {
        let Some(child) = process.child.as_mut() else {
            continue;
        };
        match child {
            RuntimeChild::Standard(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            #[cfg(windows)]
            RuntimeChild::Windows(_) => panic!("test helper only spawns standard children"),
        }
    }
}

#[test]
fn detached_process_reaping_checks_identity_before_accepting_a_live_pid() {
    for stale_identity in [false, true] {
        let mut process = spawn_test_stdin_process(1, "main");
        let mut child = process.child.take().expect("retain cleanup ownership");
        if stale_identity {
            process.process_identity.creation_time =
                process.process_identity.creation_time.saturating_sub(1);
        }
        let instance_id = "detached-process-identity";
        let mut supervisor = RuntimeSupervisor::default();
        supervisor.insert_running(test_running_summary(instance_id), None, vec![process]);

        let result = supervisor.reap_exited();
        let unrelated_process_survived = child.try_wait().expect("poll retained child").is_none();
        child.terminate_owned().expect("terminate retained child");
        child.wait_code().expect("reap retained child");

        assert!(unrelated_process_survived);
        assert_eq!(
            result.expect("reap stale registration").len(),
            usize::from(stale_identity)
        );
        assert_eq!(supervisor.is_tracked(instance_id), !stale_identity);
    }
}

#[cfg(windows)]
#[test]
fn startup_handoff_rejects_a_reused_workload_pid() {
    let mut root = spawn_test_stdin_process(1, "root");
    let mut root_child = root.child.take().expect("retain root ownership");
    root_child.terminate_owned().expect("terminate root");
    let root_exit = root_child.wait_code().expect("reap root");
    let mut workload = spawn_test_stdin_process(2, "workload");
    let mut workload_child = workload.child.take().expect("retain workload ownership");
    let mut stale_identity = workload.process_identity.clone();
    stale_identity.creation_time = stale_identity.creation_time.saturating_sub(1);
    let mut spawned = SpawnedProcess {
        child: Some(root_child),
        pid: workload.pid,
        process_identity: stale_identity,
        root_process_identity: root.process_identity,
        log_path: String::new(),
        uses_script_entrypoint: false,
        #[cfg(windows)]
        requires_workload_handoff: false,
        hidden_desktop: None,
    };

    let result = stabilize_spawned_process("unused.exe", &mut spawned, Duration::ZERO);
    let unrelated_process_survived = workload_child.try_wait().expect("poll workload").is_none();
    workload_child
        .terminate_owned()
        .expect("terminate workload");
    workload_child.wait_code().expect("reap workload");

    assert!(unrelated_process_survived);
    assert_eq!(result.expect("stabilize stale workload"), root_exit);
}

#[test]
fn runtime_command_dispatch_lease_rejects_overlapping_dispatch() {
    let instance_id = "stdin-lease-exclusive";
    let process = spawn_test_stdin_process(1, "main");
    let pid = process.pid;
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(test_running_summary(instance_id), None, vec![process]);

    let lease = supervisor
        .begin_command_dispatch(instance_id, Some("main"))
        .expect("begin first dispatch");
    let overlapping = supervisor.begin_command_dispatch(instance_id, Some("main"));
    supervisor.finish_command_dispatch(lease);

    let mut instance = supervisor
        .take_running_for_stop(instance_id)
        .expect("take test instance for cleanup");
    terminate_test_managed_instance(&mut instance);

    assert!(matches!(
        overlapping,
        Err(RuntimeProcessError::MissingTrackedProcessStdin { pid: error_pid })
            if error_pid == pid
    ));
}

#[test]
fn runtime_command_dispatch_lease_rejects_an_unknown_explicit_process() {
    let instance_id = "stdin-lease-explicit-process";
    let process = spawn_test_stdin_process(1, "main");
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(test_running_summary(instance_id), None, vec![process]);

    let result = supervisor.begin_command_dispatch(instance_id, Some("missing"));
    if let Ok(lease) = result.as_ref() {
        panic!(
            "unknown explicit process unexpectedly selected `{}`",
            lease.target().process_key
        );
    }

    let mut instance = supervisor
        .take_running_for_stop(instance_id)
        .expect("take test instance for cleanup");
    terminate_test_managed_instance(&mut instance);

    assert!(matches!(
        result,
        Err(RuntimeProcessError::TrackedProcessNotFound {
            instance_id: ref error_instance,
            process_key: ref error_process,
        }) if error_instance == instance_id && error_process == "missing"
    ));
}

#[test]
fn runtime_command_dispatch_lease_restores_matching_process_stdin() {
    let instance_id = "stdin-lease-restore";
    let process = spawn_test_stdin_process(1, "main");
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(test_running_summary(instance_id), None, vec![process]);

    let lease = supervisor
        .begin_command_dispatch(instance_id, Some("main"))
        .expect("begin first dispatch");
    supervisor.finish_command_dispatch(lease);

    let restored_outcome = match supervisor.begin_command_dispatch(instance_id, Some("main")) {
        Ok(mut restored) => {
            let outcome = restored
                .write_stdin_line("lease-restored")
                .map_err(|error| error.to_string());
            supervisor.finish_command_dispatch(restored);
            outcome
        }
        Err(error) => Err(error.to_string()),
    };

    let mut instance = supervisor
        .take_running_for_stop(instance_id)
        .expect("take test instance for cleanup");
    terminate_test_managed_instance(&mut instance);

    assert_eq!(restored_outcome, Ok(()));
}

#[test]
fn stale_runtime_command_dispatch_lease_does_not_restore_into_replacement_process() {
    struct ReplacementIdentityCase {
        name: &'static str,
        run_id: i64,
        reuse_old_pid: bool,
        process_key: &'static str,
    }

    let cases = [
        ReplacementIdentityCase {
            name: "run-id",
            run_id: 2,
            reuse_old_pid: true,
            process_key: "main",
        },
        ReplacementIdentityCase {
            name: "pid",
            run_id: 1,
            reuse_old_pid: false,
            process_key: "main",
        },
        ReplacementIdentityCase {
            name: "process-key",
            run_id: 1,
            reuse_old_pid: true,
            process_key: "replacement",
        },
    ];

    for case in cases {
        let instance_id = format!("stdin-lease-replacement-{}", case.name);
        let old_process = spawn_test_stdin_process(1, "main");
        let old_pid = old_process.pid;
        let mut supervisor = RuntimeSupervisor::default();
        supervisor.insert_running(test_running_summary(&instance_id), None, vec![old_process]);

        let stale_lease = supervisor
            .begin_command_dispatch(&instance_id, None)
            .expect("begin old process dispatch");
        let mut old_instance = supervisor
            .take_running_for_stop(&instance_id)
            .expect("detach old process");

        let mut replacement = spawn_test_stdin_process(case.run_id, case.process_key);
        if case.reuse_old_pid {
            replacement.pid = old_pid;
        }
        let replacement_pid = replacement.pid;
        supervisor.insert_running(test_running_summary(&instance_id), None, vec![replacement]);
        let replacement_lease = supervisor
            .begin_command_dispatch(&instance_id, None)
            .expect("begin replacement process dispatch");

        supervisor.finish_command_dispatch(stale_lease);
        let overlapping = supervisor.begin_command_dispatch(&instance_id, None);
        let stale_restore_was_rejected = match overlapping {
            Err(RuntimeProcessError::MissingTrackedProcessStdin { pid }) => pid == replacement_pid,
            Err(_) => false,
            Ok(unexpected_lease) => {
                supervisor.finish_command_dispatch(unexpected_lease);
                false
            }
        };

        supervisor.finish_command_dispatch(replacement_lease);
        let restored_outcome = match supervisor.begin_command_dispatch(&instance_id, None) {
            Ok(mut restored) => {
                let outcome = restored
                    .write_stdin_line("replacement-lease-restored")
                    .map_err(|error| error.to_string());
                supervisor.finish_command_dispatch(restored);
                outcome
            }
            Err(error) => Err(error.to_string()),
        };

        let mut replacement_instance = supervisor
            .take_running_for_stop(&instance_id)
            .expect("take replacement process for cleanup");
        terminate_test_managed_instance(&mut replacement_instance);
        terminate_test_managed_instance(&mut old_instance);

        assert!(
            stale_restore_was_rejected,
            "{} mismatch accepted a stale dispatch lease",
            case.name
        );
        assert_eq!(
            restored_outcome,
            Ok(()),
            "{} replacement did not recover its own stdin",
            case.name
        );
    }
}

#[test]
fn runtime_supervisor_restores_ownership_after_a_failed_stop_attempt() {
    let summary = InstanceSummary {
        id: String::from("stop-recovery"),
        name: String::from("Stop Recovery"),
        module_id: String::from("demo"),
        status: InstanceStatus::Running,
        active_process_count: 1,
        bind_ip: String::from("127.0.0.1"),
        port_count: 0,
        autostart: false,
    };
    let process = ManagedProcess {
        run_id: 1,
        process_key: String::from("main"),
        display_name: String::from("Server"),
        pid: std::process::id(),
        process_identity: inspect_process_identity(std::process::id())
            .expect("inspect test process")
            .expect("test process is running"),
        root_process_identity: inspect_process_identity(std::process::id())
            .expect("inspect test process")
            .expect("test process is running"),
        log_path: String::from("runtime.log"),
        is_primary: true,
        uses_script_entrypoint: false,
        performance_policy: RuntimePerformancePolicy::default(),
        last_performance_refresh: None,
        last_performance_target_count: None,
        last_performance_application: None,
        child: None,
        hidden_desktop: None,
    };
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(summary, Some(String::from("session-1")), vec![process]);

    let detached = supervisor
        .take_running_for_stop("stop-recovery")
        .expect("detach managed instance");
    assert!(!supervisor.is_tracked("stop-recovery"));
    assert!(supervisor.restore_running_after_failed_stop(detached));
    assert!(supervisor.is_tracked("stop-recovery"));
}
