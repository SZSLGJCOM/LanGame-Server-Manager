// Scripted text fixtures are adapted to the same native-tool state machine.
// Production never turns assistant message text into executable tool calls.
async fn run_assistant_investigation<M, MF, T, TF>(
    initial_prompt: String,
    initial_reads: Vec<AssistantReadTool>,
    model: M,
    read_tool: T,
) -> Result<String, String>
where
    M: FnMut(String) -> MF,
    MF: Future<Output = Result<String, String>>,
    T: FnMut(AssistantReadTool) -> TF,
    TF: Future<Output = Result<Value, String>>,
{
    run_assistant_investigation_checked(
        initial_prompt,
        initial_reads,
        model,
        read_tool,
        &|_| Ok(()),
    )
    .await
}

async fn run_assistant_investigation_checked<M, MF, T, TF>(
    initial_prompt: String,
    initial_reads: Vec<AssistantReadTool>,
    mut model: M,
    read_tool: T,
    validate_operation: &(dyn Fn(&str) -> Result<(), String> + Sync),
) -> Result<String, String>
where
    M: FnMut(String) -> MF,
    MF: Future<Output = Result<String, String>>,
    T: FnMut(AssistantReadTool) -> TF,
    TF: Future<Output = Result<Value, String>>,
{
    let mut turn = 0;
    run_assistant_tool_investigation(
        AssistantInvestigationContext {
            prompt: initial_prompt,
            initial_reads,
            tools: assistant_fixture_tools(),
            draft: None,
            instance: None,
            module: None,
            completion: AssistantInvestigationCompletion::OperationProposal,
        },
        |messages, _| {
            turn += 1;
            let response = model(assistant_fixture_transcript(&messages));
            let call_id = format!("fixture_{turn}");
            async move { Ok(assistant_fixture_reply(&response.await?, &call_id)) }
        },
        read_tool,
        validate_operation,
    )
    .await
}

fn assistant_fixture_reply(content: &str, id: &str) -> AssistantToolReply {
    let calls = serde_json::from_str::<Value>(content)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .map(|mut arguments| {
            let name = arguments
                .remove("tool")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| String::from("propose_operation"));
            vec![AssistantToolCall {
                id: id.into(),
                name,
                arguments: Value::Object(arguments),
            }]
        })
        .unwrap_or_default();
    AssistantToolReply {
        content: content.into(),
        calls,
        raw_message: Value::Null,
    }
}

fn assistant_fixture_transcript(messages: &[AssistantToolMessage]) -> String {
    messages
        .iter()
        .map(|message| match message {
            AssistantToolMessage::User(content) => content.clone(),
            AssistantToolMessage::Assistant(reply) => reply.content.clone(),
            AssistantToolMessage::ToolResult { content, .. } => content.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assistant_fixture_tools() -> Vec<AssistantToolDefinition> {
    [
        "list_module_settings",
        "read_module_settings",
        "search_module_settings",
        "search_game_docs",
        "read_game_doc",
        "list_settings",
        "read_runtime",
        "read_settings",
        "list_config_files",
        "search_settings",
        "read_config_file",
        "read_mod_state",
        "read_workshop_items",
        "inspect_installed_mods",
        "inspect_launch",
        "propose_operation",
    ]
    .into_iter()
    .map(|name| AssistantToolDefinition {
        name: name.into(),
        description: String::new(),
        parameters: json!({"type":"object"}),
    })
    .collect()
}
