use super::*;

fn request(settings: Value) -> AssistantTaskRequirements {
    serde_json::from_value(
        json!({"settings": settings, "ports": [], "forbiddenActions": [], "unverified": []}),
    )
    .unwrap()
}

fn setting(key: &str, expected: Value) -> Value {
    json!({"key":key,"expected":expected,"description":"Requested setting","sourceText":"requested"})
}

fn fixture() -> (InstanceDetails, ModuleDetails) {
    let (_, mut instance) = task_tests::task_fixture(false);
    instance.summary.module_id = String::from("fixture");
    instance.settings_json =
        json!({"players": 4,"mode":"normal","bind_ip":"127.0.0.1"}).to_string();
    instance.ports = vec![PortBinding {
        name: "game".into(),
        protocol: "udp".into(),
        port: 27015,
    }];
    let module = ModuleDetails {
        summary: ModuleSummary {
            id: "fixture".into(),
            name: "Fixture".into(),
            version: "1".into(),
            description: None,
            steam_app_id: None,
            install_state: app_core::InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec!["windows".into()],
        },
        schema_json: Some(
            json!({"properties": {
                "players":{"type":"integer","minimum":1,"maximum":64},
                "mode":{"type":"string","enum":["normal","hard"]},
                "label":{"type":"string","minLength":1,"maxLength":4},
                "fixed":{"type":"string","const":"only"}
            }})
            .to_string(),
        ),
        default_ports: instance.ports.clone(),
        install: None,
        process: None,
        workshop: None,
        mods: None,
        runtime: Default::default(),
    };
    (instance, module)
}

#[test]
fn requirements_validate_real_keys_types_bounds_and_source_excerpts() {
    let (instance, module) = fixture();
    for (key, value) in [
        ("players", json!(4)),
        ("mode", json!("hard")),
        ("label", json!("中文")),
        ("fixed", json!("only")),
    ] {
        assert!(
            request(json!([setting(key, value)]))
                .validated("requested", Some(&instance), Some(&module))
                .is_ok()
        );
    }
    for (key, value) in [
        ("unknown", json!(4)),
        ("players", json!("4")),
        ("players", json!(1.5)),
        ("players", json!(0)),
        ("players", json!(65)),
        ("mode", json!("invented")),
        ("label", json!("")),
        ("label", json!("12345")),
        ("fixed", json!("different")),
    ] {
        assert!(
            request(json!([setting(key, value)]))
                .validated("requested", Some(&instance), Some(&module))
                .is_err()
        );
    }
    let mut requirements = request(json!([setting("players", json!(4))]));
    requirements.settings[0].source_text = "request ed".into();
    assert!(
        requirements
            .validated("requested", Some(&instance), Some(&module))
            .is_err()
    );
    requirements.settings[0].source_text = " ".into();
    assert!(
        requirements
            .validated(" ", Some(&instance), Some(&module))
            .is_err()
    );
    requirements.settings[0].source_text = "界".repeat(171);
    assert!(
        requirements
            .validated(&"界".repeat(171), Some(&instance), Some(&module))
            .is_err()
    );
}

#[test]
fn requirements_reject_unknown_fields_duplicates_and_resource_overflow() {
    let (instance, module) = fixture();
    assert!(
        serde_json::from_value::<AssistantTaskRequirements>(
            json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[],"extra":true})
        )
        .is_err()
    );
    let mut value = setting("players", json!(4));
    value["extra"] = json!(true);
    assert!(serde_json::from_value::<AssistantSettingRequirement>(value).is_err());
    let duplicate = request(json!([
        setting("players", json!(4)),
        setting("players", json!(4))
    ]));
    assert!(
        duplicate
            .validated("requested", Some(&instance), Some(&module))
            .is_err()
    );
    let mut requirements = request(json!([]));
    requirements.unverified = (0..33)
        .map(|_| AssistantUnverifiedRequirement {
            description: "Unsupported effect".into(),
            source_text: "requested".into(),
            reason: "No effect verifier".into(),
        })
        .collect();
    assert!(
        requirements
            .validated("requested", None, None)
            .unwrap_err()
            .contains("32-item")
    );
    let mut oversized = request(json!([setting("mode", json!("x".repeat(16 * 1024)))]));
    assert!(
        oversized
            .validated("requested", Some(&instance), Some(&module))
            .unwrap_err()
            .contains("16 KiB")
    );
    oversized.settings[0].expected = json!("normal");
    oversized.settings[0].description = "x".repeat(513);
    assert!(
        oversized
            .validated("requested", Some(&instance), Some(&module))
            .is_err()
    );
}

