include!("config_documents.rs");

#[cfg(test)]
pub(super) fn normalize_assistant_match_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(ch))
        .collect()
}

pub(super) fn assistant_task_needs_config_context(goal: AssistantTaskGoal) -> bool {
    goal != AssistantTaskGoal::Inspect
}

pub(super) fn assistant_log_snapshot_has_content(snapshot: &LogTailSnapshot) -> bool {
    snapshot.source_path.is_some() || snapshot.read_error.is_some() || !snapshot.lines.is_empty()
}

#[cfg(test)]
pub(super) async fn read_assistant_instance_runtime_log_snapshot(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: &str,
) -> Result<Option<LogTailSnapshot>, String> {
    const LOG_LINES: usize = 80;
    if let Some(snapshot) = pending_start_console_log_snapshot(state, instance_id, LOG_LINES)
        && assistant_log_snapshot_has_content(&snapshot)
    {
        return Ok(Some(snapshot));
    }

    let snapshot = read_instance_log_document(&storage.paths, instance_id, LOG_LINES, None)
        .await
        .map_err(|error| error.to_string())?;
    if assistant_log_snapshot_has_content(&snapshot) {
        Ok(Some(snapshot))
    } else {
        Ok(None)
    }
}

pub(super) fn format_assistant_runtime_log_snapshot(snapshot: &LogTailSnapshot) -> String {
    let source_name = snapshot
        .source_path
        .as_deref()
        .and_then(|source_path| {
            source_path
                .rsplit(['/', '\\'])
                .find(|segment| !segment.is_empty())
        })
        .unwrap_or("- unknown");
    let mut parts = vec![
        format!("source: {source_name}"),
        format!("totalLines: {}", snapshot.total_lines),
        format!("truncated: {}", snapshot.truncated),
    ];

    if let Some(read_error) = snapshot
        .read_error
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("readError: {read_error}"));
    }

    parts.push(if snapshot.lines.is_empty() {
        String::from("lines:\n- none")
    } else {
        format!("lines:\n{}", snapshot.lines.join("\n"))
    });

    parts.join("\n")
}

pub(super) fn summarize_assistant_schema_keys(schema_json: Option<&str>) -> String {
    let Some(schema_json) = schema_json.map(str::trim).filter(|value| !value.is_empty()) else {
        return String::from("-");
    };
    let Ok(schema) = serde_json::from_str::<Value>(schema_json) else {
        return String::from("-");
    };
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return String::from("-");
    };
    let keys = properties
        .keys()
        .take(80)
        .map(|key| redact_assistant_provider_text(key))
        .collect::<Vec<_>>();
    if keys.is_empty() {
        String::from("-")
    } else {
        format!(
            "{} keys: {}{}",
            properties.len(),
            keys.join(", "),
            if properties.len() > keys.len() {
                ", ..."
            } else {
                ""
            }
        )
    }
}

#[cfg(test)]
pub(super) fn assistant_game_aliases(module_id: &str) -> &'static [&'static str] {
    match module_id {
        "terraria" => &["terraria", "\u{6cf0}\u{62c9}\u{745e}\u{4e9a}"],
        "dontstarve" => &[
            "dontstarve",
            "don't starve",
            "dst",
            "\u{9965}\u{8352}",
            "\u{9965}\u{8352}\u{8054}\u{673a}",
            "\u{9965}\u{8352}\u{8054}\u{673a}\u{7248}",
        ],
        "minecraft" => &[
            "minecraft",
            "minecraft java",
            "mc",
            "\u{6211}\u{7684}\u{4e16}\u{754c}",
        ],
        "projectzomboid" => &[
            "project zomboid",
            "zomboid",
            "pzb",
            "\u{50f5}\u{5c38}\u{6bc1}\u{706d}\u{5de5}\u{7a0b}",
        ],
        "valheim" => &["valheim", "\u{82f1}\u{7075}\u{795e}\u{6bbf}"],
        "palworld" => &["palworld", "\u{5e7b}\u{517d}\u{5e15}\u{9c81}"],
        "sevendaystodie" => &[
            "7 days to die",
            "7dtd",
            "sevendaystodie",
            "\u{4e03}\u{65e5}\u{6740}",
        ],
        "vrising" => &["v rising", "vrising", "\u{591c}\u{65cf}\u{5d1b}\u{8d77}"],
        "rimworld" => &[
            "rimworld",
            "rimworld together",
            "\u{73af}\u{4e16}\u{754c}",
            "\u{73af}\u{4e16}\u{754c}\u{8054}\u{673a}",
        ],
        "unturned" => &["unturned", "\u{672a}\u{8f6c}\u{53d8}\u{8005}"],
        "enshrouded" => &["enshrouded", "\u{96fe}\u{9501}\u{738b}\u{56fd}"],
        "corekeeper" => &[
            "core keeper",
            "corekeeper",
            "\u{62a4}\u{6838}\u{7eaa}\u{5143}",
        ],
        "rust" => &["rust", "\u{8150}\u{8680}"],
        "abioticfactor" => &[
            "abiotic factor",
            "abioticfactor",
            "\u{975e}\u{751f}\u{7269}\u{56e0}\u{7d20}",
        ],
        "arksurvivalascended" => &[
            "ark survival ascended",
            "ark ascended",
            "asa",
            "\u{65b9}\u{821f}\u{98de}\u{5347}",
            "\u{65b9}\u{821f}\u{751f}\u{5b58}\u{98de}\u{5347}",
        ],
        "arksurvivalevolved" => &[
            "ark survival evolved",
            "ark evolved",
            "ase",
            "\u{65b9}\u{821f}\u{8fdb}\u{5316}",
            "\u{65b9}\u{821f}\u{751f}\u{5b58}\u{8fdb}\u{5316}",
        ],
        _ => &[],
    }
}

