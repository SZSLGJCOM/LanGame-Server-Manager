use super::*;
use serde_json::json;

pub(super) fn reply(
    input: &AssistantRunInput,
    messages: &[AssistantToolMessage],
    tools: &[AssistantToolDefinition],
) -> Result<AssistantToolReply, String> {
    let mut prompt = messages
        .iter()
        .filter_map(|message| match message {
            AssistantToolMessage::User(content) => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if prompt.is_empty() {
        prompt.clone_from(&input.prompt);
    }
    let mut read_index = messages
        .iter()
        .filter(|message| {
            matches!(message,
        AssistantToolMessage::User(content) if content.starts_with("\nRead "))
        })
        .count();
    for message in messages {
        if let AssistantToolMessage::ToolResult { name, content, .. } = message
            && !matches!(
                name.as_str(),
                "record_task_requirements" | "finish_task_requirements"
            )
        {
            read_index += 1;
            prompt.push_str(&format!("\nRead {read_index}: {content}\n"));
        }
    }
    let mut arguments: Value =
        serde_json::from_str(&mock_assistant_operation_plan_content(&prompt))
            .map_err(|_| "mock assistant returned invalid tool arguments")?;
    let mut name = arguments
        .as_object_mut()
        .and_then(|object| object.remove("tool"))
        .and_then(|tool| tool.as_str().map(str::to_owned))
        .unwrap_or_else(|| "propose_operation".into());
    if name == "propose_operation"
        && tools
            .iter()
            .any(|tool| tool.name == "record_task_requirements")
    {
        let recorded = messages.iter().rev().find(|message| {
            matches!(message,
            AssistantToolMessage::ToolResult { name, .. } if name == "record_task_requirements")
        });
        if let Some(AssistantToolMessage::ToolResult {
            content, is_error, ..
        }) = recorded
        {
            let result: Value = serde_json::from_str(content)
                .map_err(|_| "mock requirements result is not valid JSON")?;
            if *is_error
                || result["ok"] != true
                || !result["data"]["errors"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
            {
                return Err(String::from(
                    "mock requirements fixture was rejected; it cannot be silently completed",
                ));
            }
            name = "finish_task_requirements".into();
            arguments = json!({});
        } else {
            let requirements = if let Some(requirements) = arguments.get("taskRequirements") {
                requirements.clone()
            } else if let Some(raw) =
                mock_assistant_extract_marker_value(&input.prompt, "mock-task-requirements:")
            {
                mock_assistant_parse_json_value(raw)
                    .ok_or("mock requirements fixture contains invalid JSON")?
            } else {
                json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]})
            };
            arguments = map_requirements(&requirements, &prompt)?;
            name = "record_task_requirements".into();
        }
    } else if let Some(object) = arguments.as_object_mut() {
        object.remove("taskRequirements");
    }
    let id = format!("mock_{}_0", messages.len());
    // No raw provider state exists in the fixture; the protocol can construct its
    // canonical assistant message when a loopback test elects to serialize it.
    Ok(AssistantToolReply {
        content: String::new(),
        calls: vec![AssistantToolCall {
            id,
            name,
            arguments,
        }],
        raw_message: Value::Null,
    })
}

pub(super) fn map_requirements(requirements: &Value, prompt: &str) -> Result<Value, String> {
    let mut mapped = json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]});
    for group in ["settings", "ports", "forbiddenActions", "unverified"] {
        let items = requirements[group]
            .as_array()
            .ok_or("mock requirements fixture must declare all four arrays")?;
        for item in items {
            let snippet = item["sourceText"]
                .as_str()
                .filter(|text| !text.is_empty())
                .ok_or("mock requirement fixture needs a source excerpt")?;
            let source_id = source_id(prompt, snippet)?;
            let fields = match group {
                "settings" => &["key", "expected"][..],
                "ports" => &["name", "expected"][..],
                "forbiddenActions" => &["action"][..],
                _ => &["reason"][..],
            };
            let mut value = json!({"sourceId":source_id});
            for field in fields {
                value[*field] = item
                    .get(*field)
                    .cloned()
                    .ok_or("mock requirement fixture is missing a declared field")?;
            }
            mapped[group]
                .as_array_mut()
                .ok_or("mock requirement group is not an array")?
                .push(value);
        }
    }
    Ok(mapped)
}

fn source_id(prompt: &str, snippet: &str) -> Result<String, String> {
    let raw = prompt
        .rsplit_once("Original request references:\n")
        .map(|(_, raw)| raw)
        .ok_or("mock requirement fixture has no request source catalog")?;
    let catalog: Value = serde_json::Deserializer::from_str(raw)
        .into_iter::<Value>()
        .next()
        .transpose()
        .map_err(|_| "mock request source catalog is invalid")?
        .ok_or("mock request source catalog is missing")?;
    let sources = catalog["sources"]
        .as_array()
        .ok_or("mock request source catalog has no source array")?;
    let readable: Vec<_> = sources
        .iter()
        .filter(|source| source["readable"] == true)
        .collect();
    let mut matches: Vec<_> = readable
        .iter()
        .filter(|source| source["text"].as_str() == Some(snippet))
        .collect();
    if matches.is_empty() {
        matches = readable
            .iter()
            .filter(|source| {
                source["text"]
                    .as_str()
                    .is_some_and(|text| text.contains(snippet))
            })
            .collect();
    }
    if matches.len() != 1 {
        return Err(String::from(
            "mock requirement excerpt must identify one exact readable source; no source ID was guessed",
        ));
    }
    matches[0]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "mock request source has no ID".into())
}
