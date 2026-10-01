use app_core::RuntimeHealth;

#[derive(Clone, Copy)]
pub(super) enum Event {
    Reset,
    WorldReady,
    SessionReady,
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
        "LogGameState: Match State Changed from EnteringMap to WaitingToStart" => {
            Some(Event::WorldReady)
        }
        "LogSquadOnlineServices: Session created: Started USQOnlineServicesUpdateSessionManager updates" => {
            Some(Event::SessionReady)
        }
        _ if message.starts_with("LogLoad: LoadMap: ")
            || message.starts_with("LogSquad: OnPreLoadMap: Loading map with URL: ")
            || message.starts_with("LogCore: Engine exit requested") =>
        {
            Some(Event::Reset)
        }
        _ => None,
    }
}

pub(super) fn analyze(lines: &[String]) -> RuntimeHealth {
    let mut world = false;
    let mut session = false;
    let mut matched = None;
    for line in lines {
        match event(line) {
            Some(Event::Reset) => {
                world = false;
                session = false;
            }
            Some(Event::WorldReady) => world = true,
            Some(Event::SessionReady) => session = true,
            None => continue,
        }
        matched = Some(line.clone());
    }
    let ready = world && session;
    RuntimeHealth {
        status: if ready { "ready" } else { "starting" }.into(),
        summary: if ready {
            "Squad loaded its current world and created its online session."
        } else {
            "Waiting for Squad's current world and online session."
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
        matched_line: matched,
    }
}