#[test]
fn requirements_allow_partial_steps_but_reject_contradictions_and_early_start() {
    let (mut instance, module) = fixture();
    let requirements = request(json!([
        setting("players", json!(24)),
        setting("mode", json!("hard"))
    ]));
    requirements
        .validated("requested", Some(&instance), Some(&module))
        .unwrap();
    let mut plan = assistant_safe_none_plan("fixture".into());
    plan.action = AssistantOperationAction::CustomizeConfig;
    plan.settings_patch = Some(json!({"mode":"hard"}));
    assert!(
        requirements
            .validate_action(&plan, Some(&instance), None)
            .is_ok()
    );
    plan.settings_patch = Some(json!({"mode":"normal"}));
    let error = requirements
        .validate_action(&plan, Some(&instance), None)
        .unwrap_err();
    assert!(error.contains("Requirement 2"));
    plan.settings_patch = None;
    plan.action = AssistantOperationAction::StartServer;
    let error = requirements.validate_action(&plan, None, None).unwrap_err();
    assert!(error.contains("requirement_1"));
    assert!(error.contains("players"));
    assert!(error.contains("24"));
    assert!(error.contains("unknown"));
    assert!(
        requirements
            .checks(None, None)
            .iter()
            .all(|check| check.status == AssistantTaskCheckStatus::Unknown)
    );
    assert!(
        requirements
            .validate_action(&plan, Some(&instance), None)
            .is_err()
    );
    instance.settings_json = json!({"players":24}).to_string();
    assert_eq!(
        requirements.checks(Some(&instance), None)[1].status,
        AssistantTaskCheckStatus::Unknown
    );
    assert!(
        requirements
            .validate_action(&plan, Some(&instance), None)
            .is_err()
    );
    instance.settings_json = json!({"players":24,"mode":"hard"}).to_string();
    assert!(
        requirements
            .validate_action(&plan, Some(&instance), None)
            .is_ok()
    );
}

#[test]
fn requirements_bind_ip_checks_saved_field_and_listener_metadata() {
    let (mut instance, module) = fixture();
    let requirements = request(json!([setting("bind_ip", json!("127.0.0.1"))]));
    requirements
        .validated("requested", None, Some(&module))
        .unwrap();
    assert!(
        request(json!([setting("bind_ip", json!("2001:0db8::1"))]))
            .validated("requested", None, Some(&module))
            .is_err()
    );
    assert!(
        request(json!([setting("bind_ip", json!("2001:db8::1"))]))
            .validated("requested", None, Some(&module))
            .is_ok()
    );
    assert!(
        request(json!([setting("bind_ip", json!("not an IP"))]))
            .validated("requested", None, Some(&module))
            .is_err()
    );
    assert_eq!(
        requirements.checks(Some(&instance), None)[0].status,
        AssistantTaskCheckStatus::Satisfied
    );
    instance.summary.bind_ip = "0.0.0.0".into();
    assert_eq!(
        requirements.checks(Some(&instance), None)[0].status,
        AssistantTaskCheckStatus::Failed
    );
    instance.summary.bind_ip = "127.0.0.1".into();
    instance.settings_json = json!({"bind_ip":"0.0.0.0"}).to_string();
    assert_eq!(
        requirements.checks(Some(&instance), None)[0].status,
        AssistantTaskCheckStatus::Failed
    );
}

