fn validate_assistant_workshop_plan(plan: &AssistantOperationPlan) -> Result<(), String> {
    if plan.action == AssistantOperationAction::InstallFunMod
        && (plan.workshop_item_ids.is_empty()
            || plan.workshop_item_ids.len() > 20
            || plan.workshop_item_ids.iter().any(|id| {
                id.is_empty() || id.len() > 20 || !id.bytes().all(|byte| byte.is_ascii_digit())
            }))
    {
        return Err(String::from(
            "Workshop installation requires 1 to 20 explicit numeric item IDs in the reviewed plan. No default Mod is selected.",
        ));
    }
    Ok(())
}

pub(super) fn summarize_assistant_operation_plan(
    input: &AssistantExecuteOperationInput,
    plan: &AssistantOperationPlan,
) -> String {
    let action = match plan.action {
        AssistantOperationAction::StartServer => "Start server",
        AssistantOperationAction::StopServer => "Stop the current managed server run",
        AssistantOperationAction::RestartServer => {
            "Stop the current run, then start and verify a new run"
        }
        AssistantOperationAction::CreateBackup => {
            "Create a save backup of this stopped instance; existing retention rules apply"
        }
        AssistantOperationAction::RestoreBackup => {
            "Restore the selected save backup, preserving a safeguard of the current saves; the server stays stopped"
        }
        AssistantOperationAction::CreateServer => "Create server instance without starting it",
        AssistantOperationAction::InstallServer => "Install server files",
        AssistantOperationAction::ValidateServer => {
            "Validate server files using the game's file verification workflow"
        }
        AssistantOperationAction::ApplyBeginnerConfig => "Apply beginner configuration",
        AssistantOperationAction::CustomizeConfig => "Update server configuration",
        AssistantOperationAction::PatchInstanceText => "Patch one private instance Mod text file",
        AssistantOperationAction::PatchInstanceFiles => {
            "Patch private instance text files with backups and readback"
        }
        AssistantOperationAction::InstallFunMod => "Install Workshop mods",
        AssistantOperationAction::InstallSiteMod => "Install site or local mods",
        AssistantOperationAction::RepairPorts => "Update port configuration",
        AssistantOperationAction::RunGmCommand => "Send a GM command",
        AssistantOperationAction::Broadcast => "Send a server broadcast",
        AssistantOperationAction::None => "No operation",
    };
    let instance_id = plan
        .instance_id
        .as_deref()
        .or(input.selected_instance_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let module_id = plan
        .module_id
        .as_deref()
        .or(input.selected_module_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut details = Vec::new();
    if let Some(backup_id) = &plan.backup_id {
        details.push(format!("backup={backup_id}"));
    }
    if let Some(instance_id) = instance_id {
        details.push(format!("instance={instance_id}"));
    }
    if let Some(module_id) = module_id {
        details.push(format!("module={module_id}"));
    }
    if let Some(patch) = plan.settings_patch.as_ref() {
        let patch = serde_json::to_string(patch).unwrap_or_else(|_| String::from("{}"));
        details.push(format!(
            "settingsPatch={}",
            redact_assistant_provider_text(&patch)
        ));
    }
    for patch in &plan.file_patches {
        details.push(format!(
            "file={}; source SHA256={}; edits={}",
            patch.file,
            patch.source_sha256,
            patch.edits.len()
        ));
    }
    if let Some(patch) = &plan.text_patch {
        details.push(format!(
            "file={}; source SHA256={}",
            patch.file, patch.source_sha256
        ));
    }
    if let Ok(entries) = plan
        .port_patch
        .as_ref()
        .map(assistant_port_patch_entries)
        .transpose()
        && let Some(entries) = entries
    {
        let mut values = entries
            .into_iter()
            .filter_map(|(name, port)| port.map(|port| format!("{name}={port}")))
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        values.sort();
        if !values.is_empty() {
            details.push(format!("ports={}", values.join(", ")));
        }
    }
    if !plan.runtime_commands.is_empty() {
        details.push(format!(
            "GM command={}",
            summarize_text(
                &redact_assistant_provider_text(&plan.runtime_commands.join("; ")),
                240
            )
        ));
    }
    if !plan.workshop_item_ids.is_empty() {
        details.push(format!(
            "Workshop items={}",
            plan.workshop_item_ids.join(", ")
        ));
    }
    if !plan.mod_references.is_empty() || !plan.source_paths.is_empty() {
        let mut references = plan.mod_references.clone();
        references.extend(plan.source_paths.iter().map(|source_path| {
            let label = Path::new(source_path)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("local folder");
            format!("local={label}")
        }));
        details.push(format!(
            "mod sources={}",
            summarize_text(&redact_assistant_provider_text(&references.join(", ")), 240)
        ));
    }
    if let Some(intent) = plan
        .broadcast_intent
        .as_deref()
        .map(str::trim)
        .filter(|intent| !intent.is_empty())
    {
        details.push(format!(
            "broadcast intent={}",
            summarize_text(&redact_assistant_provider_text(intent), 240)
        ));
    }

    if details.is_empty() {
        action.to_string()
    } else {
        format!("{action} ({})", details.join("; "))
    }
}

pub(super) fn summarize_assistant_operation_preview(
    input: &AssistantExecuteOperationInput,
    plan: &AssistantOperationPlan,
    prepared_broadcast: Option<&AssistantPreparedBroadcast>,
) -> String {
    let summary = summarize_assistant_operation_plan(input, plan);
    let Some(prepared) = prepared_broadcast else {
        return summary;
    };
    let exact_message = serde_json::to_string(&prepared.message)
        .unwrap_or_else(|_| String::from("\"<unavailable>\""));
    format!("{summary}\nExact broadcast text: {exact_message}")
}

fn bind_assistant_task_preview_target(
    task: &AssistantTaskContract,
    plan: &mut AssistantOperationPlan,
    instances: &[InstanceSummary],
    modules: &[ModuleSummary],
) -> Result<(), String> {
    if plan.action == AssistantOperationAction::None {
        return Ok(());
    }
    if plan
        .instance_id
        .as_ref()
        .is_some_and(|id| Some(id) != task.instance_id.as_ref())
        || plan
            .module_id
            .as_ref()
            .is_some_and(|id| Some(id) != task.module_id.as_ref())
    {
        return Err(String::from(
            "The preview target conflicts with the bound task target.",
        ));
    }
    let module_id = task
        .module_id
        .as_deref()
        .filter(|id| modules.iter().any(|module| module.id == *id))
        .ok_or("The task's bound server module is unavailable.")?;
    let needs_instance = !matches!(
        plan.action,
        AssistantOperationAction::CreateServer
            | AssistantOperationAction::InstallServer
            | AssistantOperationAction::ValidateServer
    );
    if let Some(instance_id) = task.instance_id.as_deref() {
        if plan.action == AssistantOperationAction::CreateServer {
            return Err(String::from(
                "Creating a server cannot replace the task's bound instance.",
            ));
        }
        if !instances
            .iter()
            .any(|instance| instance.id == instance_id && instance.module_id == module_id)
        {
            return Err(String::from(
                "The task's bound server is unavailable or its game changed.",
            ));
        }
    } else if needs_instance {
        return Err(String::from(
            "This operation requires the task's already resolved instance.",
        ));
    }
    // References, exclusions and quoted names in the original request cannot
    // rediscover a target after the model and core have established its contract.
    plan.instance_id = task.instance_id.clone();
    plan.module_id = Some(module_id.to_owned());
    Ok(())
}

#[cfg(test)]
fn bind_assistant_preview_target(
    input: &AssistantExecuteOperationInput,
    plan: &mut AssistantOperationPlan,
    selected_module_id: Option<&str>,
    instances: &[InstanceSummary],
    modules: &[ModuleSummary],
    context_instance: Option<&InstanceDetails>,
) -> Result<(), String> {
    match plan.action {
        AssistantOperationAction::None => Ok(()),
        AssistantOperationAction::InstallServer
        | AssistantOperationAction::ValidateServer
        | AssistantOperationAction::CreateServer => {
            if plan.action != AssistantOperationAction::CreateServer
                && let Some(instance) = context_instance
            {
                if plan
                    .instance_id
                    .as_deref()
                    .is_some_and(|id| id != instance.summary.id)
                    || plan
                        .module_id
                        .as_deref()
                        .is_some_and(|id| id != instance.summary.module_id)
                {
                    return Err(String::from(
                        "The file preparation target conflicts with the selected server.",
                    ));
                }
                if !modules
                    .iter()
                    .any(|module| module.id == instance.summary.module_id)
                {
                    return Err(String::from("The selected server's game is unavailable."));
                }
                // Preserve the core's selected/inferred instance so the preview
                // captures its precondition and verification can continue on it.
                plan.instance_id = Some(instance.summary.id.clone());
                plan.module_id = Some(instance.summary.module_id.clone());
                return Ok(());
            }
            let module = find_assistant_module_target(
                &input.prompt,
                plan,
                input.selected_module_id.as_deref(),
                modules,
            )
            .ok_or_else(|| String::from("AI could not identify the server module to preview."))?;
            plan.module_id = Some(module.id.clone());
            Ok(())
        }
        AssistantOperationAction::StartServer => {
            if let Some(instance) = find_assistant_instance_target(
                &input.prompt,
                plan,
                input.selected_instance_id.as_deref(),
                selected_module_id,
                instances,
            ) {
                plan.instance_id = Some(instance.id.clone());
                plan.module_id = Some(instance.module_id.clone());
                return Ok(());
            }
            Err(String::from(
                "Create and configure an instance before requesting start_server.",
            ))
        }
        AssistantOperationAction::ApplyBeginnerConfig
        | AssistantOperationAction::StopServer
        | AssistantOperationAction::RestartServer
        | AssistantOperationAction::CreateBackup
        | AssistantOperationAction::RestoreBackup
        | AssistantOperationAction::CustomizeConfig
        | AssistantOperationAction::PatchInstanceText
        | AssistantOperationAction::PatchInstanceFiles
        | AssistantOperationAction::InstallFunMod
        | AssistantOperationAction::InstallSiteMod
        | AssistantOperationAction::RepairPorts
        | AssistantOperationAction::RunGmCommand
        | AssistantOperationAction::Broadcast => {
            let instance = find_assistant_instance_target(
                &input.prompt,
                plan,
                input.selected_instance_id.as_deref(),
                selected_module_id,
                instances,
            )
            .ok_or_else(|| String::from("AI could not identify the instance target to preview."))?;
            plan.instance_id = Some(instance.id.clone());
            plan.module_id = Some(instance.module_id.clone());
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod plan_tests;

pub(super) fn parse_assistant_operation_plan_response(
    content: &str,
) -> Result<AssistantOperationPlan, String> {
    let visible_content = strip_assistant_think_blocks(content);
    let mut candidates = extract_json_objects(&visible_content).into_iter();
    let candidate = candidates.next().ok_or_else(|| {
        String::from("assistant operation planner did not return one valid operation JSON object")
    })?;
    if candidates.next().is_some() {
        return Err(String::from(
            "assistant operation planner returned multiple operation JSON objects",
        ));
    }
    let value: Value = serde_json::from_str(&candidate).map_err(|_| String::from(
        "assistant operation planner returned an invalid operation JSON object; supply valid JSON matching the advertised tool schema.",
    ))?;
    if let Some(patches) = value.get("filePatches")
        && (!patches.is_array()
            || patches.as_array().is_some_and(|files| {
                files.iter().any(|file| {
                    !file.is_object() || !file.get("edits").is_some_and(Value::is_array)
                })
            }))
    {
        return Err(String::from(
            "propose_operation.filePatches must be a native JSON array of objects, each with file, sourceSha256 and an edits array. Do not encode arrays or objects inside a JSON string. Put reason and instanceId beside filePatches, not inside it. No change was prepared.",
        ));
    }
    if let Some(ids) = value
        .get("workshopItemIds")
        .or_else(|| value.get("workshop_item_ids"))
    {
        assistant_workshop_item_ids(ids.clone())?;
    }
    let plan = serde_json::from_value::<AssistantOperationPlan>(value).map_err(|_| String::from(
        "assistant operation planner returned an invalid operation JSON object: its field types do not match the advertised tool schema. Keep arrays/objects as native JSON values and text as strings; no change was prepared.",
    ))?;
    validate_assistant_text_patch_plan(&plan)?;
    Ok(plan)
}

fn assistant_safe_none_plan(reason: String) -> AssistantOperationPlan {
    AssistantOperationPlan {
        action: AssistantOperationAction::None,
        backup_id: None,
        task_requirements: None,
        instance_id: None,
        module_id: None,
        settings_patch: None,
        text_patch: None,
        file_patches: Vec::new(),
        port_patch: None,
        workshop_item_ids: Vec::new(),
        mod_references: Vec::new(),
        source_paths: Vec::new(),
        broadcast_intent: None,
        runtime_commands: Vec::new(),
        process_key: None,
        transport: None,
        port_name: None,
        password_setting_key: None,
        enabled_setting_key: None,
        reason: Some(reason),
    }
}

pub(super) fn parse_assistant_operation_plan_safely(
    content: &str,
) -> (AssistantOperationPlan, Option<String>) {
    match parse_assistant_operation_plan_response(content) {
        Ok(plan) => (plan, None),
        Err(error) => (
            assistant_safe_none_plan(String::from(
                "The operation planner response was ambiguous, so no operation was planned.",
            )),
            Some(error),
        ),
    }
}

pub(super) fn extract_json_objects(content: &str) -> Vec<String> {
    let mut objects = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in content.char_indices() {
        if start.is_none() {
            if ch == '{' {
                start = Some(index);
                depth = 1;
            }
            continue;
        }

        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }

        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth = depth.saturating_sub(1);
            if depth == 0
                && let Some(start_index) = start.take()
            {
                objects.push(content[start_index..=index].to_string());
            }
        }
    }

    objects
}

pub(super) fn strip_assistant_think_blocks(content: &str) -> String {
    let mut visible = content.to_string();
    loop {
        let lower = visible.to_ascii_lowercase();
        let Some(start) = lower.find("<think>") else {
            break;
        };
        let body_start = start + "<think>".len();
        let Some(relative_end) = lower[body_start..].find("</think>") else {
            visible.truncate(start);
            break;
        };
        let end = body_start + relative_end + "</think>".len();
        visible.replace_range(start..end, "");
    }
    visible
}

pub(super) fn merge_assistant_settings_patch(
    current: &Value,
    patch: &Value,
) -> Result<AssistantSettingsPatchMerge, String> {
    let current_object = current
        .as_object()
        .ok_or_else(|| String::from("current instance settings must be a JSON object"))?;
    let patch_object = patch
        .as_object()
        .ok_or_else(|| String::from("assistant settingsPatch must be a JSON object"))?;
    let mut settings = current_object.clone();
    let mut applied_keys = Vec::new();
    let mut rejected_keys = Vec::new();

    for (key, value) in patch_object {
        if value.to_string().contains("[REDACTED]") {
            return Err(String::from(
                "Assistant cannot write redacted placeholders into server settings.",
            ));
        }
        if settings.contains_key(key) {
            settings.insert(key.clone(), value.clone());
            applied_keys.push(key.clone());
        } else {
            rejected_keys.push(key.clone());
        }
    }

    applied_keys.sort();
    rejected_keys.sort();

    Ok(AssistantSettingsPatchMerge {
        settings: Value::Object(settings),
        applied_keys,
        rejected_keys,
    })
}

pub(super) fn assistant_port_patch_port_value(value: &Value) -> Option<u16> {
    let raw = match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        Value::Object(object) => {
            return object.get("port").and_then(assistant_port_patch_port_value);
        }
        _ => None,
    }?;
    if (1..=u64::from(u16::MAX)).contains(&raw) {
        Some(raw as u16)
    } else {
        None
    }
}

pub(super) fn assistant_port_patch_entries(
    patch: &Value,
) -> Result<Vec<(String, Option<u16>)>, String> {
    let object = patch
        .as_object()
        .ok_or_else(|| String::from("assistant portPatch must be a JSON object"))?;
    if let Some(ports) = object.get("ports").and_then(Value::as_array) {
        return Ok(ports
            .iter()
            .map(|entry| {
                let name = entry
                    .as_object()
                    .and_then(|object| object.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                (name, assistant_port_patch_port_value(entry))
            })
            .collect());
    }

    Ok(object
        .iter()
        .map(|(name, value)| {
            (
                name.trim().to_string(),
                assistant_port_patch_port_value(value),
            )
        })
        .collect())
}

pub(super) fn merge_assistant_port_patch(
    current: &[PortBinding],
    patch: &Value,
) -> Result<AssistantPortPatchMerge, String> {
    let entries = assistant_port_patch_entries(patch)?;
    let known_indices = current
        .iter()
        .enumerate()
        .map(|(index, port)| (port.name.to_ascii_lowercase(), index))
        .collect::<HashMap<_, _>>();
    let mut ports = current.to_vec();
    let mut applied_names = HashSet::new();
    let mut rejected_names = HashSet::new();

    for (name, port_value) in entries {
        let port_name = name.trim();
        if port_name.is_empty() {
            rejected_names.insert(String::from("<empty>"));
            continue;
        }
        let Some(port_value) = port_value else {
            rejected_names.insert(port_name.to_string());
            continue;
        };
        let Some(index) = known_indices.get(&port_name.to_ascii_lowercase()).copied() else {
            rejected_names.insert(port_name.to_string());
            continue;
        };
        ports[index].port = port_value;
        applied_names.insert(ports[index].name.clone());
    }

    let mut applied_names = applied_names.into_iter().collect::<Vec<_>>();
    let mut rejected_names = rejected_names.into_iter().collect::<Vec<_>>();
    applied_names.sort();
    rejected_names.sort();

    Ok(AssistantPortPatchMerge {
        ports,
        applied_names,
        rejected_names,
    })
}
