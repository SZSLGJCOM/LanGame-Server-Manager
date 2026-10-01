use super::*;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

const LIVE_REQUEST_LIMIT: usize = 8;
const LIVE_TRIAL_TIMEOUT: Duration = Duration::from_secs(180);
const LIVE_MODULE_ID: &str = "minecraft";

struct ReadReceipt {
    tool: &'static str,
    content: String,
    delivered: bool,
}

struct ConfiguredTrial {
    input: AssistantRunInput,
    deadline: tokio::time::Instant,
    requests: AtomicUsize,
    receipts: Mutex<Vec<ReadReceipt>>,
}

fn configured_live_settings() -> Result<AssistantProviderSettings, String> {
    if std::env::var("LANGAME_ASSISTANT_LIVE").as_deref() != Ok("1") {
        return Err("Set LANGAME_ASSISTANT_LIVE=1 to authorize the real-model trial.".into());
    }
    let mut configured = Vec::new();
    for name in [
        "LANGAME_ASSISTANT_LIVE_PROVIDER",
        "LANGAME_ASSISTANT_LIVE_MODEL",
        "LANGAME_ASSISTANT_LIVE_BASE_URL",
    ] {
        configured.push(match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
            Err(std::env::VarError::NotPresent) => None,
            _ => return Err(format!("{name} must be a nonempty UTF-8 value.")),
        });
    }
    let (provider, model, base_url) = match configured.as_slice() {
        // This is the only default remote destination authorized for this trial.
        [None, None, None] => (
            "openai-compatible".into(),
            "deepseek-flash".into(),
            "https://api.deepseek.com".into(),
        ),
        [Some(provider), Some(model), Some(base_url)] => {
            (provider.clone(), model.clone(), base_url.clone())
        }
        _ => {
            return Err(
                "Set provider, model and base URL together to select a different authorized service."
                    .into(),
            );
        }
    };
    if !matches!(
        provider.as_str(),
        "openai-compatible" | "anthropic-compatible" | "ollama"
    ) {
        return Err(
            "The live trial requires one of the three supported provider protocols.".into(),
        );
    }
    let url = reqwest::Url::parse(&base_url).map_err(|_| "Invalid live service URL.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("The live service URL cannot contain credentials, query or fragment.".into());
    }
    Ok(AssistantProviderSettings {
        provider,
        model,
        base_url,
        // Production resolves this provider/endpoint's existing SystemKeyring entry.
        // This trial never reads a key into its own state, environment or report.
        api_key: String::new(),
    })
}

fn source_module(root: &Path) -> Result<ModuleDetails, String> {
    let descriptor = app_modules::discover_modules(root)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|module| module.summary.id == LIVE_MODULE_ID)
        .ok_or("The source catalog has no Minecraft module.")?;
    Ok(ModuleDetails {
        summary: descriptor.summary,
        schema_json: descriptor.schema_json,
        default_ports: descriptor.default_ports,
        install: descriptor.install,
        process: descriptor.process,
        workshop: descriptor.workshop,
        mods: None,
        runtime: descriptor.runtime,
    })
}

