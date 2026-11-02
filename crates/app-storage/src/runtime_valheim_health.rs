use super::startup_readiness::ValheimReadiness;
use crate::atomic_file::compare_and_swap_optional_file_atomically;
use crate::managed_console_log::owned_fs::{FileIdentity, identity, reject_links};
use crate::managed_console_log::{ManagedLogSegment, open_log_segments};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

#[path = "runtime_valheim_log_binding.rs"]
mod log_binding;
use log_binding::{NativeBinding, RunOwner};
pub use log_binding::{ValheimLogHandoff, record_valheim_native_log};

const SCAN_BYTES: u64 = 1024 * 1024;
const STATE_BYTES: u64 = 16 * 1024;
const LINE_BYTES: usize = 64 * 1024;
const ANCHOR_BYTES: u64 = 64;

/// Unlike the generic string checkpoint, no log fragments are serialized here:
/// PlayFab messages contain private join codes, names and network identifiers.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    run_id: i64,
    source: String,
    offset: u64,
    segment_start: u64,
    file_identity: Option<FileIdentity>,
    anchor_length: u64,
    anchor_digest: [u8; 32],
    readiness: ValheimReadiness,
    failed: bool,
    discard_fragment: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<RunOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_binding: Option<NativeBinding>,
}

pub(super) fn health(ready: bool, failed: bool) -> app_core::RuntimeHealth {
    let (status, summary, reason) = if failed {
        (
            "error",
            "Valheim reported a fatal runtime error.",
            "fatal_log_pattern",
        )
    } else if ready {
        (
            "ready",
            "Valheim opened its world and hosting backend.",
            "ready_signal",
        )
    } else {
        (
            "starting",
            "Waiting for Valheim's world and hosting backend.",
            "starting_tasks",
        )
    };
    app_core::RuntimeHealth {
        status: status.into(),
        summary: summary.into(),
        reason: super::runtime_health_reason(reason, &[]),
        matched_line: None,
    }
}

pub(super) fn observe_for_run(
    run: &app_core::ActiveInstanceRun,
) -> io::Result<app_core::RuntimeHealth> {
    observe_for_run_with(run, log_binding::process_is_current)
}

fn checkpoint_path(console: &Path) -> io::Result<std::path::PathBuf> {
    reject_links(console)?;
    let path = console
        .parent()
        .ok_or_else(|| io::Error::other("Valheim log has no directory"))?
        .join(".valheim-session.json");
    reject_links(&path)?;
    Ok(path)
}

