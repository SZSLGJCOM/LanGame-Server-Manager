use crate::atomic_file::compare_and_swap_optional_file_atomically;
use crate::managed_console_log::owned_fs::{FileIdentity, identity, reject_links};
use crate::managed_console_log::{ManagedLogSegment, open_log_segments};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const SCAN_BYTES: u64 = 1024 * 1024;
const STATE_BYTES: u64 = 16 * 1024;
const LINE_BYTES: usize = 1024;
const CHECK_BYTES: u64 = 64;

#[derive(Clone, Copy)]
pub(super) enum SessionKind {
    Enshrouded,
    Dragonwilds,
    Unturned,
    Squad,
    Soulmask,
    TheForest,
    SonsOfTheForest,
    Nightingale,
    Windrose,
}

impl SessionKind {
    fn checkpoint_name(self) -> &'static str {
        match self {
            Self::Enshrouded => ".enshrouded-session.json",
            Self::Dragonwilds => ".dragonwilds-session.json",
            Self::Unturned => ".unturned-session.json",
            Self::Squad => ".squad-session.json",
            Self::Soulmask => ".soulmask-session.json",
            Self::TheForest => ".theforest-session.json",
            Self::SonsOfTheForest => ".sonsoftheforest-session.json",
            Self::Nightingale => ".nightingale-session.json",
            Self::Windrose => ".windrose-session.json",
        }
    }

    fn valid(self, state: &Checkpoint) -> bool {
        if !matches!(self, Self::Dragonwilds)
            && (state.native_process.is_some()
                || state.fatal.is_some()
                || !state.native_head.is_empty()
                || state.native_latest_end.is_some()
                || state.native_fatal_end.is_some())
        {
            return false;
        }
        match self {
            Self::Dragonwilds => {
                state.latest.is_some() == state.native_latest_end.is_some()
                    && state.fatal.is_some() == state.native_fatal_end.is_some()
                    && state.native_latest_end.is_none_or(|end| {
                        end <= state.offset
                            && state
                                .latest
                                .as_ref()
                                .is_some_and(|line| end >= line.len() as u64)
                    })
                    && state.native_fatal_end.is_none_or(|end| {
                        end <= state.offset
                            && state
                                .fatal
                                .as_ref()
                                .is_some_and(|line| end >= line.len() as u64)
                    })
                    && state.steam.is_none()
                    && state.online_session.is_none()
                    && state.engine_initialized.is_none()
                    && state
                        .latest
                        .as_deref()
                        .is_none_or(|line| super::dragonwilds_health::event(line).is_some())
                    && state.fatal.as_deref().is_none_or(|line| {
                        matches!(
                            super::dragonwilds_health::event(line),
                            Some(super::dragonwilds_health::Event::Fatal)
                        )
                    })
            }
            Self::Enshrouded => {
                state.steam.is_none()
                    && state.online_session.is_none()
                    && state.engine_initialized.is_none()
                    && state
                        .latest
                        .as_ref()
                        .is_none_or(|line| super::enshrouded_session_state(line).is_some())
            }
            Self::Unturned => {
                state.online_session.is_none()
                    && state.engine_initialized.is_none()
                    && state.latest.as_ref().is_none_or(|line| {
                        matches!(
                            super::unturned_health::event(line),
                            Some(super::unturned_health::Event::Level(_))
                        )
                    })
                    && state.steam.as_ref().is_none_or(|line| {
                        matches!(
                            super::unturned_health::event(line),
                            Some(super::unturned_health::Event::Steam(_))
                        )
                    })
            }
            Self::Squad => {
                state.steam.is_none()
                    && state.engine_initialized.is_none()
                    && state.latest.as_ref().is_none_or(|line| {
                        matches!(
                            super::squad_health::event(line),
                            Some(
                                super::squad_health::Event::Reset
                                    | super::squad_health::Event::WorldReady
                            )
                        )
                    })
                    && state.online_session.as_ref().is_none_or(|line| {
                        matches!(
                            super::squad_health::event(line),
                            Some(super::squad_health::Event::SessionReady)
                        )
                    })
            }
            Self::Soulmask => {
                state.steam.is_none()
                    && state.online_session.is_none()
                    && state.latest.as_ref().is_none_or(|line| {
                        matches!(
                            super::soulmask_health::event(line),
                            Some(
                                super::soulmask_health::Event::MapLoading
                                    | super::soulmask_health::Event::GameStarted
                                    | super::soulmask_health::Event::Exiting
                            )
                        )
                    })
                    && state.engine_initialized.as_ref().is_none_or(|line| {
                        matches!(
                            super::soulmask_health::event(line),
                            Some(super::soulmask_health::Event::EngineInitialized)
                        )
                    })
            }
            Self::TheForest => {
                state.steam.is_none()
                    && state.online_session.is_none()
                    && state.engine_initialized.is_none()
                    && state
                        .latest
                        .as_ref()
                        .is_none_or(|line| super::theforest_health::event(line).is_some())
            }
            Self::SonsOfTheForest => {
                state.steam.is_none()
                    && state.online_session.is_none()
                    && state.engine_initialized.is_none()
                    && state
                        .latest
                        .as_ref()
                        .is_none_or(|line| super::sonsoftheforest_health::event(line).is_some())
            }
            Self::Nightingale => {
                state.steam.is_none()
                    && state.online_session.is_none()
                    && state.engine_initialized.is_none()
                    && state
                        .latest
                        .as_ref()
                        .is_none_or(|line| super::nightingale_health::event(line).is_some())
            }
            Self::Windrose => {
                state.steam.is_none()
                    && state.online_session.is_none()
                    && state.engine_initialized.is_none()
                    && state.latest.as_ref().is_none_or(|line| {
                        super::windrose_health::windrose_native_stage(line).is_some()
                    })
            }
        }
    }

    fn consume(self, state: &mut Checkpoint, line: &str) {
        match self {
            Self::Dragonwilds => match super::dragonwilds_health::event(line) {
                Some(super::dragonwilds_health::Event::Fatal) => state.fatal = Some(line.into()),
                Some(_) => state.latest = Some(line.into()),
                None => {}
            },
            Self::Enshrouded if super::enshrouded_session_state(line).is_some() => {
                state.latest = Some(line.into())
            }
            Self::Unturned => match super::unturned_health::event(line) {
                Some(super::unturned_health::Event::Steam(_)) => state.steam = Some(line.into()),
                Some(super::unturned_health::Event::Level(_)) => state.latest = Some(line.into()),
                None => {}
            },
            Self::Squad => match super::squad_health::event(line) {
                Some(super::squad_health::Event::Reset) => {
                    state.latest = Some(line.into());
                    state.online_session = None;
                }
                Some(super::squad_health::Event::WorldReady) => state.latest = Some(line.into()),
                Some(super::squad_health::Event::SessionReady) => {
                    state.online_session = Some(line.into())
                }
                None => {}
            },
            Self::Soulmask => match super::soulmask_health::event(line) {
                Some(super::soulmask_health::Event::Exiting) => {
                    state.latest = Some(line.into());
                    state.engine_initialized = None;
                }
                // Engine initialization belongs to this process, while map
                // loading revokes only the readiness of its current world.
                Some(
                    super::soulmask_health::Event::MapLoading
                    | super::soulmask_health::Event::GameStarted,
                ) => state.latest = Some(line.into()),
                Some(super::soulmask_health::Event::EngineInitialized) => {
                    state.engine_initialized = Some(line.into())
                }
                None => {}
            },
            Self::TheForest if super::theforest_health::event(line).is_some() => {
                state.latest = Some(line.into())
            }
            Self::SonsOfTheForest if super::sonsoftheforest_health::event(line).is_some() => {
                // A later Ready (or startup banner) cannot recover a fatal native
                // process. Only the existing run/source/file-generation reset
                // clears this evidence; retain it after it leaves the UI tail.
                if !matches!(
                    state
                        .latest
                        .as_deref()
                        .and_then(super::sonsoftheforest_health::event),
                    Some(super::sonsoftheforest_health::Event::Fatal)
                ) {
                    state.latest = Some(line.into());
                }
            }
            Self::Nightingale if super::nightingale_health::event(line).is_some() => {
                state.latest = Some(line.into())
            }
            Self::Windrose if super::windrose_health::windrose_native_stage(line).is_some() => {
                state.latest = Some(line.into())
            }
            _ => {}
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedProcess {
    pid: u32,
    session_id: Option<String>,
    identity: app_core::ProcessIdentity,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    run_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_process: Option<ObservedProcess>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fatal: Option<String>,
    source: String,
    offset: u64,
    partial: Vec<u8>,
    overlong: bool,
    latest: Option<String>,
    anchor_segment: u64,
    anchor_identity: Option<FileIdentity>,
    anchor: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    native_head: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_latest_end: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_fatal_end: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    steam: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    online_session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    engine_initialized: Option<String>,
}

/// One derived checkpoint per instance log directory, bounded independently of
/// transcript length. It follows logical managed offsets across segment rotation.
pub(super) fn observe(kind: SessionKind, path: &Path, run_id: i64) -> io::Result<Vec<String>> {
    observe_with_process(kind, path, run_id, None, false)
}

/// Shared native files need the current OS creation identity in addition to the
/// recorded run. Immutable managed transcripts already belong to one run.
pub(super) fn observe_dragonwilds(
    path: &Path,
    run: &app_core::ActiveInstanceRun,
) -> io::Result<Vec<String>> {
    let shared_native = run
        .log_path
        .as_deref()
        .is_none_or(|source| Path::new(source) != path);
    if !shared_native {
        return observe_with_process(SessionKind::Dragonwilds, path, run.run_id, None, false);
    }
    let Some(process) = super::health_evidence::dragonwilds_primary(run) else {
        return Ok(Vec::new());
    };
    observe_with_process(
        SessionKind::Dragonwilds,
        path,
        run.run_id,
        Some(process),
        shared_native,
    )
}

fn observe_with_process(
    kind: SessionKind,
    path: &Path,
    run_id: i64,
    process: Option<&app_core::InstanceProcessState>,
    shared_native: bool,
) -> io::Result<Vec<String>> {
    let observed_process = process
        .map(|process| {
            Ok::<_, io::Error>(ObservedProcess {
                pid: process
                    .pid
                    .ok_or_else(|| io::Error::other("process PID is missing"))?,
                session_id: process.session_id.clone(),
                identity: process
                    .process_identity
                    .clone()
                    .ok_or_else(|| io::Error::other("process identity is missing"))?,
            })
        })
        .transpose()?;
    if run_id <= 0 {
        return Err(io::Error::other("invalid session run ID"));
    }
    reject_links(path)?;
    let checkpoint_path = path
        .parent()
        .ok_or_else(|| io::Error::other("session log has no directory"))?
        .join(kind.checkpoint_name());
    reject_links(&checkpoint_path)?;
    for _ in 0..3 {
        let expected = match File::open(&checkpoint_path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(STATE_BYTES + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > STATE_BYTES {
                    return Err(io::Error::other("session checkpoint is oversized"));
                }
                Some(bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let mut state: Checkpoint = expected
            .as_ref()
            .map(|bytes| serde_json::from_slice(bytes))
            .transpose()
            .map_err(io::Error::other)?
            .unwrap_or_default();
        if state.native_head.len() as u64 > CHECK_BYTES
            || state
                .fatal
                .as_ref()
                .is_some_and(|line| line.len() > LINE_BYTES)
            || state.partial.len() > LINE_BYTES
            || state.anchor.len() as u64 > CHECK_BYTES
            || state
                .latest
                .as_ref()
                .is_some_and(|line| line.len() > LINE_BYTES)
            || state
                .steam
                .as_ref()
                .is_some_and(|line| line.len() > LINE_BYTES)
            || state
                .online_session
                .as_ref()
                .is_some_and(|line| line.len() > LINE_BYTES)
            || state
                .engine_initialized
                .as_ref()
                .is_some_and(|line| line.len() > LINE_BYTES)
            || !kind.valid(&state)
        {
            return Err(io::Error::other("invalid session checkpoint"));
        }
        // A concurrent request for an older run must not replace newer evidence.
        if state.run_id > run_id
            || (state.run_id == run_id
                && state
                    .native_process
                    .as_ref()
                    .zip(observed_process.as_ref())
                    .is_some_and(|(stored, observed)| {
                        stored.identity.creation_time > observed.identity.creation_time
                    }))
        {
            return Ok(Vec::new());
        }
        let source = path.to_string_lossy();
        if state.run_id != run_id
            || state.source != source
            || state.native_process != observed_process
        {
            state = Checkpoint {
                run_id,
                native_process: observed_process.clone(),
                source: source.into_owned(),
                ..Default::default()
            };
        }
        let mut segments = match open_log_segments(path)? {
            Some(segments) => segments,
            None => vec![ManagedLogSegment {
                path: path.into(),
                file: File::open(path)?,
                start_offset: 0,
            }],
        };
        let caught_up = advance(kind, &mut segments, &mut state, process, shared_native)?;
        // A shared native file can be replaced while its old handle is scanned.
        // Never publish readiness for the displaced generation.
        if shared_native {
            reject_links(path)?;
            if state.anchor_identity.as_ref() != Some(&identity(&File::open(path)?)?) {
                return Ok(Vec::new());
            }
        }
        let result = if caught_up {
            state
                .fatal
                .iter()
                .chain(state.steam.iter())
                .chain(state.latest.iter())
                .chain(state.online_session.iter())
                .chain(state.engine_initialized.iter())
                .cloned()
                .collect()
        } else {
            state.fatal.iter().cloned().collect()
        };
        let replacement = serde_json::to_vec(&state).map_err(io::Error::other)?;
        if replacement.len() as u64 > STATE_BYTES {
            return Err(io::Error::other("session checkpoint exceeds limit"));
        }
        if expected.as_deref() == Some(replacement.as_slice())
            || compare_and_swap_optional_file_atomically(
                &checkpoint_path,
                expected.as_deref(),
                Some(&replacement),
            )?
        {
            return Ok(result);
        }
    }
    Err(io::Error::other(
        "session checkpoint changed during observation",
    ))
}

fn advance(
    kind: SessionKind,
    segments: &mut [ManagedLogSegment],
    state: &mut Checkpoint,
    process: Option<&app_core::InstanceProcessState>,
    shared_native: bool,
) -> io::Result<bool> {
    let first = segments
        .first()
        .ok_or_else(|| io::Error::other("session log has no retained segment"))?
        .start_offset;
    let last = segments.last().unwrap();
    let end = last
        .start_offset
        .checked_add(last.file.metadata()?.len())
        .ok_or_else(|| io::Error::other("session log offset overflow"))?;
    let mut reset = state.offset < first || state.offset > end;
    if shared_native && let Some(first) = segments.first_mut() {
        reset |= region(&mut first.file, 0, state.native_head.len() as u64)? != state.native_head;
        for (line, end) in [
            (state.latest.as_ref(), state.native_latest_end),
            (state.fatal.as_ref(), state.native_fatal_end),
        ] {
            if let (Some(line), Some(end)) = (line, end) {
                reset |= region(
                    &mut first.file,
                    end.saturating_sub(line.len() as u64),
                    line.len() as u64,
                )? != line.as_bytes();
            }
        }
    }
    if let Some(expected) = &state.anchor_identity {
        if let Some(segment) = segments
            .iter_mut()
            .find(|segment| segment.start_offset == state.anchor_segment)
        {
            let relative = state.offset.saturating_sub(segment.start_offset);
            reset |= identity(&segment.file)? != *expected
                || region(
                    &mut segment.file,
                    relative.saturating_sub(state.anchor.len() as u64),
                    state.anchor.len() as u64,
                )? != state.anchor;
        } else if state.offset != first {
            reset = true;
        }
    }
    if reset {
        state.offset = first;
        state.partial.clear();
        state.overlong = false;
        state.latest = None;
        state.fatal = None;
        state.native_latest_end = None;
        state.native_fatal_end = None;
        state.native_head.clear();
        state.steam = None;
        state.online_session = None;
        state.engine_initialized = None;
        state.anchor.clear();
        state.anchor_identity = None;
    }
    let mut remaining = SCAN_BYTES;
    for segment in segments.iter_mut() {
        let length = segment.file.metadata()?.len();
        let segment_end = segment
            .start_offset
            .checked_add(length)
            .ok_or_else(|| io::Error::other("session segment offset overflow"))?;
        if state.offset > segment_end {
            continue;
        }
        if state.offset < segment.start_offset {
            // A gap may have contained an offline transition; never retain ready.
            state.offset = segment.start_offset;
            state.latest = None;
            state.fatal = None;
            state.native_latest_end = None;
            state.native_fatal_end = None;
            state.native_head.clear();
            state.steam = None;
            state.online_session = None;
            state.engine_initialized = None;
            state.partial.clear();
            state.overlong = false;
        }
        let relative = state.offset - segment.start_offset;
        let bytes = region(
            &mut segment.file,
            relative,
            remaining.min(length.saturating_sub(relative)),
        )?;
        remaining -= bytes.len() as u64;
        state.offset += bytes.len() as u64;
        let chunk_start = state.offset - bytes.len() as u64;
        for (index, byte) in bytes.into_iter().enumerate() {
            if byte == b'\n' || byte == b'\r' {
                if !state.overlong {
                    let line = String::from_utf8_lossy(&state.partial).into_owned();
                    let is_current = !shared_native
                        || process.is_some_and(|process| {
                            super::ark_readiness::native_line_is_current(process, &line)
                                || (super::ark_readiness::native_line_timestamp(&line).is_none()
                                    && matches!(
                                        super::dragonwilds_health::event(&line),
                                        Some(super::dragonwilds_health::Event::Fatal)
                                    ))
                        });
                    if is_current {
                        kind.consume(state, &line);
                        if matches!(kind, SessionKind::Dragonwilds) {
                            match super::dragonwilds_health::event(&line) {
                                Some(super::dragonwilds_health::Event::Fatal) => {
                                    state.native_fatal_end = Some(chunk_start + index as u64)
                                }
                                Some(_) => {
                                    state.native_latest_end = Some(chunk_start + index as u64)
                                }
                                None => {}
                            }
                        }
                    }
                }
                state.partial.clear();
                state.overlong = false;
            } else if state.partial.len() < LINE_BYTES && !state.overlong {
                state.partial.push(byte);
            } else {
                state.partial.clear();
                state.overlong = true;
            }
        }
        let relative = state.offset - segment.start_offset;
        state.anchor_segment = segment.start_offset;
        state.anchor_identity = Some(identity(&segment.file)?);
        state.anchor = region(
            &mut segment.file,
            relative.saturating_sub(CHECK_BYTES),
            relative.min(CHECK_BYTES),
        )?;
        if remaining == 0 {
            break;
        }
    }
    if shared_native && let Some(first) = segments.first_mut() {
        state.native_head = region(&mut first.file, 0, CHECK_BYTES)?;
    }
    // Readiness requires a complete transition and no unread later log bytes.
    Ok(state.offset == end && state.partial.is_empty() && !state.overlong)
}

fn region(file: &mut File, offset: u64, limit: u64) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}
