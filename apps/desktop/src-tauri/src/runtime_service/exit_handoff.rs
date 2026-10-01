//! A short-lived native owner lets the interface exit before service IPC or saves.
use std::io::{Read, Write};
use std::os::windows::{io::AsRawHandle, process::CommandExt};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{client::Client, exit_deadline, security};

const HELPER: &str = "--runtime-service-exit-watchdog";
const HANDOFF_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REQUEST_BYTES: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    pid: u32,
    process: app_core::ProcessIdentity,
    sid: String,
    nonce: String,
    deadline_tick_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    pid: u32,
    nonce: String,
    deadline_owned: bool,
}

pub(super) fn handoff(
    identity: &security::ServiceIdentity,
    deadline_tick_ms: u64,
) -> Result<(), String> {
    let request = Request {
        pid: identity.pid,
        process: identity.process.clone(),
        sid: security::user_sid().map_err(|e| e.to_string())?,
        nonce: uuid::Uuid::new_v4().simple().to_string(),
        deadline_tick_ms,
    };
    let argument = serde_json::to_string(&request).map_err(|e| e.to_string())?;
    if argument.len() > MAX_REQUEST_BYTES {
        return Err("Exit ownership request exceeds its size limit".into());
    }
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg(HELPER)
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000)
        .spawn()
        .map_err(|e| format!("Cannot start independent exit owner: {e}"))?;
    let result = read_ready(&mut child, &request);
    if result.is_err() {
        // A failed handoff keeps the interface's original watchdog responsible.
        // Terminate only this newly spawned helper and bound its cleanup wait.
        let _ = child.kill();
        unsafe {
            windows_sys::Win32::System::Threading::WaitForSingleObject(
                child.as_raw_handle().cast(),
                500,
            );
        }
    }
    // Success deliberately releases only the parent's handle. The helper owns
    // the original service handle, and exits itself when that process settles.
    result
}

