use crate::assistant::{
    AssistantToolCall, AssistantToolDefinition, AssistantToolMessage, AssistantToolReply,
};

const ASSISTANT_INVESTIGATION_CALL_LIMIT: usize = 20;
const ASSISTANT_REQUIREMENT_DRAFT_CALL_LIMIT: usize = 8;
const ASSISTANT_READ_ONLY_ANSWER_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AssistantInvestigationCompletion {
    OperationProposal,
    ReadOnlyAnswer,
}

struct AssistantInvestigationContext<'a> {
    prompt: String,
    initial_reads: Vec<AssistantReadTool>,
    tools: Vec<AssistantToolDefinition>,
    draft: Option<AssistantRequirementsDraft>,
    instance: Option<&'a InstanceDetails>,
    module: Option<&'a ModuleDetails>,
    completion: AssistantInvestigationCompletion,
}

struct AssistantInvestigationHistory {
    messages: Vec<AssistantToolMessage>,
    reads: usize,
    read_limit: usize,
    draft_calls: usize,
    calls: usize,
    rejected: usize,
    call_ids: HashSet<String>,
    pending_reads: std::collections::VecDeque<AssistantReadTool>,
    mod_evidence: AssistantModEvidenceFollowUp,
}

#[cfg(test)]
async fn run_assistant_tool_investigation<M, MF, T, TF>(
    context: AssistantInvestigationContext<'_>,
    model: M,
    read_tool: T,
    validate_operation: &(dyn Fn(&str) -> Result<(), String> + Sync),
) -> Result<String, String>
where
    M: FnMut(Vec<AssistantToolMessage>, Vec<AssistantToolDefinition>) -> MF,
    MF: Future<Output = Result<AssistantToolReply, String>>,
    T: FnMut(AssistantReadTool) -> TF,
    TF: Future<Output = Result<Value, String>>,
{
    run_assistant_tool_investigation_in_session(
        context,
        model,
        read_tool,
        validate_operation,
        None,
        None,
    )
    .await
}

