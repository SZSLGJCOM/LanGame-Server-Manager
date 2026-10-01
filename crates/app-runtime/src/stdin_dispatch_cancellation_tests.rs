use super::*;
use crate::*;
use std::io::ErrorKind;
use std::os::windows::process::CommandExt;
use std::sync::mpsc;
use std::time::Instant;

struct FixtureCleanup(WindowsProcessHandle);

struct WriterThread<'a> {
    thread: Option<std::thread::JoinHandle<()>>,
    process: &'a FixtureCleanup,
}

impl Drop for WriterThread<'_> {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // Release the inherited read end before joining on every failure path.
            let _ = self.process.0.terminate();
            let _ = thread.join();
        }
    }
}

impl Drop for FixtureCleanup {
    fn drop(&mut self) {
        if self.0.is_running().unwrap_or(true) {
            let _ = self.0.terminate();
        }
        let _ = self.0.wait_for_exit(3000);
    }
}

fn stdin_process() -> (ManagedProcess, FixtureCleanup) {
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "stdin_write_tests::ordinary_stdin_child_fixture",
            "--nocapture",
        ])
        .env("LGSM_STDIN_PIPE_CHILD", "unread")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .unwrap();
    let pid = child.id();
    let cleanup = FixtureCleanup(
        WindowsProcessHandle::open(pid, PROCESS_TERMINATE)
            .unwrap()
            .unwrap(),
    );
    let identity = cleanup.0.identity().unwrap();
    (
        ManagedProcess {
            run_id: 1,
            process_key: "main".into(),
            display_name: "Controlled unread stdin".into(),
            pid,
            process_identity: identity.clone(),
            root_process_identity: identity,
            log_path: "controlled-stdin.log".into(),
            is_primary: true,
            uses_script_entrypoint: false,
            performance_policy: RuntimePerformancePolicy::default(),
            last_performance_refresh: None,
            last_performance_target_count: None,
            last_performance_application: None,
            child: Some(RuntimeChild::Standard(child)),
            hidden_desktop: None,
        },
        cleanup,
    )
}

fn running_summary() -> app_core::InstanceSummary {
    app_core::InstanceSummary {
        id: "stdin-cancellation".into(),
        name: "Stdin cancellation".into(),
        module_id: "demo".into(),
        status: app_core::InstanceStatus::Running,
        active_process_count: 1,
        bind_ip: "127.0.0.1".into(),
        port_count: 0,
        autostart: false,
    }
}

fn write_error(lease: &mut RuntimeCommandDispatchLease) -> ErrorKind {
    match lease.write_stdin_line("cancelled") {
        Err(RuntimeProcessError::WriteTrackedProcessStdin { source, .. }) => source.kind(),
        result => panic!("expected cancelled write, got {result:?}"),
    }
}

#[test]
fn pending_stdin_observation_reclaims_returned_writers_and_rejects_another_run() {
    let (process, _cleanup) = stdin_process();
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(running_summary(), None, vec![process]);
    assert!(
        !supervisor
            .has_pending_command_dispatches("stdin-cancellation", 1)
            .unwrap()
    );
    let lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    assert!(
        supervisor
            .has_pending_command_dispatches("stdin-cancellation", 1)
            .unwrap()
    );
    assert!(matches!(
        supervisor.has_pending_command_dispatches("stdin-cancellation", 2),
        Err(RuntimeProcessError::TrackedInstanceRunChanged {
            expected_run_id: 2,
            ..
        })
    ));
    // Return through the owned channel without calling finish: observation
    // itself must reclaim the exact writer before the next stop command.
    assert_eq!(lease.return_stdin(), "stdin-cancellation");
    assert!(
        !supervisor
            .has_pending_command_dispatches("stdin-cancellation", 1)
            .unwrap()
    );
    let next = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    supervisor.finish_command_dispatch(next);
    assert!(
        !supervisor
            .has_pending_command_dispatches("absent-instance", 1)
            .unwrap()
    );
}

#[test]
fn taking_instance_for_stop_cancels_its_stdin_lease_before_killing_the_process() {
    let (process, cleanup) = stdin_process();
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(running_summary(), None, vec![process]);
    let mut lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    let stopped = supervisor
        .take_running_for_stop("stdin-cancellation")
        .unwrap();

    assert!(cleanup.0.is_running().unwrap());
    assert_eq!(write_error(&mut lease), ErrorKind::Interrupted);
    drop(stopped);
}

#[test]
fn dropping_supervisor_cancels_a_lease_owned_by_a_writer() {
    let (process, cleanup) = stdin_process();
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(running_summary(), None, vec![process]);
    let mut lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    drop(supervisor);

    assert!(cleanup.0.is_running().unwrap());
    assert_eq!(write_error(&mut lease), ErrorKind::Interrupted);
}

#[test]
fn workload_pid_handoff_preserves_stdin_for_the_same_process_owner() {
    let (process, cleanup) = stdin_process();
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(running_summary(), None, vec![process]);
    let mut lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    lease.write_stdin_line("before handoff").unwrap();
    // Reaping a script launcher can promote its workload PID while retaining the
    // same child handle, Job and stdin. Model that identity transition directly.
    supervisor
        .instances
        .get_mut("stdin-cancellation")
        .unwrap()
        .processes[0]
        .pid = cleanup.0.pid.saturating_add(1);
    supervisor.finish_command_dispatch(lease);

    let mut after_handoff = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    after_handoff.write_stdin_line("after handoff").unwrap();
    supervisor.finish_command_dispatch(after_handoff);
}

