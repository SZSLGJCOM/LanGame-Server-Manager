use super::intent_tests::{intent_input, intent_reply};
use super::*;
use crate::assistant::{AssistantToolCall, AssistantToolMessage, AssistantToolReply};

fn host_call(arguments: Value) -> AssistantToolReply {
    AssistantToolReply {
        content: String::new(),
        calls: vec![AssistantToolCall {
            id: "host-1".into(),
            name: "read_host_info".into(),
            arguments,
        }],
        raw_message: Value::Null,
    }
}

fn text_reply(content: &str) -> AssistantToolReply {
    AssistantToolReply {
        content: content.into(),
        calls: Vec::new(),
        raw_message: Value::Null,
    }
}

#[test]
fn assistant_host_tool_rejects_target_overrides_commands_and_mixed_batches() {
    let tool = assistant_host_info_tool();
    assert_eq!(tool.parameters["additionalProperties"], false);
    assert_eq!(tool.parameters["properties"], json!({}));
    validate_assistant_host_call(&host_call(json!({})), false).unwrap();
    for arguments in [
        Value::Null,
        json!([]),
        json!({"path":"C:/private"}),
        json!({"command":"whoami"}),
        json!({"instanceId":"server-a"}),
    ] {
        assert!(validate_assistant_host_call(&host_call(arguments), false).is_err());
    }
    let mut mixed = host_call(json!({}));
    mixed.calls.extend(intent_reply(json!({})).calls);
    assert!(validate_assistant_host_call(&mixed, false).is_err());
    assert!(validate_assistant_host_call(&host_call(json!({})), true).is_err());
}

#[test]
fn assistant_host_evidence_projects_only_hardware_facts_and_marks_unknown_fields() {
    let snapshot = SystemSnapshot {
        cpu_name: "Fixture CPU".into(),
        cpu_physical_cores: 8,
        cpu_logical_cores: 16,
        memory_total_bytes: 32 * 1024 * 1024 * 1024,
        memory_available_bytes: 12 * 1024 * 1024 * 1024,
        disk_label: "PRIVATE_PATH".into(),
        disk_volume_name: "PRIVATE_VOLUME".into(),
        ..SystemSnapshot::default()
    };
    let evidence = assistant_host_info_evidence(&snapshot, "fixture", 0);
    assert_eq!(evidence["scope"], "manager_host");
    assert_eq!(evidence["cpu"]["name"], "Fixture CPU");
    assert_eq!(evidence["cpu"]["reportedLogicalCores"], 16);
    assert_eq!(
        evidence["cpu"]["profileScope"],
        "first_processor_or_process_fallback"
    );
    assert!(evidence["cpu"].get("logicalCores").is_none());
    assert!(evidence["cpu"].get("totalHostCores").is_none());
    assert!(
        evidence["unavailableFields"]
            .as_array()
            .unwrap()
            .contains(&json!("cpu.totalHostCores"))
    );
    assert_eq!(
        evidence["memory"]["totalBytes"],
        32_u64 * 1024 * 1024 * 1024
    );
    assert_eq!(evidence["memory"]["totalDisplay"], "32.00 GiB (34.36 GB)");
    assert_eq!(
        evidence["memory"]["availableDisplay"],
        "12.00 GiB (12.88 GB)"
    );
    assert_eq!(evidence["os"]["platform"], std::env::consts::OS);
    assert!(evidence["os"]["version"].is_null());
    assert!(!evidence.to_string().contains("PRIVATE_"));
    assert!(evidence.to_string().len() < 4096);
    let missing = assistant_host_info_evidence(&SystemSnapshot::default(), "fixture", 0);
    assert!(missing["cpu"]["name"].is_null());
    assert!(missing["cpu"]["reportedLogicalCores"].is_null());
    assert!(missing["memory"]["totalBytes"].is_null());
    assert!(missing["memory"]["availableBytes"].is_null());
    assert!(missing["memory"]["totalDisplay"].is_null());
    assert!(missing["memory"]["availableDisplay"].is_null());
}