async fn run_assistant_tool_investigation_in_session<M, MF, T, TF>(
    mut context: AssistantInvestigationContext<'_>,
    mut model: M,
    mut read_tool: T,
    validate_operation: &(dyn Fn(&str) -> Result<(), String> + Sync),
    session: Option<&std::sync::Arc<AssistantSession>>,
    draft_slot: Option<&StdMutex<Option<AssistantRequirementsDraft>>>,
) -> Result<String, String>
where
    M: FnMut(Vec<AssistantToolMessage>, Vec<AssistantToolDefinition>) -> MF,
    MF: Future<Output = Result<AssistantToolReply, String>>,
    T: FnMut(AssistantReadTool) -> TF,
    TF: Future<Output = Result<Value, String>>,
{
    let mut prompt = format!("{}\n{ASSISTANT_INVESTIGATION_GUIDE}", context.prompt);
    if context
        .instance
        .is_some_and(|instance| instance.summary.module_id == "dontstarve")
    {
        prompt.push_str("\nOptional installed-metadata adapter guidance: ");
        prompt.push_str(ASSISTANT_DST_EVIDENCE_GUIDANCE);
    }
    if let Some(draft) = &context.draft
        && draft.is_active()
    {
        prompt.push_str(&format!(
            "\nOriginal request references:\n{}",
            draft.source_catalog()
        ));
    }
    if let Some(session) = session {
        session.pin_context(&prompt)?;
    }
    let mut messages = session
        .map(|session| session.messages())
        .transpose()?
        .unwrap_or_default();
    messages.push(AssistantToolMessage::User(prompt));
    let mut history = AssistantInvestigationHistory {
        messages,
        reads: 0,
        read_limit: if session.is_some() {
            128
        } else {
            ASSISTANT_INVESTIGATION_STEPS
        },
        draft_calls: 0,
        calls: 0,
        rejected: 0,
        call_ids: HashSet::new(),
        pending_reads: std::mem::take(&mut context.initial_reads).into(),
        mod_evidence: AssistantModEvidenceFollowUp::default(),
    };
    let tools = context.visible_tools()?;
    history.check_budget(&tools).map_err(|_| {
        String::from(
            "The initial investigation exceeds the context budget; no operation was executed.",
        )
    })?;
    let outcome = async {
    let call_limit = if session.is_some() { 128 } else { ASSISTANT_INVESTIGATION_CALL_LIMIT };
    for _ in 0..call_limit {
        if let Some(session) = session {
            session.check_active()?;
            session.replace_messages(history.messages.clone())?;
            history.messages = session.messages()?;
        }
        let tools = context.visible_tools()?;
        while let Some(request) = history.pending_reads.pop_front() {
            history.reserve_read()?;
            let result = wrap_assistant_read_result(assistant_cancellable_read(session, read_tool(request.clone())).await);
            history.mod_evidence.observe(
                &request,
                &result,
                history.reads,
                &mut history.pending_reads,
            );
            let content = format!(
                "\nRead {}: {}\n{}\n",
                history.reads,
                redact_assistant_provider_text(
                    &serde_json::to_string(&request).map_err(|error| error.to_string())?
                ),
                assistant_read_result_text(&request, &result)
            );
            history.append_evidence(AssistantToolMessage::User(content), &tools)?;
            assistant_trace_read(&request, &result);
        }
        let control = assistant_investigation_control(
            history.reads,
            history.read_limit,
            context.draft_pending(),
            context.completion,
        );
        let mut request_messages = history.messages.clone();
        request_messages.push(AssistantToolMessage::User(control));
        assistant_check_conversation_budget(&request_messages, &tools)?;
        let reply = assistant_cancellable_read(session, model(request_messages, tools.clone())).await?;
        #[cfg(test)]
        if std::env::var("LANGAME_ASSISTANT_LIVE").as_deref() == Ok("1") {
            eprintln!(
                "ASSISTANT_LIVE_MODEL_RESPONSE={}",
                redact_assistant_provider_text(
                    &serde_json::to_string(&reply.calls).map_err(|error| error.to_string())?
                )
            );
        }
        let calls = reply.calls.clone();
        let read_only_answer = (calls.is_empty()
            && context.completion == AssistantInvestigationCompletion::ReadOnlyAnswer)
            .then(|| reply.content.clone());
        // Preserve the exact assistant turn before its results, including failed
        // proposals. Provider-specific signed/thinking blocks stay paired with it.
        history
            .messages
            .push(AssistantToolMessage::Assistant(reply));
        history.check_budget(&tools)?;
        if calls.is_empty() {
            if let Some(answer) = read_only_answer {
                let operation = assistant_read_only_answer(&answer)?;
                validate_operation(&operation)?;
                return Ok(operation);
            }
            history.reject("Use exactly one final proposal tool or the supplied read tools. Text alone is not an operation.")?;
            history.messages.push(AssistantToolMessage::User(String::from(
                "Response rejected; no operation was executed. Use the supplied native tools; do not return a JSON plan as message text.",
            )));
            continue;
        }
        for call in &calls {
            if !history.call_ids.insert(call.id.clone()) {
                return Err(String::from(
                    "Assistant repeated a tool call ID; no operation was executed.",
                ));
            }
        }
        if calls.iter().any(|call| {
            matches!(
                call.name.as_str(),
                "propose_operation" | "report_limitation"
            )
        }) && calls.len() != 1
        {
            history.reject(
                "A final proposal must be the only call in its turn; read results first.",
            )?;
            for call in calls {
                history.push_result(&call, Err(String::from("A final proposal must be the only call in its turn; no operation was executed.")), &tools)?;
            }
            continue;
        }
        for call in calls {
            history.calls += 1;
            if history.calls > call_limit {
                return Err(String::from(
                    "Assistant investigation reached its total call limit; no operation was executed.",
                ));
            }
            if !tools.iter().any(|tool| tool.name == call.name) {
                let error = String::from(
                    "This tool is unavailable in the current task phase or target scope.",
                );
                history.reject(&error)?;
                history.push_result(&call, Err(error), &tools)?;
                continue;
            }
            if call.name == "report_limitation" {
                let operation = assistant_limitation_reason(&call.arguments)
                    .map(|reason| json!({"action":"none", "reason":reason}).to_string())
                    .and_then(|operation| {
                        validate_operation(&operation)?;
                        Ok(operation)
                    });
                match operation {
                    Ok(operation) => {
                        history.push_result(&call, Ok(json!({"status":"finished_without_operation","executed":false})), &tools)?;
                        return Ok(operation);
                    }
                    Err(error) => {
                        history.reject(&error)?;
                        history.push_result(&call, Err(error), &tools)?;
                    }
                }
                continue;
            }
            if matches!(
                call.name.as_str(),
                "record_task_requirements" | "finish_task_requirements"
            ) {
                history.draft_calls += 1;
                if history.draft_calls > ASSISTANT_REQUIREMENT_DRAFT_CALL_LIMIT {
                    return Err(String::from(
                        "Assistant requirement drafting reached its call limit; no operation was executed.",
                    ));
                }
                let draft = context
                    .draft
                    .as_mut()
                    .ok_or("This task has no editable requirement draft.")?;
                let mut result = wrap_assistant_read_result(draft.handle_tool(
                    &call.name,
                    &call.arguments,
                    context.instance,
                    context.module,
                ));
                if result
                    .pointer("/data/errors")
                    .and_then(Value::as_array)
                    .is_some_and(|errors| !errors.is_empty())
                {
                    result["ok"] = json!(false);
                    result["error"] = json!(
                        "Some requirement entries were rejected. Valid entries were retained; correct the indexed errors before finishing."
                    );
                }
                history.push_wrapped_result(&call, result, &tools)?;
                continue;
            }
            if call.name == "propose_operation" {
                if let Some(guidance) = context.activate_lifecycle_requirements(&call.arguments) {
                    // This is a task phase transition, not a completed proposal.
                    // Keep history and all counters so activation cannot reset budgets.
                    history.push_result(&call, Err(String::from("Lifecycle and backup operations require a complete request-constraints draft before a preview can be prepared. Use record_task_requirements and finish_task_requirements; no operation was executed.")), &tools)?;
                    // Original request references are authorization context,
                    // never an optional read that may degrade to an evidence gap.
                    history.messages.push(AssistantToolMessage::User(guidance));
                    history.check_budget(&context.visible_tools()?)?;
                    continue;
                }
                let operation =
                    context
                        .operation_from_call(&call.arguments)
                        .and_then(|operation| {
                            validate_operation(&operation)?;
                            Ok(operation)
                        });
                match operation {
                    Ok(operation) => {
                        history.push_result(&call, Ok(json!({"status":"awaiting_confirmation","executed":false,"guidance":"The proposal has not executed. A later confirmed_operation result will report execution and independent verification."})), &tools)?;
                        return Ok(operation);
                    }
                    Err(error) => {
                        history.reject(&error)?;
                        history.push_result(
                            &call,
                            Err(format!(
                                "Response rejected; no operation was executed. {error}"
                            )),
                            &tools,
                        )?;
                    }
                }
                continue;
            }
            let request = assistant_read_call(&call);
            match request {
                Ok(request) => {
                    history.reserve_read()?;
                    let result = wrap_assistant_read_result(assistant_cancellable_read(session, read_tool(request.clone())).await);
                    history.mod_evidence.observe(
                        &request,
                        &result,
                        history.reads,
                        &mut history.pending_reads,
                    );
                    assistant_trace_read(&request, &result);
                    history.push_encoded_result(&call, assistant_read_result_text(&request, &result), &tools)?;
                }
                Err(error) => {
                    history.reject(&error)?;
                    history.push_result(&call, Err(error), &tools)?;
                }
            }
        }
    }
    Err(String::from(
        "Assistant investigation reached its turn limit; no operation was executed.",
    ))
    }.await;
    if outcome.is_err()
        && let Some(slot) = draft_slot
    {
        *slot
            .lock()
            .map_err(|_| "Assistant requirement checkpoint is unavailable.")? =
            context.draft.take();
    }
    if let Some(session) = session {
        assistant_close_pending_tool_results(
            &mut history.messages,
            outcome.as_ref().err().map(String::as_str),
        );
        session.replace_messages(history.messages)?;
    }
    outcome
}

