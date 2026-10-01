use serde_json::Value;
use std::path::PathBuf;

pub(super) fn mock_assistant_operation_plan_content(prompt: &str) -> String {
    let normalized_prompt = mock_assistant_normalized_prompt(prompt);
    if normalized_prompt.contains("mock-investigate-mod-order") {
        if normalized_prompt.contains("mock-require-read-failure") {
            assert!(prompt.contains("Initial evidence read failures"));
            assert!(prompt.contains("256 KiB safe read size limit"));
        }
        if !prompt.contains("Read 1:") {
            return String::from(r#"{"tool":"read_mod_state"}"#);
        }
        if !prompt.contains("Read 2:") {
            return String::from(r#"{"tool":"read_settings","keys":["mods"]}"#);
        }
        return serde_json::json!({"action": "customize_config", "settingsPatch": {"mods": "dependency\naddon"},
            "reason": "Apply the operator-supplied dependency order after reading current mod settings."}).to_string();
    }
    let module_id = mock_assistant_infer_module_id(&normalized_prompt);
    let action = mock_assistant_infer_action(&normalized_prompt);
    let settings_patch = mock_assistant_infer_settings_patch(prompt);
    let port_patch = mock_assistant_infer_port_patch(prompt);
    let mocked_runtime_commands = mock_assistant_infer_runtime_commands(prompt);
    let runtime_commands = match action.as_str() {
        "runGmCommand" => {
            if !mocked_runtime_commands.is_empty() {
                mocked_runtime_commands
            } else {
                vec![mock_assistant_infer_gm_command(&normalized_prompt)]
            }
        }
        _ => Vec::new(),
    };
    let transport = if action == "runGmCommand" {
        Some(mock_assistant_infer_transport(
            module_id,
            &normalized_prompt,
        ))
    } else {
        None
    };
    let mod_references = mock_assistant_infer_mod_references(&normalized_prompt);
    let source_paths = if action == "installSiteMod" {
        mock_assistant_infer_source_paths(&normalized_prompt)
    } else {
        Vec::new()
    };

    let mut plan = serde_json::Map::new();
    let _ = plan.insert(String::from("action"), Value::String(action));
    if let Some(settings_patch) = settings_patch {
        let _ = plan.insert(String::from("settingsPatch"), settings_patch);
    }
    if let Some(port_patch) = port_patch {
        let _ = plan.insert(String::from("portPatch"), port_patch);
    }
    if let Some(workshop_item_ids) =
        mock_assistant_extract_marker_value(prompt, "mock-workshop-item-ids:")
            .and_then(mock_assistant_parse_json_value)
    {
        let _ = plan.insert(String::from("workshopItemIds"), workshop_item_ids);
    }
    if let Some(module_id) = module_id {
        let _ = plan.insert(
            String::from("moduleId"),
            Value::String(module_id.to_string()),
        );
    }
    if !mod_references.is_empty() {
        let _ = plan.insert(
            String::from("modReferences"),
            Value::Array(mod_references.into_iter().map(Value::String).collect()),
        );
    }
    if !source_paths.is_empty() {
        let _ = plan.insert(
            String::from("sourcePaths"),
            Value::Array(source_paths.into_iter().map(Value::String).collect()),
        );
    }
    if !runtime_commands.is_empty() {
        let _ = plan.insert(
            String::from("runtimeCommands"),
            Value::Array(runtime_commands.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(transport) = transport {
        let _ = plan.insert(String::from("transport"), Value::String(transport));
    }
    let _ = plan.insert(
        String::from("reason"),
        Value::String(String::from("mocked assistant response")),
    );

    Value::Object(plan).to_string()
}

pub(super) fn mock_assistant_normalized_prompt(prompt: &str) -> String {
    let mut collecting = false;
    let mut task_lines = Vec::new();
    let stop_markers = [
        "actions:",
        "available instances:",
        "available modules:",
        "recommended workshop ids:",
        "selected instance ports:",
        "selected instance settings_json:",
        "selected module schema_json:",
        "readable instance config documents:",
        "latest instance runtime log:",
        "additional ui context:",
    ];

    for raw_line in prompt.lines() {
        let trimmed = raw_line.trim();
        let lower = trimmed.to_ascii_lowercase();

        if lower.starts_with("user request:") {
            collecting = true;
            let inline = trimmed
                .get("user request:".len()..)
                .map(str::trim)
                .unwrap_or_default();
            if !inline.is_empty() {
                task_lines.push(inline.to_string());
            }
            continue;
        }

        if collecting {
            if stop_markers.contains(&lower.as_str()) {
                break;
            }
            task_lines.push(trimmed.to_string());
        }
    }

    if task_lines.is_empty() {
        return prompt.to_lowercase();
    }

    task_lines.join("\n").to_lowercase()
}

pub(super) fn mock_assistant_infer_action(normalized_prompt: &str) -> String {
    if let Some(mocked_action) = mock_assistant_infer_mock_action(normalized_prompt) {
        return mocked_action;
    }

    if normalized_prompt.contains("shut down") {
        return String::from("none");
    }
    if normalized_prompt.contains("gm command")
        || normalized_prompt.contains("ai gm")
        || normalized_prompt.contains("status now")
        || normalized_prompt.contains("run this gm")
        || normalized_prompt.contains("run this ai gm")
        || normalized_prompt.contains("run an ai gm")
    {
        return String::from("runGmCommand");
    }
    if mock_assistant_task_is_start_command(normalized_prompt) {
        return String::from("startServer");
    }
    if normalized_prompt.contains("install this server mod")
        || normalized_prompt.contains("modrinth")
        || normalized_prompt.contains("curseforge")
        || normalized_prompt.contains("nexus")
    {
        return String::from("installSiteMod");
    }
    if normalized_prompt.contains("install a fun") && normalized_prompt.contains("mod") {
        return String::from("installFunMod");
    }
    if normalized_prompt.contains("send a broadcast")
        || (normalized_prompt.contains("broadcast")
            && normalized_prompt.contains("maintenance starts"))
    {
        return String::from("broadcast");
    }
    if normalized_prompt.contains("install or validate")
        || normalized_prompt.contains("install the selected")
        || normalized_prompt.contains("install dedicated")
    {
        return String::from("installServer");
    }
    if normalized_prompt.contains("apply a safe startup")
        || normalized_prompt.contains("update")
        || normalized_prompt.contains("settings")
        || normalized_prompt.contains("apply beginner")
    {
        return String::from("customizeConfig");
    }
    String::from("none")
}

pub(super) fn mock_assistant_task_is_start_command(prompt: &str) -> bool {
    let prompt = prompt.to_lowercase();

    let collect_words = |text: &str| -> Vec<String> {
        text.split(|c: char| {
            c.is_ascii_whitespace() || c == '.' || c == ';' || c == '?' || c == '!' || c == '\n'
        })
        .filter(|token| !token.is_empty())
        .map(|token| {
            token
                .trim_matches(|c: char| {
                    c == '"'
                        || c == '\''
                        || c == '`'
                        || c == '('
                        || c == ')'
                        || c == ':'
                        || c == ','
                })
                .trim()
                .to_string()
        })
        .filter(|token| !token.is_empty())
        .collect()
    };

    let has_start_word =
        |text: &str| -> bool { collect_words(text).iter().any(|word| word == "start") };

    let is_sentence_avoiding_start = prompt.split(['.', ';', '\n', '!']).any(|sentence| {
        let sentence = sentence.trim();
        !sentence.is_empty()
            && (sentence.contains("do not") || sentence.contains("don't"))
            && has_start_word(sentence)
    });
    if is_sentence_avoiding_start {
        return false;
    }

    if prompt.contains("do not start") || prompt.contains("don't start") {
        return false;
    }

    let words = collect_words(&prompt);
    words.iter().any(|word| *word == "start")
}

pub(super) fn mock_assistant_infer_module_id(normalized_prompt: &str) -> Option<&'static str> {
    if normalized_prompt.contains("arksurvivalascended")
        || normalized_prompt.contains("ark ")
        || normalized_prompt.contains("ark survival ascended")
        || normalized_prompt.contains("asa")
    {
        return Some("arksurvivalascended");
    }
    if normalized_prompt.contains("minecraft") {
        return Some("minecraft");
    }
    if normalized_prompt.contains("projectzomboid") || normalized_prompt.contains("project zomboid")
    {
        return Some("projectzomboid");
    }
    if normalized_prompt.contains("vrising") {
        return Some("vrising");
    }
    if normalized_prompt.contains("dontstarve") || normalized_prompt.contains("dst") {
        return Some("dontstarve");
    }
    if normalized_prompt.contains("sevendaystodie") || normalized_prompt.contains("7 days to die") {
        return Some("sevendaystodie");
    }
    if normalized_prompt.contains("terraria") {
        return Some("terraria");
    }
    if normalized_prompt.contains("palworld") {
        return Some("palworld");
    }
    if normalized_prompt.contains("rust") {
        return Some("rust");
    }
    None
}

pub(super) fn mock_assistant_infer_gm_command(normalized_prompt: &str) -> String {
    if let Some(command) = mock_assistant_extract_explicit_gm_command(normalized_prompt) {
        return command;
    }

    let prompt = normalized_prompt.to_lowercase();
    if prompt.contains("show server status")
        || prompt.contains("server status now")
        || prompt.contains("status now")
    {
        return String::from("status");
    }
    if prompt.contains("list connected players")
        || prompt.contains("list players")
        || prompt.contains("show players")
    {
        return String::from("list");
    }

    if normalized_prompt
        .split_whitespace()
        .any(|token| token.eq_ignore_ascii_case("status"))
    {
        return String::from("status");
    }
    if normalized_prompt
        .split_whitespace()
        .any(|token| token.eq_ignore_ascii_case("list"))
    {
        return String::from("list");
    }

    if let Some(mut token) = normalized_prompt
        .split(':')
        .next_back()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        while let Some(suffix) = token.chars().next_back() {
            if matches!(
                suffix,
                '"' | '\'' | '.' | ';' | '!' | '?' | '\n' | '\r' | ' '
            ) {
                token.pop();
                continue;
            }
            break;
        }
        while let Some(prefix) = token.chars().next() {
            if matches!(prefix, '"' | '\'' | '`') {
                token.remove(0);
                continue;
            }
            break;
        }
        while let Some(suffix) = token.chars().next_back() {
            if matches!(suffix, '"' | '\'' | '`') {
                token.pop();
                continue;
            }
            break;
        }

        if !token.is_empty() && token != "broadcast" && token != "status now." {
            if token.eq_ignore_ascii_case("status now")
                || token.eq_ignore_ascii_case("show status")
                || token.eq_ignore_ascii_case("show server status now")
            {
                return String::from("status");
            }
            let token = token
                .split(&[';', '.', '!', '?', '\n', '\r'][..])
                .next()
                .unwrap_or(&token)
                .trim();
            let token = token.trim_matches(|c: char| c == '"' || c == '\'' || c == '`');
            if token.len() > 2 {
                return token.to_string();
            }
        }
    }

    if normalized_prompt.contains("status") {
        String::from("status")
    } else if normalized_prompt.contains("list") {
        String::from("list")
    } else {
        String::from("status")
    }
}

pub(super) fn mock_assistant_extract_explicit_gm_command(prompt: &str) -> Option<String> {
    let normalized_prompt = prompt.to_lowercase();
    let markers = [
        "run this gm command:",
        "run this ai gm command:",
        "run an ai gm command:",
        "run gm command:",
    ];

    for marker in markers {
        if let Some(index) = normalized_prompt.find(marker) {
            let raw = normalized_prompt.get(index + marker.len()..)?;
            let candidate = raw
                .split(&[';', '\n', '\r', '.', '!', '?'][..])
                .next()
                .unwrap_or(raw)
                .trim();
            let candidate = candidate.trim_matches(|c: char| c == '"' || c == '\'' || c == '`');
            if candidate.len() > 2 {
                return Some(String::from(candidate));
            }
        }
    }

    None
}

pub(super) fn mock_assistant_infer_transport(
    module_id: Option<&str>,
    normalized_prompt: &str,
) -> String {
    if let Some(transport) =
        mock_assistant_extract_marker_value(normalized_prompt, "mock-transport:")
    {
        return mock_assistant_normalize_transport_token(&transport);
    }
    if normalized_prompt.contains("websocket") {
        return String::from("websocket_rcon");
    }
    if normalized_prompt.contains("telnet") {
        return String::from("telnet");
    }
    if normalized_prompt.contains("stdin") {
        return String::from("stdin");
    }
    if normalized_prompt.contains("palworld_rest") {
        return String::from("palworld_rest");
    }
    if normalized_prompt.contains("source rcon") || normalized_prompt.contains("source_rcon") {
        return String::from("source_rcon");
    }

    if let Some(module_id) = module_id {
        match module_id {
            "dontstarve" => "stdin".into(),
            "terraria" => "stdin".into(),
            "sevendaystodie" => "telnet".into(),
            "rust" => "websocket_rcon".into(),
            "palworld" => "palworld_rest".into(),
            _ => "source_rcon".into(),
        }
    } else if normalized_prompt.contains("websocket") {
        String::from("websocket_rcon")
    } else {
        String::from("source_rcon")
    }
}

pub(super) fn mock_assistant_infer_settings_patch(normalized_prompt: &str) -> Option<Value> {
    let raw = mock_assistant_extract_marker_value(normalized_prompt, "mock-settings-patch:")?;
    mock_assistant_parse_json_value(raw)
}

pub(super) fn mock_assistant_infer_port_patch(normalized_prompt: &str) -> Option<Value> {
    let raw = mock_assistant_extract_marker_value(normalized_prompt, "mock-port-patch:")?;
    mock_assistant_parse_json_value(raw)
}

pub(super) fn mock_assistant_infer_runtime_commands(normalized_prompt: &str) -> Vec<String> {
    let raw = mock_assistant_extract_marker_value(normalized_prompt, "mock-runtime-commands:")
        .or_else(|| {
            mock_assistant_extract_marker_value(normalized_prompt, "mock-runtime-command:")
        });
    let raw = match raw {
        Some(raw) => raw,
        None => return Vec::new(),
    };

    if let Some(commands) = mock_assistant_parse_json_value(raw.clone())
        && let Some(entries) = commands.as_array()
    {
        return entries
            .iter()
            .filter_map(|entry| entry.as_str().map(str::to_string))
            .collect();
    }

    raw.split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.trim_matches(|c: char| c == '"' || c == '\'' || c == '`'))
        .filter(|token| !token.is_empty())
        .map(String::from)
        .collect()
}

pub(super) fn mock_assistant_normalize_transport_token(value: &str) -> String {
    let normalized = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    match normalized.as_str() {
        "palworldrest" => String::from("palworld_rest"),
        "websocketrcon" => String::from("websocket_rcon"),
        "websocket" => String::from("websocket_rcon"),
        "telnet" => String::from("telnet"),
        "stdin" => String::from("stdin"),
        "sourcercon" => String::from("source_rcon"),
        "rconsource" => String::from("source_rcon"),
        _ => String::from("source_rcon"),
    }
}

pub(super) fn mock_assistant_parse_json_candidate(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let open = bytes.first()?;
    let close = match open {
        b'{' => b'}',
        b'[' => b']',
        _ => return Some(String::from(trimmed)),
    };

    let mut depth: isize = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if *byte == b'\\' && in_string {
            escaped = true;
            continue;
        }
        if *byte == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if *byte == *open {
            depth += 1;
            continue;
        }
        if *byte == close {
            depth -= 1;
            if depth == 0 {
                return Some(trimmed[..=index].to_string());
            }
        }
    }

    Some(String::from(trimmed))
}

pub(super) fn mock_assistant_parse_json_value(raw: String) -> Option<Value> {
    let candidate = mock_assistant_parse_json_candidate(&raw)?
        .trim()
        .to_string();
    serde_json::from_str(&candidate).ok()
}

pub(super) fn mock_assistant_infer_mock_action(normalized_prompt: &str) -> Option<String> {
    let raw = mock_assistant_extract_marker_value(normalized_prompt, "mock-action:")?;
    let raw = raw
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();

    let action = match raw.as_str() {
        "startserver" => "startServer",
        "installserver" => "installServer",
        "applybeginnerconfig" => "applyBeginnerConfig",
        "customizeconfig" => "customizeConfig",
        "installfunmod" => "installFunMod",
        "installsitemod" => "installSiteMod",
        "repairports" => "repairPorts",
        "rungmcommand" => "runGmCommand",
        "broadcast" => "broadcast",
        "none" => "none",
        "noneed" | "noop" => "none",
        _ => return None,
    };

    Some(String::from(action))
}

pub(super) fn mock_assistant_extract_marker_value(
    normalized_prompt: &str,
    marker: &str,
) -> Option<String> {
    for line in normalized_prompt.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        let marker_position = match lower.find(marker) {
            Some(index) => index,
            None => continue,
        };
        let mut value = match trimmed.get(marker_position + marker.len()..) {
            Some(value) => value,
            None => continue,
        };
        value = value.trim();
        if value.is_empty() {
            continue;
        }
        let lower = value.to_ascii_lowercase();
        if let Some(next_marker_position) = lower.find(" mock-") {
            value = value.get(..next_marker_position).unwrap_or(value);
            value = value.trim();
        }
        let value = value.trim_end_matches([',', ';', '.']);
        if !value.is_empty() {
            return Some(String::from(value));
        }
    }
    None
}

