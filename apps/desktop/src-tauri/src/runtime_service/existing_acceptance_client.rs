//! Opt-in acceptance only: pin an explicitly selected existing backend. The
//! production client continues requiring its own executable as the pipe owner.
use super::wire;
use serde_json::Value;
use std::collections::BTreeSet;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub(crate) const STOP_ERROR_PREFIX: &str = "backend rejected stop_instance_process; categories=";
pub(crate) const STOP_ERROR_CATEGORIES: &[(&str, &str)] = &[
    ("native_shutdown_crash", "native_shutdown_crash"),
    ("rcon completion read failed", "rcon_completion_read_failed"),
    ("os error 10054", "connection_reset"),
    ("runtime stdin write failed", "stdin_write_failed"),
    (
        "complete managed process tree",
        "owned_tree_exit_not_confirmed",
    ),
    ("normal stop is unavailable", "normal_stop_unavailable"),
    ("no declared shutdown", "shutdown_strategy_missing"),
    ("timed out", "timeout"),
    ("process identity", "process_identity_verification_failed"),
    ("access is denied", "access_denied"),
    ("os error 5)", "access_denied"),
];

pub(crate) fn redact_backend_stop_error(reason: &str) -> String {
    let lower = reason.to_ascii_lowercase();
    let categories: BTreeSet<_> = STOP_ERROR_CATEGORIES
        .iter()
        .filter_map(|(pattern, category)| lower.contains(pattern).then_some(*category))
        .collect();
    format!(
        "{STOP_ERROR_PREFIX}{}",
        if categories.is_empty() {
            "unclassified_backend_error".into()
        } else {
            categories.into_iter().collect::<Vec<_>>().join(",")
        }
    )
}

pub(crate) struct ExistingClient {
    pipe_name: String,
    executable: PathBuf,
    pid: u32,
    creation_time: u64,
}

impl ExistingClient {
    pub(crate) fn new(executable: &Path, pid: u32, creation_time: u64) -> Result<Self, String> {
        if pid == 0 || creation_time == 0 || !executable.is_absolute() {
            return Err(
                "existing backend requires an absolute executable and nonzero identity".into(),
            );
        }
        let executable = dunce::canonicalize(executable)
            .map_err(|_| "selected backend executable is unavailable")?;
        let sid = current_sid().map_err(|_| "current Windows SID is unavailable")?;
        Ok(Self {
            pipe_name: format!(r"\\.\pipe\LanGame.Runtime.{sid}"),
            executable,
            pid,
            creation_time,
        })
    }

    pub(crate) async fn request(&self, command: &str, args: Value) -> Result<Value, String> {
        if !allowed(command) {
            return Err("existing-instance acceptance command is not allowed".into());
        }
        let name = if command == "runtime_service_status" {
            format!("{}.Control", self.pipe_name)
        } else {
            self.pipe_name.clone()
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let mut pipe = loop {
            match ClientOptions::new()
                .security_qos_flags(0x0001_0000)
                .open(&name)
            {
                Ok(pipe) => break pipe,
                Err(error)
                    if error.raw_os_error() == Some(231)
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(_) => return Err("existing backend connection failed; no request sent".into()),
            }
        };
        self.verify(&pipe)?;
        let request = wire::Request {
            protocol: wire::PROTOCOL,
            command: command.into(),
            args,
        };
        tokio::time::timeout(
            Duration::from_secs(15),
            wire::write_frame(&mut pipe, &request),
        )
        .await
        .map_err(|_| "request transmission uncertain; inspect before retrying")?
        .map_err(|_| "request transmission failed; operation state is uncertain")?;
        let response_seconds =
            if matches!(command, "start_instance_process" | "stop_instance_process") {
                1830
            } else {
                15
            };
        let response: wire::Response = tokio::time::timeout(
            Duration::from_secs(response_seconds),
            wire::read_frame(&mut pipe),
        )
        .await
        .map_err(|_| "backend response timed out; operation may still be running")?
        .map_err(|_| "backend response unavailable; operation state is uncertain")?;
        // The server closes each ordinary connection after its response. Its
        // original process generation must still match, but querying the now
        // disconnected pipe owner would introduce a response/close race.
        self.verify_process()?;
        // Backend errors can contain raw launch arguments. Only fixed stop
        // diagnostic categories cross this test-only boundary, never raw text.
        response.result.map_err(|reason| {
            if command == "stop_instance_process" {
                redact_backend_stop_error(&reason)
            } else {
                format!("backend rejected {command}; inspect retained logs")
            }
        })
    }

    fn verify(&self, pipe: &NamedPipeClient) -> Result<(), String> {
        let mut pid = 0;
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle().cast(), &mut pid) } == 0
            || pid != self.pid
        {
            return Err("existing backend pipe owner changed; request refused".into());
        }
        self.verify_process()
    }

    fn verify_process(&self) -> Result<(), String> {
        let process = app_runtime::inspect_process_identity(self.pid)
            .map_err(|_| "existing backend identity unavailable")?
            .ok_or("existing backend exited")?;
        let image = dunce::canonicalize(&process.image_path)
            .map_err(|_| "existing backend image unavailable")?;
        if process.creation_time != self.creation_time
            || !image
                .to_string_lossy()
                .eq_ignore_ascii_case(&self.executable.to_string_lossy())
        {
            return Err(
                "existing backend generation or executable changed; request refused".into(),
            );
        }
        Ok(())
    }
}

fn current_sid() -> std::io::Result<String> {
    // The production endpoint derives this from TokenUser, never an environment
    // variable or a supplied pipe name. Keep the acceptance connection local.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let result = (|| {
            let mut needed = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
            if needed == 0 || needed > 65536 {
                return Err(std::io::Error::last_os_error());
            }
            let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
            if GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut text = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
            Ok(result)
        })();
        CloseHandle(token);
        result
    }
}

fn allowed(command: &str) -> bool {
    matches!(
        command,
        "runtime_service_status"
            | "runtime_service_events"
            | "list_instances_from_storage"
            | "read_instance_details_from_storage"
            | "preview_instance_launch"
            | "preview_dontstarve_world_start"
            | "start_instance_process"
            | "stop_instance_process"
            | "read_instance_runtime_overview_from_storage"
            | "read_instance_runtime_window_snapshot"
            | "read_instance_log_document_from_storage"
    )
}

#[test]
fn existing_acceptance_client_only_allows_lifecycle_and_readback() {
    for forbidden in [
        "create_instance_record",
        "update_instance_record",
        "delete_instance_record",
        "restore_instance_backup",
        "runtime_service_shutdown",
        "send_instance_runtime_command",
    ] {
        assert!(!allowed(forbidden));
    }
    assert!(allowed("start_instance_process"));
    assert!(allowed("stop_instance_process"));
    assert!(allowed("preview_dontstarve_world_start"));
}
