use super::*;

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires an existing local Ollama model; sends four real conversation turns and reads actual host metrics"]
async fn assistant_live_ollama_host_conversation() -> Result<(), String> {
    let settings = local_chat_test_settings().await?;
    let state = DesktopState::default();
    let slots = tokio::sync::Semaphore::new(1);
    let mut history = Vec::new();
    let mut factual_errors = Vec::new();
    for (class, prompt) in [
        ("greeting", "hi"),
        ("identity", "你是谁啊"),
        ("host", "我们本机配置是什么"),
        ("follow_up", "没懂"),
    ] {
        let input = AssistantRequestInput {
            conversation_id: None,
            settings: settings.clone(),
            prompt: prompt.into(),
            prior_requests: Vec::new(),
            conversation_messages: history.clone(),
            context: Some(r#"{"interfaceLanguage":"zh-CN"}"#.into()),
            selected_instance_id: None,
            selected_module_id: None,
        };
        let called = std::sync::Mutex::new(Vec::<String>::new());
        let measured = std::sync::Mutex::new(None::<Value>);
        let started = Instant::now();
        let result = resolve_assistant_task_intent_with_tools(
            &input,
            &[],
            &[],
            &slots,
            ASSISTANT_INTENT_TIMEOUT,
            |messages, tools| {
                let settings = &settings;
                let called = &called;
                async move {
                    let response = crate::assistant::run_assistant_tool_turn(
                        &AssistantRunInput {
                            settings: settings.clone(),
                            prompt_label: "Host conversation acceptance".into(),
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
                        .map_err(|_| "test observation lock")?
                        .extend(response.calls.iter().map(|call| call.name.clone()));
                    Ok(response)
                }
            },
            || async {
                let evidence = read_assistant_host_info(&state).await?;
                *measured.lock().map_err(|_| "test observation lock")? = Some(evidence.clone());
                Ok(evidence)
            },
        )
        .await;
        let calls = called.into_inner().map_err(|_| "test observation lock")?;
        let evidence = measured.into_inner().map_err(|_| "test observation lock")?;
        let content = match result {
            Ok(AssistantIntentResolution::Reply(content)) => content,
            other => {
                println!(
                    "ASSISTANT_HOST_CHAT_LIVE={}",
                    json!({"promptClass":class,"model":settings.model,
                    "nativeTools":calls,"outcome":"not_a_conversation_reply","elapsedMs":started.elapsed().as_millis()})
                );
                return Err(format!(
                    "The {class} turn did not produce a conversational reply: {other:?}"
                ));
            }
        };
        let chinese_chars = content
            .chars()
            .filter(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
            .count();
        println!(
            "ASSISTANT_HOST_CHAT_LIVE={}",
            json!({"promptClass":class,"model":settings.model,
            "nativeTools":calls,"chineseCharacters":chinese_chars,"hostEvidence":evidence,
            "reply":truncate_assistant_prompt_text(&content,8192),"elapsedMs":started.elapsed().as_millis()})
        );
        if content.trim().is_empty() || (class != "greeting" && chinese_chars < 2) {
            return Err(format!(
                "The {class} turn did not answer in the current conversation language."
            ));
        }
        if class == "host" {
            if calls
                .iter()
                .filter(|name| name.as_str() == "read_host_info")
                .count()
                != 1
            {
                return Err(
                    "The host question did not perform exactly one native host read.".into(),
                );
            }
            let evidence = evidence.ok_or("The host question has no measured evidence.")?;
            let name = evidence["cpu"]["name"]
                .as_str()
                .ok_or("The actual CPU name was unavailable.")?;
            let total = evidence["memory"]["totalBytes"]
                .as_u64()
                .ok_or("The actual memory size was unavailable.")?;
            let cpu_identifier = name
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .find(|word| word.len() >= 3 && word.bytes().any(|ch| ch.is_ascii_digit()))
                .unwrap_or(name);
            if !content
                .to_lowercase()
                .contains(&cpu_identifier.to_lowercase())
            {
                factual_errors.push("The hardware reply did not identify the measured CPU.");
            }
            if !host_reply_reports_memory_size(&content, total) {
                factual_errors.push("The hardware reply did not contain the measured memory size with a correct capacity unit.");
            }
        } else if class == "follow_up" {
            // Reply excludes an accepted resolve_task (which returns Resolved).
            // One rejected task call can be corrected into a plain reply or
            // a host read within the same bounded conversation, without work.
            let allowed = ["read_host_info", "resolve_task"];
            if calls.iter().any(|name| !allowed.contains(&name.as_str()))
                || allowed.iter().any(|allowed_name| {
                    calls
                        .iter()
                        .filter(|name| name.as_str() == *allowed_name)
                        .count()
                        > 1
                })
            {
                return Err("The explanation follow-up entered an unsupported workflow.".into());
            }
            // The transcript is emitted for semantic review: Chinese text alone
            // cannot prove that the model explained the preceding hardware reply.
        } else if !calls.is_empty() {
            return Err(format!(
                "The {class} conversation unnecessarily entered a tool workflow."
            ));
        }
        history.push(AssistantConversationMessage::User(prompt.into()));
        history.push(AssistantConversationMessage::Assistant(content));
    }
    if factual_errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} Review the complete emitted transcript.",
            factual_errors.join(" ")
        ))
    }
}

