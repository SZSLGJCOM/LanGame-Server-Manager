use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager};

use super::current_unix_ms;
use crate::runtime_log_stream::{
    RUNTIME_LOG_STREAM_EVENT, RuntimeLogStreamPayload, RuntimeLogTailState,
    finish_runtime_log_tail, read_runtime_log_delta_bounded,
};
use crate::state::{DesktopState, RuntimeLogStreamLease, RuntimeLogStreamStatus};

#[derive(Clone, Copy)]
struct StreamLimits {
    read_bytes: usize,
    pending_bytes: usize,
    final_reads: usize,
    final_budget: Duration,
    poll_interval: Duration,
}

const LIMITS: StreamLimits = StreamLimits {
    read_bytes: 256 * 1024,
    pending_bytes: 64 * 1024,
    final_reads: 256,
    final_budget: Duration::from_secs(2),
    poll_interval: Duration::from_millis(250),
};
const INCOMPLETE: &str = "[LanGame] Console streaming ended before all final output could be read. Check the retained log files for additional output.";

#[path = "commands_runtime_game_log_stream.rs"]
mod game_log;
pub(crate) use game_log::start_runtime_game_log_stream;

pub(crate) fn start_runtime_log_stream(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
    process_key: Option<&str>,
    display_name: Option<&str>,
    run_id: Option<i64>,
    log_path: &str,
) -> Result<(), String> {
    let Some(app_handle) = app_handle else {
        return Ok(());
    };
    let lease = state
        .runtime_log_streams
        .lock()
        .map_err(|_| String::from("runtime log stream registry lock poisoned"))?
        .reserve(instance_id, log_path);
    let Some(lease) = lease else {
        return Ok(());
    };
    spawn_runtime_log_stream(
        app_handle,
        instance_id,
        process_key,
        display_name,
        run_id,
        log_path,
        lease,
    );
    Ok(())
}

fn spawn_runtime_log_stream(
    app_handle: &tauri::AppHandle,
    instance_id: &str,
    process_key: Option<&str>,
    display_name: Option<&str>,
    run_id: Option<i64>,
    log_path: &str,
    lease: RuntimeLogStreamLease,
) {
    let app = app_handle.clone();
    let instance_id = instance_id.to_owned();
    let process_key = process_key.map(String::from);
    let display_name = display_name.map(String::from);
    let log_path = log_path.to_owned();
    tauri::async_runtime::spawn(async move {
        run_stream(
            PathBuf::from(&log_path),
            LIMITS,
            || {
                app.state::<DesktopState>()
                    .runtime_log_streams
                    .lock()
                    .map(|streams| streams.status(&instance_id, &log_path, lease))
                    .unwrap_or(RuntimeLogStreamStatus::Cancelled)
            },
            |lines, byte_offset, stream_error| {
                let state = app.state::<DesktopState>();
                let Ok(streams) = state.runtime_log_streams.lock() else {
                    return;
                };
                // Keep generation validation and event publication under the same
                // short lock: cancellation/replacement cannot slip between them.
                if streams.is_active(&instance_id, &log_path, lease) {
                    let _ = app.emit(
                        RUNTIME_LOG_STREAM_EVENT,
                        RuntimeLogStreamPayload {
                            instance_id: instance_id.clone(),
                            process_key: process_key.clone(),
                            display_name: display_name.clone(),
                            run_id,
                            log_path: log_path.clone(),
                            lines,
                            byte_offset,
                            emitted_at_unix_ms: current_unix_ms(),
                            snapshot: None,
                            snapshot_revision: None,
                            stream_error,
                        },
                    );
                }
            },
        )
        .await;
        if let Ok(mut streams) = app.state::<DesktopState>().runtime_log_streams.lock() {
            streams.release(&instance_id, &log_path, lease);
        }
    });
}

