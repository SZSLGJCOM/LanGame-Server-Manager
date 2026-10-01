use super::*;

fn network_instance() -> InstanceDetails {
    let (_, mut instance) = task_tests::task_fixture(true);
    instance.summary.status = InstanceStatus::Running;
    instance.summary.active_process_count = 1;
    instance.ports = vec![app_core::PortBinding {
        name: "game".into(),
        protocol: "udp".into(),
        port: 20000,
    }];
    instance.active_run = Some(app_core::ActiveInstanceRun {
        run_id: 7,
        session_id: Some("fixture-session".into()),
        pid: Some(100),
        log_path: None,
        process_count: 1,
        processes: vec![app_core::InstanceProcessState {
            run_id: 7,
            session_id: Some("fixture-session".into()),
            process_key: "main".into(),
            display_name: "Server".into(),
            pid: Some(100),
            process_identity: Some(app_core::ProcessIdentity {
                creation_time: 10,
                image_path: "C:/fixture/server.exe".into(),
            }),
            status: "running".into(),
            started_at: None,
            stopped_at: None,
            exit_code: None,
            crash_flag: false,
            log_path: None,
            is_primary: true,
        }],
    });
    instance
}

fn network_inspection(count: usize) -> app_platform_win::ProcessNetworkInspectionResult {
    app_platform_win::ProcessNetworkInspectionResult {
        inspected_process_count: 1,
        endpoints: (0..count)
            .map(|index| ProcessNetworkEndpoint {
                protocol: "udp".into(),
                local_address: "127.0.0.1".into(),
                local_port: 20000 + index as u16,
                owning_pid: 100,
                process_key: "main".into(),
                relation: "target".into(),
            })
            .collect(),
    }
}

#[test]
fn network_tool_requires_complete_bound_process_identities() {
    let instance = network_instance();
    let targets = assistant_network_targets(&instance).unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].process_identity.creation_time, 10);
    for variant in 0..4 {
        let mut invalid = instance.clone();
        match variant {
            0 => invalid.active_run = None,
            1 => invalid.active_run.as_mut().unwrap().processes[0].process_identity = None,
            2 => invalid.active_run.as_mut().unwrap().processes[0].run_id = 8,
            _ => invalid.active_run.as_mut().unwrap().process_count = 2,
        }
        assert!(assistant_network_targets(&invalid).is_err());
    }
}

#[test]
fn network_evidence_is_bounded_and_never_claims_connectivity() {
    let instance = network_instance();
    let evidence =
        assistant_network_evidence(&instance, &instance, network_inspection(70)).unwrap();
    assert_eq!(
        evidence["endpoints"].as_array().unwrap().len(),
        ASSISTANT_NETWORK_ENDPOINTS
    );
    assert_eq!(evidence["omittedEndpointCount"], 6);
    assert_eq!(evidence["runId"], 7);
    assert!(
        evidence["guidance"]
            .as_str()
            .unwrap()
            .contains("not proof of game readiness")
    );
    assert!(!evidence.to_string().contains("C:/fixture"));
    let mut unverified = network_inspection(0);
    unverified.inspected_process_count = 0;
    assert!(assistant_network_evidence(&instance, &instance, unverified).is_err());
}

#[test]
fn network_evidence_discards_changed_run_or_port_context() {
    let instance = network_instance();
    for variant in 0..3 {
        let mut after = instance.clone();
        match variant {
            0 => after.active_run.as_mut().unwrap().run_id += 1,
            1 => {
                after.active_run.as_mut().unwrap().processes[0]
                    .process_identity
                    .as_mut()
                    .unwrap()
                    .creation_time += 1
            }
            _ => after.ports.clear(),
        }
        assert!(assistant_network_evidence(&instance, &after, network_inspection(1)).is_err());
    }
}
