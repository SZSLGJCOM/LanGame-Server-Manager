use std::fs::File;
use std::io::{self, Write};
use std::os::windows::io::FromRawHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::pseudo_console::{create_pipe, read_pipe, record_capture_failure, set_nonblocking};

const DRAIN_BUDGET: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// One reader per launch, with no queued output buffers. A slow sink applies
/// pipe backpressure. A failed sink is never retried as if bytes were durable.
#[derive(Debug)]
pub(super) struct ManagedProcessOutput {
    closing: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl ManagedProcessOutput {
    pub(super) fn new(writer: Box<dyn Write + Send>) -> io::Result<(Self, File)> {
        let (read, write) = create_pipe()?;
        set_nonblocking(read.as_raw())?;
        let read = unsafe { File::from_raw_handle(read.into_raw()) };
        let write = unsafe { File::from_raw_handle(write.into_raw()) };
        let closing = Arc::new(AtomicBool::new(false));
        let reader_closing = Arc::clone(&closing);
        let reader = std::thread::Builder::new()
            .name("managed-process-output".into())
            .spawn(move || drain_output(read, writer, reader_closing))?;
        Ok((
            Self {
                closing,
                reader: Some(reader),
            },
            write,
        ))
    }
}

impl Drop for ManagedProcessOutput {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn drain_output(read: File, writer: Box<dyn Write + Send>, closing: Arc<AtomicBool>) {
    drain_output_with_reader(
        |buffer| read_pipe(&read, buffer),
        writer,
        closing,
        DRAIN_BUDGET,
    );
}

fn drain_output_with_reader(
    mut read: impl FnMut(&mut [u8]) -> io::Result<usize>,
    mut writer: impl Write,
    closing: Arc<AtomicBool>,
    drain_budget: Duration,
) {
    let mut buffer = [0_u8; 16 * 1024];
    let mut close_deadline = None;
    let mut failed = false;
    loop {
        if closing.load(Ordering::Acquire) {
            let deadline = close_deadline.get_or_insert_with(|| Instant::now() + drain_budget);
            if Instant::now() >= *deadline {
                if !failed {
                    record_capture_failure(
                        &mut writer,
                        &io::Error::new(
                            io::ErrorKind::TimedOut,
                            "The output drain deadline expired before EOF.",
                        ),
                    );
                }
                break;
            }
        }
        match read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => {
                if !failed && let Err(error) = writer.write_all(&buffer[..size]) {
                    // The storage sink retains its failure for application
                    // diagnostics; keep draining so disk failure cannot freeze
                    // the server behind a permanently full stdout pipe.
                    eprintln!("Managed console output could not be persisted: {error}");
                    failed = true;
                }
            }
            Err(error) if error.raw_os_error() == Some(232) => std::thread::sleep(POLL_INTERVAL),
            Err(error) if error.raw_os_error() == Some(109) => break,
            Err(error) => {
                eprintln!("Managed console output pipe failed: {error}");
                if !failed {
                    record_capture_failure(&mut writer, &error);
                }
                break;
            }
        }
    }
    if !failed && let Err(error) = writer.flush() {
        eprintln!("Managed console output could not be flushed: {error}");
    }
}

#[cfg(test)]
#[path = "windows_process_output_tests.rs"]
mod tests;
