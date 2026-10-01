use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use app_core::ProcessIdentity;
use serde_json::Value;

use super::{HELPER_FLAG, MAX_RESPONSE_BYTES, NativePlayerConsoleError as Error, valid_request};

#[path = "native_player_console_win32.rs"]
mod console;

const POLL: Duration = Duration::from_millis(20);
const HELPER_TIMEOUT: Duration = Duration::from_secs(5);

struct OwnedHelper(Child);

impl Drop for OwnedHelper {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            // TerminateProcess is asynchronous. Bound cleanup as well as the
            // query; never follow cancellation with Child::wait's infinite wait.
            unsafe {
                crate::WaitForSingleObject(self.0.as_raw_handle(), 500);
            }
            let _ = self.0.try_wait();
        }
    }
}

pub(super) fn collect(pid: u32, identity: &ProcessIdentity, nonce: &str) -> Result<String, Error> {
    let executable = std::env::current_exe().map_err(|_| Error::Io)?;
    collect_with_executable(&executable, pid, identity, nonce)
}

fn collect_with_executable(
    executable: &std::path::Path,
    pid: u32,
    identity: &ProcessIdentity,
    nonce: &str,
) -> Result<String, Error> {
    if !crate::process_matches_identity(pid, identity).map_err(|_| Error::Io)? {
        return Err(Error::ProcessUnavailable);
    }
    let request = serde_json::json!({
        "pid": pid, "creation_time": identity.creation_time,
        "image_path": identity.image_path, "nonce": nonce,
    });
    let mut child = OwnedHelper(
        crate::windows_console_control::with_windows_console_ctrl_lock(|| {
            Command::new(executable)
                .arg(HELPER_FLAG)
                .arg(request.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(crate::CREATE_NO_WINDOW)
                .spawn()
        })
        .map_err(|_| Error::Io)?,
    );
    let stdout = child.0.stdout.take().ok_or(Error::Io)?;
    let deadline = Instant::now() + Duration::from_millis(5500);
    let bytes = read_helper_output(&mut child, &stdout, deadline)?;
    if !crate::process_matches_identity(pid, identity).map_err(|_| Error::Io)? {
        return Err(Error::ProcessUnavailable);
    }
    let response: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Incomplete)?;
    if response.get("nonce").and_then(Value::as_str) != Some(nonce) {
        return Err(Error::Incomplete);
    }
    if let Some(error) = response.get("error").and_then(Value::as_str) {
        return Err(decode_error(error));
    }
    response
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| text.len() <= MAX_RESPONSE_BYTES)
        .map(str::to_owned)
        .ok_or(Error::Incomplete)
}

fn read_helper_output(
    child: &mut OwnedHelper,
    stdout: &std::process::ChildStdout,
    deadline: Instant,
) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    loop {
        // Observe exit before inspecting the pipe: an empty Peek followed by an
        // exit check can otherwise lose a response written between those calls.
        let status = child.0.try_wait().map_err(|_| Error::Io)?;
        // Peek first: a silent or stuck console host never blocks the owner on
        // ReadFile, and an overlong response cannot make it allocate unboundedly.
        let mut available = 0_u32;
        let peeked = unsafe {
            console::PeekNamedPipe(
                stdout.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 {
            if std::io::Error::last_os_error().raw_os_error() != Some(109) {
                return Err(Error::Io);
            }
        } else if available > 0 {
            let length = (available as usize).min(4096);
            if bytes.len().saturating_add(length) > MAX_RESPONSE_BYTES + 4096 {
                return Err(Error::CaptureLimit);
            }
            let mut buffer = [0_u8; 4096];
            let mut count = 0;
            if unsafe {
                console::ReadFile(
                    stdout.as_raw_handle(),
                    buffer.as_mut_ptr().cast(),
                    length as u32,
                    &mut count,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(Error::Io);
            }
            bytes.extend_from_slice(&buffer[..count as usize]);
            continue;
        }
        if let Some(status) = status {
            if !status.success() || bytes.is_empty() {
                return Err(Error::Io);
            }
            return Ok(bytes);
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        std::thread::sleep(POLL);
    }
}

pub(super) fn run_helper(argument: Option<&OsStr>) -> i32 {
    // Preserve the private pipe before AttachConsole replaces this process's
    // standard handles. No result, player data or error is written to CONOUT$.
    let mut output = match duplicate_output() {
        Ok(output) => output,
        Err(_) => return 2,
    };
    let Some(argument) = argument
        .and_then(OsStr::to_str)
        .filter(|arg| arg.len() <= 16 * 1024)
    else {
        return 2;
    };
    let Ok(request) = serde_json::from_str::<Value>(argument) else {
        return 2;
    };
    let Some(pid) = request
        .get("pid")
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
    else {
        return 2;
    };
    let Some(creation_time) = request.get("creation_time").and_then(Value::as_u64) else {
        return 2;
    };
    let Some(image_path) = request.get("image_path").and_then(Value::as_str) else {
        return 2;
    };
    let Some(nonce) = request.get("nonce").and_then(Value::as_str) else {
        return 2;
    };
    let identity = ProcessIdentity {
        creation_time,
        image_path: image_path.to_owned(),
    };
    if !valid_request(pid, &identity, nonce) {
        return 2;
    }
    let response = match console::capture(pid, &identity, nonce, HELPER_TIMEOUT) {
        Ok(text) => serde_json::json!({"nonce": nonce, "text": text}),
        Err(error) => serde_json::json!({"nonce": nonce, "error": encode_error(&error)}),
    };
    if output.write_all(response.to_string().as_bytes()).is_err() {
        2
    } else {
        0
    }
}

fn duplicate_output() -> Result<File, Error> {
    let process = unsafe { crate::GetCurrentProcess() };
    let handle = unsafe { console::GetStdHandle(-11_i32 as u32) };
    if unsafe { console::GetFileType(handle) } != 3 {
        return Err(Error::Io);
    }
    let mut duplicate = std::ptr::null_mut();
    if unsafe {
        crate::DuplicateHandle(
            process,
            handle,
            process,
            &mut duplicate,
            0,
            0,
            crate::DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(Error::Io);
    }
    // SAFETY: DuplicateHandle returned a new owned handle, closed by File.
    Ok(unsafe { File::from_raw_handle(duplicate) })
}

fn encode_error(error: &Error) -> &'static str {
    match error {
        Error::ProcessUnavailable => "process",
        Error::Timeout => "timeout",
        Error::CaptureLimit => "limit",
        Error::Incomplete => "incomplete",
        Error::Unsupported => "unsupported",
        Error::Io => "io",
    }
}

fn decode_error(error: &str) -> Error {
    match error {
        "process" => Error::ProcessUnavailable,
        "timeout" => Error::Timeout,
        "limit" => Error::CaptureLimit,
        "incomplete" => Error::Incomplete,
        "unsupported" => Error::Unsupported,
        _ => Error::Io,
    }
}

#[cfg(test)]
#[path = "native_player_console_windows_tests.rs"]
mod tests;
