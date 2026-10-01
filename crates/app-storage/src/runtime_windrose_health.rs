use app_core::RuntimeHealth;

/// Native stages shared by the bootstrap and the current-run health observer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindroseNativeStage {
    Loading,
    HostReady,
    Stopping,
}

pub fn windrose_native_stage(line: &str) -> Option<WindroseNativeStage> {
    let line = line.trim();
    let message = if line.starts_with('[') {
        super::ark_readiness::native_line_timestamp(line)?;
        let (frame, message) = line.get(25..)?.strip_prefix('[')?.split_once(']')?;
        frame.trim().parse::<u32>().ok()?;
        message
    } else {
        line
    };
    if message.starts_with("LogLoad: LoadMap: ") {
        return Some(WindroseNativeStage::Loading);
    }
    if message.starts_with("LogCore: Engine exit requested (") {
        return Some(WindroseNativeStage::Stopping);
    }
    // R5 includes its own frame and function fields after the Unreal category.
    // Lobby completion and EngineInit precede this host-connect declaration.
    let message = message.strip_prefix("R5LogCoopProxy:")?.trim_start();
    let (frame, message) = message.strip_prefix('[')?.split_once(']')?;
    frame.trim().parse::<u32>().ok()?;
    let (function, message) = message.trim_start().split_once(char::is_whitespace)?;
    if !function.ends_with("::SetIsReadyForHostOwnerConnect") {
        return None;
    }
    let semaphore = message
        .trim_start()
        .strip_prefix("Host server is ready for owner to connect. Semaphore ")?;
    if semaphore.trim().is_empty() {
        return None;
    }
    Some(WindroseNativeStage::HostReady)
}

pub(super) fn analyze(lines: &[String]) -> RuntimeHealth {
    let latest = lines
        .iter()
        .rev()
        .find_map(|line| windrose_native_stage(line).map(|stage| (line, stage)));
    let (status, summary, reason) = match latest.map(|(_, stage)| stage) {
        Some(WindroseNativeStage::HostReady) => (
            "ready",
            "Windrose reports that its current host is ready for a player to connect.",
            "ready_signal",
        ),
        Some(WindroseNativeStage::Stopping) => ("idle", "Windrose is stopping.", "stopping"),
        _ => (
            "starting",
            "Waiting for Windrose to load its world and accept a player connection.",
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
