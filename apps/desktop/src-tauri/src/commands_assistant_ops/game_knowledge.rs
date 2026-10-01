fn assistant_knowledge_module_id<'a>(
    instance: Option<&'a InstanceDetails>,
    module: Option<&'a ModuleDetails>,
) -> Result<&'a str, String> {
    match (instance, module) {
        (Some(instance), Some(module)) if instance.summary.module_id != module.summary.id => {
            Err("The selected game no longer matches this server. Refresh its context.".into())
        }
        (Some(instance), _) => Ok(&instance.summary.module_id),
        (_, Some(module)) => Ok(&module.summary.id),
        _ => Err("Select one supported game before reading its documentation.".into()),
    }
}

async fn read_assistant_game_knowledge(
    state: &DesktopState,
    _storage: &StorageBootstrap,
    instance: Option<&InstanceDetails>,
    module: Option<&ModuleDetails>,
    tool: AssistantReadTool,
) -> Result<Value, String> {
    let module_id = assistant_knowledge_module_id(instance, module)?.to_string();
    let lease = state.begin_storage_context_operation("assistant documentation read")?;
    let permit = ASSISTANT_EVIDENCE_SLOTS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)))
        .clone().try_acquire_owned()
        .map_err(|_| "Documentation readers are busy. Retry after the pending reads finish.".to_string())?;
    let library = state.knowledge.library().await?;
    // Keep the slot and storage lease until the real task exits, even if the
    // assistant turn times out during a model load or SQLite operation.
    let worker = crate::state::spawn_storage_context_task(&lease, async move {
        let _permit = permit;
        assistant_game_knowledge_evidence(&library, &module_id, tool).await
    });
    tokio::time::timeout(ASSISTANT_EVIDENCE_READ_TIMEOUT, worker).await
        .map_err(|_| "Documentation lookup timed out; no complete evidence was supplied.".to_string())?
        .map_err(|error| format!("Documentation lookup failed: {error}"))?
}

async fn assistant_game_knowledge_evidence(
    library: &app_knowledge::KnowledgeLibrary,
    module_id: &str,
    tool: AssistantReadTool,
) -> Result<Value, String> {
    let evidence = match tool {
        AssistantReadTool::SearchGameDocs { query, offset } => serde_json::to_value(
            library.search(module_id, &query, offset).await
                .map_err(|error| error.to_string())?,
        ),
        AssistantReadTool::ReadGameDoc { document_id, offset } => serde_json::to_value(
            library.read(module_id, &document_id, offset).await
                .map_err(|error| error.to_string())?,
        ),
        _ => return Err("This request is not a game documentation read.".into()),
    };
    let mut evidence = evidence.map_err(|_| "Game documentation could not be encoded.")?;
    evidence["scope"] = json!("game_documentation");
    evidence["instanceObserved"] = json!(false);
    Ok(evidence)
}

#[cfg(test)]
#[path = "game_knowledge_tests.rs"]
mod game_knowledge_tests;

#[cfg(test)]
#[path = "configured_live_tests.rs"]
mod configured_live_tests;