fn host_reply_reports_memory_size(content: &str, total_bytes: u64) -> bool {
    let content = content.to_ascii_lowercase();
    let bytes = content.as_bytes();
    let units = [
        ("tib", 1_099_511_627_776.0),
        ("tb", 1_000_000_000_000.0),
        ("gib", 1_073_741_824.0),
        ("gb", 1_000_000_000.0),
        ("mib", 1_048_576.0),
        ("mb", 1_000_000.0),
        ("kib", 1024.0),
        ("kb", 1000.0),
        ("bytes", 1.0),
        ("byte", 1.0),
        ("b", 1.0),
        ("字节", 1.0),
    ];
    let mut cursor = 0;
    while cursor < bytes.len() {
        if !bytes[cursor].is_ascii_digit() {
            cursor += 1;
            continue;
        }
        let start = cursor;
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_digit() || matches!(bytes[cursor], b'.' | b','))
        {
            cursor += 1;
        }
        if start > 0
            && (bytes[start - 1].is_ascii_alphanumeric() || matches!(bytes[start - 1], b'.' | b'-'))
        {
            continue;
        }
        let number = content[start..cursor].replace(',', "");
        let Ok(reported) = number.parse::<f64>() else {
            continue;
        };
        if !reported.is_finite() || reported <= 0.0 {
            continue;
        }
        let decimals = number
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len());
        if decimals > 12 {
            continue;
        }
        let suffix = content[cursor..].trim_start();
        for (unit, divisor) in units {
            let Some(rest) = suffix.strip_prefix(unit) else {
                continue;
            };
            if rest.starts_with(|ch: char| ch.is_ascii_alphanumeric())
                || rest.trim_start().starts_with('/')
            {
                continue;
            }
            let scale = 10_f64.powi(decimals as i32);
            let measured = total_bytes as f64 / divisor;
            // A displayed value can be rounded or truncated at its stated
            // precision. Require a capacity unit so CPU model digits cannot pass.
            let rounded = (measured * scale).round() / scale;
            let truncated = (measured * scale).floor() / scale;
            let tolerance = f64::EPSILON * measured.max(1.0) * 4.0;
            if (reported - rounded).abs() <= tolerance || (reported - truncated).abs() <= tolerance
            {
                return true;
            }
        }
    }
    false
}

#[test]
fn assistant_live_memory_evidence_accepts_display_precision_and_requires_capacity_units() {
    let measured = 100_516_388_864;
    for reply in [
        "总内存：约100GB，可用约75GB",
        "总内存约101 GB",
        "100.5 GB",
        "100.51GB",
        "100.52GB",
        "100,516,388,864 bytes",
    ] {
        assert!(host_reply_reports_memory_size(reply, measured), "{reply}");
    }
    for reply in [
        "CPU 9950X3D",
        "CPU 100516388864，内存16GB",
        "总内存100.50GB",
        "96GB",
        "100 GBit/s",
        "100GB/s",
        "100GB / s",
        "-100GB",
        "Model100GB",
    ] {
        assert!(!host_reply_reports_memory_size(reply, measured), "{reply}");
    }
    for reply in ["32GiB", "34 GB", "34.4GB", "34.35GB", "34.36GB"] {
        assert!(
            host_reply_reports_memory_size(reply, 32 * 1024 * 1024 * 1024),
            "{reply}"
        );
    }
    assert!(!host_reply_reports_memory_size(
        "32 GB",
        32 * 1024 * 1024 * 1024
    ));
}