fn load_checkpoint(path: &Path) -> io::Result<(Option<Vec<u8>>, Checkpoint)> {
    let expected = match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(STATE_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > STATE_BYTES {
                return Err(io::Error::other(
                    "Valheim checkpoint exceeds its size limit",
                ));
            }
            Some(bytes)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let state: Checkpoint = expected
        .as_ref()
        .map(|bytes| serde_json::from_slice(bytes))
        .transpose()
        .map_err(io::Error::other)?
        .unwrap_or_default();
    if state.anchor_length > ANCHOR_BYTES
        || state.segment_start > state.offset
        || state.anchor_length > state.offset - state.segment_start
    {
        return Err(io::Error::other("Invalid Valheim checkpoint offsets"));
    }
    log_binding::validate_checkpoint(&state)?;
    Ok((expected, state))
}

fn store_checkpoint(path: &Path, expected: Option<&[u8]>, state: &Checkpoint) -> io::Result<bool> {
    let replacement = serde_json::to_vec(state).map_err(io::Error::other)?;
    if replacement.len() as u64 > STATE_BYTES {
        return Err(io::Error::other(
            "Valheim checkpoint exceeds its size limit",
        ));
    }
    if expected == Some(replacement.as_slice()) {
        // Even unchanged state must not authorize a stale result after another
        // observer has published a replacement run or a revocation.
        let (current, _) = load_checkpoint(path)?;
        return Ok(current.as_deref() == expected);
    }
    compare_and_swap_optional_file_atomically(path, expected, Some(&replacement))
}

fn observe_for_run_with(
    run: &app_core::ActiveInstanceRun,
    mut current: impl FnMut(u32, &app_core::ProcessIdentity) -> io::Result<bool>,
) -> io::Result<app_core::RuntimeHealth> {
    if run.run_id <= 0 {
        return Err(io::Error::other("Valheim health requires a recorded run"));
    }
    let path =
        Path::new(run.log_path.as_deref().ok_or_else(|| {
            io::Error::other("Valheim health requires the recorded console path")
        })?);
    let checkpoint = checkpoint_path(path)?;
    for _ in 0..3 {
        let (expected, mut state) = load_checkpoint(&checkpoint)?;
        if !log_binding::adopt_run(&mut state, run, &mut current)? {
            return Ok(health(false, false));
        }
        if let Some(owner) = &state.owner
            && !current(owner.pid, &owner.identity)?
        {
            return Ok(health(false, state.failed));
        }
        let observed = if state.native_binding.is_some() {
            log_binding::advance_native(&mut state)
        } else {
            advance_managed(path, &mut state)
        };
        if let Some(owner) = &state.owner
            && !current(owner.pid, &owner.identity)?
        {
            return Ok(health(false, state.failed));
        }
        if observed.is_err() && state.native_binding.is_some() {
            log_binding::invalidate_native(&mut state);
        }
        if store_checkpoint(&checkpoint, expected.as_deref(), &state)? {
            return observed
                .map(|caught_up| health(caught_up && state.readiness.is_ready(), state.failed));
        }
    }
    Err(io::Error::other(
        "Valheim checkpoint changed during observation",
    ))
}

fn advance_managed(path: &Path, state: &mut Checkpoint) -> io::Result<bool> {
    let mut segments = match open_log_segments(path)? {
        Some(segments) => segments,
        None => vec![ManagedLogSegment {
            path: path.into(),
            file: File::open(path)?,
            start_offset: 0,
        }],
    };
    let caught_up = advance(&mut segments, state)?;
    for segment in &segments {
        reject_links(&segment.path)?;
        if identity(&File::open(&segment.path)?)? != identity(&segment.file)? {
            return Ok(false);
        }
    }
    Ok(caught_up)
}

fn advance(segments: &mut [ManagedLogSegment], state: &mut Checkpoint) -> io::Result<bool> {
    let first = segments
        .first()
        .ok_or_else(|| io::Error::other("Valheim log has no retained segment"))?
        .start_offset;
    let last = segments
        .last()
        .ok_or_else(|| io::Error::other("Valheim log has no retained segment"))?;
    let end = last
        .start_offset
        .checked_add(last.file.metadata()?.len())
        .ok_or_else(|| io::Error::other("Valheim log offset overflow"))?;
    let mut reset = state.offset < first || state.offset > end;
    if let Some(expected) = &state.file_identity {
        if let Some(segment) = segments
            .iter_mut()
            .find(|segment| segment.start_offset == state.segment_start)
        {
            let relative = state.offset.saturating_sub(segment.start_offset);
            reset |= identity(&segment.file)? != *expected
                || digest(
                    &mut segment.file,
                    relative.saturating_sub(state.anchor_length),
                    state.anchor_length,
                )? != state.anchor_digest;
        } else if state.offset != first {
            reset = true;
        }
    }
    if reset {
        state.offset = first;
        state.file_identity = None;
        state.readiness = ValheimReadiness::default();
        state.failed = false;
        state.discard_fragment = first > 0;
    }
    scan(segments, state, end)
}

fn scan(segments: &mut [ManagedLogSegment], state: &mut Checkpoint, end: u64) -> io::Result<bool> {
    // A retained segment may start inside a discarded line. Its first fragment
    // cannot become a complete native readiness marker.
    let mut read_offset = state.offset;
    let mut remaining = SCAN_BYTES;
    let mut partial = Vec::new();
    for segment in segments.iter_mut() {
        let length = segment.file.metadata()?.len();
        let segment_end = segment
            .start_offset
            .checked_add(length)
            .ok_or_else(|| io::Error::other("Valheim log offset overflow"))?;
        if read_offset >= segment_end {
            continue;
        }
        if read_offset < segment.start_offset {
            read_offset = segment.start_offset;
            state.offset = read_offset;
            state.readiness = ValheimReadiness::default();
            state.failed = false;
            partial.clear();
            state.discard_fragment = true;
        }
        let relative = read_offset - segment.start_offset;
        let bytes = region(
            &mut segment.file,
            relative,
            remaining.min(length - relative),
        )?;
        remaining -= bytes.len() as u64;
        for byte in bytes {
            read_offset += 1;
            if matches!(byte, b'\n' | b'\r') {
                if !state.discard_fragment {
                    let line = String::from_utf8_lossy(&partial);
                    if !line.trim_start().starts_with("[LanGame")
                        && !line.trim_start().starts_with('{')
                    {
                        state.failed |=
                            !crate::runtime_fatal_log_lines("valheim", &[line.to_string()])
                                .is_empty();
                        state.readiness.observe_line(&line);
                    }
                }
                partial.clear();
                state.discard_fragment = false;
                state.offset = read_offset;
            } else if state.discard_fragment {
                state.offset = read_offset;
            } else if partial.len() < LINE_BYTES {
                partial.push(byte);
            } else {
                return Err(io::Error::other(
                    "Valheim log record exceeds the bounded observation limit",
                ));
            }
        }
        if remaining == 0 {
            break;
        }
    }
    // Incomplete lines are re-read next time, never copied into a checkpoint.
    // Hash the committed boundary rather than storing any raw anchor bytes.
    for segment in segments.iter_mut().rev() {
        if state.offset >= segment.start_offset
            && state.offset <= segment.start_offset + segment.file.metadata()?.len()
        {
            let relative = state.offset - segment.start_offset;
            state.segment_start = segment.start_offset;
            state.file_identity = Some(identity(&segment.file)?);
            state.anchor_length = relative.min(ANCHOR_BYTES);
            state.anchor_digest = digest(
                &mut segment.file,
                relative - state.anchor_length,
                state.anchor_length,
            )?;
            break;
        }
    }
    Ok(state.offset == end && partial.is_empty())
}

#[cfg(test)]
fn observe(path: &Path, run_id: i64) -> io::Result<app_core::RuntimeHealth> {
    observe_for_run(&app_core::ActiveInstanceRun {
        run_id,
        session_id: None,
        pid: None,
        log_path: Some(path.to_string_lossy().into_owned()),
        process_count: 0,
        processes: Vec::new(),
    })
}

fn region(file: &mut File, offset: u64, length: u64) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(length).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn digest(file: &mut File, offset: u64, length: u64) -> io::Result<[u8; 32]> {
    Ok(Sha256::digest(region(file, offset, length)?).into())
}

#[cfg(test)]
#[path = "runtime_valheim_health_tests.rs"]
mod tests;
