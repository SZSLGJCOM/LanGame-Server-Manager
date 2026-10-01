use app_core::RuntimeHealth;

#[derive(Clone, Copy)]
pub(super) enum Event {
    Starting,
    Ready,
    Fatal,
}

pub(super) fn event(line: &str) -> Option<Event> {
    if !crate::runtime_fatal_log_lines("sonsoftheforest", &[line.to_owned()]).is_empty() {
        return Some(Event::Fatal);
    }
    // A2S starts answering before this build finishes loading the world.
    // Only its final DSL notification establishes world readiness.
    match line.trim() {
        "#DSL [Self-Tests] Running self tests..."
        | "#DSL [Dedicated] Starting Sons of the Forest Dedicated Server..." => {
            Some(Event::Starting)
        }
        "#DSL Dedicated server loaded." => Some(Event::Ready),
        _ => None,
    }
}

pub(super) fn analyze(lines: &[String]) -> RuntimeHealth {
    if let Some(line) = crate::runtime_fatal_log_lines("sonsoftheforest", lines).pop() {
        return RuntimeHealth {
            status: "error".into(),
            summary: "A fatal runtime error pattern was detected in the current run.".into(),
            reason: super::runtime_health_reason("fatal_log_pattern", &[]),
            matched_line: Some(line),
        };
    }
    let latest = lines
        .iter()
        .rev()
        .find_map(|line| event(line).map(|event| (event, line)));
    let ready = matches!(latest.as_ref().map(|(event, _)| event), Some(Event::Ready));
    RuntimeHealth {
        status: if ready { "ready" } else { "starting" }.into(),
        summary: if ready {
            "Sons of the Forest completed loading its dedicated server world."
        } else {
            "Waiting for Sons of the Forest to finish loading its dedicated server world."
        }
        .into(),
        reason: super::runtime_health_reason(
            if ready {
                "ready_signal"
            } else {
                "starting_tasks"
            },
            &[],
        ),
        matched_line: latest.map(|(_, line)| line.clone()),
    }
}
