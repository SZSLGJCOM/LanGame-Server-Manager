use super::*;

fn planner_input(prompt: String) -> AssistantExecuteOperationInput {
    AssistantExecuteOperationInput {
        task: Default::default(),
        settings: AssistantProviderSettings {
            provider: String::from("ollama"),
            model: String::from("fixture-model"),
            base_url: String::from("http://127.0.0.1:11434/v1"),
            api_key: String::new(),
        },
        prompt,
        context: None,
        selected_instance_id: None,
        selected_module_id: None,
    }
}

#[test]
fn planner_preserves_long_chinese_requests_and_their_final_restrictions() {
    let restriction = "限制：仅检查现有模组顺序；不要删除存档，不要停服，不要安装新模组。";
    let input = planner_input(format!(
        "{}\n{restriction}",
        "请检查启动日志中的模组加载错误。".repeat(50)
    ));
    assert!(input.prompt.len() > 800);

    let prompt = build_assistant_operation_planner_prompt(&input, &[], &[], None, None, &[], None)
        .expect("complete request fits the planner budget");

    assert!(prompt.starts_with(&format!("User request:\n{}\n\n", input.prompt)));
    assert!(prompt.contains(restriction));
    assert!(prompt.contains(ASSISTANT_OPERATION_ACTION_GUIDE));
    assert!(prompt.len() <= ASSISTANT_PLANNER_PROMPT_BYTES);
}

#[test]
fn planner_service_goals_include_configuration_without_prompt_keywords() {
    let (_, mut details) = super::task_tests::task_fixture(false);
    details.settings_json = json!({"fixture_setting": "fixture_current_value"}).to_string();
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: details.summary.module_id.clone(),
            name: String::from("Fixture game"),
            version: String::from("1.0.0"),
            description: None,
            steam_app_id: None,
            install_state: Default::default(),
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![],
        },
        schema_json: Some(
            json!({"properties": {"fixture_schema_key": {"type": "string"}}}).to_string(),
        ),
        default_ports: vec![],
        install: None,
        process: None,
        workshop: None,
        mods: None,
        runtime: Default::default(),
    };
    let documents = [AssistantOperationConfigDocument {
        path: String::from("server.ini"),
        content: String::from("fixture_document_marker=present"),
        truncated: false,
    }];
    for goal in [
        AssistantTaskGoal::LaunchService,
        AssistantTaskGoal::RestoreService,
        AssistantTaskGoal::ApplyChange,
    ] {
        let mut input = planner_input(String::from("继续"));
        input.task.goal = goal;
        let prompt = build_assistant_operation_planner_prompt(
            &input,
            std::slice::from_ref(&details.summary),
            std::slice::from_ref(&module.summary),
            Some(&details),
            Some(&module),
            &documents,
            None,
        )
        .unwrap();
        for marker in [
            "fixture_current_value",
            "fixture_schema_key",
            "fixture_document_marker",
        ] {
            assert!(prompt.contains(marker), "{goal:?}: {marker}");
        }
        assert!(prompt.contains(&input.prompt));
        assert!(prompt.contains(ASSISTANT_OPERATION_ACTION_GUIDE));
        assert!(prompt.len() <= ASSISTANT_PLANNER_PROMPT_BYTES);
    }
}

#[test]
fn planner_rejects_an_oversized_request_instead_of_dropping_its_tail() {
    let input = planner_input(format!(
        "{}\n禁止修改实例，先说明调查证据。",
        "启动日志中的模组依赖错误。".repeat(ASSISTANT_PLANNER_PROMPT_BYTES)
    ));
    let error = build_assistant_operation_planner_prompt(&input, &[], &[], None, None, &[], None)
        .expect_err("an oversized user request must never become a truncated plan");
    assert!(error.contains("context budget"));
    assert!(error.contains("no operation was executed"));
}

