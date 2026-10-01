use super::*;

fn patch_plan() -> Value {
    json!({"action":"patch_instance_text", "textPatch": {
        "file":"runtime/mods/example/modmain.lua", "sourceSha256":"a".repeat(64),
        "before":"priority = 0", "after":"priority = 1"
    }})
}

#[test]
fn file_patch_plan_requires_one_changed_bounded_replacement() {
    assert!(parse_assistant_operation_plan_response(&patch_plan().to_string()).is_ok());
    for field in ["file", "sourceSha256", "before", "after"] {
        let mut plan = patch_plan();
        plan["textPatch"].as_object_mut().unwrap().remove(field);
        assert!(parse_assistant_operation_plan_response(&plan.to_string()).is_err());
    }
    for (key, value) in [
        ("before", ""),
        ("sourceSha256", "invalid"),
        ("after", "priority = 0"),
    ] {
        let mut plan = patch_plan();
        plan["textPatch"][key] = json!(value);
        assert!(parse_assistant_operation_plan_response(&plan.to_string()).is_err());
    }
    let mut plan = patch_plan();
    plan["textPatch"]["after"] = json!("x".repeat(8192));
    assert!(parse_assistant_operation_plan_response(&plan.to_string()).is_err());
}

#[test]
fn file_patch_cannot_hide_another_operation_or_use_another_action() {
    let mut plan = patch_plan();
    plan["settingsPatch"] = json!({"cluster_name":"unrelated"});
    assert!(parse_assistant_operation_plan_response(&plan.to_string()).is_err());
    plan.as_object_mut().unwrap().remove("settingsPatch");
    plan["action"] = json!("start_server");
    assert!(parse_assistant_operation_plan_response(&plan.to_string()).is_err());
}

#[test]
fn file_patch_follow_up_keeps_instance_binding_and_operation_limit() {
    let plan = parse_assistant_operation_plan_response(&patch_plan().to_string()).unwrap();
    assert!(
        validate_assistant_repair_follow_up(
            &plan,
            "target",
            "dontstarve",
            1,
            AssistantTaskGoal::RestoreService
        )
        .is_ok()
    );
    assert!(
        validate_assistant_repair_follow_up(
            &plan,
            "target",
            "dontstarve",
            assistant_operation_limit(AssistantTaskGoal::RestoreService),
            AssistantTaskGoal::RestoreService
        )
        .is_err()
    );
    let mut changed = plan;
    changed.instance_id = Some("another".into());
    assert!(
        validate_assistant_repair_follow_up(
            &changed,
            "target",
            "dontstarve",
            1,
            AssistantTaskGoal::RestoreService
        )
        .is_err()
    );
}

#[test]
fn restore_task_accepts_private_file_repair_only_for_a_stopped_target() {
    let (task, mut instance) = task_tests::task_fixture(true);
    let plan = parse_assistant_operation_plan_response(&patch_plan().to_string()).unwrap();
    assert!(task.validate_plan(&plan, Some(&instance)).is_ok());
    instance.summary.active_process_count = 1;
    assert!(task.validate_plan(&plan, Some(&instance)).is_err());
    instance.summary.active_process_count = 0;
    instance.summary.status = InstanceStatus::Running;
    assert!(task.validate_plan(&plan, Some(&instance)).is_err());
}

#[test]
fn private_mod_file_content_uses_filename_and_cross_line_credential_redaction() {
    let value = "synthetic-test-only";
    let masked =
        redact_assistant_file_content(Path::new("runtime/mods/example/api_key.txt"), value)
            .unwrap();
    assert!(!masked.contains(value));
    let ordinary = redact_assistant_file_content(
        Path::new("runtime/mods/example/modmain.lua"),
        "local count = 1\r\nreturn count\r\n",
    )
    .unwrap();
    assert_eq!(ordinary, "local count = 1\r\nreturn count\r\n");
    for source in ["{ \"count\": 1 }\r\n", "first\r\nsecond\n\n", "\r\n  \n"] {
        assert_eq!(
            redact_assistant_file_content(Path::new("runtime/mods/example/data.json"), source)
                .unwrap(),
            source
        );
    }
    let yaml = format!("password: |\n  {value}\n");
    assert!(
        !redact_assistant_file_content(Path::new("runtime/mods/example/settings.txt"), &yaml)
            .unwrap()
            .contains(value)
    );
}
