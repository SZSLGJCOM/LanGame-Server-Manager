//! An operator-approved elevated launch keeps its Job in a minimal native helper.
//! The helper owns no UI, database or network service and exits with its parent.
use std::fs::File;
use std::io::{self, Write};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::{OwnedWindowsHandle, RuntimeChild, WindowsProcessHandle, WindowsSpawnedChild};

mod control;
mod pipe;
mod request;
mod security;

const HELPER_FLAG: &str = "--langame-elevated-launcher";
const START_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_TIMEOUT: Duration = Duration::from_secs(4);

/// Called before any desktop or service initialization. No fixture bypass exists
/// in production: this mode always requires the approved elevated token.
pub fn run_elevated_launcher_if_requested() -> Option<i32> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some(HELPER_FLAG) {
        return None;
    }
    let result = (|| {
        security::require_elevated()?;
        if args.len() != 4 {
            return Err(invalid("invalid launcher arguments"));
        }
        let pid = args[2]
            .parse()
            .map_err(|_| invalid("invalid launcher parent"))?;
        let creation = args[3]
            .parse()
            .map_err(|_| invalid("invalid launcher identity"))?;
        run_helper(&args[1], pid, creation)
    })();
    Some(match result {
        Ok(code) => code,
        Err(error) => {
            report_failure(&mut std::io::stderr(), &error);
            1
        }
    })
}

fn report_failure(writer: &mut impl io::Write, error: &io::Error) {
    // A GUI launch often has no stderr. Diagnostics must never panic during exit.
    let _ = writeln!(writer, "elevated launcher failed: {error}");
}

pub(super) fn spawn(
    executable: &str,
    args: &[String],
    directory: &Path,
    stdout: File,
    stderr: File,
    background: bool,
) -> io::Result<(RuntimeChild, Option<crate::WindowsHiddenDesktop>)> {
    let nonce = pipe::nonce()?;
    let channel = pipe::Channel::server(&nonce)?;
    let parent = crate::query_windows_process_identity_from_handle(std::process::id(), unsafe {
        crate::GetCurrentProcess()
    })
    .map_err(io::Error::other)?;
    let helper = shell_execute(&nonce, parent.creation_time)?;
    let packet = json!({
        "executable": executable, "args": args, "directory": directory,
        "stdout": stdout.as_raw_handle() as usize, "stderr": stderr.as_raw_handle() as usize,
        "background": background,
    });
    // Keep both source handles alive until the helper has duplicated them and
    // acknowledged creation inside its Job. A failed handshake closes the pipe.
    let child = complete_start(channel, helper, &packet)?;
    Ok((child, None))
}

