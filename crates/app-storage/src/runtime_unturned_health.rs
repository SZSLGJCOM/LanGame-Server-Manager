use app_core::RuntimeHealth;

#[derive(Clone, Copy)]
pub(super) enum Event {
    Steam(bool),
    Level(u8),
}

pub(super) fn event(line: &str) -> Option<Event> {
    let line = line.trim();
    if line == "Steam servers ready!" {
        return Some(Event::Steam(true));
    }
    // These templates are present in the installed Assembly-CSharp.dll. Steam
    // readiness precedes level loading, and losing it revokes join readiness.
    if line == "Waiting for Steam servers..."
        || line
            .strip_prefix("Lost connection to Steam servers because ")
            .is_some_and(|reason| !reason.is_empty())
        || line
            .strip_prefix("Failed to connect to Steam servers because ")
            .is_some_and(|reason| {
                reason.ends_with(", still retrying") || reason.ends_with(", no longer retrying")
            })
    {
        return Some(Event::Steam(false));
    }
    line.strip_prefix("Loading level: ")?
        .strip_suffix('%')
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))?
        .parse::<u8>()
        .ok()
        .filter(|value| *value <= 100)
        .map(Event::Level)
}

pub(super) fn analyze(lines: &[String]) -> RuntimeHealth {
    let mut steam_ready = false;
    let mut level_ready = false;
    let mut steam_line = None;
    let mut level_line = None;
    for line in lines {
        match event(line) {
            Some(Event::Steam(ready)) => {
                steam_ready = ready;
                steam_line = Some(line.clone());
            }
            Some(Event::Level(progress)) => {
                level_ready = progress == 100;
                level_line = Some(line.clone());
            }
            None => {}
        }
    }
    let ready = steam_ready && level_ready;
    RuntimeHealth {
        status: if ready { "ready" } else { "starting" }.into(),
        summary: if ready {
            "Unturned connected to Steam and completed loading its level."
        } else {
            "Waiting for Unturned to connect to Steam and finish loading its level."
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
        matched_line: if steam_ready {
            level_line.or(steam_line)
        } else {
            steam_line.or(level_line)
        },
    }
}
