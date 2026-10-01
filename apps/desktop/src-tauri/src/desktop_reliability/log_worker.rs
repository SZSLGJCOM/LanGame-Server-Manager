use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::Emitter;

use super::server::{COMMAND, INSTANCE_ID};
use crate::runtime_log_stream::{
    RUNTIME_LOG_STREAM_EVENT, RuntimeLogStreamPayload, RuntimeLogTailState,
    read_runtime_log_delta_bounded,
};

#[derive(Default)]
pub(super) struct Output {
    pub lines: VecDeque<String>,
    pub markers: Vec<u32>,
    pub error: Option<String>,
    pub batches: u64,
}

pub(super) struct LogWorker {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LogWorker {
    pub fn start(
        app: tauri::AppHandle,
        path: PathBuf,
        output: Arc<Mutex<Output>>,
    ) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("desktop-fixture-log".into())
            .spawn(move || {
                let mut tail = RuntimeLogTailState::default();
                let deadline = Instant::now() + Duration::from_secs(180);
                let result = (|| -> Result<(), String> {
                    while !cancelled.load(Ordering::Acquire) {
                        if Instant::now() >= deadline {
                            return Err("Desktop reliability fixture exceeded 180 seconds".into());
                        }
                        let delta =
                            read_runtime_log_delta_bounded(&path, &mut tail, 256 * 1024, 64 * 1024)
                                .map_err(|error| error.to_string())?;
                        if !delta.lines.is_empty() || delta.stream_error.is_some() {
                            let stream_error = delta.stream_error.clone();
                            {
                                let mut observation = super::lock(&output);
                                for line in &delta.lines {
                                    if let Some(number) = line
                                        .strip_prefix(&format!("{COMMAND}_"))
                                        .and_then(|value| value.parse::<u32>().ok())
                                    {
                                        if observation.markers.len() >= 3 {
                                            return Err("Duplicate server output marker".into());
                                        }
                                        observation.markers.push(number);
                                    }
                                    if observation.lines.len() == 400 {
                                        observation.lines.pop_front();
                                    }
                                    observation.lines.push_back(line.clone());
                                }
                                observation.batches += 1;
                            }
                            app.emit(
                                RUNTIME_LOG_STREAM_EVENT,
                                RuntimeLogStreamPayload {
                                    instance_id: INSTANCE_ID.into(),
                                    process_key: Some("main".into()),
                                    display_name: Some("Fixture server".into()),
                                    run_id: Some(1),
                                    snapshot: None,
                                    snapshot_revision: None,
                                    stream_error: delta.stream_error,
                                    log_path: path.to_string_lossy().into_owned(),
                                    lines: delta.lines,
                                    byte_offset: delta.byte_offset,
                                    emitted_at_unix_ms: SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .map_err(|error| error.to_string())?
                                        .as_millis(),
                                },
                            )
                            .map_err(|error| error.to_string())?;
                            if let Some(error) = stream_error {
                                return Err(format!(
                                    "Desktop reliability log capture incomplete: {error}"
                                ));
                            }
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    super::lock(&output).error = Some(error);
                    app.exit(1);
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| "Fixture log worker panicked".to_owned())?;
        }
        Ok(())
    }
}

impl Drop for LogWorker {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("{error}");
        }
    }
}