#[cfg(test)]
pub(super) fn assistant_text_matches_module(prompt: &str, module_id: &str, name: &str) -> bool {
    let prompt_key = normalize_assistant_match_text(prompt);
    let mut candidates = vec![module_id.to_string(), name.to_string()];
    candidates.extend(
        assistant_game_aliases(module_id)
            .iter()
            .map(|value| value.to_string()),
    );

    candidates
        .iter()
        .map(|candidate| normalize_assistant_match_text(candidate))
        .any(|candidate| !candidate.is_empty() && prompt_key.contains(&candidate))
}

#[cfg(test)]
pub(super) fn assistant_text_matches_instance(prompt: &str, instance: &InstanceSummary) -> bool {
    assistant_text_matches_module(prompt, &instance.module_id, &instance.name)
        || normalize_assistant_match_text(prompt)
            .contains(&normalize_assistant_match_text(&instance.name))
}

#[cfg(test)]
pub(super) fn assistant_text_matches_instance_identity(
    prompt: &str,
    instance: &InstanceSummary,
) -> bool {
    let prompt_key = normalize_assistant_match_text(prompt);
    let id_key = normalize_assistant_match_text(&instance.id);
    let name_key = normalize_assistant_match_text(&instance.name);
    (!id_key.is_empty() && prompt_key.contains(&id_key))
        || (!name_key.is_empty() && prompt_key.contains(&name_key))
}

#[cfg(test)]
pub(super) fn assistant_selected_id_matches(selected_id: Option<&str>, target_id: &str) -> bool {
    selected_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value == target_id)
}

#[cfg(test)]
pub(super) fn assistant_instance_target_is_authorized(
    prompt: &str,
    selected_instance_id: Option<&str>,
    _selected_module_id: Option<&str>,
    instance: &InstanceSummary,
) -> bool {
    assistant_selected_id_matches(selected_instance_id, &instance.id)
        || assistant_text_matches_instance_identity(prompt, instance)
}

#[cfg(test)]
pub(super) fn assistant_unique_text_matched_instance(
    prompt: &str,
    instances: &[InstanceSummary],
) -> Option<InstanceSummary> {
    let matches = instances
        .iter()
        .filter(|instance| assistant_text_matches_instance(prompt, instance))
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Some(matches[0].clone())
    } else {
        None
    }
}

#[cfg(test)]
pub(super) fn assistant_module_target_is_authorized(
    prompt: &str,
    selected_module_id: Option<&str>,
    module: &ModuleSummary,
) -> bool {
    assistant_selected_id_matches(selected_module_id, &module.id)
        || assistant_text_matches_module(prompt, &module.id, &module.name)
}

