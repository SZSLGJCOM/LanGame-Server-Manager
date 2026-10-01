use super::*;

#[tokio::test(flavor = "current_thread")]
async fn native_requirement_partial_record_returns_an_error_receipt_with_retained_items() {
    let module =
        module_settings_tests::schema_module(json!({"properties":{"players":{"type":"integer"}}}));
    let request = "Set 4 players.";
    let mut turn = 0;
    let operation = Box::pin(run_assistant_tool_investigation(
        AssistantInvestigationContext {
            prompt: request.into(), initial_reads: vec![], tools:assistant_investigation_tools(None,Some(&module),true),
            draft:Some(AssistantRequirementsDraft::new(request)),instance:None,module:Some(&module),
            completion:AssistantInvestigationCompletion::OperationProposal,
        },
        |messages, _| {
            turn += 1;
            let (name, arguments) = match turn {
                1 => ("record_task_requirements", json!({"settings":[
                    {"sourceId":"request_1","key":"players","expected":4},
                    {"sourceId":"invented_source","key":"players","expected":20}
                ],"ports":[],"forbiddenActions":[],"unverified":[]})),
                2 => {
                    let (content, failed) = messages.iter().find_map(|message| match message {
                        AssistantToolMessage::ToolResult { call_id, content, is_error, .. } if call_id=="draft-1" => Some((content, *is_error)),
                        _=>None,
                    }).unwrap();
                    let result:Value=serde_json::from_str(content).unwrap();
                    assert!(failed);
                    assert_eq!(result["ok"],false);
                    assert_eq!(result["data"]["itemCount"],1);
                    assert_eq!(result["data"]["accepted"],json!(["settings[0]"]));
                    assert_eq!(result["data"]["errors"][0]["field"],"settings[1].sourceId");
                    ("record_task_requirements",json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]}))
                }
                3 => ("finish_task_requirements",json!({})),
                _ => ("propose_operation",json!({"action":"none","reason":"Requirements recorded; no mutation requested."})),
            };
            std::future::ready(Ok(AssistantToolReply { content:String::new(),raw_message:Value::Null,
                calls:vec![AssistantToolCall {id:format!("draft-{turn}"),name:name.into(),arguments}] }))
        },
        |_| std::future::ready(Err(String::from("unexpected read"))), &|_| Ok(()),
    )).await.unwrap();
    let plan: Value = serde_json::from_str(&operation).unwrap();
    assert_eq!(turn, 4);
    assert_eq!(
        plan["taskRequirements"]["settings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(plan["taskRequirements"]["settings"][0]["expected"], 4);
    assert_eq!(
        plan["taskRequirements"]["settings"][0]["sourceText"],
        request
    );
}
