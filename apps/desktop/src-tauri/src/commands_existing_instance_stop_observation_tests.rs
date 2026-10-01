use super::*;
use app_core::{ActiveInstanceRun, InstanceProcessState, InstanceStatus, InstanceSummary};

fn scope() -> StartedScope {
    StartedScope {
        instance_id: "fixture-instance".into(),
        module_id: "fixture".into(),
        run_id: 11,
        session_id: Some("current-session".into()),
        processes: BTreeSet::from([
            (11, "primary".into(), Some(101)),
            (12, "secondary".into(), Some(102)),
        ]),
        valid: true,
    }
}

fn identities() -> Vec<(u32, ProcessIdentity)> {
    [101, 102]
        .into_iter()
        .map(|pid| {
            (
                pid,
                ProcessIdentity {
                    creation_time: 1000 + u64::from(pid),
                    image_path: "fixture.exe".into(),
                },
            )
        })
        .collect()
}

fn finished_crash() -> InstanceRunRecord {
    InstanceRunRecord {
        run_id: 11,
        session_id: scope().session_id,
        status: "error".into(),
        pid: Some(101),
        started_at: Some("start".into()),
        stopped_at: Some("stop".into()),
        exit_code: Some(0x8000_0003_u32 as i32),
        crash_flag: true,
        log_path: None,
        process_count: 2,
        processes: identities()
            .into_iter()
            .enumerate()
            .map(|(index, (pid, identity))| InstanceProcessState {
                run_id: 11 + index as i64,
                session_id: scope().session_id,
                process_key: if index == 0 { "primary" } else { "secondary" }.into(),
                display_name: String::new(),
                pid: Some(pid),
                process_identity: Some(identity),
                status: if index == 0 { "error" } else { "stopped" }.into(),
                started_at: Some("start".into()),
                stopped_at: Some("stop".into()),
                exit_code: if index == 0 {
                    Some(0x8000_0003_u32 as i32)
                } else {
                    Some(0)
                },
                crash_flag: index == 0,
                log_path: None,
                is_primary: index == 0,
            })
            .collect(),
    }
}

fn inactive() -> InstanceDetails {
    InstanceDetails {
        summary: InstanceSummary {
            id: scope().instance_id,
            module_id: scope().module_id,
            name: String::new(),
            status: InstanceStatus::Error,
            active_process_count: 0,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        config_file_path: String::new(),
        saves_path: String::new(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 0,
        settings_json: "{}".into(),
        ports: Vec::new(),
        active_run: None,
    }
}

#[test]
fn existing_stop_observation_finalized_crash_allows_cleanup_only() {
    let mut sample = Sample {
        tracked_exited: 2,
        ..Default::default()
    };
    sample.storage_inactive = Some(inactive_instance(&scope(), &inactive()));
    sample.same_run_finalized = Some(finalized(
        &scope(),
        &[finished_crash()],
        &identities(),
        &mut sample,
    ));
    assert!(sample.safe(expected_identity_count(Some(&scope()), &identities())));
    let evidence = serde_json::to_value(&sample).unwrap();
    assert_eq!(evidence["run_exits"][0]["status"], "error");
    assert_eq!(
        evidence["run_exits"][0]["native_crash_reason"],
        "STATUS_BREAKPOINT"
    );
    assert_eq!(evidence["run_exits"][0]["crash_flag"], true);
}

#[test]
fn existing_stop_observation_requires_every_original_process_finalized() {
    for mutate in [
        (|run: &mut InstanceRunRecord| run.run_id = 99) as fn(&mut InstanceRunRecord),
        |run| run.session_id = Some("old-session".into()),
        |run| run.pid = Some(999),
        |run| run.status = "stopping".into(),
        |run| run.stopped_at = None,
        |run| run.process_count = 1,
        |run| {
            run.processes.pop();
        },
        |run| run.processes[1] = run.processes[0].clone(),
        |run| run.processes[1].run_id = 99,
        |run| run.processes[1].process_key = "unexpected".into(),
        |run| run.processes[1].pid = Some(999),
        |run| run.processes[1].status = "running".into(),
        |run| run.processes[1].stopped_at = None,
        |run| run.processes[1].session_id = Some("old-session".into()),
        |run| run.processes[1].process_identity = None,
        |run| {
            run.processes[1]
                .process_identity
                .as_mut()
                .unwrap()
                .creation_time += 1
        },
        |run| {
            run.processes[1]
                .process_identity
                .as_mut()
                .unwrap()
                .image_path = "other.exe".into()
        },
    ] {
        let mut run = finished_crash();
        mutate(&mut run);
        assert!(!finalized(
            &scope(),
            &[run],
            &identities(),
            &mut Sample::default()
        ));
    }
    assert!(!finalized(
        &scope(),
        &[],
        &identities(),
        &mut Sample::default()
    ));
}

#[test]
fn existing_stop_observation_rejects_unknown_or_mismatched_start_identities() {
    assert_eq!(expected_identity_count(Some(&scope()), &identities()), 2);
    assert_eq!(expected_identity_count(None, &identities()), 0);
    let mut invalid = scope();
    invalid.valid = false;
    assert_eq!(expected_identity_count(Some(&invalid), &identities()), 0);
    for mutate in [
        (|identities: &mut Vec<(u32, ProcessIdentity)>| {
            identities.pop();
        }) as fn(&mut Vec<(u32, ProcessIdentity)>),
        |identities| identities[1] = identities[0].clone(),
        |identities| identities[1].0 = 999,
        |identities| identities[1].1.creation_time = 0,
        |identities| identities[1].1.image_path.clear(),
    ] {
        let mut actual = identities();
        mutate(&mut actual);
        assert_eq!(expected_identity_count(Some(&scope()), &actual), 0);
        assert!(!finalized(
            &scope(),
            &[finished_crash()],
            &actual,
            &mut Sample::default()
        ));
    }
}

#[test]
fn existing_stop_observation_requires_inactive_matching_instance_and_no_active_run() {
    for status in [InstanceStatus::Stopped, InstanceStatus::Error] {
        let mut actual = inactive();
        actual.summary.status = status;
        assert!(inactive_instance(&scope(), &actual));
    }
    for mutate in [
        (|actual: &mut InstanceDetails| actual.summary.id = "other".into())
            as fn(&mut InstanceDetails),
        |actual| actual.summary.module_id = "other".into(),
        |actual| actual.summary.status = InstanceStatus::Running,
        |actual| actual.summary.active_process_count = 1,
        |actual| {
            actual.active_run = Some(ActiveInstanceRun {
                run_id: 99,
                session_id: Some("replacement".into()),
                pid: Some(999),
                log_path: None,
                process_count: 1,
                processes: Vec::new(),
            })
        },
    ] {
        let mut actual = inactive();
        mutate(&mut actual);
        assert!(!inactive_instance(&scope(), &actual));
    }
}

#[test]
fn existing_stop_observation_keeps_native_crash_category_without_raw_error() {
    let redacted = redact_backend_stop_error(
        "native_shutdown_crash: STATUS_BREAKPOINT; private path and command",
    );
    let evidence = serde_json::to_value(ApiFailure::from_reason(&redacted)).unwrap();
    assert_eq!(evidence["outcome"], "backend_rejected");
    assert_eq!(evidence["categories"], json!(["native_shutdown_crash"]));
    assert!(!redacted.contains("private"));
}
