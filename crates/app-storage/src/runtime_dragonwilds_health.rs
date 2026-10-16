use app_core::RuntimeHealth;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Ready(bool),
    OwnerInvalid,
    Fatal,
}

pub(super) fn event(line: &str) -> Option<Event> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("[LanGame ") || trimmed.starts_with("[LanGame]") {
        return None;
    }
    if !crate::runtime_fatal_log_lines("runescapedragonwilds", &[line.to_owned()]).is_empty() {
        return Some(Event::Fatal);
    }

    let message = native_message(line)?;
    let normalized = message.to_ascii_lowercase();
    if let Some(ready) = readiness_value(&normalized) {
        return Some(Event::Ready(ready));
    }
    if normalized.contains("dedicatedserver.ini configuration") && normalized.contains("ownerid") {
        return Some(Event::OwnerInvalid);
    }
    None
}

fn native_message(line: &str) -> Option<&str> {
    let line = line.trim();
    // Manager messages contain operator-controlled names and launch arguments.
    // Only a native timestamp may introduce a bracketed game message.
    if !line.starts_with('[') {
        return Some(line);
    }
    super::ark_readiness::native_line_timestamp(line)?;
    let message = line.get(25..)?.trim_start();
    let Some(frame) = message.strip_prefix('[') else {
        return Some(message);
    };
    let (frame, message) = frame.split_once(']')?;
    let frame = frame.trim();
    if frame.is_empty() || !frame.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(message.trim_start())
}

fn readiness_value(message: &str) -> Option<bool> {
    match message {
        "readytojoin value[1]" => return Some(true),
        "readytojoin value[0]" => return Some(false),
        _ => {}
    }
    // Match the property and its own value within one complete native setting.
    // A server name or another setting containing both words is not readiness.
    let setting = message
        .strip_prefix("lognetsessionsettings: setting [\"readytojoin\"] written with key[")?;
    let (key, value) = setting.split_once("] value[")?;
    if key.is_empty() || key.contains(['[', ']', '\r', '\n']) {
        return None;
    }
    match value {
        "1]" => Some(true),
        "0]" => Some(false),
        _ => None,
    }
}

pub(super) fn analyze(lines: &[String]) -> Option<RuntimeHealth> {
    if let Some(line) = lines
        .iter()
        .rev()
        .find(|line| event(line) == Some(Event::Fatal))
    {
        return Some(RuntimeHealth {
            status: "error".into(),
            summary: "A fatal runtime error pattern was detected in the recent log.".into(),
            reason: super::runtime_health_reason("fatal_log_pattern", &[]),
            matched_line: Some(line.clone()),
        });
    }
    for line in lines.iter().rev() {
        match event(line) {
            Some(Event::Ready(true)) => {
                let map_name = latest_map_name(lines);
                let summary = if let Some(map_name) = map_name.as_deref() {
                    format!(
                        "RuneScape: Dragonwilds reported ReadyToJoin after creating the GameSession on map {map_name}."
                    )
                } else {
                    String::from(
                        "RuneScape: Dragonwilds reported ReadyToJoin after creating the GameSession.",
                    )
                };
                return Some(RuntimeHealth {
                    status: "ready".into(),
                    summary,
                    reason: if let Some(map_name) = map_name {
                        super::runtime_health_reason("dragonwilds_ready_map", &[("map", map_name)])
                    } else {
                        super::runtime_health_reason("dragonwilds_ready", &[])
                    },
                    matched_line: Some(line.clone()),
                });
            }
            Some(Event::Ready(false)) => {
                return Some(RuntimeHealth {
                    status: "starting".into(),
                    summary: "RuneScape: Dragonwilds has not made its current GameSession ready for players."
                        .into(),
                    reason: super::runtime_health_reason("starting_tasks", &[]),
                    matched_line: Some(line.clone()),
                });
            }
            Some(Event::OwnerInvalid) => {
                return Some(RuntimeHealth {
                    status: "error".into(),
                    summary: String::from(
                        "RuneScape: Dragonwilds stopped at DedicatedServer.ini validation because OwnerId is missing or invalid.",
                    ),
                    reason: super::runtime_health_reason("dragonwilds_owner_invalid", &[]),
                    matched_line: Some(line.clone()),
                });
            }
            Some(Event::Fatal) | None => {}
        }
    }
    None
}

fn latest_map_name(lines: &[String]) -> Option<String> {
    lines.iter().rev().find_map(|line| {
        let (_, rest) = line.split_once("MapName[")?;
        let (map_name, _) = rest.split_once(']')?;
        let trimmed = map_name.trim();
        (!trimmed.is_empty()).then(|| String::from(trimmed))
    })
}
