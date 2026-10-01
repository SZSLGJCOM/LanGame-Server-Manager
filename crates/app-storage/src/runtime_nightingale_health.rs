use app_core::RuntimeHealth;

#[derive(Clone, Copy)]
pub(super) enum Event {
    Starting,
    Ready,
    Stopping,
}

pub(super) fn event(line: &str) -> Option<Event> {
    let line = line.trim();
    let message = if line.starts_with('[') {
        super::ark_readiness::native_line_timestamp(line)?;
        let (frame, message) = line.get(25..)?.strip_prefix('[')?.split_once(']')?;
        frame.trim().parse::<u32>().ok()?;
        message
    } else {
        line
    };
    match message {
        "LogNWXGameMode: OnAllLevelsLoaded: levels finished loading, finished restoring world state and building navigation. Going ready." => {
            Some(Event::Ready)
        }
        "LogNWXGameMode: SimStateManager ready, waiting for all RRP levels to finish loading before going ready."
        | "LogNWXGameMode: OnAllLevelsLoaded: levels finished loading, restoring world state and building navigation." => {
            Some(Event::Starting)
        }
        _ if message.starts_with("LogLoad: LoadMap: ") => Some(Event::Starting),
        _ if message.starts_with("LogCore: Engine exit requested (") => Some(Event::Stopping),
        _ => None,
    }
}

pub(super) fn analyze(lines: &[String]) -> RuntimeHealth {
    let latest = lines
        .iter()
        .rev()
        .find_map(|line| event(line).map(|event| (line, event)));
    let (status, summary, reason) = match latest.map(|(_, event)| event) {
        Some(Event::Ready) => (
            "ready",
            "Nightingale finished restoring its realm and building navigation.",
            "ready_signal",
        ),
        Some(Event::Stopping) => ("idle", "Nightingale is stopping.", "stopping"),
        _ => (
            "starting",
            "Waiting for Nightingale to finish preparing its current realm.",
            "starting_tasks",
        ),
    };
    RuntimeHealth {
        status: status.into(),
        summary: summary.into(),
        reason: super::runtime_health_reason(reason, &[]),
        matched_line: latest.map(|(line, _)| line.clone()),
    }
}
