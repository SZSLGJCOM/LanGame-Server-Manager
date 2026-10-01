use app_core::RuntimeHealth;

#[derive(Clone, Copy)]
pub(super) enum Event {
    Starting,
    Ready,
    Stopping,
}

pub(super) fn event(line: &str) -> Option<Event> {
    // The native bootstrap banner precedes Scene.FinishGameLoad. Only the
    // owned bridge's observation of the current world's operable state proves
    // readiness; a later false observation revokes the retained checkpoint.
    match line.trim() {
        "Starting dedicated server"
        | "Dedicated Server Running"
        | "[LanGame native control] world_ready=False" => Some(Event::Starting),
        "[LanGame native control] world_ready=True" => Some(Event::Ready),
        // CoopSteamManagerDS.Shutdown logs this after Steam logoff/cleanup.
        "Shutdown." => Some(Event::Stopping),
        _ => None,
    }
}

pub(super) fn analyze(lines: &[String]) -> RuntimeHealth {
    let latest = lines
        .iter()
        .rev()
        .find_map(|line| event(line).map(|event| (event, line)));
    let (status, reason, summary) = match latest.as_ref().map(|(event, _)| event) {
        Some(Event::Ready) => (
            "ready",
            "ready_signal",
            "The Forest completed loading its dedicated server world.",
        ),
        Some(Event::Stopping) => ("idle", "stopping", "The Forest is shutting down."),
        _ => (
            "starting",
            "starting_tasks",
            "Waiting for The Forest to finish loading its dedicated server world.",
        ),
    };
    RuntimeHealth {
        status: status.into(),
        summary: summary.into(),
        reason: super::runtime_health_reason(reason, &[]),
        matched_line: latest.map(|(_, line)| line.clone()),
    }
}
