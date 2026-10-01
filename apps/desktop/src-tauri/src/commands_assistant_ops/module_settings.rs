fn read_assistant_module_schema(
    module: Option<&ModuleDetails>,
    tool: AssistantReadTool,
) -> Result<Value, String> {
    if !matches!(
        &tool,
        AssistantReadTool::ListModuleSettings { .. }
            | AssistantReadTool::ReadModuleSettings { .. }
            | AssistantReadTool::SearchModuleSettings { .. }
    ) {
        return Err(String::from(
            "Select one server instance before reading its settings, files or runtime.",
        ));
    }
    let module = module
        .ok_or_else(|| String::from("Select one game module before reading its setting schema."))?;
    let schema = module.schema_json.as_deref().ok_or_else(|| {
        String::from("The selected module has no setting schema; no schema evidence was supplied.")
    })?;
    let parsed: Value = serde_json::from_str(schema)
        .map_err(|_| String::from("The selected module setting schema is not valid JSON."))?;
    let module_properties = parsed
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            String::from("The selected module setting schema has no properties object.")
        })?;
    // Storage inserts this manager field when creating and normalizing every
    // instance. It is discoverable here without claiming a module declaration.
    let manager_bind = assistant_manager_bind_declaration(module);
    let mut properties = module_properties.clone();
    properties.insert(String::from("bind_ip"), manager_bind["declaration"].clone());
    let candidate_schema = json!({"properties":properties}).to_string();
    let mut explicit_keys = false;
    let mut unknown_keys = Vec::new();
    let mut evidence = match tool {
        AssistantReadTool::ListModuleSettings { offset } => {
            // Reuse the current-setting catalog's key and encoded-byte limits,
            // but supply only schema/manager names, never defaults as values.
            let names = properties
                .keys()
                .map(|key| (key.clone(), Value::Null))
                .collect::<serde_json::Map<_, _>>();
            assistant_settings_catalog_evidence(&Value::Object(names).to_string(), offset)?
        }
        AssistantReadTool::ReadModuleSettings { keys, offset } => {
            if keys.len() > 40 {
                return Err(String::from("Read at most 40 setting keys at a time."));
            }
            explicit_keys = !keys.is_empty();
            let valid_keys = keys
                .into_iter()
                .filter(|key| {
                    if properties.contains_key(key) && assistant_module_schema_key_is_readable(key)
                    {
                        true
                    } else {
                        unknown_keys.push(truncate_assistant_prompt_text(
                            &redact_assistant_provider_text(key),
                            ASSISTANT_SETTINGS_CATALOG_KEY_BYTES,
                        ));
                        false
                    }
                })
                .collect::<Vec<_>>();
            // Empty keys normally page all properties. An explicitly requested
            // set with no known names must not silently become a full read.
            if explicit_keys && valid_keys.is_empty() {
                json!({"entries":[], "totalKeys":properties.len(), "nextOffset":null})
            } else {
                assistant_settings_evidence("{}", Some(&candidate_schema), &valid_keys, offset)?
            }
        }
        AssistantReadTool::SearchModuleSettings { query, offset } => {
            assistant_search_settings_evidence("{}", Some(&candidate_schema), &query, offset)?
        }
        _ => {
            return Err(String::from(
                "This read requires a selected server instance.",
            ));
        }
    };
    if let Some(entries) = evidence.get_mut("entries").and_then(Value::as_array_mut) {
        let original_count = entries.len();
        entries.retain(|entry| {
            entry["key"]
                .as_str()
                .is_some_and(assistant_module_schema_key_is_readable)
        });
        let omitted = original_count - entries.len();
        for entry in entries {
            if entry["key"] == "bind_ip" {
                *entry = manager_bind.clone();
                continue;
            }
            if let Some(entry) = entry.as_object_mut() {
                // A schema declaration must not masquerade as a missing/null
                // current setting. No instance or saved setting was read here.
                entry.remove("value");
                entry.remove("exists");
                entry.insert(String::from("instanceExists"), Value::Bool(false));
                entry.insert(String::from("source"), json!("module"));
            }
        }
        evidence["omittedSensitiveKeys"] = json!(omitted);
    }
    if evidence["keys"]
        .as_array()
        .is_some_and(|keys| keys.contains(&json!("bind_ip")))
    {
        evidence["managerSettings"] = json!([manager_bind]);
    }
    if let Some(object) = evidence.as_object_mut() {
        object.remove("unknownKeys");
        if object.get("totalMatches").and_then(Value::as_u64) != Some(0) {
            object.remove("hint");
        }
    }
    if explicit_keys {
        evidence["partial"] = json!(!unknown_keys.is_empty());
        if !unknown_keys.is_empty() {
            evidence["hint"] = json!(
                "Only exact known schema declarations were returned. unknownKeys are missing or unreadable declarations, not missing saved settings. No aliases were applied. Use the catalog or a shorter literal search to correct the unknown keys."
            );
        }
        evidence["unknownKeys"] = json!(unknown_keys);
    }
    evidence["scope"] = json!("module_schema");
    evidence["moduleId"] = json!(module.summary.id);
    evidence["schemaExists"] = json!(true);
    evidence["instanceExists"] = json!(false);
    evidence["note"] = json!(
        "These are schema-only declarations for the selected module plus explicitly labeled manager settings, not current instance settings. source=manager does not claim a module schema property. Defaults are not saved values or permission to change protected settings. Use read_module_settings for exact keys; after creation read the instance's actual settings before proposing a patch."
    );
    Ok(evidence)
}

fn assistant_manager_bind_declaration(module: &ModuleDetails) -> Value {
    json!({"key":"bind_ip", "source":"manager", "schemaExists":false,
        "instanceExists":false, "declaration":{
            "type":"string", "title":"Instance listener bind address",
            "description":"Manager-owned IPv4 or IPv6 instance listener address. A specific address requires strict bind-address support and any required setting enabled; declared support is not proof of an actual socket binding."
        }, "bindAddressCapability": module.runtime.bind_address})
}

fn assistant_module_schema_key_is_readable(key: &str) -> bool {
    if key.len() > ASSISTANT_SETTINGS_CATALOG_KEY_BYTES {
        return false;
    }
    serde_json::to_string(key)
        .ok()
        .and_then(|encoded| {
            serde_json::from_str::<Value>(&redact_assistant_provider_text(&encoded)).ok()
        })
        .is_some_and(|redacted| redacted.as_str() == Some(key))
}

#[cfg(test)]
#[path = "module_settings_tests.rs"]
mod module_settings_tests;
