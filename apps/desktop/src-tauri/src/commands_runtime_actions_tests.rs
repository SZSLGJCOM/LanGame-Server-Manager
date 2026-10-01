use super::super::commands_assistant_ops::InstanceRuntimeCommandInput;
use super::*;
use serde_json::json;

fn action(id: &str, template: &str, target_encoding: Option<&str>) -> ModulePlayerActionSpec {
    ModulePlayerActionSpec {
        id: id.to_string(),
        kind: None,
        label: id.to_string(),
        label_zh_cn: None,
        transport: String::from("telnet"),
        command_template: template.to_string(),
        target_label: None,
        target_label_zh_cn: None,
        target_placeholder: None,
        target_placeholder_zh_cn: None,
        target_required: template.contains("{{target}}"),
        target_encoding: target_encoding.map(str::to_string),
        role_values: Vec::new(),
        process_key: None,
        port_name: Some(String::from("trusted-port")),
        password_setting_key: Some(String::from("trusted-password")),
        enabled_setting_key: Some(String::from("trusted-enabled")),
        destructive: false,
    }
}

fn descriptor(actions: Vec<ModulePlayerActionSpec>) -> ModuleDescriptor {
    ModuleDescriptor {
        root: PathBuf::from("D:/fixture/module"),
        manifest_toml: String::new(),
        schema_json: None,
        default_ports: Vec::new(),
        install: None,
        process: None,
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec {
            player_actions: actions,
            ..app_core::ModuleRuntimeSpec::default()
        },
        storage: app_modules::ModuleStorageSpec::default(),
        summary: ModuleSummary {
            id: String::from("fixture"),
            name: String::from("Fixture"),
            version: String::from("1.0.0"),
            description: None,
            steam_app_id: None,
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

fn resolution_input<'a>(
    action_id: Option<&'a str>,
    target: Option<&'a str>,
    role: Option<&'a str>,
) -> RuntimeCommandResolutionInput<'a> {
    RuntimeCommandResolutionInput {
        command: "forged-command\nquit",
        process_key: Some("forged-process"),
        transport: Some("stdin"),
        port_name: Some("forged-port"),
        password_setting_key: Some("forged-password"),
        enabled_setting_key: Some("forged-enabled"),
        runtime_action_id: action_id,
        runtime_action_target: target,
        runtime_action_role: role,
    }
}

#[test]
fn runtime_command_input_accepts_camel_case_action_metadata() {
    let input = serde_json::from_value::<InstanceRuntimeCommandInput>(json!({
        "instanceId": "fixture-instance",
        "command": "forged-command",
        "runtimeActionId": "kick",
        "runtimeActionTarget": "player",
        "runtimeActionRole": "admin",
    }))
    .expect("deserialize runtime action input");

    assert_eq!(input.runtime_action_id.as_deref(), Some("kick"));
    assert_eq!(input.runtime_action_target.as_deref(), Some("player"));
    assert_eq!(input.runtime_action_role.as_deref(), Some("admin"));
}

#[test]
fn runtime_action_rejects_target_injection_fragments() {
    let raw_action = action("kick", "kick {{target}}", None);
    for target in [
        "player\nquit",
        "{{role}}",
        "player;quit",
        "player && quit",
        "player || quit",
        "player`quit",
        "player$(quit)",
        "two players",
    ] {
        assert!(
            render_runtime_action_command(&raw_action, Some(target), None, false).is_err(),
            "unsafe target was accepted: {target:?}"
        );
    }

    let quoted_action = action("kick", "kick {{target}}", Some("quoted_string"));
    for target in [
        "player\nquit",
        "{{role}}",
        "player;quit",
        "player && quit",
        "player || quit",
        "player`quit",
        "player$(quit)",
    ] {
        assert!(
            render_runtime_action_command(&quoted_action, Some(target), None, false).is_err(),
            "unsafe quoted target was accepted: {target:?}"
        );
    }
}

#[test]
fn runtime_action_uses_declared_route_and_ignores_forged_client_fields() {
    let descriptor = descriptor(vec![action("players", "ListPlayers", None)]);

    let resolved = resolve_runtime_command(
        Some(&descriptor),
        resolution_input(Some("players"), None, None),
    )
    .expect("resolve declared read action");

    assert_eq!(resolved.command, "ListPlayers");
    assert_eq!(resolved.transport, "telnet");
    assert_eq!(resolved.process_key, None);
    assert_eq!(resolved.port_name.as_deref(), Some("trusted-port"));
    assert_eq!(
        resolved.password_setting_key.as_deref(),
        Some("trusted-password")
    );
    assert_eq!(
        resolved.enabled_setting_key.as_deref(),
        Some("trusted-enabled")
    );
    assert_eq!(resolved.runtime_action_id.as_deref(), Some("players"));
}

#[test]
fn targetless_read_action_resolves_without_user_parameters() {
    let descriptor = descriptor(vec![action("players", "ListPlayers", None)]);

    let resolved = resolve_runtime_command(
        Some(&descriptor),
        RuntimeCommandResolutionInput {
            command: "",
            process_key: None,
            transport: None,
            port_name: None,
            password_setting_key: None,
            enabled_setting_key: None,
            runtime_action_id: Some("players"),
            runtime_action_target: None,
            runtime_action_role: None,
        },
    )
    .expect("resolve targetless read action");

    assert_eq!(resolved.command, "ListPlayers");
    assert_eq!(resolved.runtime_action_id.as_deref(), Some("players"));
}

#[test]
fn missing_action_fields_preserve_the_manual_console_channel() {
    let resolved = resolve_runtime_command(
        None,
        RuntimeCommandResolutionInput {
            command: "  status  ",
            process_key: Some("main"),
            transport: Some("stdin"),
            port_name: Some("client-port"),
            password_setting_key: Some("client-password"),
            enabled_setting_key: Some("client-enabled"),
            runtime_action_id: None,
            runtime_action_target: None,
            runtime_action_role: None,
        },
    )
    .expect("resolve explicit manual command");

    assert_eq!(resolved.command, "status");
    assert_eq!(resolved.transport, "stdin");
    assert_eq!(resolved.process_key.as_deref(), Some("main"));
    assert_eq!(resolved.port_name.as_deref(), Some("client-port"));
    assert_eq!(resolved.runtime_action_id, None);
}

#[test]
fn quoted_target_is_validated_then_escaped_once() {
    let action = action("kick", "kick {{target}}", Some("quoted_string"));

    assert_eq!(
        render_runtime_action_command(&action, Some("Player \"One\" \\ Alpha"), None, false)
            .expect("render quoted target"),
        "kick \"Player \\\"One\\\" \\\\ Alpha\""
    );
}

#[test]
fn canonical_player_usernames_keep_significant_surrounding_spaces() {
    let descriptor = descriptor(vec![action(
        "kick_user",
        "kickuser {{target}}",
        Some("quoted_string"),
    )]);
    let canonical = resolve_declared_runtime_action(
        &descriptor,
        "kick_user",
        Some(" Alice "),
        None,
        None,
        true,
    )
    .expect("authoritative username");
    assert_eq!(canonical.command, "kickuser \" Alice \"");
    let manual = resolve_runtime_command(
        Some(&descriptor),
        resolution_input(Some("kick_user"), Some(" Alice "), None),
    )
    .expect("manual username");
    assert_eq!(manual.command, "kickuser \"Alice\"");
    assert!(
        resolve_declared_runtime_action(
            &descriptor,
            "kick_user",
            Some(" Alice\nquit "),
            None,
            None,
            true
        )
        .is_err()
    );
}

#[test]
fn role_must_be_a_declared_safe_value() {
    let mut action = action("role", "role {{target}} {{role}}", None);
    action.role_values = vec![String::from("admin"), String::from("moderator")];

    assert_eq!(
        render_runtime_action_command(&action, Some("player"), Some("admin"), false)
            .expect("declared role"),
        "role player admin"
    );
    for role in [
        "owner",
        "admin;quit",
        "admin && quit",
        "admin || quit",
        "admin`quit",
        "admin\nquit",
        "{{role}}",
        "admin$(quit)",
    ] {
        assert!(
            render_runtime_action_command(&action, Some("player"), Some(role), false).is_err(),
            "unsafe or undeclared role was accepted: {role:?}"
        );
    }

    action.role_values.push(String::from("admin root"));
    assert!(
        render_runtime_action_command(&action, Some("player"), Some("admin root"), false).is_err(),
        "declared role values must still satisfy the runtime single-token policy"
    );
}

#[test]
fn action_metadata_without_action_id_cannot_fall_back_to_manual_command() {
    let descriptor = descriptor(vec![action("players", "ListPlayers", None)]);

    let error = resolve_runtime_command(
        Some(&descriptor),
        resolution_input(None, Some("player"), None),
    )
    .expect_err("action target without id must not enter the manual command channel");

    assert!(error.contains("runtimeActionId"));
}

#[test]
fn internal_runtime_action_binds_only_a_valid_request_id() {
    let action = action(
        "players",
        "print [LGM-DST-PLAYERS-BEGIN]\\t{{request_id}}",
        None,
    );
    let request_id = "0123456789abcdef0123456789abcdef";

    assert_eq!(
        render_runtime_action_command_with_request_id(
            &action,
            None,
            None,
            false,
            Some(request_id),
        )
        .expect("render correlated list action"),
        format!("print [LGM-DST-PLAYERS-BEGIN]\\t{request_id}")
    );
    assert!(render_runtime_action_command(&action, None, None, false).is_err());
    for invalid in [
        "ABCDEF0123456789ABCDEF0123456789",
        "0123456789abcdef",
        "0123456789abcdef0123456789abcdeg",
        "0123456789abcdef0123456789abcde;",
    ] {
        assert!(
            render_runtime_action_command_with_request_id(
                &action,
                None,
                None,
                false,
                Some(invalid),
            )
            .is_err(),
            "invalid request id was accepted: {invalid:?}"
        );
    }
}

#[test]
fn internal_request_id_cannot_be_added_to_an_unbound_action() {
    let action = action("players", "ListPlayers", None);
    assert!(
        render_runtime_action_command_with_request_id(
            &action,
            None,
            None,
            false,
            Some("0123456789abcdef0123456789abcdef"),
        )
        .is_err()
    );
}

#[test]
fn structured_player_actions_cannot_bypass_the_snapshot_service() {
    let mut descriptor = descriptor(vec![
        action("list_online_players", "list {{request_id}}", None),
        action("kick_userid", "kick {{target}}", Some("quoted_string")),
        action("broadcast", "say hello", None),
    ]);
    descriptor.runtime.player_list = Some(app_core::ModulePlayerListSpec {
        scope: app_core::ModulePlayerListScope::Online,
        source: app_core::ModulePlayerListSource::StructuredLog,
        action_id: Some(String::from("list_online_players")),
        player_action_ids: vec![String::from("kick_userid")],
        response_codec: app_core::ModulePlayerListCodec::DstClientTableV1,
        identity_kind: app_core::RuntimePlayerIdentityKind::KleiUserId,
        refresh_interval_ms: 30_000,
    });

    assert!(runtime_action_requires_live_player_service(
        &descriptor,
        "list_online_players"
    ));
    assert!(runtime_action_requires_live_player_service(
        &descriptor,
        "kick_userid"
    ));
    assert!(!runtime_action_requires_live_player_service(
        &descriptor,
        "broadcast"
    ));
}

#[test]
fn humanitz_runtime_actions_require_the_entire_native_net_id() {
    let mut descriptor = descriptor(vec![
        action("kick_player", "kick {{target}}", None),
        action("ban_player", "ban {{target}}", None),
        action("unban_player", "unban {{target}}", None),
    ]);
    descriptor.summary.id = "humanitz".to_owned();
    let full = "0123456789abcdef0123456789ABCDEF|FEDCBA9876543210fedcba9876543210";
    let product_only = "|FEDCBA9876543210fedcba9876543210";
    for (id, command) in [
        ("kick_player", "kick"),
        ("ban_player", "ban"),
        ("unban_player", "unban"),
    ] {
        for target in [full, product_only] {
            let resolved = resolve_runtime_command(
                Some(&descriptor),
                resolution_input(Some(id), Some(target), None),
            )
            .unwrap();
            assert_eq!(resolved.command, format!("{command} {target}"));
        }
        for target in [
            None,
            Some("76561198000000001"),
            Some("FEDCBA9876543210fedcba9876543210"),
            Some("Host_Offline"),
            Some("|"),
            Some("0123|4567"),
        ] {
            assert!(
                resolve_runtime_command(
                    Some(&descriptor),
                    resolution_input(Some(id), target, None)
                )
                .is_err()
            );
            assert!(
                resolve_declared_runtime_action(&descriptor, id, target, None, None, true).is_err()
            );
        }
    }
    descriptor.summary.id = "other".to_owned();
    assert!(
        resolve_runtime_command(
            Some(&descriptor),
            resolution_input(Some("kick_player"), Some("76561198000000001"), None)
        )
        .is_ok()
    );
}
