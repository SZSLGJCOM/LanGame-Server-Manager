use super::module_settings_tests::schema_module;
use super::task_tests::task_fixture;
use super::*;

#[test]
fn investigation_catalog_bootstrap_distinguishes_module_schema_and_bound_launch_settings() {
    let (original, instance) = task_fixture(true);
    let mut task = original.as_ref().clone();
    task.request.goal = AssistantTaskGoal::LaunchService;
    task.instance_id = None;
    task.configuration_stage = AssistantConfigurationStage::Unconfigured;
    assert!(matches!(
        assistant_initial_investigation_reads(&task, false).as_slice(),
        [AssistantReadTool::ListModuleSettings { offset: 0 }]
    ));
    let selected_module = task.module_id.take();
    assert!(assistant_initial_investigation_reads(&task, false).is_empty());
    task.module_id = selected_module;

    task.instance_id = Some(instance.summary.id);
    for stage in [
        AssistantConfigurationStage::Unconfigured,
        AssistantConfigurationStage::Configuring,
    ] {
        task.configuration_stage = stage;
        let reads = assistant_initial_investigation_reads(&task, true);
        assert!(matches!(
            reads.as_slice(),
            [AssistantReadTool::ListSettings { offset: 0 }]
        ));
    }
    task.configuration_stage = AssistantConfigurationStage::Protected;
    assert!(assistant_initial_investigation_reads(&task, true).is_empty());
}

#[tokio::test]
async fn investigation_module_catalog_reaches_first_model_and_shares_the_eight_read_limit() {
    let (original, _) = task_fixture(true);
    let mut task = original.as_ref().clone();
    task.request.goal = AssistantTaskGoal::LaunchService;
    task.instance_id = None;
    let module = schema_module(json!({"properties": {
        "master_world_size":{"type":"string", "enum":["small","medium"], "default":"medium"}
    }}));
    task.module_id = Some(module.summary.id.clone());
    let before_schema = module.schema_json.clone();
    let mut calls = 0;
    let mut reads = 0;
    let result = run_assistant_investigation_checked(
        String::from("Create a new server with a small world."),
        assistant_initial_investigation_reads(&task, false),
        |prompt| {
            calls += 1;
            assert!(prompt.len() <= ASSISTANT_INVESTIGATION_PROMPT_BYTES);
            assert!(prompt.contains("\"scope\":\"module_schema\""));
            assert!(prompt.contains("\"instanceExists\":false"));
            assert!(prompt.contains("master_world_size"));
            assert!(prompt.contains(&format!("Read budget remaining: {} of 8", 8 - calls)));
            if calls > 1 {
                assert!(prompt.contains("\"enum\":[\"small\",\"medium\"]"));
            }
            std::future::ready(Ok(
                json!({"tool":"read_module_settings", "keys":["master_world_size"]}).to_string(),
            ))
        },
        |tool| {
            reads += 1;
            std::future::ready(read_assistant_module_schema(Some(&module), tool))
        },
        &|_| Ok(()),
    )
    .await
    .unwrap_err();
    assert!(result.contains("read limit"));
    assert_eq!(
        reads, 8,
        "the automatic catalog must consume one of the eight reads"
    );
    assert_eq!(calls, 8);
    assert_eq!(module.schema_json, before_schema);
    assert!(task.instance_id.is_none());
}

