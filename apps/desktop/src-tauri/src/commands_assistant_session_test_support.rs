fn seed_assistant_session(
    state: &DesktopState,
    settings: &AssistantProviderSettings,
    user_request: &str,
    assistant_reply: &str,
) -> Result<String, String> {
    let binding = {
        let current = state.app_state.read().map_err(|_| "fixture catalog lock")?;
        crate::assistant_sessions::AssistantSessionBinding {
            provider: settings.provider.trim().to_string(),
            model: settings.model.trim().to_string(),
            base_url: settings.base_url.trim().trim_end_matches('/').to_string(),
            storage_identity: [
                current.settings.servers_root.clone(),
                current.settings.games_root.clone(),
                current.settings.modules_root.clone(),
                current.settings.steamcmd_root.clone(),
                current.storage.database_path.clone(),
                current.storage.migrations_path.clone(),
            ],
        }
    };
    let lease = state.assistant_sessions.begin(None, binding, true)?;
    let session = lease.session();
    session.register_user_request(user_request)?;
    session.append_messages(vec![
        crate::assistant::AssistantToolMessage::User(user_request.into()),
        crate::assistant::AssistantToolMessage::Assistant(crate::assistant::AssistantToolReply {
            content: assistant_reply.into(),
            calls: Vec::new(),
            raw_message: json!({"role":"assistant","content":assistant_reply}),
        }),
    ])?;
    Ok(session.id().to_string())
}
