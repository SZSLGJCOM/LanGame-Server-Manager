use super::*;
use app_core::{InstanceStatus, InstanceSummary, StoppedProcess};

fn scope() -> StartedScope {
    StartedScope {
        instance_id: "test-instance".into(),
        module_id: "fixture".into(),
        run_id: 11,
        session_id: Some("session-current".into()),
        processes: BTreeSet::from([
            (11, "primary".into(), Some(101)),
            (12, "secondary".into(), Some(102)),
        ]),
        valid: true,
    }
}

fn stopped() -> StopInstanceResult {
    StopInstanceResult {
        summary: InstanceSummary {
            id: "test-instance".into(),
            module_id: "fixture".into(),
            name: "Fixture".into(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        run_id: 11,
        session_id: Some("session-current".into()),
        pid: Some(101),
        log_path: None,
        exit_code: Some(0),
        process_count: 2,
        processes: [(11, "primary", 101), (12, "secondary", 102)]
            .into_iter()
            .map(|(run_id, key, pid)| StoppedProcess {
                run_id,
                process_key: key.into(),
                display_name: String::new(),
                pid: Some(pid),
                log_path: None,
                exit_code: Some(0),
            })
            .collect(),
    }
}

fn observed() -> StopEvidence {
    let mut evidence = StopEvidence::default();
    evidence.observe_response(Some(&scope()), &stopped());
    evidence.tracked_identities = [101, 102]
        .into_iter()
        .map(|pid| IdentityExitEvidence {
            pid,
            expected_creation_time: 1000,
            outcome: IdentityOutcome::Exited,
        })
        .collect();
    evidence
}

#[test]
fn existing_stop_evidence_api_and_exit_zero_do_not_prove_normal_stop() {
    let mut evidence = StopEvidence::default();
    evidence.observe_response(Some(&scope()), &stopped());
    assert!(
        !evidence.corroborates(true, true, true),
        "no tracked exit observation"
    );
    evidence = observed();
    assert!(evidence.corroborates(true, true, true));
    assert!(!evidence.corroborates(false, true, true));
    assert!(!evidence.corroborates(true, false, true));
    assert!(!evidence.corroborates(true, true, false));
    evidence.tracked_identities[1].outcome = IdentityOutcome::StillRunning;
    assert!(
        !evidence.corroborates(true, true, true),
        "secondary remains alive despite exit0 receipt"
    );
    evidence.tracked_identities[1].outcome = IdentityOutcome::InspectionFailed;
    assert!(!evidence.corroborates(true, true, true));
    evidence.tracked_identities.pop();
    assert!(
        !evidence.corroborates(true, true, true),
        "missing secondary observation"
    );
}

#[test]
fn existing_stop_late_cleanup_never_turns_a_failed_stop_into_success() {
    let mut evidence = observed();
    let mut later = observation::PostErrorObservation::default();
    later.safe_to_continue = true;
    evidence.post_error_observation = Some(later);
    evidence.failures.push("normal_stop_api_failed");
    assert!(evidence.safe_after_late_exit());
    assert!(!evidence.corroborates(false, true, true));
}

#[test]
fn existing_stop_evidence_requires_exact_session_and_process_set() {
    let expected = scope();
    let mut result = stopped();
    result.processes.reverse();
    assert!(
        expected.matches_stop(&result),
        "order is not process identity"
    );
    for mutate in [
        (|result: &mut StopInstanceResult| result.session_id = Some("old-session".into()))
            as fn(&mut StopInstanceResult),
        |result| result.run_id = 10,
        |result| result.pid = Some(999),
        |result| result.summary.id = "another-instance".into(),
        |result| result.summary.module_id = "another-module".into(),
        |result| result.summary.active_process_count = 1,
        |result| result.process_count = 1,
        |result| {
            result.processes.pop();
        },
        |result| result.processes[1] = result.processes[0].clone(),
        |result| result.processes[1].pid = Some(999),
        |result| result.processes[1].process_key = "unknown-shard".into(),
        |result| result.processes[1].run_id = 99,
    ] {
        let mut result = stopped();
        mutate(&mut result);
        assert!(!expected.matches_stop(&result));
    }
    let mut evidence = observed();
    evidence.observe_response(None, &stopped());
    assert!(!evidence.corroborates(true, true, true));
}

#[test]
fn existing_stop_evidence_rejects_native_crash_even_with_successful_stop_receipt() {
    let mut result = stopped();
    result.processes[1].exit_code = Some(0x80000003_u32 as i32);
    let mut evidence = observed();
    evidence.observe_response(Some(&scope()), &result);
    assert!(
        !evidence.corroborates(true, true, true),
        "STATUS_BREAKPOINT is a native crash, not a normal stop"
    );
    assert!(evidence.failures.contains(&"native_shutdown_crash"));
    let receipt = serde_json::to_value(&evidence).unwrap();
    assert_eq!(
        receipt["processes"][1]["native_crash_reason"],
        "STATUS_BREAKPOINT"
    );

    let mut result = stopped();
    result.exit_code = Some(0xC0000005_u32 as i32);
    let mut evidence = observed();
    evidence.observe_response(Some(&scope()), &result);
    assert!(
        !evidence.corroborates(true, true, true),
        "the primary exit receipt must also be checked"
    );
}

#[test]
fn existing_stop_evidence_unknown_and_nonzero_exit_codes_remain_explicit() {
    let mut result = stopped();
    result.processes[0].exit_code = None;
    result.processes[1].exit_code = Some(-1073741510); // Windows console interrupt.
    let mut evidence = observed();
    evidence.observe_response(Some(&scope()), &result);
    assert_eq!(evidence.unknown_exit_code_count, 1);
    assert_eq!(evidence.nonzero_exit_code_count, 1);
    assert!(evidence.corroborates(true, true, true));
    let receipt = serde_json::to_value(&evidence).unwrap();
    assert_eq!(receipt["owned_tree_observation"], "not_exposed_by_ipc");
    assert_eq!(
        receipt["processes"][0]["exit_code"],
        serde_json::Value::Null
    );
    assert_eq!(
        receipt["required_backend_contract"],
        "normal_stop_returns_only_after_full_owned_tree_exits_without_force"
    );

    let mut result = stopped();
    result.exit_code = Some(-1);
    result.processes[0].exit_code = Some(-1); // ARK's documented existing receipt.
    let mut evidence = observed();
    evidence.observe_response(Some(&scope()), &result);
    assert!(evidence.corroborates(true, true, true));
}

#[test]
fn existing_stop_evidence_pid_reuse_and_inspection_failure_are_distinct() {
    let expected = ProcessIdentity {
        creation_time: 100,
        image_path: "synthetic.exe".into(),
    };
    let outcome = identity_outcome(&expected, Ok(Some(expected.clone())));
    assert!(matches!(outcome, IdentityOutcome::StillRunning));
    assert!(matches!(
        identity_outcome(&expected, Ok(None)),
        IdentityOutcome::Exited
    ));
    let replacement = ProcessIdentity {
        creation_time: 101,
        ..expected.clone()
    };
    assert!(matches!(
        identity_outcome(&expected, Ok(Some(replacement))),
        IdentityOutcome::ExitedPidReused
    ));
    assert!(matches!(
        identity_outcome(&expected, Err(())),
        IdentityOutcome::InspectionFailed
    ));
}

#[test]
fn existing_stop_evidence_rejects_incomplete_or_ambiguous_start_sets() {
    let processes = scope().processes;
    assert!(valid_processes(&processes, 2));
    assert!(!valid_processes(&processes, 1));
    assert!(!valid_processes(&BTreeSet::new(), 0));
    for second in [
        (11, "secondary".into(), Some(102)),
        (12, "primary".into(), Some(102)),
        (12, "secondary".into(), Some(101)),
        (12, "secondary".into(), None),
        (12, "secondary".into(), Some(0)),
    ] {
        assert!(!valid_processes(
            &BTreeSet::from([(11, "primary".into(), Some(101)), second]),
            2
        ));
    }
}
