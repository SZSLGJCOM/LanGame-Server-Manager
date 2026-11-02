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
    read_retries: usize,
}

const LIMITS: StreamLimits = StreamLimits {
    read_bytes: 256 * 1024,
    pending_bytes: 64 * 1024,
    final_reads: 256,
    final_budget: Duration::from_secs(2),
    poll_interval: Duration::from_millis(33),
    read_retries: 3,
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
    state
        .runtime_log_streams
        .lock()
        .map_err(|_| String::from("runtime log stream registry lock poisoned"))?
        .begin_console_document(
            instance_id,
            log_path,
            lease,
            process_key,
            display_name,
            run_id,
        );
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
        let complete = run_stream(
            PathBuf::from(&log_path),
            LIMITS,
            || {
                app.state::<DesktopState>()
                    .runtime_log_streams
                    .lock()
                    .map(|streams| streams.status(&instance_id, &log_path, lease))
                    .unwrap_or(RuntimeLogStreamStatus::Cancelled)
            },
            |lines, byte_offset, stream_error, partial_lines, reset| {
                let lines: Vec<_> = lines
                    .iter()
                    .map(|line| app_core::abiotic_output::redact_modio_token_text(line))
                    .collect();
                let partial_lines = partial_lines
                    .iter()
                    .map(|line| app_core::abiotic_output::redact_modio_token_text(line))
                    .collect();
                let state = app.state::<DesktopState>();
                let Ok(mut streams) = state.runtime_log_streams.lock() else {
                    return;
                };
                // Keep generation validation and event publication under the same
                // short lock: cancellation/replacement cannot slip between them.
                if streams.is_active(&instance_id, &log_path, lease) {
                    let console = streams.publish_console_document(
                        &instance_id,
                        &log_path,
                        lease,
                        &lines,
                        partial_lines,
                        reset,
                    );
                    streams.set_console_offset(&instance_id, &log_path, lease, byte_offset);
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
                            console,
                            document_patch: None,
                        },
                    );
                }
            },
        )
        .await;
        if let Ok(mut streams) = app.state::<DesktopState>().runtime_log_streams.lock() {
            if !complete {
                streams.invalidate_console_document(&instance_id, &log_path, lease);
            }
            streams.release(&instance_id, &log_path, lease);
        }
    });
}