impl AssistantInvestigationContext<'_> {
    fn draft_pending(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(|draft| draft.is_active() && !draft.is_ready())
    }

    fn activate_lifecycle_requirements(&mut self, arguments: &Value) -> Option<String> {
        let action = serde_json::from_value(arguments.get("action")?.clone()).ok()?;
        let draft = self.draft.as_mut()?;
        if !draft.activate_for_lifecycle(action) {
            return None;
        }
        Some(format!(
            "{ASSISTANT_REQUIREMENTS_GUIDE}\nOriginal request references:\n{}",
            draft.source_catalog()
        ))
    }

    fn visible_tools(&self) -> Result<Vec<AssistantToolDefinition>, String> {
        let mut tools = self.tools.clone();
        if self.draft_pending() {
            tools.retain(|tool| tool.name != "propose_operation");
            if let Some(draft) = &self.draft {
                tools.extend(draft.tool_definitions(self.instance, self.module)?);
            }
        }
        tools.push(AssistantToolDefinition {
            name: String::from("report_limitation"),
            description: String::from("Finish without modifying anything. Explain an unsupported request or missing evidence in the user's language."),
            parameters: json!({"type":"object","properties":{"reason":{"type":"string","minLength":1,"maxLength":2048}},"required":["reason"],"additionalProperties":false}),
        });
        Ok(tools)
    }

    fn operation_from_call(&self, arguments: &Value) -> Result<String, String> {
        if self.draft_pending() {
            return Err(String::from(
                "Finish the requirement draft before proposing an operation.",
            ));
        }
        let mut plan = arguments
            .as_object()
            .cloned()
            .ok_or("Operation arguments must be an object.")?;
        if plan.contains_key("tool") || plan.contains_key("taskRequirements") {
            return Err(String::from(
                "Use only operation fields. The application supplies the fixed task requirements.",
            ));
        }
        let (tool, _) =
            parse_assistant_investigation_response(&Value::Object(plan.clone()).to_string())?;
        if tool.is_some() {
            return Err(String::from(
                "An operation proposal cannot be a read request.",
            ));
        }
        let definition = assistant_operation_tool();
        let properties = definition.parameters["properties"]
            .as_object()
            .ok_or("Operation tool schema is missing its fields.")?;
        if plan.keys().any(|key| !properties.contains_key(key)) {
            return Err(String::from(
                "Unknown operation field. Use the exact fields in the supplied propose_operation schema.",
            ));
        }
        if let Some(requirements) = self
            .draft
            .as_ref()
            .and_then(AssistantRequirementsDraft::requirements)
        {
            plan.insert(
                String::from("taskRequirements"),
                serde_json::to_value(requirements).map_err(|error| error.to_string())?,
            );
        }
        Ok(Value::Object(plan).to_string())
    }
}

