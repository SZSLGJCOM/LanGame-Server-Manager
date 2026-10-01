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
        match self {
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

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    run_id: i64,
    source: String,
    offset: u64,
    partial: Vec<u8>,
    overlong: bool,
    latest: Option<String>,
    anchor_segment: u64,
    anchor_identity: Option<FileIdentity>,
    anchor: Vec<u8>,
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
        if state.partial.len() > LINE_BYTES
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
        if state.run_id > run_id {
            return Ok(Vec::new());
        }
        let source = path.to_string_lossy();
        if state.run_id != run_id || state.source != source {
            state = Checkpoint {
                run_id,
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
        let caught_up = advance(kind, &mut segments, &mut state)?;
        let result = if caught_up {
            state
                .steam
                .iter()
                .chain(state.latest.iter())
                .chain(state.online_session.iter())
                .chain(state.engine_initialized.iter())
                .cloned()
                .collect()
        } else {
            Vec::new()
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
        state.steam = None;
        state.online_session = None;
        state.engine_initialized = None;
        state.anchor.clear();
        state.anchor_identity = None;
    }
    let mut remaining = SCAN_BYTES;
    for segment in segments {
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
        for byte in bytes {
            if byte == b'\n' || byte == b'\r' {
                if !state.overlong {
                    let line = String::from_utf8_lossy(&state.partial).into_owned();
                    kind.consume(state, &line);
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
    // Readiness requires a complete transition and no unread later log bytes.
    Ok(state.offset == end && state.partial.is_empty() && !state.overlong)
}

fn region(file: &mut File, offset: u64, limit: u64) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}