#[test]
fn requirements_ports_use_declared_identity_and_reject_conflicting_patch() {
    let (mut instance, module) = fixture();
    let mut requirements = request(json!([]));
    requirements.ports.push(AssistantPortRequirement {
        name: "game".into(),
        expected: 27016,
        description: "Requested port".into(),
        source_text: "requested".into(),
    });
    requirements
        .validated("requested", Some(&instance), Some(&module))
        .unwrap();
    let mut plan = assistant_safe_none_plan("fixture".into());
    plan.action = AssistantOperationAction::RepairPorts;
    plan.port_patch = Some(json!({"ports":[{"name":"GAME","port":27015}]}));
    assert!(
        requirements
            .validate_action(&plan, Some(&instance), None)
            .is_err()
    );
    plan.port_patch = Some(json!({"GAME":27016}));
    assert!(
        requirements
            .validate_action(&plan, Some(&instance), None)
            .is_ok()
    );
    assert_eq!(
        requirements.checks(Some(&instance), None)[0].status,
        AssistantTaskCheckStatus::Failed
    );
    instance.ports[0].port = 27016;
    assert_eq!(
        requirements.checks(Some(&instance), None)[0].status,
        AssistantTaskCheckStatus::Satisfied
    );
    instance.ports.clear();
    assert_eq!(
        requirements.checks(Some(&instance), None)[0].status,
        AssistantTaskCheckStatus::Unknown
    );
    let mut duplicate = requirements.ports[0].clone();
    duplicate.name = "GAME".into();
    requirements.ports.push(duplicate);
    assert!(
        requirements
            .validated("requested", None, Some(&module))
            .is_err()
    );
}

#[test]
fn requirements_unverified_prevents_all_mutations_and_forbidden_action_is_explicit() {
    let (instance, _) = fixture();
    let mut requirements = request(json!([]));
    requirements
        .forbidden_actions
        .push(AssistantForbiddenActionRequirement {
            action: AssistantOperationAction::InstallServer,
            description: "No explicit install".into(),
            source_text: "requested".into(),
        });
    requirements.validated("requested", None, None).unwrap();
    let mut plan = assistant_safe_none_plan("fixture".into());
    plan.action = AssistantOperationAction::InstallServer;
    assert!(
        requirements
            .validate_action(&plan, Some(&instance), None)
            .is_err()
    );
    plan.action = AssistantOperationAction::CustomizeConfig;
    assert!(
        requirements
            .validate_action(&plan, Some(&instance), None)
            .is_ok()
    );
    let check = &requirements.checks(None, None)[0];
    assert_eq!(check.status, AssistantTaskCheckStatus::Satisfied);
    assert!(check.summary.contains("indirect"));
    requirements
        .unverified
        .push(AssistantUnverifiedRequirement {
            description: "No network effects".into(),
            source_text: "requested".into(),
            reason: "No available network-effect verifier".into(),
        });
    requirements.validated("requested", None, None).unwrap();
    for action in [
        AssistantOperationAction::CreateServer,
        AssistantOperationAction::StartServer,
        AssistantOperationAction::InstallServer,
        AssistantOperationAction::ValidateServer,
        AssistantOperationAction::ApplyBeginnerConfig,
        AssistantOperationAction::CustomizeConfig,
        AssistantOperationAction::InstallFunMod,
        AssistantOperationAction::InstallSiteMod,
        AssistantOperationAction::RepairPorts,
        AssistantOperationAction::RunGmCommand,
        AssistantOperationAction::Broadcast,
    ] {
        plan.action = action;
        assert!(
            requirements
                .validate_action(&plan, Some(&instance), None)
                .is_err()
        );
    }
    plan.action = AssistantOperationAction::None;
    assert!(
        requirements
            .validate_action(&plan, Some(&instance), None)
            .is_ok()
    );
}

