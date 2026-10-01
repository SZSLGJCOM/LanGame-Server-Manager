use super::*;

fn action(template: &str, encoding: Option<&str>) -> ModulePlayerActionSpec {
    ModulePlayerActionSpec {
        id: String::from("ban"),
        kind: None,
        label: String::from("Ban"),
        label_zh_cn: None,
        transport: String::from("stdin"),
        command_template: template.to_string(),
        target_label: None,
        target_label_zh_cn: None,
        target_placeholder: None,
        target_placeholder_zh_cn: None,
        target_required: true,
        target_encoding: encoding.map(str::to_string),
        role_values: Vec::new(),
        process_key: None,
        port_name: None,
        password_setting_key: None,
        enabled_setting_key: None,
        destructive: true,
    }
}

#[test]
fn direct_action_binds_canonical_target() {
    assert_eq!(
        render_runtime_action_command(
            &action("ban {{target}}", None),
            Some("76561198000000001"),
            None,
            true,
        )
        .expect("render"),
        "ban 76561198000000001"
    );
}

#[test]
fn quoted_target_is_escaped_by_module_declaration() {
    assert_eq!(
        render_runtime_action_command(
            &action("ban {{target}}", Some("quoted_string")),
            Some("Player \"One\""),
            None,
            true,
        )
        .expect("render"),
        "ban \"Player \\\"One\\\"\""
    );
}

#[test]
fn verification_requires_non_empty_roster_response() {
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Add,
            "76561198000000001",
            None,
        )
        .is_err()
    );
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Remove,
            "76561198000000001",
            Some("  "),
        )
        .is_err()
    );
}

#[test]
fn verification_checks_target_presence_for_each_operation() {
    let roster = "1 76561198000000001 LanGame\n2 76561198000000002 Other";
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Add,
            "76561198000000001",
            Some(roster),
        )
        .is_ok()
    );
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Remove,
            "76561198000000003",
            Some(roster),
        )
        .is_ok()
    );
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Remove,
            "76561198000000001",
            Some(roster),
        )
        .is_err()
    );
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Remove,
            "76561198000000003",
            Some("76561198000000002\n[LanGame: response truncated at 16384 characters]"),
        )
        .is_err()
    );
}

#[test]
fn verification_rejects_error_responses_even_when_the_target_is_echoed() {
    for response in [
        "Unknown command: ban 76561198000000001",
        "ERROR while listing 76561198000000001",
        "Command failed for 76561198000000001",
        "Usage: ban list 76561198000000001",
        "Permission denied for 76561198000000001",
    ] {
        for operation in [
            PlayerAccessMutationOperation::Add,
            PlayerAccessMutationOperation::Remove,
        ] {
            let error =
                verify_player_access_response(operation, "76561198000000001", Some(response))
                    .expect_err("error output must never verify a mutation");

            assert!(error.contains("error response"), "{error}");
        }
    }
}

#[test]
fn removal_of_the_last_ban_accepts_a_complete_zero_identity_roster() {
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Remove,
            "76561198000000001",
            Some("No bans"),
        )
        .is_ok()
    );
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Add,
            "76561198000000001",
            Some("No bans"),
        )
        .is_err()
    );
}

#[test]
fn zero_identity_removal_still_rejects_unknown_error_and_truncated_responses() {
    for response in [
        "Unknown command",
        "ERROR while listing bans",
        "[LanGame: response truncated at 16384 characters]",
    ] {
        assert!(
            verify_player_access_response(
                PlayerAccessMutationOperation::Remove,
                "76561198000000001",
                Some(response),
            )
            .is_err(),
            "unsafe zero-identity response was accepted: {response:?}"
        );
    }
}

#[test]
fn verification_matches_only_complete_17_digit_steam64_tokens() {
    let roster = "765611980000000019 embedded\n76561198000000002 other";

    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Add,
            "76561198000000001",
            Some(roster),
        )
        .is_err()
    );
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Remove,
            "76561198000000001",
            Some(roster),
        )
        .is_ok()
    );
    assert!(
        verify_player_access_response(
            PlayerAccessMutationOperation::Add,
            "765611980000000019",
            Some(roster),
        )
        .is_err()
    );
}

#[test]
fn seven_days_unban_renders_an_offline_platform_account_without_dropping_namespace() {
    for target in [
        "Steam_76561198000000001",
        "EOS_0002604bc42244e099c1bf05145fb71f",
    ] {
        assert_eq!(
            render_runtime_action_command(
                &action("ban remove {{target}}", None),
                Some(target),
                None,
                true
            )
            .expect("qualified offline account"),
            format!("ban remove {target}")
        );
    }
    assert!(
        render_runtime_action_command(
            &action("ban remove {{target}}", None),
            Some("Steam_76561198000000001;quit"),
            None,
            true,
        )
        .is_err()
    );
}