#[test]
fn investigation_catalog_bootstrap_depends_on_the_resolved_goal_not_prompt_keywords() {
    let (original, _) = task_fixture(true);
    for goal in [
        AssistantTaskGoal::RestoreService,
        AssistantTaskGoal::ApplyChange,
        AssistantTaskGoal::LaunchService,
    ] {
        let mut task = original.as_ref().clone();
        task.request.goal = goal;
        let reads = assistant_initial_investigation_reads(&task, true);
        if goal == AssistantTaskGoal::RestoreService {
            assert!(matches!(
                reads.as_slice(),
                [
                    AssistantReadTool::ReadRuntime { lines: 80 },
                    AssistantReadTool::ListConfigFiles { offset: 0 }
                ]
            ));
        } else {
            assert!(reads.is_empty(), "The model can request diagnostic tools without keyword prefetch.");
        }
        assert!(assistant_initial_investigation_reads(&task, false).is_empty());
    }
    let mut configuring = original.as_ref().clone();
    configuring.request.goal = AssistantTaskGoal::LaunchService;
    configuring.configuration_stage = AssistantConfigurationStage::Configuring;
    let reads = assistant_initial_investigation_reads(&configuring, true);
    assert!(matches!(
        reads.as_slice(),
        [AssistantReadTool::ListSettings { offset: 0 }]
    ));
}

#[tokio::test]
async fn investigation_catalog_reaches_the_first_model_and_counts_toward_the_read_budget() {
    let (original, instance) = task_fixture(true);
    let before = instance.settings_json.clone();
    let mut task = original.as_ref().clone();
    task.request.goal = AssistantTaskGoal::LaunchService;
    task.configuration_stage = AssistantConfigurationStage::Unconfigured;
    let catalog = assistant_settings_catalog_evidence(&before, 0).unwrap();
    let schema = json!({"properties": {"cluster_name": {
        "type": "string", "description": "Public cluster name shown in the server browser"
    }}})
    .to_string();
    let expected = json!({
        "action": "customize_config", "instanceId": instance.summary.id,
        "moduleId": instance.summary.module_id,
        "settingsPatch": {"cluster_name": "Requested server"},
        "reason": "Use the current setting name and schema to configure the server"
    });
    let validator = |response: &str| {
        let plan = parse_assistant_operation_plan_response(response)?;
        task.validate_plan(&plan, Some(&instance))
    };
    let mut model_calls = 0;
    let mut reads = Vec::new();
    let response = run_assistant_investigation_checked(
        String::from("Set the new server name to Requested server before starting it."),
        assistant_initial_investigation_reads(&task, true),
        |prompt| {
            model_calls += 1;
            let response = match model_calls {
                1 => {
                    assert!(prompt.contains(&catalog.to_string()));
                    assert!(prompt.contains("cluster_name"));
                    assert!(!prompt.contains("Initial name"));
                    assert!(prompt.contains("Read budget remaining: 7 of 8"));
                    json!({"tool": "read_settings", "keys": ["cluster_name"]}).to_string()
                }
                2 => {
                    assert!(prompt.contains("Read budget remaining: 6 of 8"));
                    assert!(prompt.contains("Initial name"));
                    assert!(prompt.contains("Public cluster name shown in the server browser"));
                    expected.to_string()
                }
                _ => panic!("the known setting must need no extra model calls"),
            };
            std::future::ready(Ok(response))
        },
        |tool| {
            let evidence = match tool {
                AssistantReadTool::ListSettings { offset: 0 } => {
                    reads.push("list_settings");
                    assistant_settings_catalog_evidence(&instance.settings_json, 0)
                }
                AssistantReadTool::ReadSettings { keys, offset: 0 } => {
                    assert_eq!(keys, ["cluster_name"]);
                    reads.push("read_settings");
                    assistant_settings_evidence(&instance.settings_json, Some(&schema), &keys, 0)
                }
                other => panic!("unexpected initial-configuration read: {other:?}"),
            };
            std::future::ready(evidence)
        },
        &validator,
    )
    .await
    .unwrap();
    assert_eq!(model_calls, 2);
    assert_eq!(reads, ["list_settings", "read_settings"]);
    assert_eq!(serde_json::from_str::<Value>(&response).unwrap(), expected);
    assert_eq!(instance.settings_json, before);
    assert_eq!(task.initial_settings, original.initial_settings);
    assert_eq!(
        task.configuration_stage,
        AssistantConfigurationStage::Unconfigured
    );
}
