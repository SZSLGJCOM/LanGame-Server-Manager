use std::fs;
use std::path::Path;

use crate::runtime_log_stream::file_identity::{LogFileIdentity, log_identity};

use crate::runtime_log_stream::{
    RuntimeLogBoundedDelta, RuntimeLogTailState, read_runtime_log_generation_delta_bounded,
};

use super::dst_client_table::MAX_LINE_BYTES;

#[cfg(test)]
#[path = "log_capture_tests.rs"]
mod tests;

#[derive(Clone)]
pub(super) struct RuntimeLogBaseline {
    identity: LogFileIdentity,
    pub(super) byte_offset: u64,
}

pub(super) fn log_baseline(path: &Path) -> Result<RuntimeLogBaseline, String> {
    let (identity, byte_offset) = log_file_snapshot(path)?;
    Ok(RuntimeLogBaseline {
        identity,
        byte_offset,
    })
}

pub(super) fn read_checked_delta(
    path: &Path,
    baseline: &RuntimeLogBaseline,
    state: &mut RuntimeLogTailState,
    max_bytes: usize,
) -> Result<RuntimeLogBoundedDelta, String> {
    read_checked_delta_using(
        path,
        baseline,
        state,
        max_bytes,
        |path, state, max_bytes| {
            read_runtime_log_generation_delta_bounded(path, state, max_bytes, MAX_LINE_BYTES)
        },
    )
}

fn read_checked_delta_using(
    path: &Path,
    baseline: &RuntimeLogBaseline,
    state: &mut RuntimeLogTailState,
    max_bytes: usize,
    read: impl FnOnce(&Path, &mut RuntimeLogTailState, usize) -> std::io::Result<RuntimeLogBoundedDelta>,
) -> Result<RuntimeLogBoundedDelta, String> {
    let start_offset = state.byte_offset;
    verify_log_identity(path, baseline, start_offset)?;
    let delta = read(path, state, max_bytes)
        .map_err(|error| format!("Failed to read the player log: {error}"))?;
    // The general log viewer recovers from truncation by resetting its cursor.
    // A player capture must reject that recovery and any replacement between its
    // initial identity check and the reader's subsequent file open.
    if state.byte_offset != start_offset.saturating_add(delta.bytes_read as u64)
        || delta.byte_offset != state.byte_offset
    {
        return Err(String::from(
            "The player log position changed during collection.",
        ));
    }
    verify_log_identity(path, baseline, state.byte_offset)?;
    Ok(delta)
}

fn verify_log_identity(
    path: &Path,
    baseline: &RuntimeLogBaseline,
    minimum_length: u64,
) -> Result<(), String> {
    let (current_identity, byte_length) = log_file_snapshot(path)?;
    if current_identity != baseline.identity || byte_length < minimum_length {
        return Err(String::from(
            "The active server log changed while the player list was being collected.",
        ));
    }
    Ok(())
}

fn log_file_snapshot(path: &Path) -> Result<(LogFileIdentity, u64), String> {
    let file =
        fs::File::open(path).map_err(|error| format!("Player log is unavailable: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Player log metadata is unavailable: {error}"))?;
    if !metadata.is_file() {
        return Err(String::from("The selected player log is not a file."));
    }
    // Read length and identity from the same open file: path lookups between
    // those reads could observe different generations during log replacement.
    let identity = log_identity(path, &file)
        .map_err(|error| format!("Failed to identify the player log: {error}"))?;
    Ok((identity, metadata.len()))
}
