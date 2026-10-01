use super::*;

fn instance() -> InstanceDetails {
    InstanceDetails {
        summary: InstanceSummary {
            id: String::from("selected-server"),
            name: String::from("Selected server"),
            module_id: String::from("minecraft"),
            status: InstanceStatus::Running,
            active_process_count: 1,
            bind_ip: String::from("127.0.0.1"),
            port_count: 2,
            autostart: false,
        },
        config_file_path: String::from("C:/instances/selected-server/server.properties"),
        saves_path: String::from("C:/instances/selected-server/world"),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: true,
        backup_retention_count: 3,
        settings_json: json!({"max_players": 8, "mod_ids": ["base", "addon"]}).to_string(),
        ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("tcp"),
                port: 25565,
            },
            PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 25566,
            },
        ],
        active_run: Some(ActiveInstanceRun {
            run_id: 7,
            session_id: Some(String::from("current-session")),
            pid: Some(1234),
            log_path: None,
            process_count: 1,
            processes: vec![],
        }),
    }
}

#[test]
fn confirmation_precondition_accepts_unchanged_state_and_port_reordering() {
    let original = instance();
    let precondition = AssistantOperationPrecondition::from_details(&original);
    assert_eq!(
        precondition.expected_settings_json(),
        original.settings_json
    );
    assert!(precondition.validate(&original).is_ok());
    let mut reordered = original.clone();
    reordered.ports.reverse();
    assert!(precondition.validate(&reordered).is_ok());
}

#[test]
fn confirmation_precondition_rejects_each_preview_relevant_state_change() {
    let original = instance();
    let changes: [fn(&mut InstanceDetails); 12] = [
        |value| value.settings_json = json!({"max_players": 16}).to_string(),
        |value| value.ports[0].port = 25570,
        |value| value.ports[0].protocol = String::from("udp"),
        |value| value.summary.bind_ip = String::from("0.0.0.0"),
        |value| value.summary.id = String::from("another-server"),
        |value| value.summary.module_id = String::from("projectzomboid"),
        |value| value.summary.status = InstanceStatus::Stopped,
        |value| value.active_run = None,
        |value| value.active_run.as_mut().unwrap().run_id += 1,
        |value| value.active_run.as_mut().unwrap().pid = Some(4321),
        |value| {
            value.active_run.as_mut().unwrap().session_id =
                Some(String::from("replacement-session"))
        },
        |value| value.config_file_path = String::from("C:/instances/other/server.properties"),
    ];
    for (index, change) in changes.into_iter().enumerate() {
        let precondition = AssistantOperationPrecondition::from_details(&original);
        let mut current = original.clone();
        change(&mut current);
        assert!(
            precondition.validate(&current).is_err(),
            "changed field {index}"
        );
    }
}

#[test]
fn confirmation_precondition_rejects_reused_process_id_with_new_os_identity() {
    let mut details = instance();
    details
        .active_run
        .as_mut()
        .unwrap()
        .processes
        .push(app_core::InstanceProcessState {
            run_id: 7,
            session_id: Some(String::from("current-session")),
            process_key: String::from("main"),
            display_name: String::from("Main server"),
            pid: Some(1234),
            process_identity: Some(ProcessIdentity {
                creation_time: 100,
                image_path: String::from("C:/server.exe"),
            }),
            status: String::from("running"),
            started_at: None,
            stopped_at: None,
            exit_code: None,
            crash_flag: false,
            log_path: None,
            is_primary: true,
        });
    let precondition = AssistantOperationPrecondition::from_details(&details);
    details.active_run.as_mut().unwrap().processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time = 200;
    assert!(precondition.validate(&details).is_err());
}

#[test]
fn settings_readback_verifies_full_result_and_applied_key_set() {
    let details = instance();
    let expected = json!({"max_players": 8, "mod_ids": ["base", "addon"]});
    let applied = vec![String::from("mod_ids")];
    assert!(verify_assistant_settings_result(&details, &expected, &applied).is_ok());
    assert!(verify_assistant_settings_result(&details, &expected, &[]).is_err());
    assert!(
        verify_assistant_settings_result(&details, &expected, &[String::from("absent")]).is_err()
    );
    assert!(
        verify_assistant_settings_result(
            &details,
            &expected,
            &[String::from("mod_ids"), String::from("mod_ids")]
        )
        .is_err()
    );
    let mut unrelated_changed = details.clone();
    unrelated_changed.settings_json =
        json!({"max_players": 32, "mod_ids": ["base", "addon"]}).to_string();
    assert!(verify_assistant_settings_result(&unrelated_changed, &expected, &applied).is_err());
    let mut applied_not_saved = details;
    applied_not_saved.settings_json =
        json!({"max_players": 8, "mod_ids": ["addon", "base"]}).to_string();
    assert!(verify_assistant_settings_result(&applied_not_saved, &expected, &applied).is_err());
}

#[test]
fn settings_readback_rejects_invalid_or_non_object_state_without_echoing_contents() {
    let mut details = instance();
    details.settings_json = String::from("invalid fixture contents");
    let error = verify_assistant_settings_result(
        &details,
        &json!({"max_players": 8}),
        &[String::from("max_players")],
    )
    .unwrap_err();
    assert!(!error.contains("invalid fixture contents"));
    details.settings_json = String::from("[]");
    assert!(
        verify_assistant_settings_result(&details, &json!([]), &[String::from("max_players")])
            .is_err()
    );
}

#[test]
fn ports_readback_verifies_every_binding_and_applied_name() {
    let details = instance();
    let expected = details.ports.clone();
    let applied = vec![String::from("game")];
    assert!(verify_assistant_ports_result(&details, &expected, &applied).is_ok());
    assert!(verify_assistant_ports_result(&details, &expected, &[]).is_err());
    assert!(verify_assistant_ports_result(&details, &expected, &[String::from("absent")]).is_err());
    let mut current = details.clone();
    current.ports.reverse();
    assert!(verify_assistant_ports_result(&current, &expected, &applied).is_ok());
    current.ports[0].port = 25580;
    assert!(verify_assistant_ports_result(&current, &expected, &applied).is_err());
    let mut duplicate = details;
    duplicate.ports.push(expected[0].clone());
    assert!(verify_assistant_ports_result(&duplicate, &duplicate.ports, &applied).is_err());
}
