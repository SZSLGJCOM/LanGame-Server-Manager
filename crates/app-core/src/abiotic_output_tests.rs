use super::*;

#[test]
fn marked_modio_tokens_hide_quotes_case_and_multiple_parameters() {
    let original = r#"Command line: -Port=7777 -mOdIoToKeN=synthetic-first -Other=keep -ModioToken="synthetic second" '-ModioToken=synthetic-third' -ModioToken 'synthetic fourth' -ModioToken=\"synthetic-fifth\""#;
    let redacted = redact_modio_token_text(original);
    assert_eq!(
        redacted,
        r#"Command line: -Port=7777 -mOdIoToKeN=[REDACTED] -Other=keep -ModioToken="[REDACTED]" '-ModioToken=[REDACTED]' -ModioToken '[REDACTED]' -ModioToken=\"[REDACTED]\""#
    );
    for credential in [
        "synthetic-first",
        "synthetic second",
        "synthetic-third",
        "synthetic fourth",
        "synthetic-fifth",
    ] {
        assert!(!redacted.contains(credential));
    }
}

#[test]
fn partial_native_lines_hide_the_available_value_without_changing_normal_logs() {
    assert_eq!(
        redact_modio_token_text("LogInit: -ModioToken=synthetic-part"),
        "LogInit: -ModioToken=[REDACTED]"
    );
    assert_eq!(
        redact_modio_token_text("-ModioToken=\"synthetic-part"),
        "-ModioToken=\"[REDACTED]"
    );
    for normal in [
        "Session creation completed\nSaved world successfully\n",
        "玩家已加入，port=7777",
        "-ModioTokenNotAnOption=keep",
        "-ModioToken=\nSession creation completed",
    ] {
        assert_eq!(redact_modio_token_text(normal), normal);
    }
}

#[test]
fn argument_extraction_preserves_launch_values_and_understands_separate_arguments() {
    let arguments = [
        "-Port=7777",
        "-ModioToken=synthetic-token-one",
        "-modiotoken",
        "synthetic-token-two",
    ]
    .map(String::from)
    .to_vec();
    let original = arguments.clone();
    let tokens = modio_tokens_from_args(&arguments);
    assert_eq!(tokens.len(), 2);
    assert!(tokens.contains(&String::from("synthetic-token-one")));
    assert!(tokens.contains(&String::from("synthetic-token-two")));
    assert_eq!(arguments, original);
}

#[test]
fn public_preview_and_both_start_response_plan_copies_hide_the_token() {
    let token = "synthetic-test-only";
    let actual = crate::LaunchPlan {
        instance_id: "synthetic".into(),
        instance_name: "Synthetic Abiotic output test".into(),
        module_id: "abioticfactor".into(),
        install_root: String::new(),
        install_state: crate::InstallState::NotInstalled,
        uses_private_runtime: true,
        working_directory: String::new(),
        executable_path: String::new(),
        executable_exists: false,
        ready_to_launch: false,
        validation_issues: Vec::new(),
        args: vec!["-Port=7777".into(), format!("-ModioToken={token}")],
        environment: Default::default(),
        command_line: format!("server -Port=7777 -ModioToken={token}"),
        window_policy: crate::ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: crate::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
        performance_policy: Default::default(),
        performance_preview: Default::default(),
    };
    let mut preview = actual.clone();
    redact_abiotic_launch_preview(&mut preview);
    let mut response = crate::StartInstanceResult {
        warnings: Vec::new(),
        summary: crate::InstanceSummary {
            id: "synthetic".into(),
            name: "Synthetic Abiotic output test".into(),
            module_id: "abioticfactor".into(),
            status: crate::InstanceStatus::Running,
            bind_ip: "127.0.0.1".into(),
            port_count: 1,
            autostart: false,
            active_process_count: 1,
        },
        run_id: 1,
        session_id: None,
        pid: 1,
        log_path: String::new(),
        launch_plan: actual.clone(),
        process_count: 1,
        processes: Vec::new(),
        launch_plans: vec![crate::ProcessLaunchPlan {
            process_key: "main".into(),
            display_name: "Abiotic".into(),
            log_path: String::new(),
            launch_plan: actual.clone(),
        }],
        startup_schedule: crate::RuntimeStartupSchedule {
            delay_ms: 0,
            instance_stagger_ms: 0,
            effective_stagger_ms: 0,
            child_process_stagger_ms: 0,
            running_instance_count: 0,
            active_process_count: 0,
            process_count: 1,
            queued_start_count: 0,
            reason: String::new(),
        },
    };
    redact_abiotic_start_result(&mut response);
    for public in [
        &preview,
        &response.launch_plan,
        &response.launch_plans[0].launch_plan,
    ] {
        assert_eq!(public.args[1], "-ModioToken=[REDACTED]");
        assert!(!public.command_line.contains(token));
        assert_eq!(public.args[0], "-Port=7777");
    }
    assert_eq!(actual.args[1], format!("-ModioToken={token}"));
    assert!(actual.command_line.contains(token));
    let mut other_module = actual.clone();
    other_module.module_id = "another-game".into();
    redact_abiotic_launch_preview(&mut other_module);
    assert_eq!(other_module.args, actual.args);
}
