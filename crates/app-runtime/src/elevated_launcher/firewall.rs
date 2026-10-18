//! One approved firewall operation; only the executable's fixed handler can act.
use super::{OwnedWindowsHandle, WindowsProcessHandle, denied, invalid, pipe, security};
use serde_json::{Value, json};
use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const HELPER_FLAG: &str = "--langame-elevated-firewall";
const OPERATION_TIMEOUT: Duration = Duration::from_secs(40);
const EXIT_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_RULES: usize = 128;
const ERROR_CODES: &[&str] = &[
    "invalid_rules",
    "rule_conflict",
    "permission_denied",
    "verification_failed",
    "cleanup_failed",
    "timeout",
    "apply_failed",
];
type Handler = fn(&Value) -> Result<Value, String>;

/// Register the compiled firewall backend before initializing any desktop state.
/// The request cannot select an executable, script, output file or callback.
pub fn run_elevated_firewall_if_requested(handler: Handler) -> Option<i32> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some(HELPER_FLAG) {
        return None;
    }
    let result = (|| {
        security::require_elevated()?;
        if args.len() != 4 {
            return Err(invalid("invalid firewall helper arguments"));
        }
        let parent = args[2]
            .parse()
            .map_err(|_| invalid("invalid firewall helper parent"))?;
        let creation = args[3]
            .parse()
            .map_err(|_| invalid("invalid firewall helper identity"))?;
        run_helper(&args[1], parent, creation, handler)
    })();
    Some(result.unwrap_or(1))
}

/// The caller supplies backend-built rule data. Cancellation of UAC retains the
/// original Windows error; uncertain completion never implies firewall rollback.
/// The 40-second operation deadline starts after ShellExecuteExW returns an
/// approved helper. Windows owns the consent dialog: waiting for approval is not
/// bounded by this deadline. Cancelling an async caller awaiting this blocking
/// operation does not dismiss that UI or retract a later approval. Any late
/// helper still authenticates the parent; owner exit or pipe closure ends it.
pub fn request_elevated_firewall(rules: &Value) -> io::Result<Value> {
    let packet = request_packet(rules)?;
    let nonce = pipe::nonce()?;
    let channel = pipe::Channel::server(&nonce)?;
    let parent = crate::query_windows_process_identity_from_handle(std::process::id(), unsafe {
        crate::GetCurrentProcess()
    })
    .map_err(io::Error::other)?;
    let helper = super::shell_execute_mode(HELPER_FLAG, &nonce, parent.creation_time)?;
    complete_request(channel, helper, &packet)
}

fn request_packet(rules: &Value) -> io::Result<Value> {
    if rules
        .as_array()
        .is_none_or(|rules| rules.is_empty() || rules.len() > MAX_RULES)
    {
        return Err(invalid("invalid firewall rule array"));
    }
    struct Budget(usize);
    impl io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| invalid("firewall request exceeds its size limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    // Count before cloning the input or allocating its encoded form. Ten bytes
    // are reserved for the fixed {"rules":...} envelope.
    serde_json::to_writer(&mut Budget(pipe::MAX_FRAME - 10), rules)
        .map_err(|_| invalid("firewall request exceeds its size limit"))?;
    Ok(json!({"rules": rules}))
}

fn parse_request(packet: &Value) -> io::Result<&Value> {
    if packet.as_object().is_none_or(|object| object.len() != 1) {
        return Err(invalid("invalid firewall helper request"));
    }
    let rules = packet
        .get("rules")
        .ok_or_else(|| invalid("invalid firewall helper request"))?;
    request_packet(rules)?;
    Ok(rules)
}

fn complete_request(
    channel: pipe::Channel,
    helper: OwnedWindowsHandle,
    packet: &Value,
) -> io::Result<Value> {
    let result = (|| {
        let deadline = Instant::now() + OPERATION_TIMEOUT;
        channel.accept(deadline)?;
        let pid = unsafe { crate::GetProcessId(helper.as_raw()) };
        if pid == 0 {
            return Err(io::Error::last_os_error());
        }
        if channel.peer_pid(true)? != pid {
            return Err(denied("firewall helper client PID mismatch"));
        }
        security::verify_handle(helper.as_raw(), pid)?;
        channel.send(packet, deadline)?;
        parse_response(channel.receive(deadline)?)
    })();
    // Closing the authenticated channel ends the helper even if its callback is
    // still waiting. Its process exit closes the callback's private utility Job.
    drop(channel);
    let exit = wait_helper_exit(&helper, Instant::now() + EXIT_TIMEOUT)?;
    match result {
        Ok(value) if exit == 0 => Ok(value),
        Ok(_) => Err(io::Error::other("firewall helper failed after its receipt")),
        Err(error) if ERROR_CODES.contains(&error.to_string().as_str()) => Err(error),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("firewall operation result was not confirmed; rules may have changed: {error}"),
        )),
    }
}