async fn run_stream(
    path: PathBuf,
    limits: StreamLimits,
    status: impl Fn() -> RuntimeLogStreamStatus,
    emit: impl Fn(Vec<String>, u64, Option<String>, Vec<String>, bool),
) -> bool {
    let mut tail = RuntimeLogTailState::default();
    let mut final_deadline = None;
    let mut final_reads = 0;
    let mut first_read = true;
    let mut previous_partial = Vec::new();
    let mut consecutive_failures = 0;
    loop {
        let before_read = status();
        if before_read == RuntimeLogStreamStatus::Cancelled {
            break false;
        }
        if before_read == RuntimeLogStreamStatus::ProducerFinished {
            let deadline =
                final_deadline.get_or_insert_with(|| Instant::now() + limits.final_budget);
            if Instant::now() >= *deadline || final_reads >= limits.final_reads {
                emit(
                    Vec::new(),
                    tail.byte_offset,
                    Some(persist_stream_diagnostic(&path, INCOMPLETE.to_owned()).await),
                    previous_partial.clone(),
                    false,
                );
                break false;
            }
            final_reads += 1;
        }
        let read_path = path.clone();
        let previous_offset = tail.byte_offset;
        let previous_generation = tail.source_generation();
        let read = tokio::task::spawn_blocking(move || {
            // A failed read must not commit a partially changed generation/cursor.
            let previous_tail = tail.clone();
            let mut partial = Vec::new();
            let delta = app_runtime::with_live_console_preview(&read_path, |preview| {
                let delta = read_runtime_log_delta_bounded(
                    &read_path,
                    &mut tail,
                    limits.read_bytes,
                    limits.pending_bytes,
                )?;
                partial.extend(tail.partial_preview());
                // The preview and file are sampled under the producer's lock.
                // Do not show its current row ahead of unread committed bytes.
                if !delta.limit_exhausted {
                    partial.extend_from_slice(preview);
                }
                Ok::<_, std::io::Error>(delta)
            })
            .and_then(|result| result);
            if delta.is_err() {
                tail = previous_tail;
            }
            (tail, delta, partial)
        })
        .await;
        // A stop/cancel may have arrived while filesystem I/O was in progress.
        if status() == RuntimeLogStreamStatus::Cancelled {
            break false;
        }
        let (next_tail, result, partial) = match read {
            Ok(result) => result,
            Err(error) => {
                emit(
                    Vec::new(),
                    previous_offset,
                    Some(
                        persist_stream_diagnostic(
                            &path,
                            format!("{INCOMPLETE} Reader task failed: {error}"),
                        )
                        .await,
                    ),
                    previous_partial.clone(),
                    false,
                );
                break false;
            }
        };
        tail = next_tail;
        let mut catch_up = false;
        match result {
            Ok(delta) => {
                consecutive_failures = 0;
                let reset = first_read || previous_generation != tail.source_generation();
                catch_up = delta.limit_exhausted && delta.bytes_read > 0;
                if reset
                    || !delta.lines.is_empty()
                    || delta.stream_error.is_some()
                    || partial != previous_partial
                {
                    emit(
                        delta.lines,
                        delta.byte_offset,
                        match delta.stream_error {
                            Some(error) => Some(persist_stream_diagnostic(&path, error).await),
                            None => None,
                        },
                        partial.clone(),
                        reset,
                    );
                    previous_partial = partial;
                    first_read = false;
                }
                // An idle live writer is not EOF. The producer must have been
                // closed before this read began, including its output drain.
                if before_read == RuntimeLogStreamStatus::ProducerFinished
                    && (delta.bytes_read == 0 || !delta.limit_exhausted)
                {
                    let final_delta = finish_runtime_log_tail(&mut tail);
                    let complete = final_delta.stream_error.is_none();
                    if !final_delta.lines.is_empty()
                        || final_delta.stream_error.is_some()
                        || !previous_partial.is_empty()
                    {
                        emit(
                            final_delta.lines,
                            final_delta.byte_offset,
                            match final_delta.stream_error {
                                Some(error) => Some(persist_stream_diagnostic(&path, error).await),
                                None => None,
                            },
                            Vec::new(),
                            false,
                        );
                    }
                    break complete;
                }
            }
            Err(error)
                if error.kind() == ErrorKind::NotFound
                    && before_read == RuntimeLogStreamStatus::Active
                    && first_read
                    && tail.byte_offset == 0 => {}
            Err(error) => {
                consecutive_failures += 1;
                let retry = consecutive_failures <= limits.read_retries;
                let message = if retry {
                    format!(
                        "[LanGame] Console log read failed; retrying ({consecutive_failures}/{}): {error}",
                        limits.read_retries
                    )
                } else {
                    format!("{INCOMPLETE} Log read failed: {error}")
                };
                emit(
                    Vec::new(),
                    tail.byte_offset,
                    Some(persist_stream_diagnostic(&path, message).await),
                    previous_partial.clone(),
                    false,
                );
                if retry {
                    tokio::time::sleep(Duration::from_millis(
                        50 * (1 << (consecutive_failures - 1)),
                    ))
                    .await;
                    continue;
                }
                break false;
            }
        }
        if status() == RuntimeLogStreamStatus::Active {
            if catch_up {
                tokio::task::yield_now().await;
            } else {
                tokio::time::sleep(limits.poll_interval).await;
            }
        }
    }
}

async fn persist_stream_diagnostic(path: &std::path::Path, message: String) -> String {
    let path = path.to_path_buf();
    let diagnostic = message.clone();
    match tokio::task::spawn_blocking(move || {
        app_storage::startup_sessions::record_reader_error(&path, &diagnostic)
    })
    .await
    {
        Ok(Ok(())) => message,
        Ok(Err(error)) => format!("{message}; could not persist the reader diagnostic: {error}"),
        Err(error) => format!("{message}; reader diagnostic task failed: {error}"),
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
                    console: None,
                    document_patch: None,
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
