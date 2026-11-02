use app_core::{ActiveInstanceRun, InstanceProcessState, LogTailSnapshot, RuntimeHealth};
use std::borrow::Cow;
use std::path::Path;

/// A shared NWX log must belong to the registered primary process's install,
/// not merely have a familiar filename and a recent modification time.
pub(super) fn native_source_process<'a>(
    path: &Path,
    run: &'a ActiveInstanceRun,
) -> Option<&'a InstanceProcessState> {
    let process = super::health_evidence::dragonwilds_primary(run)?;
    let identity = process.process_identity.as_ref()?;
    let image = Path::new(&identity.image_path);
    let name = image.file_name()?.to_str()?;
    let root = if name.eq_ignore_ascii_case("NWXServer.exe") {
        image.parent()?
    } else if name.eq_ignore_ascii_case("NWXServer-Win64-Shipping.exe") {
        let win64 = image.parent()?;
        let binaries = win64.parent()?;
        let nwx = binaries.parent()?;
        if !win64.file_name()?.to_str()?.eq_ignore_ascii_case("Win64")
            || !binaries
                .file_name()?
                .to_str()?
                .eq_ignore_ascii_case("Binaries")
            || !nwx.file_name()?.to_str()?.eq_ignore_ascii_case("NWX")
        {
            return None;
        }
        nwx.parent()?
    } else {
        return None;
    };
    let expected = root.join("NWX/Saved/Logs/NWX.log");
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches("//?/")
            .to_ascii_lowercase()
    };
    (run.run_id > 0 && normalize(path) == normalize(&expected)).then_some(process)
}

pub(super) fn current_run_lines<'a>(
    snapshot: &'a LogTailSnapshot,
    run: Option<&ActiveInstanceRun>,
    include_undated: bool,
) -> Cow<'a, [String]> {
    let Some(run) = run.filter(|run| run.run_id > 0) else {
        return Cow::Borrowed(&[]);
    };
    let Some(source) = snapshot.source_path.as_deref() else {
        return Cow::Borrowed(&[]);
    };
    if run.log_path.as_deref() == Some(source) {
        return Cow::Borrowed(&snapshot.lines);
    }
    let Some(process) = native_source_process(Path::new(source), run) else {
        return Cow::Borrowed(&[]);
    };
    Cow::Owned(
        snapshot
            .lines
            .iter()
            .filter(|line| {
                super::ark_readiness::native_line_is_current(process, line)
                    || (include_undated
                        && super::ark_readiness::native_line_timestamp(line).is_none())
            })
            .cloned()
            .collect(),
    )
}

pub(super) fn is_fatal(line: &str) -> bool {
    let line = line.trim_start();
    !line.starts_with("[LanGame ")
        && !line.starts_with("[LanGame]")
        && !line.starts_with('{')
        && !crate::runtime_fatal_log_lines("nightingale", &[line.to_owned()]).is_empty()
}

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
