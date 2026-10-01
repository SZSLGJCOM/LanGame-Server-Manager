use super::*;

fn file_set_plan() -> Value {
    json!({"action":"patch_instance_files", "filePatches":[{
        "file":"data/plugins/example.json", "sourceSha256":"a".repeat(64),
        "edits":[{"before":"old", "after":"new"}]
    }]})
}

#[test]
fn file_edit_set_plan_is_bounded_and_cannot_mix_operations() {
    assert!(parse_assistant_operation_plan_response(&file_set_plan().to_string()).is_ok());
    for (field, value) in [
        (
            "textPatch",
            json!({"file":"data/plugins/a.txt", "sourceSha256":"a".repeat(64), "before":"a", "after":"b"}),
        ),
        ("settingsPatch", json!({"limit":2})),
        ("filePatches", json!([])),
        ("action", json!("start_server")),
    ] {
        let mut plan = file_set_plan();
        plan[field] = value;
        assert!(
            parse_assistant_operation_plan_response(&plan.to_string()).is_err(),
            "accepted {field}"
        );
    }
    let mut plan = file_set_plan();
    plan["filePatches"][0]["edits"][0]["before"] = json!("");
    assert!(parse_assistant_operation_plan_response(&plan.to_string()).is_err());
}

#[test]
fn file_edit_set_requires_a_stopped_target_and_preserves_target_binding() {
    let (task, mut instance) = task_tests::task_fixture(true);
    let plan = parse_assistant_operation_plan_response(&file_set_plan().to_string()).unwrap();
    assert!(task.validate_plan(&plan, Some(&instance)).is_ok());
    instance.summary.active_process_count = 1;
    assert!(task.validate_plan(&plan, Some(&instance)).is_err());
    instance.summary.active_process_count = 0;
    instance.summary.status = InstanceStatus::Running;
    assert!(task.validate_plan(&plan, Some(&instance)).is_err());
}