fn parse_response(response: Value) -> io::Result<Value> {
    if response.as_object().is_none_or(|object| object.len() != 1) {
        return Err(invalid("invalid firewall helper response"));
    }
    if let Some(value) = response.get("result") {
        return Ok(value.clone());
    }
    if let Some(code) = response.get("error").and_then(Value::as_str)
        && ERROR_CODES.contains(&code)
    {
        return Err(io::Error::other(code.to_owned()));
    }
    Err(invalid("invalid firewall helper response"))
}

fn wait_helper_exit(helper: &OwnedWindowsHandle, deadline: Instant) -> io::Result<u32> {
    loop {
        match unsafe { crate::WaitForSingleObject(helper.as_raw(), 0) } {
            crate::WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe {
                    windows_sys::Win32::System::Threading::GetExitCodeProcess(
                        helper.as_raw(),
                        &mut code,
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                return Ok(code);
            }
            crate::WAIT_TIMEOUT => {}
            _ => return Err(io::Error::last_os_error()),
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "firewall helper exit was not confirmed; firewall state may have changed",
            ));
        }
        std::thread::sleep(pipe::POLL);
    }
}

fn run_helper(
    nonce: &str,
    expected_parent: u32,
    creation: u64,
    handler: Handler,
) -> io::Result<i32> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    let channel = pipe::Channel::connect(nonce)?;
    if channel.peer_pid(false)? != expected_parent {
        return Err(denied("firewall helper server PID mismatch"));
    }
    let parent = WindowsProcessHandle::open(expected_parent, 0)
        .map_err(io::Error::other)?
        .ok_or_else(|| denied("firewall helper parent no longer exists"))?;
    security::verify_peer(&parent)?;
    if parent.identity().map_err(io::Error::other)?.creation_time != creation {
        return Err(denied("firewall helper parent creation time mismatch"));
    }
    let packet = channel.receive(deadline)?;
    let rules = parse_request(&packet)?.clone();
    if !parent.is_running().map_err(io::Error::other)? {
        return Err(denied("firewall helper parent exited before execution"));
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(handler(&rules));
    });
    loop {
        pipe::check_deadline(deadline)?;
        if !parent.is_running().map_err(io::Error::other)? {
            return Err(denied("firewall helper parent exited during execution"));
        }
        let mut unexpected = [0];
        if channel.read_available(&mut unexpected)? != 0 {
            return Err(invalid("firewall helper accepts only one request"));
        }
        match receiver.try_recv() {
            Ok(result) => {
                let (response, code) = match result {
                    Ok(value) => (json!({"result": value}), 0),
                    Err(error) => {
                        let code = if ERROR_CODES.contains(&error.as_str()) {
                            error.as_str()
                        } else {
                            "apply_failed"
                        };
                        (json!({"error": code}), 1)
                    }
                };
                channel.send(&response, deadline)?;
                return Ok(code);
            }
            Err(mpsc::TryRecvError::Empty) => std::thread::sleep(pipe::POLL),
            Err(mpsc::TryRecvError::Disconnected) => {
                channel.send(&json!({"error": "apply_failed"}), deadline)?;
                return Ok(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    const NONCE_ENV: &str = "LANGAME_FIREWALL_TRANSPORT_NONCE";
    const PARENT_ENV: &str = "LANGAME_FIREWALL_TRANSPORT_PARENT";
    const CREATION_ENV: &str = "LANGAME_FIREWALL_TRANSPORT_CREATION";
    const PROBE: &str = "elevated_launcher::firewall::tests::firewall_helper_probe";

    fn fixture_handler(rules: &Value) -> Result<Value, String> {
        match rules[0]["fixture"].as_str() {
            Some("wait") => std::thread::sleep(Duration::from_secs(20)),
            Some("fail") => return Err("private raw backend diagnostic".into()),
            _ => {}
        }
        Ok(json!({"rule_count": rules.as_array().unwrap().len()}))
    }

    #[test]
    #[ignore = "internal non-elevated transport fixture; never calls the firewall backend"]
    fn firewall_helper_probe() {
        let nonce = std::env::var(NONCE_ENV).unwrap();
        let parent = std::env::var(PARENT_ENV).unwrap().parse().unwrap();
        let creation = std::env::var(CREATION_ENV).unwrap().parse().unwrap();
        // Production entry always checks elevation. This subprocess tests only
        // the authenticated transport with a compiled, harmless test handler.
        std::process::exit(run_helper(&nonce, parent, creation, fixture_handler).unwrap_or(1));
    }

    fn fixture(nonce: &str, bad_creation: bool) -> (Child, OwnedWindowsHandle) {
        let parent =
            crate::query_windows_process_identity_from_handle(std::process::id(), unsafe {
                crate::GetCurrentProcess()
            })
            .unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", PROBE, "--nocapture"])
            .env(NONCE_ENV, nonce)
            .env(PARENT_ENV, std::process::id().to_string())
            .env(
                CREATION_ENV,
                if bad_creation {
                    1
                } else {
                    parent.creation_time
                }
                .to_string(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(crate::CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let handle = unsafe {
            crate::OpenProcess(
                crate::SYNCHRONIZE | crate::PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                child.id(),
            )
        };
        assert!(!handle.is_null());
        (child, OwnedWindowsHandle::new(handle))
    }

    #[test]
    fn firewall_packets_are_bounded_data_and_reject_command_paths() {
        let rules = json!([{"rule_name": "LanGame fixture UDP 7777"}]);
        let packet = request_packet(&rules).unwrap();
        assert_eq!(parse_request(&packet).unwrap(), &rules);
        for invalid in [
            Value::Null,
            json!([]),
            json!({"script": "arbitrary.ps1"}),
            json!(vec![json!({}); MAX_RULES + 1]),
            json!([{"rule_name": "x".repeat(pipe::MAX_FRAME)}]),
        ] {
            assert!(request_packet(&invalid).is_err());
        }
        for extra in ["script", "executable", "stdout", "directory", "command"] {
            let mut packet = packet.clone();
            packet[extra] = json!("untrusted path or command");
            assert!(parse_request(&packet).is_err());
        }
    }

    #[test]
    fn firewall_receipts_accept_only_fixed_error_codes_or_one_result() {
        assert_eq!(parse_response(json!({"result": []})).unwrap(), json!([]));
        for code in ERROR_CODES {
            assert_eq!(
                parse_response(json!({"error": code}))
                    .unwrap_err()
                    .to_string(),
                *code
            );
        }
        for packet in [
            json!({"error": "raw native body with private path"}),
            json!({"result": [], "error": "apply_failed"}),
            json!({"started": true}),
            json!({"script": "arbitrary.ps1"}),
        ] {
            assert_eq!(
                parse_response(packet).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn firewall_transport_confirms_helper_exit_and_sanitizes_backend_errors() {
        for (mode, successful) in [("success", true), ("fail", false)] {
            let nonce = pipe::nonce().unwrap();
            let channel = pipe::Channel::server(&nonce).unwrap();
            let (mut child, handle) = fixture(&nonce, false);
            let packet = request_packet(&json!([{"fixture": mode}])).unwrap();
            let result = complete_request(channel, handle, &packet);
            let status = child
                .try_wait()
                .unwrap()
                .expect("receipt must await helper exit");
            assert_eq!(status.success(), successful);
            if successful {
                assert_eq!(result.unwrap(), json!({"rule_count": 1}));
            } else {
                assert_eq!(result.unwrap_err().to_string(), "apply_failed");
            }
        }
    }

    #[test]
    fn firewall_worker_cannot_keep_the_helper_alive_after_owner_disconnect() {
        let nonce = pipe::nonce().unwrap();
        let channel = pipe::Channel::server(&nonce).unwrap();
        let (mut child, handle) = fixture(&nonce, false);
        let deadline = Instant::now() + Duration::from_secs(3);
        channel.accept(deadline).unwrap();
        assert_eq!(channel.peer_pid(true).unwrap(), child.id());
        security::verify_handle(handle.as_raw(), child.id()).unwrap();
        channel
            .send(
                &request_packet(&json!([{"fixture": "wait"}])).unwrap(),
                deadline,
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        drop(channel);
        assert_ne!(wait_helper_exit(&handle, deadline).unwrap(), 0);
        assert!(child.wait().unwrap().code().is_some());
    }

    #[test]
    fn firewall_helper_rejects_reused_parent_identity_before_request_execution() {
        let nonce = pipe::nonce().unwrap();
        let channel = pipe::Channel::server(&nonce).unwrap();
        let (mut child, handle) = fixture(&nonce, true);
        assert_ne!(
            wait_helper_exit(&handle, Instant::now() + Duration::from_secs(3)).unwrap(),
            0
        );
        assert!(!child.wait().unwrap().success());
        drop(channel);
    }
}
