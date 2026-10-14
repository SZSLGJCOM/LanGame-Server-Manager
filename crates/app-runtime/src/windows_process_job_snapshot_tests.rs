use super::*;

fn snapshot(assigned: u32, count: u32) -> ProcessIdList {
    let mut ids = [usize::MAX; MAX_WAITED_PROCESSES];
    ids[0] = 37;
    ProcessIdList {
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
    let capacity = MAX_WAITED_PROCESSES as u32;
    for (assigned, count) in [(capacity + 1, 0), (0, capacity + 1)] {
        assert!(snapshot(assigned, count).returned_process_ids().is_err());
    }
    assert_eq!(
        snapshot(capacity, capacity)
            .returned_process_ids()
            .unwrap()
            .len(),
        MAX_WAITED_PROCESSES
    );
}