impl ConfiguredTrial {
    async fn model_turn(
        &self,
        phase: &str,
        system: &str,
        messages: &[AssistantToolMessage],
        tools: &[AssistantToolDefinition],
    ) -> Result<AssistantToolReply, String> {
        if tokio::time::Instant::now() >= self.deadline {
            return Err("The live trial exhausted its shared time budget.".into());
        }
        let request = self
            .requests
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                (count < LIVE_REQUEST_LIMIT).then_some(count + 1)
            })
            .map_err(|_| "The live trial exhausted its eight-request budget.")?
            + 1;
        self.observe_delivered_receipts(messages)?;
        let public = Mutex::new(String::new());
        let observer = |text: &str| {
            public
                .lock()
                .map_err(|_| "Live text observation is unavailable.")?
                .push_str(text);
            Ok(())
        };
        let reply = tokio::time::timeout_at(
            self.deadline,
            crate::assistant::run_assistant_tool_turn_streaming(
                &self.input,
                system,
                messages,
                tools,
                &observer,
            ),
        )
        .await
        .map_err(|_| "The live trial exhausted its shared time budget.".to_string())
        .and_then(|result| result);
        let public = public
            .into_inner()
            .map_err(|_| "Live text observation is unavailable.")?;
        println!(
            "ASSISTANT_CONFIGURED_LIVE_TURN={}",
            json!({"phase":phase,"request":request,"ok":reply.is_ok(),
                "publicText":truncate_assistant_prompt_text(&redact_assistant_provider_text(&public),8192)})
        );
        let reply = reply?;
        if !reply.content.trim().is_empty()
            && reply
                .content
                .chars()
                .filter(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
                .count()
                < 2
        {
            return Err(
                "A Chinese request received English-only public progress or an answer.".into(),
            );
        }
        if reply.calls.iter().any(|call| {
            !tools.iter().any(|tool| tool.name == call.name)
                || !matches!(call.name.as_str(), "search_game_docs" | "read_game_doc")
        }) {
            return Err("The model requested a tool outside this isolated read-only trial.".into());
        }
        Ok(reply)
    }

    async fn read_document(
        &self,
        library: &app_knowledge::KnowledgeLibrary,
        request: AssistantReadTool,
    ) -> Result<Value, String> {
        let tool = match &request {
            AssistantReadTool::SearchGameDocs { .. } => "search_game_docs",
            AssistantReadTool::ReadGameDoc { .. } => "read_game_doc",
            _ => {
                return Err(
                    "Only synchronized game-document reads are authorized in this trial.".into(),
                );
            }
        };
        let data =
            assistant_game_knowledge_evidence(library, LIVE_MODULE_ID, request.clone()).await?;
        let content = assistant_read_result_text(&request, &json!({"ok":true,"data":data}));
        self.receipts
            .lock()
            .map_err(|_| "Live receipt observation is unavailable.")?
            .push(ReadReceipt {
                tool,
                content,
                delivered: false,
            });
        Ok(data)
    }

    fn observe_delivered_receipts(&self, messages: &[AssistantToolMessage]) -> Result<(), String> {
        let mut receipts = self
            .receipts
            .lock()
            .map_err(|_| "Live receipt observation is unavailable.")?;
        for message in messages {
            if let AssistantToolMessage::ToolResult {
                name,
                content,
                is_error: false,
                ..
            } = message
            {
                let receipt = receipts
                    .iter_mut()
                    .find(|receipt| receipt.tool == name && receipt.content == *content)
                    .ok_or("The outgoing native history contains an unobserved tool receipt.")?;
                receipt.delivered = true;
            }
        }
        Ok(())
    }

    fn cited_document_sources(&self, answer: &str) -> Result<Vec<String>, String> {
        let receipts = self
            .receipts
            .lock()
            .map_err(|_| "Live receipt observation is unavailable.")?;
        for tool in ["search_game_docs", "read_game_doc"] {
            if !receipts
                .iter()
                .any(|receipt| receipt.tool == tool && receipt.delivered)
            {
                return Err(format!(
                    "The model did not consume an actual successful {tool} receipt."
                ));
            }
        }
        let mut cited = Vec::new();
        for receipt in receipts
            .iter()
            .filter(|receipt| receipt.tool == "read_game_doc" && receipt.delivered)
        {
            let value: Value =
                serde_json::from_str(&receipt.content).map_err(|error| error.to_string())?;
            if value["ok"] != true
                || value["data"]["scope"] != "game_documentation"
                || value["data"]["instanceObserved"] != false
            {
                return Err("A knowledge receipt lost its read-only documentation scope.".into());
            }
            if let Some(url) = value["data"]["source"]["url"].as_str()
                && answer.contains(url)
                && !cited.iter().any(|source| source == url)
            {
                cited.push(url.to_string());
            }
        }
        if cited.is_empty() {
            return Err(
                "The final answer did not cite a source from a consumed full-document receipt."
                    .into(),
            );
        }
        Ok(cited)
    }
}

