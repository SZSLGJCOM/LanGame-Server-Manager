//! Explicitly enabled native acceptance support. This module is absent from
//! default builds; it deliberately cannot submit arbitrary console commands.
use super::{NativePlayerConsoleError as Error, valid_request};
use app_core::ProcessIdentity;

#[cfg(windows)]
const REQUEST_ENV: &str = "LANGAME_MORIA_DIFFICULTY_HELPER_REQUEST";
#[cfg(any(windows, test))]
const BEGIN: &str = "\nLGM_MORIA_ACCEPTANCE_REPLY_BEGIN\n";
#[cfg(any(windows, test))]
const END: &str = "\nLGM_MORIA_ACCEPTANCE_REPLY_END\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    QueryDifficulty,
    SetHard,
}

/// Invoke a precise libtest child in the calling executable. The caller owns
/// the disposable fixture; the usual native-console identity, exclusive lock,
/// fresh frame, size and helper-lifetime checks still apply. Never replay an
/// unsuccessful SetHard: a timeout cannot establish that it was not executed.
pub fn execute(
    pid: u32,
    expected: &ProcessIdentity,
    nonce: &str,
    operation: Command,
    helper_test: &str,
) -> Result<String, Error> {
    if !valid_request(pid, expected, nonce) || !valid_test_name(helper_test) {
        return Err(Error::ProcessUnavailable);
    }
    #[cfg(windows)]
    {
        let mut invocation =
            std::process::Command::new(std::env::current_exe().map_err(|_| Error::Io)?);
        invocation.args([
            "--exact",
            helper_test,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ]);
        invocation.env(
            REQUEST_ENV,
            serde_json::json!({
                "pid": pid,
                "creation_time": expected.creation_time,
                "image_path": expected.image_path,
                "nonce": nonce,
                "operation": match operation {
                    Command::QueryDifficulty => "difficulty",
                    Command::SetHard => "set_hard",
                },
            })
            .to_string(),
        );
        super::windows::collect_from_invocation(invocation, pid, expected, nonce, true)
    }
    #[cfg(not(windows))]
    {
        let _ = operation;
        Err(Error::Unsupported)
    }
}

fn valid_test_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 512
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_:".contains(&byte))
}

/// Called only by the opted-in, ignored acceptance test child. It must exit
/// with this code immediately, before the test runner can continue execution.
pub fn run_helper() -> Option<i32> {
    #[cfg(windows)]
    {
        let argument = std::env::var_os(REQUEST_ENV)?;
        let operation = argument
            .to_str()
            .filter(|argument| argument.len() <= 16 * 1024)
            .and_then(|argument| serde_json::from_str::<serde_json::Value>(argument).ok())
            .and_then(|request| {
                match request.get("operation").and_then(serde_json::Value::as_str) {
                    Some("difficulty") => Some(super::ConsoleCommand::Difficulty),
                    Some("set_hard") => Some(super::ConsoleCommand::SetHard),
                    _ => None,
                }
            });
        Some(match operation {
            Some(operation) => super::windows::run_helper_command(Some(&argument), operation, true),
            None => 2,
        })
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(any(windows, test))]
pub(super) fn frame_response(response: &str) -> String {
    format!("{BEGIN}{response}{END}")
}

#[cfg(any(windows, test))]
pub(super) fn response_body(bytes: &[u8]) -> Result<&[u8], Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Incomplete)?;
    let (prefix, remaining) = text.split_once(BEGIN).ok_or(Error::Incomplete)?;
    let (body, suffix) = remaining.split_once(END).ok_or(Error::Incomplete)?;
    if prefix.contains(END)
        || body.contains(BEGIN)
        || suffix.contains(BEGIN)
        || suffix.contains(END)
        || body.len() > super::MAX_RESPONSE_BYTES
    {
        return Err(Error::Incomplete);
    }
    Ok(body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_helper_reply_excludes_runner_output_and_rejects_duplicate_or_missing_frames() {
        let body = r#"{"nonce":"1234567890abcdef1234567890abcdef","text":"Preset: Hard"}"#;
        let frame = frame_response(body);
        let complete = format!("running 1 test\ntest native_helper ... {frame}ok\n");
        assert_eq!(response_body(complete.as_bytes()).unwrap(), body.as_bytes());
        for invalid in [
            body.to_owned(),
            format!("{frame}{frame}"),
            format!("{BEGIN}{body}"),
        ] {
            assert_eq!(response_body(invalid.as_bytes()), Err(Error::Incomplete));
        }
    }

    #[test]
    fn test_console_rejects_invalid_identity_nonce_and_child_name_before_spawn() {
        let identity = ProcessIdentity {
            creation_time: 1,
            image_path: "D:/owned/MoriaServer.exe".into(),
        };
        for (pid, nonce, child) in [
            (0, "1234567890abcdef1234567890abcdef", "native_helper"),
            (1, "nonce\rsetdifficulty hard", "native_helper"),
            (1, "1234567890abcdef1234567890abcdef", "--list"),
            (1, "1234567890abcdef1234567890abcdef", "native helper"),
        ] {
            assert_eq!(
                execute(pid, &identity, nonce, Command::SetHard, child),
                Err(Error::ProcessUnavailable)
            );
        }
    }
}