impl AssistantInvestigationHistory {
    fn reserve_read(&mut self) -> Result<(), String> {
        if self.reads >= self.read_limit {
            return Err(String::from(
                "Assistant investigation reached its read limit; no operation was executed.",
            ));
        }
        self.reads += 1;
        Ok(())
    }

    fn reject(&mut self, error: &str) -> Result<(), String> {
        self.rejected += 1;
        if self.rejected > 2 {
            Err(redact_assistant_provider_text(error))
        } else {
            Ok(())
        }
    }

    fn check_budget(&self, tools: &[AssistantToolDefinition]) -> Result<(), String> {
        assistant_check_conversation_budget(&self.messages, tools)
    }

    fn append_evidence(
        &mut self,
        message: AssistantToolMessage,
        tools: &[AssistantToolDefinition],
    ) -> Result<(), String> {
        self.messages.push(message);
        if self.check_budget(tools).is_err() {
            let message = self
                .messages
                .last_mut()
                .ok_or("Evidence message missing.")?;
            let gap = String::from(
                "Read output was not supplied because it exceeds the remaining investigation context budget. Request fewer lines or specific keys. This is an evidence gap, not an empty successful result.",
            );
            match message {
                AssistantToolMessage::User(content) => *content = gap,
                AssistantToolMessage::ToolResult {
                    content, is_error, ..
                } => {
                    *content = json!({"ok":false,"error":gap}).to_string();
                    *is_error = true;
                }
                AssistantToolMessage::Assistant(_) => return Err(gap),
            }
        }
        self.check_budget(tools)
    }

    fn push_result(
        &mut self,
        call: &AssistantToolCall,
        result: Result<Value, String>,
        tools: &[AssistantToolDefinition],
    ) -> Result<(), String> {
        self.push_wrapped_result(call, wrap_assistant_read_result(result), tools)
    }

    fn push_wrapped_result(
        &mut self,
        call: &AssistantToolCall,
        result: Value,
        tools: &[AssistantToolDefinition],
    ) -> Result<(), String> {
        let content = assistant_tool_result_text(&result);
        self.push_encoded_result(call, content, tools)
    }

