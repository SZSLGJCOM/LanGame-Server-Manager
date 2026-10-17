use super::*;

fn soulmask_descriptor() -> ModuleDescriptor {
    app_modules::discover_modules(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../modules"),
    )
    .expect("discover modules")
    .into_iter()
    .find(|module| module.summary.id == "soulmask")
    .expect("Soulmask module")
}

#[test]
fn soulmask_native_ban_actions_bind_quoted_complete_account_ids() {
    let descriptor = soulmask_descriptor();
    let epic = "0123456789abcdef0123456789ABCDEF|FEDCBA9876543210fedcba9876543210";
    for (action_id, mode) in [("ban_player", 1), ("unban_player", 0)] {
        for target in ["10000000000000001", epic] {
            for bound in [false, true] {
                let resolved = resolve_declared_runtime_action(
                    &descriptor,
                    action_id,
                    Some(target),
                    None,
                    None,
                    bound,
                )
                .expect("resolve native account permission action");
                assert_eq!(resolved.command, format!("usp 1 {mode} \"{target}\""));
                assert_eq!(resolved.transport, "source_rcon");
                assert_eq!(resolved.port_name.as_deref(), Some("rcon"));
                assert_eq!(
                    resolved.password_setting_key.as_deref(),
                    Some("rcon_password")
                );
            }
        }
    }
}

#[test]
fn soulmask_native_ban_actions_reject_names_partial_ids_and_command_syntax() {
    let descriptor = soulmask_descriptor();
    let invalid_epic = [
        format!("{}|{}", "a".repeat(31), "b".repeat(32)),
        format!("{}|{}", "a".repeat(32), "b".repeat(31)),
        format!("{}|{}", "a".repeat(33), "b".repeat(32)),
        format!("{}|{}", "g".repeat(32), "b".repeat(32)),
        format!("|{}", "b".repeat(32)),
        format!("{}|", "a".repeat(32)),
        format!("{}|{}|{}", "a".repeat(32), "b".repeat(32), "c".repeat(32)),
    ];
    let targets = [
        None,
        Some(""),
        Some("PlayerName"),
        Some("1000000000000001"),
        Some("100000000000000001"),
        Some("00000000000000000"),
        Some("-10000000000000001"),
        Some(" 10000000000000001 "),
        Some("\"10000000000000001\""),
        Some("10000000000000001\nquit"),
        Some("10000000000000001;quit"),
        Some("10000000000000001\" 0 \"other"),
        Some("0123456789abcdef0123456789abcdef"),
        Some("{{target}}"),
    ];
    for action_id in ["ban_player", "unban_player"] {
        for target in targets
            .into_iter()
            .chain(invalid_epic.iter().map(|value| Some(value.as_str())))
        {
            for bound in [false, true] {
                assert!(
                    resolve_declared_runtime_action(
                        &descriptor,
                        action_id,
                        target,
                        None,
                        None,
                        bound,
                    )
                    .is_err(),
                    "Soulmask accepted invalid account target: {target:?}"
                );
            }
        }
    }
}

#[test]
fn soulmask_manual_action_metadata_uses_the_same_account_boundary() {
    let descriptor = soulmask_descriptor();
    for target in ["10000000000000001", "PlayerName"] {
        let resolved = resolve_runtime_command(
            Some(&descriptor),
            RuntimeCommandResolutionInput {
                command: "forged",
                process_key: None,
                transport: Some("stdin"),
                port_name: Some("forged"),
                password_setting_key: None,
                enabled_setting_key: None,
                runtime_action_id: Some("unban_player"),
                runtime_action_target: Some(target),
                runtime_action_role: None,
            },
        );
        if target == "PlayerName" {
            assert!(resolved.is_err());
        } else {
            assert_eq!(
                resolved.expect("resolve declared action").command,
                "usp 1 0 \"10000000000000001\""
            );
        }
    }
    assert_eq!(
        resolve_declared_runtime_action(
            &descriptor,
            "kick_player",
            Some("PlayerName"),
            None,
            None,
            false,
        )
        .expect("preserve kick contract")
        .command,
        "kick PlayerName"
    );
}

#[test]
fn soulmask_launch_declares_permission_default_before_explicit_extra_arguments() {
    let descriptor = soulmask_descriptor();
    let arguments = &descriptor.process.expect("Soulmask process").args_template;
    let default = arguments
        .iter()
        .position(|argument| argument == "{{soulmask.default_server_permissions_arg}}")
        .expect("native account blacklist default");
    let extra = arguments
        .iter()
        .position(|argument| argument == "{{launch.extra_args}}")
        .expect("operator arguments");
    assert!(default < extra);
    assert!(
        !arguments
            .iter()
            .any(|argument| argument.starts_with("-serverpm="))
    );
}
