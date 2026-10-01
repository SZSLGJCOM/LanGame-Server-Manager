const ASSISTANT_INTENT_REQUIRED_FIELDS: [&str; 4] =
    ["goal", "target", "preserveExistingMods", "priorRequestIds"];

fn assistant_intent_protocol_error(reason: String) -> String {
    json!({
        "code":"assistant_request_interpretation_failed",
        "message":"The assistant could not interpret this request. No operation was executed.",
        "reason":reason
    })
    .to_string()
}

fn validate_assistant_intent_reply_size(
    reply: &crate::assistant::AssistantToolReply,
) -> Result<(), String> {
    let limit = if reply.calls.is_empty() {
        ASSISTANT_CONVERSATION_RESPONSE_BYTES
    } else {
        ASSISTANT_INTENT_RESPONSE_BYTES
    };
    // Native thinking/signatures have their own transport limit and are replayed
    // intact. Count the business reply once, as the investigation loop does.
    if serde_json::to_vec(&(&reply.content, &reply.calls))
        .map_err(|error| error.to_string())?
        .len()
        > limit
    {
        return Err(String::from(
            "Assistant task interpretation exceeded its response limit.",
        ));
    }
    Ok(())
}

fn assistant_intent_error_feedback(
    reply: &crate::assistant::AssistantToolReply,
    reason: &str,
) -> Option<Vec<crate::assistant::AssistantToolMessage>> {
    // Only native calls with valid, distinct protocol identities can be answered.
    // No member of a rejected batch may establish a task or execute an operation.
    if reply.calls.is_empty() || reply.calls.len() > 32 {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    for call in &reply.calls {
        if !assistant_intent_id_is_valid(&call.id)
            || call.name.is_empty()
            || call.name.len() > 64
            || !call
                .name
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'))
            || !seen.insert(&call.id)
        {
            return None;
        }
    }
    Some(reply.calls.iter().map(|call| {
        let missing: Vec<_> = ASSISTANT_INTENT_REQUIRED_FIELDS.iter()
            .filter(|field| call.name == "resolve_task"
                && !call.arguments.as_object().is_some_and(|args| args.contains_key(**field)))
            .copied().collect();
        crate::assistant::AssistantToolMessage::ToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content: json!({"ok":false, "missingFields":missing, "error":reason,
                "guidance":"No task was accepted or operation executed. For ordinary conversation, reply directly as text. For actual host facts, use read_host_info once with {} and a new call ID; never guess or replace failed evidence with defaults. For server evidence or operations, issue one corrected resolve_task call with a new call ID and a catalog-bound target. target=none is only valid with a nonempty clarification question and cannot begin work. Use the advertised schemas and catalog. Preserve the user's language and constraints; never invent missing authorization."}).to_string(),
            is_error: true,
        }
    }).collect())
}

fn parse_assistant_intent_response(
    reply: crate::assistant::AssistantToolReply,
    catalog: &AssistantIntentCatalog<'_>,
) -> Result<AssistantIntentResolution, String> {
    validate_assistant_intent_reply_size(&reply)?;
    if reply.calls.is_empty() {
        let visible = strip_assistant_think_blocks(&reply.content);
        let content = visible.trim();
        if content.is_empty() {
            return Err(String::from(
                "Assistant returned an empty conversation reply.",
            ));
        }
        // Text remains text, including JSON examples. Only native tool calls
        // can establish a task and enter the operation state machine.
        return Ok(AssistantIntentResolution::Reply(
            redact_assistant_provider_text(content),
        ));
    }
    if reply.calls.len() != 1
        || reply.calls[0].name != "resolve_task"
        || !assistant_intent_id_is_valid(&reply.calls[0].id)
    {
        return Err(String::from(
            "The tool call batch is unsupported. Conversation can be answered directly; server work requires one valid resolve_task call.",
        ));
    }
    let arguments = &reply.calls[0].arguments;
    if !arguments.as_object().is_some_and(|object| {
        ASSISTANT_INTENT_REQUIRED_FIELDS
            .iter()
            .all(|key| object.contains_key(*key))
    }) {
        let missing: Vec<_> = ASSISTANT_INTENT_REQUIRED_FIELDS
            .iter()
            .filter(|field| {
                !arguments
                    .as_object()
                    .is_some_and(|args| args.contains_key(**field))
            })
            .copied()
            .collect();
        return Err(format!(
            "Assistant task interpretation is missing required fields: {}.",
            missing.join(", ")
        ));
    }
    let resolved: AssistantIntentArguments = serde_json::from_value(arguments.clone())
        .map_err(|_| String::from("Assistant returned malformed task interpretation."))?;
    if let Some(clarification) = resolved.clarification {
        if resolved.goal != AssistantTaskGoal::Inspect
            || resolved.target != AssistantIntentTarget::None
            || resolved.instance_id.is_some()
            || resolved.module_id.is_some()
            || !resolved.preserve_existing_mods
            || clarification.trim().is_empty()
            || clarification.len() > 1024
            || !resolved.prior_request_ids.is_empty()
        {
            return Err(String::from(
                "Assistant clarification cannot also authorize a task.",
            ));
        }
        return Ok(AssistantIntentResolution::Clarification(
            redact_assistant_provider_text(clarification.trim()),
        ));
    }
    if resolved.target == AssistantIntentTarget::None {
        return Err(String::from(
            "A task requires a catalog-bound evidence target. target=none is only valid with a nonempty clarification question. Answer ordinary conversation directly or use read_host_info for manager-host facts.",
        ));
    }
    if resolved.goal == AssistantTaskGoal::Inspect && !resolved.preserve_existing_mods {
        return Err(String::from(
            "Read-only tasks cannot change the Mod preservation policy.",
        ));
    }
    let instance = resolved
        .instance_id
        .as_ref()
        .and_then(|id| catalog.instances.iter().find(|instance| instance.id == *id));
    let module = resolved
        .module_id
        .as_ref()
        .and_then(|id| catalog.modules.iter().find(|module| module.id == *id));
    let valid_target = match resolved.target {
        AssistantIntentTarget::ExistingInstance => instance
            .zip(module)
            .is_some_and(|(instance, module)| instance.module_id == module.id),
        AssistantIntentTarget::NewInstance => {
            resolved.instance_id.is_none()
                && module.is_some()
                && matches!(
                    resolved.goal,
                    AssistantTaskGoal::ApplyChange
                        | AssistantTaskGoal::PrepareService
                        | AssistantTaskGoal::LaunchService
                )
        }
        AssistantIntentTarget::Module => {
            resolved.instance_id.is_none()
                && module.is_some()
                && matches!(
                    resolved.goal,
                    AssistantTaskGoal::Inspect | AssistantTaskGoal::ApplyChange
                )
        }
        AssistantIntentTarget::None => false,
    };
    if !valid_target {
        return Err(String::from(
            "Assistant task target is unknown, inconsistent, or incompatible with the requested goal.",
        ));
    }
    Ok(AssistantIntentResolution::Resolved {
        request: AssistantTaskRequest {
            goal: resolved.goal,
            preserve_existing_mods: resolved.preserve_existing_mods,
        },
        target: resolved.target,
        original_request: catalog.original_request(&resolved.prior_request_ids)?,
        instance_id: resolved.instance_id,
        module_id: resolved.module_id,
    })
}