async fn run_stream(
    path: PathBuf,
    limits: StreamLimits,
    status: impl Fn() -> RuntimeLogStreamStatus,
    emit: impl Fn(Vec<String>, u64, Option<String>),
) {
    let mut tail = RuntimeLogTailState::default();
    let mut final_deadline = None;
    let mut final_reads = 0;
    loop {
        let before_read = status();
        if before_read == RuntimeLogStreamStatus::Cancelled {
            break;
        }
        if before_read == RuntimeLogStreamStatus::ProducerFinished {
            let deadline =
                final_deadline.get_or_insert_with(|| Instant::now() + limits.final_budget);
            if Instant::now() >= *deadline || final_reads >= limits.final_reads {
                emit(Vec::new(), tail.byte_offset, Some(INCOMPLETE.to_owned()));
                break;
            }
            final_reads += 1;
        }
        let read_path = path.clone();
        let previous_offset = tail.byte_offset;
        let read = tokio::task::spawn_blocking(move || {
            let delta = read_runtime_log_delta_bounded(
                &read_path,
                &mut tail,
                limits.read_bytes,
                limits.pending_bytes,
            );
            (tail, delta)
        })
        .await;
        // A stop/cancel may have arrived while filesystem I/O was in progress.
        if status() == RuntimeLogStreamStatus::Cancelled {
            break;
        }
        let (next_tail, result) = match read {
            Ok(result) => result,
            Err(error) => {
                emit(
                    Vec::new(),
                    previous_offset,
                    Some(format!("{INCOMPLETE} Reader task failed: {error}")),
                );
                break;
            }
        };
        tail = next_tail;
        match result {
            Ok(delta) => {
                if !delta.lines.is_empty() || delta.stream_error.is_some() {
                    emit(delta.lines, delta.byte_offset, delta.stream_error);
                }
                // An idle live writer is not EOF. The producer must have been
                // closed before this read began, including its output drain.
                if before_read == RuntimeLogStreamStatus::ProducerFinished
                    && (delta.bytes_read == 0 || !delta.limit_exhausted)
                {
                    let final_delta = finish_runtime_log_tail(&mut tail);
                    if !final_delta.lines.is_empty() || final_delta.stream_error.is_some() {
                        emit(
                            final_delta.lines,
                            final_delta.byte_offset,
                            final_delta.stream_error,
                        );
                    }
                    break;
                }
            }
            Err(error)
                if error.kind() == ErrorKind::NotFound
                    && before_read == RuntimeLogStreamStatus::Active => {}
            Err(error) => {
                emit(
                    Vec::new(),
                    tail.byte_offset,
                    Some(format!("{INCOMPLETE} Log read failed: {error}")),
                );
                break;
            }
        }
        if status() == RuntimeLogStreamStatus::Active {
            tokio::time::sleep(limits.poll_interval).await;
        }
    }
}

/// Cancellation is distinct from producer completion (for example a failed
/// start whose cleanup could not prove the game had stopped).
pub(crate) fn stop_runtime_log_streams_for_instance(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
) {
    if let Ok(mut streams) = state.runtime_log_streams.lock() {
        streams.release_instance(instance_id);
    }
}

pub(crate) fn stop_runtime_log_stream_for_process(
    state: &DesktopState,
    instance_id: &str,
    log_path: &str,
) {
    if let Ok(mut streams) = state.runtime_log_streams.lock() {
        streams.release_path(instance_id, log_path);
    }
}

pub(crate) fn finish_runtime_log_stream_for_process(
    state: &DesktopState,
    instance_id: &str,
    log_path: &str,
) {
    if let Ok(mut streams) = state.runtime_log_streams.lock() {
        streams.finish_path(instance_id, log_path);
    }
}

pub(crate) async fn wait_for_shutdown(app: &tauri::AppHandle) {
    if wait_until_empty(
        || {
            app.state::<DesktopState>()
                .runtime_log_streams
                .lock()
                .is_ok_and(|streams| streams.is_empty())
        },
        Duration::from_secs(3),
    )
    .await
    {
        return;
    }
    let state = app.state::<DesktopState>();
    if let Ok(mut streams) = state.runtime_log_streams.lock() {
        // Shutdown has finished stopping the games. A stalled reader must not
        // keep the app alive indefinitely or silently claim a complete stream.
        for (instance_id, log_path) in streams.paths() {
            let _ = app.emit(
                RUNTIME_LOG_STREAM_EVENT,
                RuntimeLogStreamPayload {
                    instance_id: instance_id.clone(),
                    process_key: None,
                    display_name: None,
                    run_id: None,
                    log_path: log_path.clone(),
                    lines: Vec::new(),
                    byte_offset: 0,
                    emitted_at_unix_ms: current_unix_ms(),
                    snapshot: None,
                    snapshot_revision: None,
                    stream_error: Some(INCOMPLETE.to_owned()),
                },
            );
            streams.release_path(&instance_id, &log_path);
        }
    }
    eprintln!("{INCOMPLETE} Application shutdown reached its stream-drain deadline.");
}

async fn wait_until_empty(empty: impl Fn() -> bool, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if empty() {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        tokio::time::sleep(remaining.min(Duration::from_millis(10))).await;
    }
}

#[cfg(test)]
#[path = "commands_runtime_log_stream_tests.rs"]
mod tests;
