use super::*;

#[test]
fn natural_reaping_retains_surviving_descendants_until_they_exit_themselves() {
    let mut fixture = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    let mut supervisor = fixture.register();
    fs::write(fixture.root.join("exit-launcher"), b"exit").unwrap();
    assert!(fixture.handles[0].wait_for_exit(5000).unwrap());
    for _ in 0..2 {
        assert!(supervisor.reap_exited().unwrap().is_empty());
        assert!(supervisor.is_tracked("owned-tree-probe"));
        assert!(
            fixture.handles[1..]
                .iter()
                .all(|handle| handle.is_running().unwrap()),
            "natural reconciliation must not force live descendants after a failed normal stop"
        );
    }
    fs::write(fixture.root.join("release-leaves"), b"exit").unwrap();
    assert!(fixture.leaves_stopped());
    assert!(wait_until(
        || supervisor
            .instance_process_tree_is_running("owned-tree-probe")
            .unwrap()
            == Some(false),
        Duration::from_secs(5)
    ));
    let exited = supervisor.reap_exited().unwrap();
    assert_eq!(exited.len(), 1);
    assert_eq!(exited[0].exit_code, Some(0));
    assert!(!supervisor.is_tracked("owned-tree-probe"));
}

#[test]
fn inspection_waits_for_descendants_after_launcher_exit_without_stopping_them() {
    for surface in [
        ProcessHostSurface::ManagedTerminal,
        ProcessHostSurface::ManagedPseudoConsole,
    ] {
        let mut fixture = LaunchFixture::new(surface);
        let mut supervisor = fixture.register();
        let pid = fixture.handles[0].pid;
        assert_eq!(
            supervisor
                .instance_process_tree_is_running("owned-tree-probe")
                .unwrap(),
            Some(true)
        );
        assert_eq!(
            supervisor
                .owned_process_tree_is_running("owned-tree-probe", 1, "main", pid)
                .unwrap(),
            Some(true)
        );
        fs::write(fixture.root.join("exit-launcher"), b"exit").unwrap();
        assert!(fixture.handles[0].wait_for_exit(5000).unwrap());
        assert_eq!(
            supervisor
                .instance_process_tree_is_running("owned-tree-probe")
                .unwrap(),
            Some(true),
            "the launcher's exit must not hide its surviving descendants"
        );
        assert_eq!(
            supervisor
                .owned_process_tree_is_running("owned-tree-probe", 1, "main", pid)
                .unwrap(),
            Some(true),
            "the target launch stays alive while any owned descendant survives"
        );
        assert!(
            fixture.handles[1..]
                .iter()
                .all(|handle| handle.is_running().unwrap()),
            "inspection must not terminate a workload"
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
        assert_eq!(
            supervisor
                .owned_process_tree_is_running("owned-tree-probe", 1, "main", pid)
                .unwrap(),
            Some(false)
        );
        assert!(
            supervisor.is_tracked("owned-tree-probe"),
            "inspection must leave reconciliation to the lifecycle owner"
        );
    }
}

#[test]
fn inspection_requires_every_launch_owner_and_job() {
    let mut first = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    let mut second = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    let mut supervisor = first.register();
    let first_pid = first.handles[0].pid;
    let second_pid = second.handles[0].pid;
    assert_eq!(
        supervisor
            .instance_process_tree_is_running("untracked")
            .unwrap(),
        None
    );
    for (instance_id, run_id, process_key, pid) in [
        ("untracked", 1, "main", first_pid),
        ("owned-tree-probe", 2, "main", first_pid),
        ("owned-tree-probe", 1, "extra", first_pid),
        ("owned-tree-probe", 1, "main", second_pid),
    ] {
        assert_eq!(
            supervisor
                .owned_process_tree_is_running(instance_id, run_id, process_key, pid)
                .unwrap(),
            None,
            "a different instance/run/key/PID cannot establish target ownership"
        );
    }
    let mut instance = supervisor
        .take_running_for_stop("owned-tree-probe")
        .unwrap();
    let mut other = second
        .register()
        .take_running_for_stop("owned-tree-probe")
        .unwrap();
    other.processes[0].process_key = "extra".into();
    other.processes[0].is_primary = false;
    instance.processes.append(&mut other.processes);
    instance.summary.active_process_count = 2;
    assert!(supervisor.restore_running_after_failed_stop(instance));
    fs::write(first.root.join("exit-launcher"), b"exit").unwrap();
    fs::write(first.root.join("release-leaves"), b"exit").unwrap();
    assert!(first.handles[0].wait_for_exit(5000).unwrap());
    assert!(first.leaves_stopped());
    assert_eq!(
        supervisor
            .instance_process_tree_is_running("owned-tree-probe")
            .unwrap(),
        Some(true),
        "one exited launch must not hide another live launch"
    );
    assert!(wait_until(
        || supervisor
            .owned_process_tree_is_running("owned-tree-probe", 1, "main", first_pid)
            .unwrap()
            == Some(false),
        Duration::from_secs(5),
    ));
    assert_eq!(
        supervisor
            .owned_process_tree_is_running("owned-tree-probe", 1, "extra", second_pid)
            .unwrap(),
        Some(true),
        "the completed target must be observable while another map remains alive"
    );
    assert!(
        second
            .handles
            .iter()
            .all(|handle| handle.is_running().unwrap()),
        "target inspection must preserve the other launch"
    );
    let mut instance = supervisor
        .take_running_for_stop("owned-tree-probe")
        .unwrap();
    let mut missing_child = instance.processes[0].child.take().unwrap();
    assert!(supervisor.restore_running_after_failed_stop(instance));
    assert_eq!(
        supervisor
            .instance_process_tree_is_running("owned-tree-probe")
            .unwrap(),
        None,
        "partial launch ownership cannot establish complete tree state"
    );
    assert_eq!(
        supervisor
            .owned_process_tree_is_running("owned-tree-probe", 1, "main", first_pid)
            .unwrap(),
        None,
        "a missing target owner cannot prove that the target exited"
    );
    let RuntimeChild::Windows(child) = &mut missing_child else {
        panic!("native launch owner required");
    };
    let job = child.job.take().unwrap();
    let mut instance = supervisor
        .take_running_for_stop("owned-tree-probe")
        .unwrap();
    instance.processes[0].child = Some(missing_child);
    assert!(supervisor.restore_running_after_failed_stop(instance));
    assert_eq!(
        supervisor
            .instance_process_tree_is_running("owned-tree-probe")
            .unwrap(),
        None,
        "an exited root without its Job must remain unknown"
    );
    assert_eq!(
        supervisor
            .owned_process_tree_is_running("owned-tree-probe", 1, "main", first_pid)
            .unwrap(),
        None,
        "a target root without its Job cannot prove that all descendants exited"
    );
    drop(job);
}

#[test]
fn inspection_preserves_real_job_query_errors() {
    let mut fixture = LaunchFixture::new(ProcessHostSurface::ManagedTerminal);
    let mut supervisor = fixture.register();
    let target_pid = fixture.handles[0].pid;
    let mut instance = supervisor
        .take_running_for_stop("owned-tree-probe")
        .unwrap();
    let RuntimeChild::Windows(child) = instance.processes[0].child.as_mut().unwrap() else {
        panic!("native launch owner required");
    };
    let full_access =
        windows_process_job::restrict_job_to_terminate_for_test(child.job.as_mut().unwrap())
            .unwrap();
    assert!(supervisor.restore_running_after_failed_stop(instance));
    assert!(matches!(
        supervisor.owned_process_tree_is_running("owned-tree-probe", 1, "main", target_pid),
        Err(RuntimeProcessError::WaitTrackedProcess { pid, source })
            if pid == target_pid && source.raw_os_error() == Some(5)
    ));
    assert!(matches!(
        supervisor.instance_process_tree_is_running("owned-tree-probe"),
        Err(RuntimeProcessError::WaitTrackedProcess { source, .. })
            if source.raw_os_error() == Some(5)
    ));
    assert!(
        fixture
            .handles
            .iter()
            .all(|handle| handle.is_running().unwrap())
    );
    let mut instance = supervisor
        .take_running_for_stop("owned-tree-probe")
        .unwrap();
    let RuntimeChild::Windows(child) = instance.processes[0].child.as_mut().unwrap() else {
        panic!("native launch owner required");
    };
    child.job = Some(full_access);
    assert!(supervisor.restore_running_after_failed_stop(instance));
    assert_eq!(
        supervisor
            .instance_process_tree_is_running("owned-tree-probe")
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        supervisor
            .owned_process_tree_is_running("owned-tree-probe", 1, "main", target_pid)
            .unwrap(),
        Some(true)
    );
}