fn shell_execute(nonce: &str, creation: u64) -> io::Result<OwnedWindowsHandle> {
    let executable = std::env::current_exe()?;
    let verb = crate::wide_null("runas");
    let file = crate::wide_null(&executable.to_string_lossy());
    let arguments = crate::wide_null(&format!(
        "{HELPER_FLAG} {nonce} {} {creation}",
        std::process::id()
    ));
    let directory = executable
        .parent()
        .ok_or_else(|| invalid("launcher directory missing"))?;
    let directory = crate::wide_null(&directory.to_string_lossy());
    let mut info = crate::ShellExecuteInfoW {
        cb_size: std::mem::size_of::<crate::ShellExecuteInfoW>() as u32,
        f_mask: crate::SEE_MASK_NOCLOSEPROCESS,
        hwnd: std::ptr::null_mut(),
        lp_verb: verb.as_ptr(),
        lp_file: file.as_ptr(),
        lp_parameters: arguments.as_ptr(),
        lp_directory: directory.as_ptr(),
        n_show: crate::SW_HIDE as i32,
        h_inst_app: std::ptr::null_mut(),
        lp_id_list: std::ptr::null_mut(),
        lp_class: std::ptr::null(),
        hkey_class: std::ptr::null_mut(),
        dw_hot_key: 0,
        h_icon_or_monitor: std::ptr::null_mut(),
        h_process: std::ptr::null_mut(),
    };
    if unsafe { crate::ShellExecuteExW(&mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.h_process.is_null() {
        return Err(io::Error::other(
            "elevated launch did not return a process handle",
        ));
    }
    Ok(OwnedWindowsHandle::new(info.h_process))
}

fn complete_start(
    channel: pipe::Channel,
    helper: OwnedWindowsHandle,
    packet: &serde_json::Value,
) -> io::Result<RuntimeChild> {
    let deadline = Instant::now() + START_TIMEOUT;
    channel.accept(deadline)?;
    let pid = unsafe { crate::GetProcessId(helper.as_raw()) };
    if pid == 0 {
        return Err(io::Error::last_os_error());
    }
    if channel.peer_pid(true)? != pid {
        return Err(denied("launcher client PID mismatch"));
    }
    security::verify_handle(helper.as_raw(), pid)?;
    channel.send(packet, deadline)?;
    let response = channel.receive(deadline)?;
    if response.get("started").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(io::Error::other(
            response
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("elevated launcher rejected startup")
                .to_owned(),
        ));
    }
    Ok(RuntimeChild::Windows(WindowsSpawnedChild {
        process_handle: helper.into_raw() as usize,
        process_id: pid,
        stdin: None,
        terminal: None,
        job: None,
        output: None,
        elevated: Some(ElevatedGuard {
            channel,
            confirmed: false,
            interrupt_pending: false,
            receipts: control::ReceiptReader::default(),
        }),
    }))
}

/// Dropping this guard only closes the pipe. The independent helper observes the
/// disconnect and reaps its Job; no IPC wait occurs while dropping a supervisor.
#[derive(Debug)]
pub(crate) struct ElevatedGuard {
    channel: pipe::Channel,
    confirmed: bool,
    interrupt_pending: bool,
    receipts: control::ReceiptReader,
}

impl ElevatedGuard {
    pub(crate) fn finish(&mut self, process: usize) -> io::Result<()> {
        if self.confirmed {
            return Ok(());
        }
        let deadline = Instant::now() + STOP_TIMEOUT;
        if running(process)? {
            self.channel.write_all(b"S", deadline)?;
        }
        while running(process)? {
            pipe::check_deadline(deadline)?;
            std::thread::sleep(pipe::POLL);
        }
        self.confirm_stopped(deadline)
    }

    pub(crate) fn is_running(&mut self, process: usize) -> io::Result<bool> {
        if running(process)? {
            return Ok(true);
        }
        self.confirm_stopped(Instant::now() + Duration::from_millis(100))?;
        Ok(false)
    }

    fn confirm_stopped(&mut self, deadline: Instant) -> io::Result<()> {
        while !self.confirmed {
            let response = self.receipts.receive(&self.channel, deadline)?;
            if self.interrupt_pending && control::is_interrupt_response(&response) {
                self.interrupt_pending = false;
                continue;
            }
            if response.get("stopped").and_then(serde_json::Value::as_bool) != Some(true) {
                return Err(io::Error::other(
                    "elevated launcher did not confirm process-tree cleanup",
                ));
            }
            self.confirmed = true;
        }
        Ok(())
    }
}

fn running(process: usize) -> io::Result<bool> {
    match unsafe { crate::WaitForSingleObject(process as *mut _, 0) } {
        crate::WAIT_TIMEOUT => Ok(true),
        crate::WAIT_OBJECT_0 => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

fn run_helper(nonce: &str, expected_parent: u32, creation: u64) -> io::Result<i32> {
    let channel = pipe::Channel::connect(nonce)?;
    if channel.peer_pid(false)? != expected_parent {
        return Err(denied("launcher server PID mismatch"));
    }
    let parent = WindowsProcessHandle::open(expected_parent, 0x0040)
        .map_err(io::Error::other)?
        .ok_or_else(|| denied("launcher parent no longer exists"))?; // PROCESS_DUP_HANDLE
    security::verify_peer(&parent)?;
    if parent.identity().map_err(io::Error::other)?.creation_time != creation {
        return Err(denied("launcher parent creation time mismatch"));
    }
    let packet = channel.receive(Instant::now() + START_TIMEOUT)?;
    let started = request::LaunchRequest::parse(&packet).and_then(|plan| plan.start(&parent));
    let (mut child, _desktop) = match started {
        Ok(child) => child,
        Err(error) => {
            let _ = channel.send(
                &json!({"error": error.to_string()}),
                Instant::now() + Duration::from_millis(100),
            );
            return Err(error);
        }
    };
    let interrupt = control::OwnedConsoleInterrupt::capture(&child)?;
    // child already owns a noninheritable KILL_ON_JOB_CLOSE Job. Every return,
    // including failed acknowledgement, has a cleanup owner in this stack frame.
    if let Err(error) = channel.send(
        &json!({"started": true}),
        Instant::now() + Duration::from_millis(100),
    ) {
        child.finish_process_tree()?;
        return Err(error);
    }
    let mut interrupt_response_failed = false;
    let exit_code = loop {
        if !parent.is_running().map_err(io::Error::other)? {
            break 1;
        }
        let mut command = [0];
        match channel.read_available(&mut command) {
            Ok(0) => {}
            Ok(1) if command[0] == b'S' => break 0,
            Ok(1) if command[0] == b'C' => {
                // A failed graceful request leaves the Job and helper alive.
                // Only the separate owner-cleanup command/death path may reap it.
                if !interrupt_response_failed {
                    let response = interrupt.request();
                    if let Err(error) = channel.send(&response, Instant::now() + STOP_TIMEOUT) {
                        // A partial receipt cannot be replayed safely. Keep the
                        // Job until explicit owner cleanup or owner disconnect;
                        // a failed graceful-control reply must never kill it.
                        interrupt_response_failed = true;
                        let _ = writeln!(
                            std::io::stderr(),
                            "elevated console receipt failed: {error}"
                        );
                    }
                }
            }
            Ok(_) => {
                child.finish_process_tree()?;
                return Err(invalid("invalid launcher control"));
            }
            Err(_) => break 1, // A disconnected owner cannot retain its workload.
        }
        if !workload_may_be_running(&child)? {
            break child
                .try_wait()?
                .and_then(|status| status.code())
                .unwrap_or(0);
        }
        std::thread::sleep(pipe::POLL);
    };
    child.finish_process_tree()?;
    // The success receipt is issued only after native root/descendant exit. A
    // dead/disconnected parent needs no receipt, but still gets the same cleanup.
    let _ = channel.send(
        &json!({"stopped": true}),
        Instant::now() + Duration::from_millis(100),
    );
    Ok(exit_code)
}

fn workload_may_be_running(child: &RuntimeChild) -> io::Result<bool> {
    let RuntimeChild::Windows(child) = child else {
        return Err(io::Error::other("elevated workload has no native owner"));
    };
    let job = child
        .job
        .as_ref()
        .ok_or_else(|| io::Error::other("elevated workload has no Job"))?;
    // This loop must not capture thousands of process handles or acquire the
    // inspection lock: parent death is checked every native polling interval.
    // This is only a liveness hint. finish_process_tree confirms actual cleanup.
    Ok(job.active_process_count()? != 0 || child.try_wait()?.is_none())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests;
