//! Read-only native Windows console bridge. Console attachment happens only in
//! a short-lived copy of the desktop executable, never in the desktop process.

use app_core::ProcessIdentity;
use thiserror::Error;

#[cfg(any(windows, test))]
#[path = "native_player_console_frame.rs"]
mod frame;
#[cfg(windows)]
#[path = "native_player_console_windows.rs"]
mod windows;

const HELPER_FLAG: &str = "--langame-native-player-console";
#[cfg(any(windows, test))]
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NativePlayerConsoleError {
    #[error("the native console process identity no longer matches")]
    ProcessUnavailable,
    #[error("the native player console did not finish within its deadline")]
    Timeout,
    #[error("the native player console could not be read")]
    Io,
    #[error("the native player console exceeded its capture limit")]
    CaptureLimit,
    #[error("the native player console response was incomplete or changed")]
    Incomplete,
    #[error("native player consoles are only available on Windows")]
    Unsupported,
}

/// Call before starting the application runtime, logging or Tauri. A recognized
/// helper invocation returns an exit code and must not continue app startup.
pub fn run_native_player_console_helper() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(HELPER_FLAG)) {
        return None;
    }
    #[cfg(windows)]
    {
        let argument = args.next();
        if args.next().is_some() {
            return Some(2);
        }
        Some(windows::run_helper(argument.as_deref()))
    }
    #[cfg(not(windows))]
    {
        Some(2)
    }
}

/// Synchronous, with a six-second lifetime including child cleanup. Call on a
/// bounded blocking worker. Cancellation of its caller does not remove the
/// helper deadline or transfer ownership of the helper process.
pub fn collect_native_player_console(
    pid: u32,
    expected: &ProcessIdentity,
    request_id: &str,
) -> Result<String, NativePlayerConsoleError> {
    if !valid_request(pid, expected, request_id) {
        return Err(NativePlayerConsoleError::ProcessUnavailable);
    }
    #[cfg(windows)]
    {
        windows::collect(pid, expected, request_id)
    }
    #[cfg(not(windows))]
    {
        Err(NativePlayerConsoleError::Unsupported)
    }
}

fn valid_request(pid: u32, identity: &ProcessIdentity, nonce: &str) -> bool {
    let name = identity.image_path.rsplit(['/', '\\']).next().unwrap_or("");
    pid != 0
        && identity.creation_time != 0
        && identity.image_path.len() <= 4096
        && !identity.image_path.chars().any(char::is_control)
        && ["MoriaServer.exe", "MoriaServer-Win64-Shipping.exe"]
            .iter()
            .any(|allowed| name.eq_ignore_ascii_case(allowed))
        && nonce.len() == 32
        && nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "native_player_console_tests.rs"]
mod tests;