pub(super) fn mock_assistant_infer_mod_references(normalized_prompt: &str) -> Vec<String> {
    normalized_prompt
        .split_whitespace()
        .flat_map(|token| token.split(','))
        .map(str::trim)
        .map(|token| token.trim_matches(|c: char| c.is_ascii_punctuation() || c.is_control()))
        .filter(|candidate| {
            candidate.starts_with("http://")
                || candidate.starts_with("https://")
                || candidate.starts_with("modrinth:")
                || candidate.starts_with("mr:")
                || candidate.to_ascii_lowercase().starts_with("curseforge:")
        })
        .map(|candidate| candidate.to_string())
        .collect()
}

pub(super) fn mock_assistant_infer_source_paths(normalized_prompt: &str) -> Vec<String> {
    normalized_prompt
        .split_whitespace()
        .flat_map(|token| token.split(','))
        .map(str::trim)
        .filter(|candidate| {
            let path =
                PathBuf::from(candidate.trim_matches(|c: char| c == '"' || c == '\'' || c == '`'));
            path.exists()
        })
        .map(|candidate| candidate.to_string())
        .collect()
}

pub(super) fn mock_assistant_broadcast_content(prompt: &str) -> String {
    let fallback = "Status update planned by mock assistant.";
    let lines: Vec<&str> = prompt.lines().collect();
    if let Some(last) = lines.last() {
        let trimmed = last.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    for line in lines.iter().rev() {
        let candidate = line.trim();
        if !candidate.is_empty() {
            return candidate.to_string();
        }
    }
    fallback.to_string()
}