fn read_ready(child: &mut Child, request: &Request) -> Result<(), String> {
    let mut stdout = child
        .stdout
        .take()
        .ok_or("Exit owner has no receipt pipe")?;
    let deadline = Instant::now() + HANDOFF_TIMEOUT;
    let mut bytes = Vec::new();
    loop {
        let exited = child.try_wait().map_err(|e| e.to_string())?;
        let mut available = 0;
        let peeked = unsafe {
            windows_sys::Win32::System::Pipes::PeekNamedPipe(
                stdout.as_raw_handle().cast(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked != 0 && available > 0 {
            let mut chunk = [0; 512];
            let length = (available as usize).min(chunk.len());
            let count = stdout
                .read(&mut chunk[..length])
                .map_err(|e| e.to_string())?;
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.len() > 1024 {
                return Err("Exit owner receipt exceeds its size limit".into());
            }
            if bytes.last() == Some(&b'\n') {
                let receipt: Ready = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                return validate_ready(&receipt, request);
            }
        } else if peeked == 0 && std::io::Error::last_os_error().raw_os_error() != Some(109) {
            return Err(format!(
                "Cannot read exit ownership receipt: {}",
                std::io::Error::last_os_error()
            ));
        }
        if let Some(status) = exited {
            return Err(format!(
                "Independent exit owner ended before acknowledging ownership: {status}"
            ));
        }
        if Instant::now() >= deadline {
            return Err("Independent exit owner did not acknowledge within two seconds".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn validate_ready(ready: &Ready, request: &Request) -> Result<(), String> {
    if ready.pid != request.pid || ready.nonce != request.nonce || !ready.deadline_owned {
        return Err("Independent exit owner returned an invalid ownership receipt".into());
    }
    Ok(())
}

pub(super) fn run_if_requested() -> bool {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_none_or(|arg| arg != HELPER) {
        return false;
    }
    let result = if args.len() == 2 && args[1].len() <= MAX_REQUEST_BYTES {
        serde_json::from_str::<Request>(&args[1])
            .map_err(|e| e.to_string())
            .and_then(run)
    } else {
        Err("Invalid independent exit owner arguments".into())
    };
    if let Err(error) = result {
        eprintln!("Independent exit ownership failed: {error}");
        std::process::exit(1);
    }
    true
}

fn run(request: Request) -> Result<(), String> {
    if request.pid == 0
        || request.pid == std::process::id()
        || request.sid != security::user_sid().map_err(|e| e.to_string())?
        || request.nonce.len() != 32
        || !request.nonce.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid exit ownership identity".into());
    }
    let executable = dunce::canonicalize(std::env::current_exe().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let target_image =
        dunce::canonicalize(&request.process.image_path).map_err(|e| e.to_string())?;
    if !executable
        .to_string_lossy()
        .eq_ignore_ascii_case(&target_image.to_string_lossy())
    {
        return Err("Exit ownership is restricted to the original LanGame executable".into());
    }
    let deadline = exit_deadline::local_deadline(request.deadline_tick_ms)?;
    let mut endpoint = security::Endpoint::current().map_err(|e| e.to_string())?;
    #[cfg(feature = "desktop-reliability")]
    if let Some(isolated) = super::fixture::exit_helper_endpoint()? {
        endpoint = isolated;
    }
    #[cfg(not(feature = "desktop-reliability"))]
    let _ = &mut endpoint;
    let target = app_runtime::ProcessExitTarget::capture(request.pid, &request.process)
        .map_err(|e| e.to_string())?;
    if let Some(target) = target {
        let target = Arc::new(target);
        let watchdog = app_runtime::spawn_exit_watchdog(Arc::clone(&target), deadline, |outcome| {
            exit_deadline::terminate_current_process(i32::from(
                outcome.forced || outcome.error.is_some(),
            ));
        })
        .map_err(|e| format!("Cannot arm independent exit deadline: {e}"))?;
        let client = Client::for_exit_handoff(
            endpoint,
            security::ServiceIdentity {
                pid: request.pid,
                process: request.process.clone(),
            },
            Arc::clone(&target),
        );
        // This task belongs to the helper, never to the interface or its pipe.
        tauri::async_runtime::spawn(async move {
            if let Err(error) = client.stop_for_tray_exit(request.deadline_tick_ms).await {
                super::log_service_event("error", "app.exit.background_dispatch_failed", &error);
            }
        });
        #[cfg(feature = "desktop-reliability")]
        super::fixture::record_exit_helper(request.pid, request.deadline_tick_ms)?;
        write_ready(request.pid, &request.nonce)?;
        if watchdog.join().is_err() {
            target
                .terminate()
                .map_err(|e| format!("Exit watchdog failed and native cleanup failed: {e}"))?;
            return Err("Independent exit watchdog stopped unexpectedly; original runtime termination requested".into());
        }
    } else {
        // The exact captured identity already exited; no replacement is targeted.
        #[cfg(feature = "desktop-reliability")]
        super::fixture::record_exit_helper(request.pid, request.deadline_tick_ms)?;
        write_ready(request.pid, &request.nonce)?;
    }
    Ok(())
}

fn write_ready(pid: u32, nonce: &str) -> Result<(), String> {
    let receipt = Ready {
        pid,
        nonce: nonce.to_owned(),
        deadline_owned: true,
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &receipt).map_err(|e| e.to_string())?;
    stdout
        .write_all(b"\n")
        .and_then(|()| stdout.flush())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_receipt_requires_the_original_process_and_request() {
        let request = Request {
            pid: 42,
            process: app_core::ProcessIdentity {
                creation_time: 7,
                image_path: "fixture.exe".into(),
            },
            sid: "fixture".into(),
            nonce: "original".into(),
            deadline_tick_ms: 500,
        };
        for (pid, nonce, deadline_owned, valid) in [
            (42, "original", true, true),
            (43, "original", true, false),
            (42, "replacement", true, false),
            (42, "original", false, false),
        ] {
            assert_eq!(
                validate_ready(
                    &Ready {
                        pid,
                        nonce: nonce.into(),
                        deadline_owned
                    },
                    &request
                )
                .is_ok(),
                valid
            );
        }
    }
}
