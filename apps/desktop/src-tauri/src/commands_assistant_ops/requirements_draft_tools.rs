fn assistant_draft_tool_definitions(
    sources: &[AssistantRequirementSource],
    instance: Option<&InstanceDetails>,
    module: Option<&ModuleDetails>,
) -> Result<Vec<crate::assistant::AssistantToolDefinition>, String> {
    if let (Some(instance), Some(module)) = (instance, module)
        && instance.summary.module_id != module.summary.id
    {
        return Err(String::from(
            "Requirement schema does not belong to the selected instance.",
        ));
    }
    let settings: Option<Value> = instance
        .map(|value| serde_json::from_str(&value.settings_json))
        .transpose()
        .map_err(|_| "Cannot read actual settings for requirement tools.")?;
    let schema: Option<Value> = module
        .and_then(|value| value.schema_json.as_deref())
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| "Cannot read the module schema for requirement tools.")?;
    let mut keys = std::collections::BTreeSet::new();
    if let Some(settings) = settings.as_ref().and_then(Value::as_object) {
        keys.extend(settings.keys().cloned());
    }
    if let Some(properties) = schema
        .as_ref()
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
    {
        keys.extend(properties.keys().cloned());
    }
    if module.is_some() {
        keys.insert(String::from("bind_ip"));
    }
    if keys
        .iter()
        .any(|key| !assistant_module_schema_key_is_readable(key))
    {
        return Err(String::from(
            "Some setting identifiers cannot be safely exposed. No requirement-tool targets were silently omitted.",
        ));
    }
    let port_names = instance
        .map(|instance| instance.ports.as_slice())
        .or_else(|| module.map(|module| module.default_ports.as_slice()))
        .unwrap_or_default()
        .iter()
        .map(|port| port.name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    if port_names
        .iter()
        .any(|name| !assistant_module_schema_key_is_readable(name))
    {
        return Err(String::from(
            "Some declared port identifiers cannot be safely exposed.",
        ));
    }
    let source_ids = sources
        .iter()
        .filter(|source| source.readable)
        .map(|source| source.id.clone())
        .collect::<Vec<_>>();
    let source = json!({"type":"string","enum":source_ids});
    let actions = [
        AssistantOperationAction::StartServer,
        AssistantOperationAction::StopServer,
        AssistantOperationAction::RestartServer,
        AssistantOperationAction::CreateBackup,
        AssistantOperationAction::RestoreBackup,
        AssistantOperationAction::CreateServer,
        AssistantOperationAction::InstallServer,
        AssistantOperationAction::ValidateServer,
        AssistantOperationAction::ApplyBeginnerConfig,
        AssistantOperationAction::CustomizeConfig,
        AssistantOperationAction::PatchInstanceText,
        AssistantOperationAction::PatchInstanceFiles,
        AssistantOperationAction::InstallFunMod,
        AssistantOperationAction::InstallSiteMod,
        AssistantOperationAction::RepairPorts,
        AssistantOperationAction::RunGmCommand,
        AssistantOperationAction::Broadcast,
    ];
    let array = |properties: Value, required: Value, available: bool| {
        if source_ids.is_empty() || !available {
            return json!({"type":"array","maxItems":0,"items":false});
        }
        json!({"type":"array","maxItems":32,"items":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
    };
    let mut parameters = json!({"type":"object","additionalProperties":false,
        "required":["settings","ports","forbiddenActions","unverified"],"properties":{
        "settings":array(json!({"sourceId":source,"key":{"type":"string","enum":keys},
            "expected":{"type":["null","boolean","number","string","array","object"]}}),json!(["sourceId","key","expected"]),!keys.is_empty()),
        "ports":array(json!({"sourceId":source,"name":{"type":"string","enum":port_names},
            "expected":{"type":"integer","minimum":1,"maximum":65535}}),json!(["sourceId","name","expected"]),!port_names.is_empty()),
        "forbiddenActions":array(json!({"sourceId":source,"action":{"type":"string","enum":actions}}),json!(["sourceId","action"]),true),
        "unverified":array(json!({"sourceId":source,"reason":{"type":"string","minLength":1,"maxLength":512}}),json!(["sourceId","reason"]),true),
        "reset":{"type":"boolean","default":false}
    }});
    parameters["properties"]["unverified"]["description"] = json!(
        "Only obligations the user actually requires that have no supported verification path. Any entry blocks ALL mutations. Use [] when the requested settings and forbidden actions cover the obligations. Do not include checks the user explicitly excludes, optional caveats, absent extra guarantees, or supported checks performed during startup. A request not to edit protected binaries does not demand proof that every installation byte stays unchanged. For a genuinely unsupported obligation, explain the gap with report_limitation instead of proposing a mutation."
    );
    let definitions = vec![
        crate::assistant::AssistantToolDefinition {
            name: String::from("record_task_requirements"),
            description: String::from(
                "Record or correct the unconfirmed request requirements using exact source IDs and declared targets. Valid items are retained; errors identify rejected entries. expected must satisfy the actual setting schema, which read tools can inspect. Explicit reset clears only this in-memory draft. No server operation is performed.",
            ),
            parameters,
        },
        crate::assistant::AssistantToolDefinition {
            name: String::from("finish_task_requirements"),
            description: String::from(
                "Finish an explicitly recorded, valid requirements draft before planning an operation. This does not prove natural-language completeness or authorize execution. The operator must review the entire requirement list in the first confirmation preview.",
            ),
            parameters: json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        },
    ];
    let encoded=json!(definitions.iter().map(|tool|json!({"name":tool.name,"description":tool.description,"parameters":tool.parameters})).collect::<Vec<_>>()).to_string();
    if encoded.len() > ASSISTANT_DRAFT_BYTES {
        return Err(String::from(
            "The complete requirement-tool schema exceeds 16 KiB; no target enum was truncated.",
        ));
    }
    Ok(definitions)
}