#[cfg(test)]
pub(super) fn find_assistant_instance_target(
    prompt: &str,
    plan: &AssistantOperationPlan,
    selected_instance_id: Option<&str>,
    selected_module_id: Option<&str>,
    instances: &[InstanceSummary],
) -> Option<InstanceSummary> {
    let unique_text_match = assistant_unique_text_matched_instance(prompt, instances);
    let unique_selected_module_match = selected_module_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|module_id| {
            let matches = instances
                .iter()
                .filter(|instance| instance.module_id == module_id)
                .collect::<Vec<_>>();
            if matches.len() == 1 {
                Some(matches[0].clone())
            } else {
                None
            }
        });
    if let Some(instance_id) = plan
        .instance_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        && let Some(instance) = instances.iter().find(|instance| {
            instance.id == instance_id
                && assistant_instance_target_is_authorized(
                    prompt,
                    selected_instance_id,
                    selected_module_id,
                    instance,
                )
                || unique_text_match
                    .as_ref()
                    .is_some_and(|matched| matched.id == instance.id)
                || unique_selected_module_match
                    .as_ref()
                    .is_some_and(|matched| matched.id == instance.id)
        })
    {
        return Some(instance.clone());
    }

    if let Some(instance_id) = selected_instance_id.filter(|value| !value.trim().is_empty())
        && let Some(instance) = instances.iter().find(|instance| instance.id == instance_id)
    {
        return Some(instance.clone());
    }

    if let Some(module_id) = plan
        .module_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        && let Some(instance) = instances.iter().find(|instance| {
            instance.module_id == module_id
                && assistant_instance_target_is_authorized(
                    prompt,
                    selected_instance_id,
                    selected_module_id,
                    instance,
                )
                || unique_text_match.as_ref().is_some_and(|matched| {
                    matched.id == instance.id && matched.module_id == module_id
                })
                || unique_selected_module_match
                    .as_ref()
                    .is_some_and(|matched| {
                        matched.id == instance.id && matched.module_id == module_id
                    })
        })
    {
        return Some(instance.clone());
    }

    unique_text_match.or(unique_selected_module_match)
}

#[cfg(test)]
pub(super) fn find_assistant_module_target(
    prompt: &str,
    plan: &AssistantOperationPlan,
    selected_module_id: Option<&str>,
    modules: &[ModuleSummary],
) -> Option<ModuleSummary> {
    if let Some(module_id) = selected_module_id.filter(|value| !value.trim().is_empty())
        && let Some(module) = modules.iter().find(|module| module.id == module_id)
    {
        return Some(module.clone());
    }

    if let Some(module_id) = plan
        .module_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        && let Some(module) = modules.iter().find(|module| {
            module.id == module_id
                && assistant_module_target_is_authorized(prompt, selected_module_id, module)
        })
    {
        return Some(module.clone());
    }

    modules
        .iter()
        .find(|module| assistant_text_matches_module(prompt, &module.id, &module.name))
        .cloned()
}

fn find_assistant_execution_instance_target(
    _mode: &AssistantOperationMode,
    _prompt: &str,
    plan: &AssistantOperationPlan,
    _selected_instance_id: Option<&str>,
    _selected_module_id: Option<&str>,
    instances: &[InstanceSummary],
) -> Option<InstanceSummary> {
    #[cfg(test)]
    if _mode.allows_target_discovery() {
        return find_assistant_instance_target(
            _prompt,
            plan,
            _selected_instance_id,
            _selected_module_id,
            instances,
        );
    }
    find_assistant_bound_instance_target(plan, instances)
}

pub(super) fn find_assistant_bound_instance_target(
    plan: &AssistantOperationPlan,
    instances: &[InstanceSummary],
) -> Option<InstanceSummary> {
    let instance_id = plan.instance_id.as_deref()?;
    instances
        .iter()
        .find(|instance| {
            instance.id == instance_id
                && plan
                    .module_id
                    .as_deref()
                    .is_none_or(|module_id| instance.module_id == module_id)
        })
        .cloned()
}

fn find_assistant_execution_module_target(
    _mode: &AssistantOperationMode,
    _prompt: &str,
    plan: &AssistantOperationPlan,
    _selected_module_id: Option<&str>,
    modules: &[ModuleSummary],
) -> Option<ModuleSummary> {
    #[cfg(test)]
    if _mode.allows_target_discovery() {
        return find_assistant_module_target(_prompt, plan, _selected_module_id, modules);
    }
    find_assistant_bound_module_target(plan, modules)
}

pub(super) fn find_assistant_bound_module_target(
    plan: &AssistantOperationPlan,
    modules: &[ModuleSummary],
) -> Option<ModuleSummary> {
    let module_id = plan.module_id.as_deref()?;
    modules
        .iter()
        .find(|module| module.id == module_id)
        .cloned()
}

#[cfg(test)]
pub(super) fn infer_assistant_prompt_context_instance(
    prompt: &str,
    instances: &[InstanceSummary],
) -> Option<InstanceSummary> {
    assistant_unique_text_matched_instance(prompt, instances)
}