#[test]
fn assistant_host_evidence_defines_core_counts_and_missing_data_without_inventing_topology() {
    for snapshot in [
        SystemSnapshot::default(),
        SystemSnapshot {
            cpu_physical_cores: 16,
            cpu_logical_cores: 32,
            ..SystemSnapshot::default()
        },
    ] {
        let evidence = assistant_host_info_evidence(&snapshot, "fixture", 0);
        let semantics = &evidence["fieldSemantics"];
        assert_eq!(
            semantics["cpu.reportedPhysicalCores"]["isCpuChipOrSocketCount"],
            false
        );
        assert_eq!(
            semantics["cpu.reportedLogicalCores"]["isProgramOrTaskCountLimit"],
            false
        );
        assert_eq!(semantics["unavailableFields"]["provesAbsence"], false);
        for field in [
            "cpu.reportedPhysicalCores",
            "cpu.reportedLogicalCores",
            "unavailableFields",
        ] {
            assert!(
                semantics[field]["meaning"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            );
        }
        assert!(evidence["cpu"].get("socketCount").is_none());
        assert!(evidence["cpu"].get("maximumTasks").is_none());
        assert!(evidence.to_string().len() < 4096);
    }
    // The tool description remains available on a plain-text follow-up, even
    // when that turn does not repeat the host measurement.
    let description = assistant_host_info_tool().description;
    assert!(description.contains("not a count of CPU chips or sockets"));
    assert!(description.contains("not a limit on the number of programs"));
    assert!(description.contains("not that hardware or software is absent"));
}

#[tokio::test]
async fn assistant_host_native_tool_reads_without_a_selected_server_and_preserves_roles() {
    let mut input = intent_input("我们本机配置是什么");
    input.selected_instance_id = None;
    input.selected_module_id = None;
    input.context = Some(r#"{"interfaceLanguage":"en"}"#.into());
    input.conversation_messages = vec![
        AssistantConversationMessage::User("hi".into()),
        AssistantConversationMessage::Assistant("Hello.".into()),
        AssistantConversationMessage::User("你是谁啊".into()),
        AssistantConversationMessage::Assistant("我是 LAN。".into()),
    ];
    let slots = tokio::sync::Semaphore::new(1);
    let mut turns = 0;
    let mut reads = 0;
    let facts = json!({"scope":"manager_host","cpu":{"name":"Fixture CPU"},"memory":{"totalBytes":34359738368_u64}});
    let result = resolve_assistant_task_intent_with_tools(
        &input, &[], &[], &slots, Duration::from_secs(1),
        |messages, tools| {
            turns += 1;
            assert!(tools.iter().any(|tool| tool.name == "read_host_info"));
            assert!(matches!(&messages[1], AssistantToolMessage::User(text) if text == "hi"));
            assert!(matches!(&messages[4], AssistantToolMessage::Assistant(reply) if reply.content == "我是 LAN。"));
            let reply = if turns == 1 {
                assert!(matches!(messages.last(), Some(AssistantToolMessage::User(text)) if text == "我们本机配置是什么"));
                host_call(json!({}))
            } else {
                assert!(matches!(&messages[messages.len() - 2], AssistantToolMessage::Assistant(reply) if reply.calls[0].name == "read_host_info"));
                let Some(AssistantToolMessage::ToolResult { call_id, name, content, is_error }) = messages.last() else { panic!("missing native host result"); };
                assert_eq!(call_id, "host-1");
                assert_eq!(name, "read_host_info");
                assert!(!*is_error);
                let result: Value = serde_json::from_str(content).unwrap();
                assert_eq!(result["data"], facts);
                text_reply("这台管理端电脑使用 Fixture CPU，内存为 32 GiB。")
            };
            std::future::ready(Ok(reply))
        },
        || { reads += 1; std::future::ready(Ok(facts.clone())) },
    ).await.unwrap();
    assert!(
        matches!(result, AssistantIntentResolution::Reply(text) if text.contains("Fixture CPU"))
    );
    assert_eq!((turns, reads), (2, 1));
    assert_eq!(slots.available_permits(), 1);
}

#[tokio::test]
async fn assistant_host_tool_failure_is_visible_and_repeated_reads_are_bounded() {
    let input = intent_input("本机内存是多少？");
    let slots = tokio::sync::Semaphore::new(1);
    let mut turns = 0;
    let mut reads = 0;
    let result = resolve_assistant_task_intent_with_tools(
        &input,
        &[],
        &[],
        &slots,
        Duration::from_secs(1),
        |messages, _| {
            turns += 1;
            if turns > 1 {
                let Some(AssistantToolMessage::ToolResult {
                    content, is_error, ..
                }) = messages.last()
                else {
                    panic!("missing host error result");
                };
                assert!(*is_error);
                let error: Value = serde_json::from_str(content).unwrap();
                assert_eq!(error["ok"], false);
                assert!(error.get("data").is_none());
                assert!(error["guidance"].as_str().unwrap().contains("defaults"));
            }
            let response = if turns < 3 {
                let mut reply = host_call(json!({}));
                reply.calls[0].id = format!("host-{turns}");
                reply
            } else {
                text_reply("本次读取失败，因此我还无法确认这台电脑的内存。")
            };
            std::future::ready(Ok(response))
        },
        || {
            reads += 1;
            std::future::ready(Err("Host probe timed out".into()))
        },
    )
    .await
    .unwrap();
    assert!(matches!(result, AssistantIntentResolution::Reply(_)));
    assert_eq!((turns, reads), (3, 1));
}

#[tokio::test]
async fn assistant_host_tool_is_never_prefetched_for_chat_or_invalid_batches() {
    for invalid_batch in [false, true] {
        let input = intent_input("没懂");
        let slots = tokio::sync::Semaphore::new(1);
        let mut turns = 0;
        let result = resolve_assistant_task_intent_with_tools(
            &input,
            &[],
            &[],
            &slots,
            Duration::from_secs(1),
            |_, _| {
                turns += 1;
                let reply = if invalid_batch && turns == 1 {
                    let mut reply = host_call(json!({}));
                    reply.calls.extend(intent_reply(json!({})).calls);
                    reply
                } else {
                    text_reply("刚才的意思是：我还没有读取这台电脑的硬件，不能确认它的配置。")
                };
                std::future::ready(Ok(reply))
            },
            std::future::pending::<Result<Value, String>>,
        )
        .await
        .unwrap();
        assert!(matches!(result, AssistantIntentResolution::Reply(_)));
        assert_eq!(turns, if invalid_batch { 2 } else { 1 });
    }
}

#[tokio::test]
async fn assistant_host_worker_cancellation_keeps_capacity_until_collection_exits() {
    let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let worker_slots = slots.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let waiter = tokio::spawn(async move {
        run_assistant_read_worker_with(worker_slots, Duration::from_secs(5), move || {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        })
        .await
    });
    started_rx.await.unwrap();
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert!(slots.try_acquire().is_err());
    release_tx.send(()).unwrap();
    let permit = tokio::time::timeout(Duration::from_secs(5), slots.acquire())
        .await
        .unwrap()
        .unwrap();
    drop(permit);
    assert_eq!(slots.available_permits(), 1);
}
