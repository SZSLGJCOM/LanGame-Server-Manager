use super::*;
use std::future::ready;
use std::sync::atomic::{AtomicUsize, Ordering};

fn read_only_context() -> AssistantInvestigationContext<'static> {
    AssistantInvestigationContext {
        prompt: "Inspect the current server. Do not change anything.".into(),
        initial_reads: Vec::new(),
        tools: assistant_fixture_tools(),
        draft: None,
        instance: None,
        module: None,
        completion: AssistantInvestigationCompletion::ReadOnlyAnswer,
    }
}

fn text_reply(content: &str) -> AssistantToolReply {
    AssistantToolReply {
        content: content.into(),
        calls: Vec::new(),
        raw_message: json!({"role":"assistant", "content":content}),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_read_only_investigation_answers_naturally_after_a_read() {
    let reads = AtomicUsize::new(0);
    let validations = AtomicUsize::new(0);
    let mut turns = 0;
    let answer = "当前实例已停止，日志显示端口冲突；本次只进行了检查。";
    let output = Box::pin(run_assistant_tool_investigation(
        read_only_context(),
        |messages, _| {
            turns += 1;
            ready(Ok(if turns == 1 {
                assert!(matches!(messages.last(), Some(AssistantToolMessage::User(control)) if control.contains("answer naturally in message text")));
                AssistantToolReply {
                    content: String::new(),
                    calls: vec![AssistantToolCall {
                        id: "read-runtime".into(), name: "read_runtime".into(), arguments: json!({"lines":2}),
                    }],
                    raw_message: Value::Null,
                }
            } else {
                assert!(messages.iter().any(|message| matches!(message,
                    AssistantToolMessage::ToolResult { call_id, is_error:false, content, .. }
                    if call_id == "read-runtime" && content.contains("port conflict"))));
                text_reply(answer)
            }))
        },
        |request| {
            assert!(matches!(request, AssistantReadTool::ReadRuntime { lines:2 }));
            reads.fetch_add(1, Ordering::SeqCst);
            ready(Ok(json!({"running":false, "log":"port conflict"})))
        },
        &|operation| {
            validations.fetch_add(1, Ordering::SeqCst);
            assert_eq!(serde_json::from_str::<Value>(operation).unwrap()["action"], "none");
            Ok(())
        },
    )).await.unwrap();
    let operation: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(operation, json!({"action":"none", "reason":answer}));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(validations.load(Ordering::SeqCst), 1);
    assert_eq!(turns, 2);
}

#[tokio::test(flavor = "current_thread")]
async fn native_read_only_answer_never_extracts_an_operation_from_message_json() {
    for text in [
        r#"{"action":"start_server","instanceId":"server-a"}"#,
        "```json\n{\"action\":\"customize_config\",\"settingsPatch\":{\"max_players\":12}}\n```",
    ] {
        let output = Box::pin(run_assistant_tool_investigation(
            read_only_context(),
            |_, _| ready(Ok(text_reply(text))),
            |_| ready(Err("Unexpected read".into())),
            &|operation| {
                let value: Value = serde_json::from_str(operation).unwrap();
                assert_eq!(value["action"], "none");
                assert_eq!(value.as_object().unwrap().len(), 2);
                Ok(())
            },
        ))
        .await
        .unwrap();
        assert!(
            serde_json::from_str::<Value>(&output).unwrap()["reason"]
                .as_str()
                .unwrap()
                .contains("action")
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_read_only_answer_rejects_empty_hidden_and_oversized_content() {
    for text in [
        String::new(),
        " \n ".into(),
        "<think>private reasoning</think>".into(),
        "x".repeat(ASSISTANT_READ_ONLY_ANSWER_BYTES + 1),
    ] {
        let validations = AtomicUsize::new(0);
        let result = Box::pin(run_assistant_tool_investigation(
            read_only_context(),
            |_, _| ready(Ok(text_reply(&text))),
            |_| ready(Err("Unexpected read".into())),
            &|_| {
                validations.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        ))
        .await;
        assert!(result.is_err());
        assert_eq!(validations.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_read_only_answer_is_redacted_and_still_passes_the_validator() {
    let marker = ["read", "only", "fixture"].join("-");
    let text =
        format!("<think>not a visible answer</think>Observed stopped state.\npassword={marker}");
    let error = Box::pin(run_assistant_tool_investigation(
        read_only_context(),
        |_, _| ready(Ok(text_reply(&text))),
        |_| ready(Err("Unexpected read".into())),
        &|operation| {
            assert!(!operation.contains(&marker));
            assert!(!operation.contains("not a visible answer"));
            assert!(operation.contains("[REDACTED]"));
            Err("Read-only validation rejected this answer".into())
        },
    ))
    .await
    .unwrap_err();
    assert_eq!(error, "Read-only validation rejected this answer");
}

#[tokio::test(flavor = "current_thread")]
async fn native_mutating_investigation_cannot_finish_with_a_text_claim_or_json_plan() {
    for text in [
        "The server has been repaired.",
        r#"{"action":"start_server"}"#,
    ] {
        let mut context = read_only_context();
        context.completion = AssistantInvestigationCompletion::OperationProposal;
        let validations = AtomicUsize::new(0);
        let mut turns = 0;
        let error = Box::pin(run_assistant_tool_investigation(
            context,
            |_, _| {
                turns += 1;
                ready(Ok(text_reply(text)))
            },
            |_| ready(Err("Unexpected read".into())),
            &|_| {
                validations.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        ))
        .await
        .unwrap_err();
        assert!(error.contains("Text alone is not an operation"));
        assert_eq!(turns, 3);
        assert_eq!(validations.load(Ordering::SeqCst), 0);
    }
}