pub(super) fn format_assistant_port_bindings(ports: &[PortBinding]) -> String {
    if ports.is_empty() {
        return String::from("- none");
    }

    ports
        .iter()
        .map(|port| {
            format!(
                "- {} | protocol={} | port={}",
                redact_assistant_provider_text(&port.name),
                redact_assistant_provider_text(&port.protocol),
                port.port
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assistant_selected_module_gm_hint(module_id: Option<&str>) -> &'static str {
    match module_id {
        Some("arksurvivalascended" | "arksurvivalevolved") => {
            "Source RCON; examples: GMSummon, DestroyWildDinos."
        }
        Some("dontstarve") => "stdin processKey=master; templated reward or revive commands only.",
        Some("terraria") => "stdin processKey=main; playing, save, kick/ban one character name.",
        Some("minecraft") => {
            "Source RCON portName=rcon passwordSettingKey=rcon_password enabledSettingKey=enable_rcon; list, save-all flush, and account moderation commands."
        }
        Some("projectzomboid") => {
            "Source RCON portName=rcon passwordSettingKey=rcon_password enabledSettingKey=rcon_enabled; players, save, and account moderation commands."
        }
        Some("palworld") => {
            "Palworld REST API transport=palworld_rest; ShowPlayers and Save resolve to declared actions. Use authoritative online-player rows for kick or ban, and the manual user-ID action for unban."
        }
        Some("sevendaystodie") => "Telnet; listplayerids, saveworld, kick, or ban commands.",
        Some("rust") => {
            "WebSocket RCON; status, players, users, server.writecfg, or account moderation commands."
        }
        Some("vrising") => "Player administration is unsupported; return none.",
        Some(_) => "No assistant GM allowlist is supplied; return none.",
        None => "No selected module; return none for GM requests.",
    }
}

pub(super) fn truncate_assistant_prompt_text(value: &str, max_bytes: usize) -> String {
    let value = value.trim();
    if value.len() <= max_bytes {
        return value.to_string();
    }

    const SUFFIX: &str = "\n[truncated]";
    if max_bytes <= SUFFIX.len() {
        return SUFFIX[..max_bytes].to_string();
    }
    let mut end = max_bytes - SUFFIX.len();
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], SUFFIX)
}

fn truncate_assistant_log_tail(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    const PREFIX: &str = "[earlier log lines omitted]\n";
    let mut start = value
        .len()
        .saturating_sub(max_bytes.saturating_sub(PREFIX.len()));
    while !value.is_char_boundary(start) {
        start += 1;
    }
    format!("{PREFIX}{}", &value[start..])
}

fn push_assistant_prompt_section(
    prompt: &mut String,
    label: &str,
    value: &str,
    section_limit: usize,
) {
    let separator = if prompt.is_empty() { "" } else { "\n\n" };
    let header = format!("{label}:\n");
    let fixed_bytes = separator.len() + header.len();
    let remaining = ASSISTANT_PLANNER_PROMPT_BYTES.saturating_sub(prompt.len());
    if remaining <= fixed_bytes {
        return;
    }
    let value_limit = section_limit.min(remaining - fixed_bytes);
    prompt.push_str(separator);
    prompt.push_str(&header);
    prompt.push_str(&truncate_assistant_prompt_text(value, value_limit));
}

pub(super) fn build_assistant_operation_planner_prompt(
    input: &AssistantExecuteOperationInput,
    instances: &[InstanceSummary],
    modules: &[ModuleSummary],
    selected_instance: Option<&InstanceDetails>,
    selected_module: Option<&ModuleDetails>,
    config_documents: &[AssistantOperationConfigDocument],
    runtime_log_snapshot: Option<&LogTailSnapshot>,
) -> Result<String, String> {
    let selected_instance_id = selected_instance.map(|details| details.summary.id.as_str());
    let selected_module_id = selected_module
        .map(|details| details.summary.id.as_str())
        .or_else(|| selected_instance.map(|details| details.summary.module_id.as_str()));
    let mut user_request = String::new();
    if let Some(instance) = selected_instance {
        user_request.push_str(&format!(
            "Selected instance id: {}\n",
            redact_assistant_provider_text(&instance.summary.id)
        ));
    }
    if let Some(module_id) = selected_module_id {
        user_request.push_str(&format!(
            "Selected module id: {}\n",
            redact_assistant_provider_text(module_id)
        ));
    }
    user_request.push_str(&redact_assistant_provider_text(input.prompt.trim()));
    // User restrictions and the action contract must remain complete together.
    let mut prompt = format!("User request:\n{user_request}\n\n{ASSISTANT_OPERATION_ACTION_GUIDE}");
    if prompt.len() > ASSISTANT_PLANNER_PROMPT_BYTES {
        return Err(String::from(
            "The complete user request and action contract exceed the planner context budget. Shorten the request; no operation was executed.",
        ));
    }
    let instances_text = instances
        .iter()
        .filter(|instance| selected_instance_id.is_none_or(|id| instance.id == id))
        .map(|instance| {
            format!(
                "- {} | id={} | module={} | status={:?} | bind={}",
                redact_assistant_provider_text(&instance.name),
                redact_assistant_provider_text(&instance.id),
                redact_assistant_provider_text(&instance.module_id),
                instance.status,
                redact_assistant_provider_text(&instance.bind_ip)
            )
        })
        .collect::<Vec<_>>();
    let instances_text = if instances_text.is_empty() {
        String::from("- none")
    } else {
        instances_text.join("\n")
    };
    let modules_text = modules
        .iter()
        .filter(|module| selected_module_id.is_none_or(|id| module.id == id))
        .map(|module| {
            format!(
                "- {} | id={} | installState={:?} | steamAppId={}",
                redact_assistant_provider_text(&module.name),
                redact_assistant_provider_text(&module.id),
                module.install_state,
                module
                    .steam_app_id
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| String::from("-"))
            )
        })
        .collect::<Vec<_>>();
    let modules_text = if modules_text.is_empty() {
        String::from("- none")
    } else {
        modules_text.join("\n")
    };
    let needs_config_context = assistant_task_needs_config_context(input.task.goal);
    let selected_settings = if needs_config_context {
        selected_instance
            .map(|details| redact_assistant_provider_text(&details.settings_json))
            .unwrap_or_else(|| String::from("{}"))
    } else {
        String::from("- omitted for non-config request")
    };
    let selected_schema = if needs_config_context {
        summarize_assistant_schema_keys(
            selected_module.and_then(|details| details.schema_json.as_deref()),
        )
    } else {
        String::from("- omitted for non-config request")
    };
    let config_text = if !needs_config_context {
        String::from("- omitted for non-config request")
    } else if config_documents.is_empty() {
        String::from("- none")
    } else {
        config_documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                format!(
                    "### config-document-{}\n{}{}",
                    index + 1,
                    redact_assistant_provider_text(&document.content),
                    if document.truncated {
                        "\n[truncated]"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let needs_runtime_log_context = input.task.goal == AssistantTaskGoal::RestoreService;
    let runtime_log_text = if !needs_runtime_log_context {
        String::from("- omitted for non-log request")
    } else {
        runtime_log_snapshot
            .map(format_assistant_runtime_log_snapshot)
            .map(|snapshot| redact_assistant_provider_text(&snapshot))
            .map(|snapshot| truncate_assistant_log_tail(&snapshot, 600))
            .unwrap_or_else(|| String::from("- none"))
    };
    let selected_ports = selected_instance
        .map(|details| format_assistant_port_bindings(&details.ports))
        .unwrap_or_else(|| String::from("- none"));
    let additional_context =
        redact_assistant_provider_text(input.context.as_deref().unwrap_or_default().trim());
    push_assistant_prompt_section(&mut prompt, "Selected instance ports", &selected_ports, 350);
    push_assistant_prompt_section(
        &mut prompt,
        "Selected module GM policy",
        assistant_selected_module_gm_hint(selected_module_id),
        220,
    );
    push_assistant_prompt_section(
        &mut prompt,
        "Selected instance settings_json",
        &selected_settings,
        550,
    );
    push_assistant_prompt_section(
        &mut prompt,
        "Selected module schema_json",
        &selected_schema,
        450,
    );
    push_assistant_prompt_section(
        &mut prompt,
        "Readable instance config documents",
        &config_text,
        700,
    );
    push_assistant_prompt_section(
        &mut prompt,
        "Latest instance runtime log",
        &runtime_log_text,
        600,
    );
    push_assistant_prompt_section(
        &mut prompt,
        "Additional UI context",
        &additional_context,
        250,
    );
    push_assistant_prompt_section(&mut prompt, "Available instances", &instances_text, 600);
    push_assistant_prompt_section(&mut prompt, "Available modules", &modules_text, 600);
    debug_assert!(prompt.len() <= ASSISTANT_PLANNER_PROMPT_BYTES);
    Ok(prompt)
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;