    fn push_encoded_result(
        &mut self,
        call: &AssistantToolCall,
        content: String,
        tools: &[AssistantToolDefinition],
    ) -> Result<(), String> {
        #[cfg(test)]
        if std::env::var("LANGAME_ASSISTANT_LIVE").as_deref() == Ok("1") {
            let name = tools
                .iter()
                .find(|tool| tool.name == call.name)
                .map(|tool| tool.name.as_str())
                .unwrap_or("unavailable");
            eprintln!(
                "ASSISTANT_LIVE_TOOL_RESULT={}\n{}",
                name,
                redact_assistant_provider_text(&content)
            );
        }
        let is_error = serde_json::from_str::<Value>(&content)
            .ok()
            .is_none_or(|value| value["ok"] != true);
        self.append_evidence(
            AssistantToolMessage::ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content,
                is_error,
            },
            tools,
        )
    }
}

fn assistant_read_call(call: &AssistantToolCall) -> Result<AssistantReadTool, String> {
    let mut arguments = call
        .arguments
        .as_object()
        .cloned()
        .ok_or("Read arguments must be an object.")?;
    if arguments.contains_key("tool") {
        return Err(String::from(
            "Tool name is supplied by the native call, not an argument.",
        ));
    }
    arguments.insert(String::from("tool"), json!(call.name));
    serde_json::from_value(Value::Object(arguments)).map_err(|error| {
        format!(
            "Invalid read arguments: {}",
            redact_assistant_provider_text(&error.to_string())
        )
    })
}

fn wrap_assistant_read_result(result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => json!({"ok":true,"data":value}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

fn assistant_limitation_reason(arguments: &Value) -> Result<String, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Limitation {
        reason: String,
    }
    let value: Limitation =
        serde_json::from_value(arguments.clone()).map_err(|_| "Invalid limitation arguments.")?;
    if value.reason.trim().is_empty() || value.reason.len() > 8192 {
        return Err(String::from(
            "Limitation reason must be nonempty and bounded.",
        ));
    }
    Ok(redact_assistant_provider_text(&value.reason))
}

fn assistant_read_only_answer(content: &str) -> Result<String, String> {
    if content.len() > ASSISTANT_READ_ONLY_ANSWER_BYTES {
        return Err(String::from(
            "Read-only assistant answer exceeded its byte limit.",
        ));
    }
    let answer = redact_assistant_provider_text(&strip_assistant_think_blocks(content));
    if answer.len() > ASSISTANT_READ_ONLY_ANSWER_BYTES {
        return Err(String::from(
            "Read-only assistant answer exceeded its byte limit.",
        ));
    }
    if answer.trim().is_empty() {
        return Err(String::from(
            "Read-only assistant answer did not include visible message text.",
        ));
    }
    // Message text is always the explanation, even when it contains JSON.
    // Only native operation calls can produce a mutating action.
    Ok(json!({"action":"none", "reason":answer.trim()}).to_string())
}

fn assistant_investigation_control(
    reads: usize,
    limit: usize,
    drafting: bool,
    completion: AssistantInvestigationCompletion,
) -> String {
    let remaining = limit.saturating_sub(reads);
    let availability = if remaining == 0 {
        "No reads remain."
    } else {
        "More reads are available."
    };
    let stage = if completion == AssistantInvestigationCompletion::ReadOnlyAnswer {
        "This request is read-only. Use native read tools when evidence is needed, then answer naturally in message text. No proposal or limitation tool is required to finish. Do not claim unsupported facts or perform changes."
    } else if drafting {
        "Prepare the COMPLETE request with record_task_requirements, then finish_task_requirements. Actions are unavailable until the draft is complete."
    } else {
        "Use evidence to propose_operation, or report_limitation. Only a later operator confirmation can execute the proposal."
    };
    format!(
        "Application control — Read budget remaining: {remaining} of {limit}. The shared task ledger may pause earlier; its counters persist across operation confirmations. {availability} {stage}"
    )
}

fn assistant_trace_read(request: &AssistantReadTool, result: &Value) {
    #[cfg(test)]
    if std::env::var("LANGAME_ASSISTANT_LIVE").as_deref() == Ok("1") {
        eprintln!(
            "ASSISTANT_LIVE_READ={}\n{}",
            redact_assistant_provider_text(&serde_json::to_string(request).unwrap_or_default()),
            assistant_tool_result_text(result)
        );
    }
    let _ = (request, result);
}

#[cfg(test)]
#[path = "investigation_session_tests.rs"]
mod investigation_session_tests;

#[cfg(test)]
#[path = "investigation_requirement_tests.rs"]
mod investigation_requirement_tests;

#[cfg(test)]
#[path = "investigation_read_only_tests.rs"]
mod investigation_read_only_tests;