#[test]
fn stale_stdin_completion_cannot_remove_a_replacement_with_the_same_identifiers() {
    let (process, _first_cleanup) = stdin_process();
    let pid = process.pid;
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(running_summary(), None, vec![process]);
    let mut old_lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    let (mut replacement, _second_cleanup) = stdin_process();
    // Reusing every externally supplied identifier still cannot transfer lease ownership.
    replacement.pid = pid;
    supervisor.insert_running(running_summary(), None, vec![replacement]);
    let mut current_lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();

    assert_eq!(write_error(&mut old_lease), ErrorKind::Interrupted);
    supervisor.finish_command_dispatch(old_lease);
    current_lease
        .write_stdin_line("replacement remains current")
        .unwrap();
    let stopped = supervisor
        .take_running_for_stop("stdin-cancellation")
        .unwrap();
    assert_eq!(write_error(&mut current_lease), ErrorKind::Interrupted);
    drop(stopped);
}

#[test]
fn stopping_one_instance_interrupts_a_blocked_writer_without_waiting_for_its_deadline() {
    let (process, cleanup) = stdin_process();
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(running_summary(), None, vec![process]);
    let (other_process, _other_cleanup) = stdin_process();
    let mut other_summary = running_summary();
    other_summary.id = "unaffected-stdin".into();
    supervisor.insert_running(other_summary, None, vec![other_process]);
    let mut unaffected = supervisor
        .begin_command_dispatch("unaffected-stdin", None)
        .unwrap();
    let mut lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let command = "x".repeat(1024 * 1024);
        started_tx.send(()).unwrap();
        let result = match lease.write_stdin_line(&command) {
            Err(RuntimeProcessError::WriteTrackedProcessStdin { source, .. }) => Err(source.kind()),
            Ok(()) => Ok(()),
            Err(error) => panic!("unexpected dispatch error: {error}"),
        };
        let _ = completed_tx.send((lease, result));
    });
    let mut writer = WriterThread {
        thread: Some(thread),
        process: &cleanup,
    };
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let started = Instant::now();
    let stopped = supervisor
        .take_running_for_stop("stdin-cancellation")
        .unwrap();
    let completed = completed_rx.recv_timeout(Duration::from_secs(1));
    if completed.is_err() {
        cleanup.0.terminate().unwrap();
    }
    writer.thread.take().unwrap().join().unwrap();
    let (lease, result) = completed.expect("stop should promptly cancel the write");
    assert_eq!(result, Err(ErrorKind::Interrupted));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(cleanup.0.is_running().unwrap());
    unaffected.write_stdin_line("unaffected command").unwrap();
    supervisor.finish_command_dispatch(unaffected);
    supervisor.finish_command_dispatch(lease);
    drop(stopped);
}

#[test]
fn reaping_an_exited_process_cancels_its_outstanding_stdin_lease() {
    let (process, cleanup) = stdin_process();
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(running_summary(), None, vec![process]);
    let mut lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    cleanup.0.terminate().unwrap();
    assert!(cleanup.0.wait_for_exit(3000).unwrap());
    assert_eq!(supervisor.reap_exited().unwrap().len(), 1);
    assert_eq!(write_error(&mut lease), ErrorKind::Interrupted);
}

fn failed_stop_returns_healthy_stdin(finish_before_restore: bool) {
    let (process, _cleanup) = stdin_process();
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(running_summary(), None, vec![process]);
    let mut lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    lease
        .write_stdin_line("fully accepted before stop")
        .unwrap();
    let detached = supervisor
        .take_running_for_stop("stdin-cancellation")
        .unwrap();
    assert_eq!(write_error(&mut lease), ErrorKind::Interrupted);
    if finish_before_restore {
        supervisor.finish_command_dispatch(lease);
        assert!(supervisor.restore_running_after_failed_stop(detached));
    } else {
        assert!(supervisor.restore_running_after_failed_stop(detached));
        supervisor.finish_command_dispatch(lease);
    }
    let mut restored = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .expect("a failed stop must return input that accepted no cancelled command bytes");
    restored.write_stdin_line("after failed stop").unwrap();
    supervisor.finish_command_dispatch(restored);
}

#[test]
fn failed_stop_recovers_stdin_returned_before_instance_restoration() {
    failed_stop_returns_healthy_stdin(true);
}

#[test]
fn failed_stop_recovers_stdin_returned_after_instance_restoration() {
    failed_stop_returns_healthy_stdin(false);
}

#[test]
fn failed_stop_never_recovers_a_pipe_closed_after_a_partial_command() {
    let (process, _cleanup) = stdin_process();
    let mut supervisor = RuntimeSupervisor::default();
    supervisor.insert_running(running_summary(), None, vec![process]);
    let mut lease = supervisor
        .begin_command_dispatch("stdin-cancellation", None)
        .unwrap();
    assert!(matches!(
        lease.write_stdin_line(&"x".repeat(1024 * 1024)),
        Err(RuntimeProcessError::WriteTrackedProcessStdin { source, .. })
            if source.kind() == ErrorKind::TimedOut
    ));
    let detached = supervisor
        .take_running_for_stop("stdin-cancellation")
        .unwrap();
    supervisor.finish_command_dispatch(lease);
    assert!(supervisor.restore_running_after_failed_stop(detached));
    assert!(matches!(
        supervisor.begin_command_dispatch("stdin-cancellation", None),
        Err(RuntimeProcessError::MissingTrackedProcessStdin { .. })
    ));
}