#[test]
fn planner_reserves_the_complete_action_contract_at_the_request_byte_boundary() {
    let request_bytes = ASSISTANT_PLANNER_PROMPT_BYTES
        - "User request:\n\n\n".len()
        - ASSISTANT_OPERATION_ACTION_GUIDE.len();
    let input = planner_input(format!(
        "{}{}",
        "界".repeat(request_bytes / 3),
        ".".repeat(request_bytes % 3)
    ));
    let prompt = build_assistant_operation_planner_prompt(&input, &[], &[], None, None, &[], None)
        .expect("exact byte boundary");
    assert_eq!(prompt.len(), ASSISTANT_PLANNER_PROMPT_BYTES);
    assert!(prompt.contains(&input.prompt));
    assert!(prompt.ends_with(ASSISTANT_OPERATION_ACTION_GUIDE));

    let oversized = planner_input(format!("{}!", input.prompt));
    assert!(
        build_assistant_operation_planner_prompt(&oversized, &[], &[], None, None, &[], None,)
            .is_err()
    );
}

#[test]
fn planner_redacts_every_external_section_before_preparing_the_prompt() {
    let secrets = (0..13)
        .map(|index| format!("metadata-fixture-{index}-end"))
        .collect::<Vec<_>>();
    let summary = InstanceSummary {
        id: format!("instance token={}", secrets[1]),
        name: format!(
            "safe-instance password={} Z:/fixture/private/instance",
            secrets[3]
        ),
        module_id: format!("module token={}", secrets[2]),
        status: Default::default(),
        active_process_count: 0,
        bind_ip: format!("token={}", secrets[4]),
        port_count: 1,
        autostart: false,
    };
    let details = InstanceDetails {
        summary: summary.clone(),
        config_file_path: String::from("Z:/fixture/private/server.ini"),
        saves_path: String::from("Z:/fixture/private/world"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: true,
        backup_retention_count: 3,
        settings_json: json!({"rcon_password": secrets[8], "max_players": 12}).to_string(),
        ports: vec![PortBinding {
            name: format!("game password={}", secrets[6]),
            protocol: format!("udp token={}", secrets[7]),
            port: 12345,
        }],
        active_run: None,
    };
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: summary.module_id.clone(),
            name: format!(
                "safe-module password={} Z:/fixture/private/module",
                secrets[5]
            ),
            version: String::from("1.0.0"),
            description: None,
            steam_app_id: None,
            install_state: Default::default(),
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![],
        },
        schema_json: Some(
            json!({"properties": {
                (format!("password={}", secrets[9])): {"type": "string"},
                "Z:/fixture/private/schema": {"type": "string"},
                "mod_ids": {"type": "string"}
            }})
            .to_string(),
        ),
        default_ports: vec![],
        install: None,
        process: None,
        workshop: None,
        mods: None,
        runtime: Default::default(),
    };
    let documents = vec![AssistantOperationConfigDocument {
        path: String::from("Z:/fixture/private/config.ini"),
        content: format!("password={}", secrets[10]),
        truncated: false,
    }];
    let log = LogTailSnapshot {
        source_path: Some(String::from("Z:/fixture/private/server.log")),
        lines: vec![format!("password={}", secrets[11])],
        total_lines: 1,
        truncated: false,
        read_error: None,
    };
    let mut input = planner_input(format!(
        "Read configuration and latest log; password={}",
        secrets[0]
    ));
    input.context = Some(format!("password={}", secrets[12]));
    let prompt = build_assistant_operation_planner_prompt(
        &input,
        &[summary],
        std::slice::from_ref(&module.summary),
        Some(&details),
        Some(&module),
        &documents,
        Some(&log),
    )
    .expect("prepared planner prompt");
    for secret in secrets {
        assert!(!prompt.contains(&secret), "unredacted external section");
    }
    assert!(!prompt.contains("Z:/fixture/private"));
    for public_value in [
        "safe-instance",
        "safe-module",
        "mod_ids",
        "max_players",
        "12345",
    ] {
        assert!(prompt.contains(public_value), "{public_value}");
    }
    assert!(prompt.contains(ASSISTANT_OPERATION_ACTION_GUIDE));
}
