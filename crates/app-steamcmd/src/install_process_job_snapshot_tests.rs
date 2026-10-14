use std::time::Duration;

use super::*;

fn snapshot(assigned: u32, count: u32) -> JobProcessIds {
    let mut ids = [usize::MAX; MAX_CLEANUP_PROCESSES];
    ids[0] = 37;
    JobProcessIds {
        assigned,
        count,
        ids,
    }
}

#[test]
fn successful_job_snapshot_can_return_no_pids_after_a_process_exits() {
    let list = snapshot(1, 0);
    assert!(list.returned_process_ids().unwrap().is_empty());
}

#[test]
fn successful_job_snapshot_reads_only_the_returned_pid_prefix() {
    let list = snapshot(3, 1);
    assert_eq!(list.returned_process_ids().unwrap(), &[37]);
}

#[test]
fn job_snapshot_keeps_the_bounded_process_limit() {
    let capacity = MAX_CLEANUP_PROCESSES as u32;
    for (assigned, count) in [(capacity + 1, 0), (0, capacity + 1)] {
        assert!(snapshot(assigned, count).returned_process_ids().is_err());
    }
    assert_eq!(
        snapshot(capacity, capacity)
            .returned_process_ids()
            .unwrap()
            .len(),
        MAX_CLEANUP_PROCESSES
    );
}

#[tokio::test]
async fn completed_native_process_job_can_be_observed_after_exit() {
    let mut command = Command::new("cmd.exe");
    command
        .args(["/d", "/c", "exit", "0"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let (mut child, guard) = spawn_managed_command(&mut command).await.unwrap();
    assert!(child.wait().await.unwrap().success());
    let deadline = InstallDeadline::new("exited native job", Duration::from_secs(10));
    guard.wait_until_empty(deadline).await.unwrap();
    let job = guard.job.as_ref().unwrap();
    assert!(job.is_empty().unwrap());
    assert!(
        job.observed_processes_exited(deadline.expires_at())
            .unwrap()
    );
}