fn require_chinese_reply(reply: &AssistantToolReply) -> Result<(), String> {
    if !reply.calls.is_empty()
        || reply
            .content
            .chars()
            .filter(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
            .count()
            < 2
    {
        return Err("The conversation did not produce a direct Chinese-language reply.".into());
    }
    Ok(())
}

async fn exercise_configured_trial(
    trial: &ConfiguredTrial,
    root: &Path,
    library: &app_knowledge::KnowledgeLibrary,
) -> Result<Vec<String>, String> {
    let mut messages = Vec::new();
    for (phase, prompt) in [
        (
            "greeting",
            "你好呀！今天先不管服务器，来打个招呼。我们这群朋友叫松果小队。",
        ),
        ("identity", "你叫什么？平时能陪我们做什么？简单聊两句就好。"),
        (
            "memory",
            "还记得刚才我说我们这群朋友叫什么吗？只告诉我名字就行。",
        ),
    ] {
        messages.push(AssistantToolMessage::User(prompt.into()));
        let reply = trial
            .model_turn(phase, ASSISTANT_INTENT_SYSTEM_PROMPT, &messages, &[])
            .await?;
        require_chinese_reply(&reply)?;
        if phase == "identity"
            && !reply
                .content
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .any(|word| word.eq_ignore_ascii_case("LAN"))
        {
            return Err("The model did not retain LAN's identity from the shared persona.".into());
        }
        if phase == "memory" && !reply.content.contains("松果小队") {
            return Err("The model did not retain the earlier synthetic user fact.".into());
        }
        messages.push(AssistantToolMessage::Assistant(reply));
    }
    let module = source_module(root)?;
    let mut tools = assistant_investigation_tools(None, Some(&module), false);
    tools.retain(|tool| matches!(tool.name.as_str(), "search_game_docs" | "read_game_doc"));
    if tools.len() != 2 {
        return Err("The production game-document tools are unavailable.".into());
    }
    let context = AssistantInvestigationContext {
        prompt: "我想给朋友开一个 Minecraft Java 服务器。请查阅内置资料，打开相关完整参考，再用中文说说开服前要准备什么，附上支持建议的来源链接。现在只了解步骤，不创建或启动服务器。".into(),
        initial_reads: Vec::new(), tools, draft: None, instance: None, module: Some(&module),
        completion: AssistantInvestigationCompletion::ReadOnlyAnswer,
    };
    let output = Box::pin(run_assistant_tool_investigation(
        context,
        |messages, mut tools| {
            // The production loop adds a final limitation tool; this trial exposes
            // only the two source-document tools and cannot invoke any other reader.
            tools.retain(|tool| matches!(tool.name.as_str(), "search_game_docs" | "read_game_doc"));
            Box::pin(async move {
                trial
                    .model_turn(
                        "game_docs",
                        ASSISTANT_OPERATION_SYSTEM_PROMPT,
                        &messages,
                        &tools,
                    )
                    .await
            })
        },
        |request| Box::pin(trial.read_document(library, request)),
        &|operation| {
            let value: Value =
                serde_json::from_str(operation).map_err(|error| error.to_string())?;
            if value["action"] != "none" {
                return Err("The trial cannot propose an operation.".into());
            }
            Ok(())
        },
    ))
    .await?;
    let result: Value = serde_json::from_str(&output).map_err(|error| error.to_string())?;
    let answer = result["reason"]
        .as_str()
        .ok_or("No documentation answer was returned.")?;
    if answer
        .chars()
        .filter(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
        .count()
        < 2
    {
        return Err("The documentation answer did not use the requested Chinese language.".into());
    }
    trial.cited_document_sources(answer)
}

/// An explicit, bounded real-model trial over synthetic conversation and source
/// documentation only. Success is evidence for this provider/model and scope;
/// it does not establish server execution, arbitrary factual accuracy or freshness.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires LANGAME_ASSISTANT_LIVE=1 and a configured authorized model; uses its existing SystemKeyring credential"]
async fn assistant_live_configured_chat_memory_and_game_docs() -> Result<(), String> {
    let settings = configured_live_settings()?;
    let cache = isolated_live_knowledge_root()?;
    let trial = ConfiguredTrial {
        input: AssistantRunInput {
            settings,
            prompt_label: "Isolated LAN conversation and documentation trial".into(),
            prompt: String::new(),
            context: String::new(),
        },
        deadline: tokio::time::Instant::now() + LIVE_TRIAL_TIMEOUT,
        requests: AtomicUsize::new(0),
        receipts: Mutex::new(Vec::new()),
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let library = app_knowledge::KnowledgeLibrary::open(&cache, &root)
        .await
        .map_err(|error| error.to_string())?;
    let outcome = tokio::time::timeout_at(trial.deadline, async {
        let status = library.status().await.map_err(|error| error.to_string())?;
        if !status.model.ready || !status.games.iter().any(|game| game.module_id == LIVE_MODULE_ID && game.sources.iter().any(|source| source.document_count > 0 && source.chunk_count > 0)) {
            return Err("The isolated knowledge cache must already contain the local model and synchronized Minecraft documents; this trial never downloads or synchronizes them.".into());
        }
        exercise_configured_trial(&trial, &root, &library).await
    }).await
        .map_err(|_| "The live trial exhausted its shared time budget.".to_string())
        .and_then(|result| result);
    // Also close on provider/protocol failures and the shared timeout, before
    // returning the test's result. No desktop/production database is opened.
    library.close().await;
    println!(
        "ASSISTANT_CONFIGURED_LIVE={}",
        json!({
            "provider":trial.input.settings.provider,"model":trial.input.settings.model,
            "endpoint":trial.input.settings.base_url,"scope":"synthetic_chat_and_isolated_synchronized_game_documents_only",
            "maxRequests":LIVE_REQUEST_LIMIT,"requests":trial.requests.load(Ordering::SeqCst),
            "timeoutSeconds":LIVE_TRIAL_TIMEOUT.as_secs(),"ok":outcome.is_ok(),
            "citedSources":outcome.as_ref().ok(),
            "error":outcome.as_ref().err().map(|error| redact_assistant_provider_text(error))
        })
    );
    outcome.map(|_| ())
}

fn isolated_live_knowledge_root() -> Result<PathBuf, String> {
    let configured = std::env::var_os("LANGAME_KNOWLEDGE_LIVE_ROOT")
        .filter(|value| !value.is_empty())
        .ok_or("Set LANGAME_KNOWLEDGE_LIVE_ROOT to an explicitly prepared, isolated synchronized cache.")?;
    validate_live_knowledge_root(
        &PathBuf::from(configured),
        &app_storage::StoragePaths::default().app_data_root,
    )
}

fn validate_live_knowledge_root(path: &Path, production: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() || !path.join("library.sqlite3").is_file() {
        return Err("LANGAME_KNOWLEDGE_LIVE_ROOT must be an absolute existing synchronized library directory.".into());
    }
    let path = path
        .canonicalize()
        .map_err(|error| format!("Cannot resolve isolated knowledge cache: {error}"))?;
    let production = production
        .canonicalize()
        .map_err(|error| format!("Cannot establish the production-data boundary: {error}"))?;
    if live_path_is_within(&path, &production) {
        return Err(
            "The live trial cannot use a cache inside LGSM's application data directory.".into(),
        );
    }
    let database = path
        .join("library.sqlite3")
        .canonicalize()
        .map_err(|error| format!("Cannot resolve isolated knowledge database: {error}"))?;
    if !live_path_is_within(&database, &path) {
        return Err("The live trial database must belong to its isolated cache directory.".into());
    }
    Ok(path)
}

fn live_path_is_within(path: &Path, root: &Path) -> bool {
    let mut parts = path.components();
    root.components().all(|expected| {
        parts.next().is_some_and(|actual| {
            if cfg!(windows) {
                actual
                    .as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&expected.as_os_str().to_string_lossy())
            } else {
                actual == expected
            }
        })
    })
}

#[test]
fn configured_live_knowledge_requires_an_existing_isolated_cache() {
    let root =
        std::env::temp_dir().join(format!("lgsm-live-cache-contract-{}", uuid::Uuid::new_v4()));
    let production = root.join("production");
    let isolated = root.join("isolated");
    std::fs::create_dir_all(production.join("knowledge")).unwrap();
    std::fs::create_dir_all(&isolated).unwrap();
    std::fs::write(production.join("knowledge/library.sqlite3"), b"fixture").unwrap();
    std::fs::write(isolated.join("library.sqlite3"), b"fixture").unwrap();
    let accepted = validate_live_knowledge_root(&isolated, &production);
    let production_result =
        validate_live_knowledge_root(&production.join("knowledge"), &production);
    let relative = validate_live_knowledge_root(Path::new("isolated"), &production);
    let missing = validate_live_knowledge_root(&root.join("missing"), &production);
    std::fs::remove_dir_all(&root).unwrap();
    assert!(accepted.is_ok());
    assert!(
        production_result
            .unwrap_err()
            .contains("application data directory")
    );
    assert!(relative.is_err());
    assert!(missing.is_err());
}
