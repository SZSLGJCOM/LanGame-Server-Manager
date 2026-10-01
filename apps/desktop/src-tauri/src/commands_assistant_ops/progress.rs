async fn assistant_model_reply_in_session(
    input: &AssistantRunInput,
    system_prompt: &str,
    messages: &[crate::assistant::AssistantToolMessage],
    tools: &[crate::assistant::AssistantToolDefinition],
    session: Option<&std::sync::Arc<AssistantSession>>,
) -> Result<crate::assistant::AssistantToolReply, String> {
    let revision = session.map(|session| session.revision());
    if let Some(session) = session {
        session.check_active()?;
        session.flush().await?;
        session.record_progress(revision.unwrap_or_default(), "model_start", "", None)?;
    }
    let observer = |delta: &str| {
        if let Some(session) = session {
            session.record_progress(revision.unwrap_or_default(), "text_delta", delta, None)?;
        }
        Ok(())
    };
    crate::assistant::run_assistant_tool_turn_streaming(
        input,
        system_prompt,
        messages,
        tools,
        &observer,
    )
    .await
}

fn assistant_tool_progress(
    session: Option<&std::sync::Arc<AssistantSession>>,
    revision: u64,
    name: &str,
    kind: &str,
) -> Result<(), String> {
    if let Some(session) = session {
        session.record_progress(revision, kind, "", Some(name))?;
    }
    Ok(())
}