#[test]
fn requirements_redact_secret_values_from_every_view_check_debug_and_error() {
    let (mut instance, _) = fixture();
    let secret = ["fixture", "credential", "value"].join("-");
    let key = ["server", "password"].join("_");
    instance.settings_json = json!({key.clone():secret,"players":4}).to_string();
    let mut requirements = request(json!([setting(&key, json!(secret))]));
    requirements.settings[0].description = format!("Use {secret}");
    requirements.settings[0].source_text = format!("requested {secret}");
    requirements
        .unverified
        .push(AssistantUnverifiedRequirement {
            description: secret.clone(),
            source_text: secret.clone(),
            reason: secret.clone(),
        });
    requirements
        .validated(&format!("requested {secret}"), Some(&instance), None)
        .unwrap();
    let views = requirements.views();
    assert!(
        views[0]
            .expected_display
            .as_ref()
            .unwrap()
            .contains("[REDACTED]")
    );
    let checks = requirements.checks(Some(&instance), None);
    for (view, check) in views.iter().zip(&checks) {
        assert_eq!(view.id, check.name);
        assert_eq!(check.evidence["requirementId"], view.id);
    }
    for text in [
        serde_json::to_string(&views).unwrap(),
        serde_json::to_string(&checks).unwrap(),
        format!("{requirements:?}"),
    ] {
        assert!(
            !text.contains(&secret),
            "sensitive expected value escaped redaction"
        );
    }
    requirements.unverified.clear();
    let mut plan = assistant_safe_none_plan("fixture".into());
    plan.action = AssistantOperationAction::CustomizeConfig;
    plan.settings_patch = Some(json!({key:format!("wrong-{secret}")}));
    assert!(
        !requirements
            .validate_action(&plan, Some(&instance), None)
            .unwrap_err()
            .contains(&secret)
    );
    assert_eq!(requirements.settings[0].expected, json!(secret));
}

#[test]
fn requirements_do_not_accept_unknown_dst_projection_as_saved_value_proof() {
    let (mut instance, _) = fixture();
    instance.summary.module_id = "dontstarve".into();
    instance.settings_json = json!({"master_world_size":"default","master_worldgenoverride_lua":"return make_world()","players":4}).to_string();
    let requirements = request(json!([
        setting("master_world_size", json!("default")),
        setting("players", json!(4))
    ]));
    assert_eq!(
        requirements.checks(Some(&instance), None)[0].status,
        AssistantTaskCheckStatus::Unknown
    );
    assert_eq!(
        requirements.checks(Some(&instance), Some("{broken"))[0].status,
        AssistantTaskCheckStatus::Unknown
    );
    assert_eq!(
        requirements.checks(Some(&instance), None)[1].status,
        AssistantTaskCheckStatus::Satisfied
    );
}

#[test]
fn requirements_scrub_partial_and_nested_secret_references_from_metadata() {
    let (mut instance, _) = fixture();
    let secret = ["partial", "fixture", "credential"].join("-");
    let sensitive_key = ["access", "token"].join("_");
    let partial = format!("banner {}={secret}", "password");
    for expected in [
        json!(partial),
        json!({"nested":[{"note":partial}]}),
        json!({"nested":[{sensitive_key.clone():secret}]}),
        json!({"nested":[{sensitive_key.clone():[secret,{"value":secret}]}]}),
    ] {
        instance.settings_json = json!({"cluster_description":expected}).to_string();
        let mut requirements = request(json!([setting("cluster_description", expected.clone())]));
        requirements.settings[0].description = format!("Use {secret}");
        requirements.settings[0].source_text = secret.clone();
        requirements
            .unverified
            .push(AssistantUnverifiedRequirement {
                description: secret.clone(),
                source_text: secret.clone(),
                reason: secret.clone(),
            });
        requirements
            .validated(&secret, Some(&instance), None)
            .unwrap();
        for output in [
            serde_json::to_string(&requirements.views()).unwrap(),
            serde_json::to_string(&requirements.checks(Some(&instance), None)).unwrap(),
            format!("{requirements:?}"),
        ] {
            assert!(
                !output.contains(&secret),
                "a recognized secret escaped through metadata"
            );
        }
        assert_eq!(requirements.settings[0].expected, expected);
    }
}
