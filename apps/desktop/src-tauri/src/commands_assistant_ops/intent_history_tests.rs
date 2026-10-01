use super::intent_tests::{intent_arguments, intent_catalog_data, intent_input, intent_reply};
use super::*;

#[test]
fn older_user_sources_keep_stable_selectable_ids_and_exact_constraints() {
    let mut input = intent_input("继续最初那项准备工作，仍然不要启动。");
    input.prior_requests = (1..=ASSISTANT_INTENT_SOURCE_REQUESTS)
        .map(|index| format!("Original user request {index}; preserve saved data."))
        .collect();
    let (instances, modules) = intent_catalog_data();
    let catalog = AssistantIntentCatalog::new(&input, &instances, &modules).unwrap();
    let prompt: Value = serde_json::from_str(&catalog.prompt(&input).unwrap()).unwrap();
    assert_eq!(
        prompt["priorUserRequestArchive"]["total"],
        ASSISTANT_INTENT_SOURCE_REQUESTS
    );
    assert_eq!(prompt["priorUserRequests"][0]["id"], "prior-123");
    assert_eq!(
        prompt["priorUserRequests"].as_array().unwrap().len(),
        ASSISTANT_INTENT_PRIOR_REQUESTS
    );
    let schema = catalog.tool().parameters;
    let ids = schema["properties"]["priorRequestIds"]["items"]["enum"]
        .as_array()
        .unwrap();
    assert_eq!(ids.first().unwrap(), "prior-1");
    assert_eq!(ids.last().unwrap(), "prior-128");
    assert_eq!(
        schema["properties"]["priorRequestIds"]["maxItems"],
        ASSISTANT_INTENT_PRIOR_REQUESTS
    );
    let original = catalog
        .original_request(&["prior-1".into(), "prior-128".into()])
        .unwrap();
    assert!(original.contains(&input.prior_requests[0]));
    assert!(original.contains(&input.prior_requests[127]));
    assert!(original.ends_with(&input.prompt));
    assert!(
        catalog
            .original_request(&["prior-128".into(), "prior-1".into()])
            .is_err()
    );
    assert!(catalog.original_request(&["prior-129".into()]).is_err());
    let too_many = (1..=ASSISTANT_INTENT_PRIOR_REQUESTS + 1)
        .map(|index| format!("prior-{index}"))
        .collect::<Vec<_>>();
    assert!(catalog.original_request(&too_many).is_err());
}

#[test]
fn recent_source_excerpts_fit_context_without_truncating_bound_originals() {
    let mut input = intent_input("Continue the specified task.");
    input.prior_requests = vec![
        format!("{}Never start the server.", "x".repeat(8000));
        ASSISTANT_INTENT_PRIOR_REQUESTS
    ];
    let catalog = AssistantIntentCatalog::new(&input, &[], &[]).unwrap();
    let encoded = catalog.prompt(&input).unwrap();
    assert!(encoded.len() < 8 * 1024);
    let prompt: Value = serde_json::from_str(&encoded).unwrap();
    assert!(
        prompt["priorUserRequests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|source| source["truncated"] == true)
    );
    let selected = (1..=ASSISTANT_INTENT_PRIOR_REQUESTS)
        .map(|index| format!("prior-{index}"))
        .collect::<Vec<_>>();
    let original = catalog.original_request(&selected).unwrap();
    assert_eq!(
        original.matches("Never start the server.").count(),
        ASSISTANT_INTENT_PRIOR_REQUESTS
    );
    assert_eq!(
        original.matches('x').count(),
        8000 * ASSISTANT_INTENT_PRIOR_REQUESTS
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_history_pages_recover_old_request_tail_before_resolving_exact_scope() {
    let store = crate::assistant_sessions::AssistantSessionStore::default();
    let lease = store
        .begin(
            None,
            AssistantSessionBinding {
                provider: "ollama".into(),
                model: "fixture".into(),
                base_url: "http://127.0.0.1:11434".into(),
                storage_identity: std::array::from_fn(|index| format!("fixture-path-{index}")),
            },
            true,
        )
        .unwrap();
    let session = lease.session();
    let original = format!(
        "准备所选游戏的服务器。{}最后约束：不要启动，不要删除已有数据。",
        "参数说明。".repeat(450)
    );
    session.register_user_request(&original).unwrap();
    for index in 1..10 {
        session
            .register_user_request(&format!("Unrelated discussion {index}"))
            .unwrap();
    }
    let mut input = intent_input("继续最初那项准备任务。");
    input.prior_requests = session.source_user_requests().unwrap();
    session.register_user_request(&input.prompt).unwrap();
    let (instances, modules) = intent_catalog_data();
    let slots = tokio::sync::Semaphore::new(1);
    let mut turns = 0;
    let mut recovered = String::new();
    let result = resolve_assistant_task_intent_with_session_tools(
        &input, AssistantConversationScope { instances:&instances, modules:&modules, session:Some(&session) },
        &slots, Duration::from_secs(1),
        |messages, _| {
            turns += 1;
            let mut byte_offset = 0;
            if let Some(AssistantToolMessage::ToolResult { name, content, is_error, .. }) = messages.last() {
                assert_eq!(name, "read_session_history");
                assert!(!*is_error);
                let result: Value = serde_json::from_str(content).unwrap();
                let page = &result["data"];
                assert_eq!(page["source"], "user_requests");
                recovered.push_str(page["messages"][0]["excerpt"].as_str().unwrap());
                byte_offset = page["nextMessageOffsetBytes"].as_u64().unwrap() as usize;
                if page["nextOffset"] == 1 {
                    let record: Value = serde_json::from_str(&recovered).unwrap();
                    assert_eq!(record, json!({"id":"prior-1","request":original}));
                    let mut arguments = intent_arguments("prepare_service", "new_instance", None, Some("dontstarve"));
                    arguments["priorRequestIds"] = json!(["prior-1"]);
                    return std::future::ready(Ok(intent_reply(arguments)));
                }
            }
            std::future::ready(Ok(AssistantToolReply {
                content:String::new(), raw_message:Value::Null,
                calls:vec![AssistantToolCall { id:format!("history-{turns}"), name:"read_session_history".into(),
                    arguments:json!({"source":"user_requests","offset":0,"limit":1,"messageOffsetBytes":byte_offset}) }],
            }))
        }, || async { Err("No host request was expected".into()) },
    ).await.unwrap();
    assert!(turns > 2);
    let AssistantIntentResolution::Resolved {
        original_request,
        target,
        request,
        ..
    } = result
    else {
        panic!("expected resolved original task");
    };
    assert_eq!(target, AssistantIntentTarget::NewInstance);
    assert_eq!(request.goal, AssistantTaskGoal::PrepareService);
    assert_eq!(
        original_request,
        format!(
            "Previous user request:\n{original}\n\nCurrent user request:\n{}",
            input.prompt
        )
    );
}
