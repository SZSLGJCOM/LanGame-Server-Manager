use app_core::RuntimeHealth;

#[derive(Clone, Copy)]
pub(super) enum Event {
    MapLoading,
    GameStarted,
    EngineInitialized,
    Exiting,
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
        "logStoreGamemode: Display: [ GAME STARTED. ]" => Some(Event::GameStarted),
        "LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()" => {
            Some(Event::EngineInitialized)
        }
        _ if message.starts_with("LogLoad: LoadMap: ") => Some(Event::MapLoading),
        _ if message.starts_with("LogCore: Engine exit requested") => Some(Event::Exiting),
        _ => None,
    }
}

pub(super) fn analyze(lines: &[String]) -> RuntimeHealth {
    let mut game_started = false;
    let mut initialized = false;
    let mut game_line = None;
    let mut engine_line = None;
    for line in lines {
        match event(line) {
            Some(Event::MapLoading) => {
                game_started = false;
                game_line = Some(line.clone());
            }
            Some(Event::GameStarted) => {
                game_started = true;
                game_line = Some(line.clone());
            }
            Some(Event::EngineInitialized) => {
                initialized = true;
                engine_line = Some(line.clone());
            }
            Some(Event::Exiting) => {
                game_started = false;
                initialized = false;
                game_line = Some(line.clone());
                engine_line = None;
            }
            None => {}
        }
    }
    let ready = game_started && initialized;
    RuntimeHealth {
        status: if ready { "ready" } else { "starting" }.into(),
        summary: if ready {
            "Soulmask started its current game and completed engine initialization."
        } else {
            "Waiting for Soulmask's current game and engine initialization."
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
        matched_line: game_line.or(engine_line),
    }
}
