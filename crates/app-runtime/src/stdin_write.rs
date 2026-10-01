use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cancels a managed stdin write when its process owner stops or loses the run.
/// Cancellation does not acknowledge execution of bytes already accepted by the pipe.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStdinCancellation {
    cancelled: Arc<AtomicBool>,
}

impl RuntimeStdinCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[cfg(windows)]
pub(crate) use windows::WindowsPipeWriter;

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::sync::{Mutex, MutexGuard, TryLockError};
    use std::time::{Duration, Instant};

    const WRITE_BUDGET: Duration = Duration::from_secs(2);
    const POLL_INTERVAL: Duration = Duration::from_millis(10);
    const WRITE_CHUNK_BYTES: usize = 4096;

    #[derive(Debug)]
    pub(crate) struct WindowsPipeWriter<W> {
        // Startup probes share ownership of one handle. Closing a partial write
        // invalidates every clone and delivers EOF even while an idle clone lives.
        pipe: Arc<Mutex<Option<W>>>,
    }

    impl<W: AsRawHandle> WindowsPipeWriter<W> {
        pub(crate) fn new(pipe: W) -> Self {
            Self {
                pipe: Arc::new(Mutex::new(Some(pipe))),
            }
        }

        pub(crate) fn into_healthy_pipe(self) -> Option<W> {
            Arc::try_unwrap(self.pipe).ok()?.into_inner().ok()?
        }

        pub(crate) fn is_healthy(&self) -> bool {
            self.try_pipe().is_ok_and(|pipe| pipe.is_some())
        }

        pub(crate) fn write_line(
            &mut self,
            command: &str,
            cancellation: &RuntimeStdinCancellation,
        ) -> io::Result<()> {
            let mut owner = self.try_pipe()?;
            let pipe = owner.as_ref().ok_or_else(unavailable)?;
            // Anonymous pipes also support PIPE_NOWAIT. This is bounded synchronous
            // polling on the existing worker, not overlapped I/O or a detached writer.
            let result = write_pipe_line(pipe.as_raw_handle(), command, cancellation);
            if let Err(failure) = result {
                if failure.invalidates_stream {
                    owner.take();
                }
                return Err(failure.error);
            }
            Ok(())
        }

        fn try_pipe(&self) -> io::Result<MutexGuard<'_, Option<W>>> {
            match self.pipe.try_lock() {
                Ok(pipe) => Ok(pipe),
                Err(TryLockError::WouldBlock) => Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Another command already owns this stdin pipe.",
                )),
                Err(TryLockError::Poisoned(error)) => {
                    error.into_inner().take();
                    Err(unavailable())
                }
            }
        }
    }

    impl WindowsPipeWriter<File> {
        pub(crate) fn try_clone(&self) -> io::Result<Self> {
            if self.try_pipe()?.is_none() {
                return Err(unavailable());
            }
            Ok(Self {
                pipe: Arc::clone(&self.pipe),
            })
        }
    }

    struct WriteFailure {
        error: io::Error,
        invalidates_stream: bool,
    }

    fn write_pipe_line(
        pipe: *mut c_void,
        command: &str,
        cancellation: &RuntimeStdinCancellation,
    ) -> Result<(), WriteFailure> {
        let deadline = Instant::now() + WRITE_BUDGET;
        let mut accepted = 0;
        if cancellation.is_cancelled() {
            return Err(WriteFailure {
                error: io::Error::new(io::ErrorKind::Interrupted, "The stdin write was cancelled."),
                invalidates_stream: false,
            });
        }
        let mode = 1_u32; // PIPE_NOWAIT | PIPE_READMODE_BYTE
        if unsafe { SetNamedPipeHandleState(pipe, &mode, std::ptr::null(), std::ptr::null()) } == 0
        {
            return Err(WriteFailure {
                error: io::Error::last_os_error(),
                invalidates_stream: true,
            });
        }

        // The newline shares the same deadline and cancellation state as the body.
        for bytes in [command.as_bytes(), b"\n".as_slice()] {
            let mut offset = 0;
            while offset < bytes.len() {
                let interrupted = if cancellation.is_cancelled() {
                    Some((io::ErrorKind::Interrupted, "The stdin write was cancelled."))
                } else if Instant::now() >= deadline {
                    Some((
                        io::ErrorKind::TimedOut,
                        "The stdin pipe did not accept the command before its deadline.",
                    ))
                } else {
                    None
                };
                if let Some((kind, message)) = interrupted {
                    return Err(WriteFailure {
                        error: io::Error::new(kind, message),
                        invalidates_stream: accepted > 0,
                    });
                }
                let length = (bytes.len() - offset).min(WRITE_CHUNK_BYTES);
                let mut written = 0;
                if unsafe {
                    WriteFile(
                        pipe,
                        bytes[offset..].as_ptr().cast(),
                        length as u32,
                        &mut written,
                        std::ptr::null_mut(),
                    )
                } == 0
                {
                    return Err(WriteFailure {
                        error: io::Error::last_os_error(),
                        invalidates_stream: true,
                    });
                }
                offset += written as usize;
                accepted += written as usize;
                if written == 0 {
                    std::thread::sleep(
                        POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
            }
        }
        Ok(())
    }

    fn unavailable() -> io::Error {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "The stdin pipe is closed after an incomplete or failed command write.",
        )
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetNamedPipeHandleState(
            pipe: *mut c_void,
            mode: *const u32,
            max_collection_count: *const u32,
            collect_data_timeout: *const u32,
        ) -> i32;
        fn WriteFile(
            pipe: *mut c_void,
            buffer: *const c_void,
            size: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }
}
