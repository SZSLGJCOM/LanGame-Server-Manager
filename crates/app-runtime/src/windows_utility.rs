use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};

use crate::pseudo_console::{create_pipe, read_pipe, set_nonblocking};
use crate::{RuntimeChild, SpawnCommand, spawn_standard_process};

const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Resolve Windows-owned executables without consulting PATH, COMSPEC, or a
/// caller-controlled working directory.
pub fn windows_system_directory() -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0_u16; 260];
    let mut length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length as usize >= buffer.len() && length <= 32768 {
        buffer.resize(length as usize, 0);
        length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    }
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize >= buffer.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows system directory is too long",
        ));
    }
    Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..length as usize],
    )))
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
}

/// Capture a short-lived Windows utility inside the same private Job boundary
/// as managed servers. Each output stream is limited to 4 MiB. The deadline
/// covers process execution and pipe draining; cleanup has a separate two-second
/// budget. Call this blocking function only from an owned worker thread.
pub fn capture_windows_utility(
    executable: &Path,
    arguments: &[&str],
    timeout: Duration,
) -> io::Result<Output> {
    let executable_text = executable.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "utility path is not Unicode")
    })?;
    let working_directory = executable
        .parent()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "utility path must be absolute")
        })?;
    let arguments = arguments
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    capture(
        &SpawnCommand {
            executable: executable_text,
            args: &arguments,
            working_directory,
            environment: &BTreeMap::new(),
        },
        timeout,
        OUTPUT_LIMIT,
    )
}

fn capture(plan: &SpawnCommand<'_>, timeout: Duration, limit: usize) -> io::Result<Output> {
    let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "utility timeout is too large")
    })?;
    ensure_before_deadline(deadline)?;
    let (mut stdout, stdout_child) = OutputPipe::new()?;
    let (mut stderr, stderr_child) = OutputPipe::new()?;
    // Use CREATE_NO_WINDOW without DETACHED_PROCESS, which would cause Windows
    // to ignore CREATE_NO_WINDOW. No utility here needs an interactive console.
    let (mut child, _desktop) =
        spawn_standard_process(plan, stdout_child, stderr_child, true, true, None)?;
    drop(child.take_stdin());
    let result = collect(&mut child, &mut stdout, &mut stderr, deadline, limit);
    // Cleanup is required on overflow, timeout, reader errors, and normal exit:
    // a utility must not leave detached descendants behind after capture ends.
    let cleanup = child.finish_process_tree();
    match (result, cleanup) {
        (Ok(status), Ok(())) => Ok(Output {
            status,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
        }),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(io::Error::new(
            error.kind(),
            format!("{error}; utility process cleanup failed: {cleanup}"),
        )),
    }
}

fn collect(
    child: &mut RuntimeChild,
    stdout: &mut OutputPipe,
    stderr: &mut OutputPipe,
    deadline: Instant,
    limit: usize,
) -> io::Result<std::process::ExitStatus> {
    let mut status = None;
    loop {
        ensure_before_deadline(deadline)?;
        // One bounded read per stream keeps a continuous stdout flood from
        // starving stderr, process-exit inspection, or the total deadline.
        let stdout_progress = stdout.read(limit)?;
        let stderr_progress = stderr.read(limit)?;
        if status.is_none() {
            status = child.try_wait()?;
        }
        if stdout.closed
            && stderr.closed
            && let Some(status) = status
        {
            return Ok(status);
        }
        if !stdout_progress && !stderr_progress {
            std::thread::sleep(
                POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
}

struct OutputPipe {
    pipe: File,
    bytes: Vec<u8>,
    closed: bool,
}

impl OutputPipe {
    fn new() -> io::Result<(Self, File)> {
        let (read, write) = create_pipe()?;
        set_nonblocking(read.as_raw())?;
        Ok((
            Self {
                pipe: unsafe { File::from_raw_handle(read.into_raw()) },
                bytes: Vec::new(),
                closed: false,
            },
            unsafe { File::from_raw_handle(write.into_raw()) },
        ))
    }

    fn read(&mut self, limit: usize) -> io::Result<bool> {
        if self.closed {
            return Ok(false);
        }
        let mut buffer = [0; 16 * 1024];
        match read_pipe(&self.pipe, &mut buffer) {
            Ok(0) => self.closed = true,
            Ok(size) => {
                if size > limit.saturating_sub(self.bytes.len()) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "utility output exceeded its capture limit",
                    ));
                }
                self.bytes.extend_from_slice(&buffer[..size]);
                return Ok(true);
            }
            Err(error) if error.raw_os_error() == Some(109) => self.closed = true,
            Err(error) if error.raw_os_error() == Some(232) => {}
            Err(error) => return Err(error),
        }
        Ok(false)
    }
}

fn ensure_before_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Windows utility exceeded its execution deadline",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "windows_utility_tests.rs"]
mod tests;