pub(super) async fn local_chat_test_settings() -> Result<AssistantProviderSettings, String> {
    const BASE_URL: &str = "http://127.0.0.1:11434";
    let model = std::env::var("LANGAME_ASSISTANT_CHAT_TEST_MODEL")
        .unwrap_or_else(|_| "qwen3:4b-q4_K_M".into());
    if model.trim().is_empty() || model.len() > 128 || model.chars().any(char::is_control) {
        return Err("The local chat test model must be a nonempty model name.".into());
    }
    let installed = crate::assistant::list_ollama_models(Some(BASE_URL)).await?;
    if !installed.contains(&model) {
        return Err(
            "The selected local chat model is not installed; this test never downloads models."
                .into(),
        );
    }
    Ok(AssistantProviderSettings {
        provider: "ollama".into(),
        model,
        base_url: BASE_URL.into(),
        api_key: "synthetic-test-only".into(),
    })
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires an already running local Ollama service and an installed chat model; performs two real conversation requests"]
async fn assistant_live_ollama_answers_identity_and_capabilities_without_tasks()
-> Result<(), String> {
    const BASE_URL: &str = "http://127.0.0.1:11434";
    let model = match std::env::var("LANGAME_ASSISTANT_CHAT_TEST_MODEL") {
        Ok(value)
            if !value.trim().is_empty()
                && value.len() <= 128
                && !value.chars().any(char::is_control) =>
        {
            value.trim().to_owned()
        }
        Err(std::env::VarError::NotPresent) => String::from("qwen3:4b-q4_K_M"),
        _ => {
            return Err(String::from(
                "The optional local chat test model must be a nonempty model name.",
            ));
        }
    };
    let installed = crate::assistant::list_ollama_models(Some(BASE_URL))
        .await
        .map_err(|_| String::from("The local Ollama model catalog could not be read."))?;
    if !installed.contains(&model) {
        return Err(String::from(
            "The selected local chat model is not installed; this test never downloads models.",
        ));
    }
    let settings = AssistantProviderSettings {
        provider: String::from("ollama"),
        model: model.clone(),
        base_url: BASE_URL.into(),
        // An explicit non-secret placeholder bypasses system credential lookup.
        // The unauthenticated loopback Ollama service does not require this value.
        api_key: String::from("synthetic-test-only"),
    };
    // Advertise synthetic selections without reading any local server data.
    // Viewing a server must not turn an identity question into server work.
    let (instances, modules) = super::intent_tests::intent_catalog_data();
    let mut history = Vec::new();
    for (class, prompt) in [("identity", "你是谁？"), ("capabilities", "你能干啥？")] {
        let input = AssistantRequestInput {
            conversation_id: None,
            settings: settings.clone(),
            prompt: prompt.into(),
            prior_requests: Vec::new(),
            conversation_messages: history.clone(),
            context: None,
            selected_instance_id: Some(instances[0].id.clone()),
            selected_module_id: Some(instances[0].module_id.clone()),
        };
        let started = std::time::Instant::now();
        let result = resolve_assistant_task_intent(&input, &instances, &modules, None, None).await;
        let (reply_type, reply_bytes) = match &result {
            Ok(AssistantIntentResolution::Reply(content)) => ("reply", content.len()),
            Ok(AssistantIntentResolution::Resolved { .. }) => ("resolved_task", 0),
            Ok(AssistantIntentResolution::Clarification(_)) => ("clarification", 0),
            Err(_) => ("error", 0),
        };
        println!(
            "ASSISTANT_CHAT_LIVE={}",
            json!({
                "model":model,"promptClass":class,"replyType":reply_type,
                "replyBytes":reply_bytes,"elapsedMs":started.elapsed().as_millis()
            })
        );
        let content = match result {
            Ok(AssistantIntentResolution::Reply(content)) if !content.trim().is_empty() => content,
            Ok(_) => {
                return Err(format!(
                    "The {class} conversation did not produce a nonempty direct reply."
                ));
            }
            Err(_) => {
                return Err(format!(
                    "The {class} conversation failed; provider content was omitted from test output."
                ));
            }
        };
        history.push(AssistantConversationMessage::User(prompt.into()));
        history.push(AssistantConversationMessage::Assistant(content));
    }
    Ok(())
}
