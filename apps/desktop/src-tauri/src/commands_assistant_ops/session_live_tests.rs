use super::*;
use crate::assistant::{AssistantToolMessage, AssistantToolReply};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Default)]
struct RecordedHostEvidence {
    native_reply: Option<AssistantToolReply>,
    facts: Option<Value>,
}

fn require_recorded_host_pair(
    messages: &[AssistantToolMessage],
    recorded: &RecordedHostEvidence,
) -> Result<(), String> {
    let original = recorded
        .native_reply
        .as_ref()
        .ok_or("No native host call was recorded.")?;
    let [call] = original.calls.as_slice() else {
        return Err("The recorded host observation must contain exactly one native call.".into());
    };
    let facts = recorded
        .facts
        .as_ref()
        .ok_or("No actual host facts were recorded.")?;
    let matching: Vec<_> = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| match message {
            AssistantToolMessage::Assistant(reply)
                if reply.calls.iter().any(|item| item.id == call.id) =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();
    let [index] = matching.as_slice() else {
        return Err(
            "The outgoing model request lost or duplicated the original native host call.".into(),
        );
    };
    let AssistantToolMessage::Assistant(reply) = &messages[*index] else {
        return Err("The original host call is no longer an assistant protocol message.".into());
    };
    if serde_json::to_value(reply).map_err(|error| error.to_string())?
        != serde_json::to_value(original).map_err(|error| error.to_string())?
    {
        return Err(
            "The outgoing request changed the original provider envelope or call arguments.".into(),
        );
    }
    match messages.get(*index + 1) {
        Some(AssistantToolMessage::ToolResult {
            call_id,
            name,
            content,
            is_error,
        }) if call_id == &call.id && name == "read_host_info" && !is_error => {
            let result: Value = serde_json::from_str(content).map_err(|error| error.to_string())?;
            if result["ok"] != true
                || result["scope"] != "manager_host"
                || result["observedAtUnixMs"]
                    .as_u64()
                    .is_none_or(|value| value == 0)
                || &result["data"] != facts
            {
                return Err(
                    "The outgoing request changed the observed facts, scope or timestamp.".into(),
                );
            }
            Ok(())
        }
        _ => Err(
            "The outgoing request has no matching native result immediately after the host call."
                .into(),
        ),
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires an existing local Ollama model; reads host metrics and verifies three real turns through backend-owned history"]
async fn assistant_live_ollama_retains_native_host_evidence_across_session_turns()
-> Result<(), String> {
    let settings = super::intent_live_tests::local_chat_test_settings().await?;
    let state = DesktopState::default();
    let binding = assistant_session_binding(&state, &settings)?;
    let created = state
        .assistant_sessions
        .begin(None, binding.clone(), false)?;
    let conversation_id = created.session().id().to_string();
    drop(created);
    let slots = tokio::sync::Semaphore::new(1);
    let recorded = Mutex::new(RecordedHostEvidence::default());
    let host_reads = AtomicUsize::new(0);
    for (class, prompt) in [
        (
            "measured_profile",
            "请实际读取管理端这台电脑的处理器资料，用阿拉伯数字告诉我报告的物理核心数和逻辑处理器数。未测到的事实不要猜。",
        ),
        (
            "referenced_fact",
            "先重复刚才测得的逻辑处理器数量，再解释：它是不是这台电脑最多能同时运行的程序数量？请简短说明。",
        ),
        (
            "ordinary_chat",
            "谢谢，我们先不讨论电脑了。给我一句适合雨天的轻松问候。",
        ),
    ] {
        let lease =
            state
                .assistant_sessions
                .begin(Some(&conversation_id), binding.clone(), true)?;
        let session = lease.session();
        let prior_requests = session.source_user_requests()?;
        session.register_user_request(prompt)?;
        let input = AssistantRequestInput {
            conversation_id: Some(conversation_id.clone()),
            settings: settings.clone(),
            prompt: prompt.into(),
            prior_requests,
            conversation_messages: Vec::new(),
            context: Some(r#"{"interfaceLanguage":"zh-CN"}"#.into()),
            selected_instance_id: None,
            selected_module_id: None,
        };
        let called = Mutex::new(Vec::<String>::new());
        let requests_with_native_evidence = AtomicUsize::new(0);
        let started = Instant::now();
        let result = resolve_assistant_task_intent_with_session_tools(
            &input,
            AssistantConversationScope {
                instances: &[],
                modules: &[],
                session: Some(&session),
            },
            &slots,
            ASSISTANT_INTENT_TIMEOUT,
            |messages, tools| {
                let settings = &settings;
                let called = &called;
                let recorded = &recorded;
                let requests_with_native_evidence = &requests_with_native_evidence;
                async move {
                    let evidence = recorded
                        .lock()
                        .map_err(|_| "Host evidence test lock failed.")?
                        .clone();
                    if evidence.facts.is_some() {
                        require_recorded_host_pair(&messages, &evidence)?;
                        requests_with_native_evidence.fetch_add(1, Ordering::SeqCst);
                    } else if class != "measured_profile" {
                        return Err(
                            "A later turn reached the provider without recorded host evidence."
                                .into(),
                        );
                    }
                    let reply = crate::assistant::run_assistant_tool_turn(
                        &AssistantRunInput {
                            settings: settings.clone(),
                            prompt_label: "Session evidence acceptance".into(),
                            prompt: String::new(),
                            context: String::new(),
                        },
                        ASSISTANT_INTENT_SYSTEM_PROMPT,
                        &messages,
                        &tools,
                    )
                    .await?;
                    called
                        .lock()
                        .map_err(|_| "Tool observation test lock failed.")?
                        .extend(reply.calls.iter().map(|call| call.name.clone()));
                    if reply.calls.iter().any(|call| call.name == "read_host_info") {
                        let mut evidence = recorded
                            .lock()
                            .map_err(|_| "Host evidence test lock failed.")?;
                        if evidence.native_reply.is_none() {
                            evidence.native_reply = Some(reply.clone());
                        }
                    }
                    Ok(reply)
                }
            },
            || async {
                host_reads.fetch_add(1, Ordering::SeqCst);
                let facts = read_assistant_host_info(&state).await?;
                recorded
                    .lock()
                    .map_err(|_| "Host evidence test lock failed.")?
                    .facts = Some(facts.clone());
                Ok(facts)
            },
        )
        .await;
        let calls = called
            .into_inner()
            .map_err(|_| "Tool observation test lock failed.")?;
        let retained_requests = requests_with_native_evidence.load(Ordering::SeqCst);
        let content = match result {
            Ok(AssistantIntentResolution::Reply(content)) => content,
            Ok(_) => {
                return Err(format!(
                    "The {class} turn requested a task or clarification instead of conversation."
                ));
            }
            Err(error) => return Err(format!("The {class} turn failed: {error}")),
        };
        let evidence = recorded
            .lock()
            .map_err(|_| "Host evidence test lock failed.")?
            .clone();
        require_recorded_host_pair(&session.messages()?, &evidence)?;
        let chinese_characters = content
            .chars()
            .filter(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
            .count();
        println!(
            "ASSISTANT_SESSION_CHAT_LIVE={}",
            json!({
                "promptClass":class, "model":settings.model, "revision":session.revision(),
                "nativeTools":calls, "requestsContainingOriginalNativeEvidence":retained_requests,
                "hostReadsTotal":host_reads.load(Ordering::SeqCst), "chineseCharacters":chinese_characters,
                "measuredCpu":evidence.facts.as_ref().map(|facts| &facts["cpu"]),
                "reply":truncate_assistant_prompt_text(&content,8192), "elapsedMs":started.elapsed().as_millis(),
            })
        );
        if chinese_characters < 4 || retained_requests == 0 {
            return Err(format!(
                "The {class} turn lacked a Chinese answer or provider-bound native evidence."
            ));
        }
        if class == "measured_profile" {
            if calls.len() != 1
                || calls[0] != "read_host_info"
                || host_reads.load(Ordering::SeqCst) != 1
            {
                return Err(
                    "The processor request did not perform exactly one native host read.".into(),
                );
            }
            let facts = evidence
                .facts
                .as_ref()
                .ok_or("The processor request has no measured facts.")?;
            for field in ["reportedPhysicalCores", "reportedLogicalCores"] {
                let count = facts["cpu"][field]
                    .as_u64()
                    .ok_or("The requested CPU profile count was unavailable.")?;
                if !content.contains(&count.to_string()) {
                    return Err(format!(
                        "The processor answer omitted the actual {field} value."
                    ));
                }
            }
        } else if !calls.is_empty() || host_reads.load(Ordering::SeqCst) != 1 {
            return Err(format!(
                "The {class} turn invoked an unnecessary tool instead of using the recorded conversation."
            ));
        }
        if class == "referenced_fact" {
            let logical = evidence
                .facts
                .as_ref()
                .and_then(|facts| facts["cpu"]["reportedLogicalCores"].as_u64())
                .ok_or("The original logical processor count is unavailable.")?;
            if !content.contains(&logical.to_string()) {
                return Err(
                    "The follow-up did not retain the measured logical processor count.".into(),
                );
            }
        }
        // Numeric checks and native-message identity prove evidence continuity,
        // not semantic correctness; review the emitted explanations as well.
    }
    Ok(())
}
