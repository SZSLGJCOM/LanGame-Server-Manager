use app_core::RuntimeHealth;

#[derive(Clone, Copy)]
pub(super) enum Event {
    MapLoading,
    GameStarted,
    EngineInitialized,
    Exiting,
}

#[derive(Default)]
pub(super) struct Readiness {
    game: bool,
    engine: bool,
}

impl Readiness {
    pub(super) fn observe(&mut self, event: Event) {
        match event {
            Event::MapLoading => self.game = false,
            Event::GameStarted => self.game = true,
            Event::EngineInitialized => self.engine = true,
            Event::Exiting => {
                self.game = false;
                self.engine = false;
            }
        }
    }

    pub(super) fn is_ready(&self) -> bool {
        self.game && self.engine
    }
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
    let mut readiness = Readiness::default();
    let mut game_line = None;
    let mut engine_line = None;
    for line in lines {
        let Some(event) = event(line) else { continue };
        readiness.observe(event);
        match event {
            Event::MapLoading => {
                game_line = Some(line.clone());
            }
            Event::GameStarted => {
                game_line = Some(line.clone());
            }
            Event::EngineInitialized => {
                engine_line = Some(line.clone());
            }
            Event::Exiting => {
                game_line = Some(line.clone());
                engine_line = None;
            }
        }
    }
    let ready = readiness.is_ready();
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
