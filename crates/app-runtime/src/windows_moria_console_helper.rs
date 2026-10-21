//! A private, once-only exit sender. Never changes the manager's console.

use super::*;
use std::io::{Read, Write};
use std::os::windows::io::IntoRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
use windows_sys::Win32::System::Pipes::PeekNamedPipe;

// WinSDK winnt.h, Job Object Specific Access Rights. windows-sys exports this
// under the otherwise unused SystemServices feature rather than JobObjects.
const JOB_OBJECT_QUERY: u32 = 0x0004;

const FLAG: &str = "--langame-moria-console-exit";
const FRAME: &str = "LGM_MORIA_EXIT_REPLY ";
const CHILD_ENV: &str = "LANGAME_MORIA_EXIT_HELPER_CHILD";
const MAX_REQUEST: usize = 64 * 1024;
const MAX_REPLY: usize = 4096;
const BUDGET: Duration = Duration::from_secs(12);

#[path = "windows_moria_console_reply.rs"]
mod reply;
use reply::{parse_reply, read_reply};
const UNIT_ENTRY: &str = "windows_moria_console::helper::tests::moria_exit_helper_entry";
const DESKTOP_ENTRY: &str = "commands::tests::manual_runtime_tests::native_lifecycle_tests::moria_console_exit_helper_entry";

struct OwnedHelper {
    process: Child,
    // Solely the manager owns this non-inheritable kill-on-close Job. The sender
    // must die on manager loss before its game-Job query handle can keep a world
    // alive. No game capability is transferred until assignment succeeds.
    lifetime: windows_process_job::OwnedProcessJob,
}
impl OwnedHelper {
    fn spawn(mut command: Command) -> std::io::Result<Self> {
        let lifetime = windows_process_job::OwnedProcessJob::new()?;
        let owner = Self {
            process: command.spawn()?,
            lifetime,
        };
        if unsafe {
            AssignProcessToJobObject(owner.lifetime.as_raw(), owner.process.as_raw_handle())
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(owner)
    }
}
impl Drop for OwnedHelper {
    fn drop(&mut self) {
        if !matches!(self.process.try_wait(), Ok(Some(_))) {
            let _ = self.process.kill();
            // Only the sender is terminated. A timed-out input write is never replayed.
            unsafe { WaitForSingleObject(self.process.as_raw_handle(), 500) };
            let _ = self.process.try_wait();
        }
    }
}

fn query_capability(job: *mut c_void, sender: &OwnedHelper) -> std::io::Result<usize> {
    let process = sender.process.as_raw_handle();
    let mut assigned = 0;
    if unsafe { IsProcessInJob(process, sender.lifetime.as_raw(), &mut assigned) } == 0
        || assigned == 0
    {
        return Err(std::io::Error::other(
            "sender lifetime Job is not established",
        ));
    }
    let mut copy = std::ptr::null_mut();
    // This handle exists only in the contained sender's handle table. It cannot
    // terminate/assign processes, and the sender never receives its lifetime Job.
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            job,
            process,
            &mut copy,
            JOB_OBJECT_QUERY,
            0,
            0,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(copy as usize)
}

fn nonce() -> Result<String, RuntimeProcessError> {
    let mut bytes = [0u8; 16];
    if unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    } < 0
    {
        return Err(error(0, "Moria exit request identity is unavailable"));
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn command() -> Result<Command, RuntimeProcessError> {
    let mut command = Command::new(
        std::env::current_exe().map_err(|_| error(0, "Moria exit helper image is unavailable"))?,
    );
    // These two fixed libtest entries exercise the same worker as production.
    // No caller-provided command, executable or test name can be substituted.
    let entry = if cfg!(test) {
        Some(UNIT_ENTRY)
    } else if std::env::var("LANGAME_MORIA_EXIT_HELPER_TEST_HOST").as_deref() == Ok("desktop")
        && std::env::var("LANGAME_NATIVE_MODULE_ID").as_deref() == Ok("returntomoria")
    {
        Some(DESKTOP_ENTRY)
    } else {
        None
    };
    if let Some(entry) = entry {
        command.args([
            "--exact",
            entry,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ]);
        command.env(CHILD_ENV, "1");
    } else {
        command.arg(FLAG);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    Ok(command)
}

pub(super) fn request(
    root: u32,
    target: u32,
    job: *mut c_void,
    pinned: &[Pinned],
) -> Result<(), RuntimeProcessError> {
    verified_members(root, target, job, pinned, false)?;
    if pinned.len() > MAX_MEMBERS {
        return Err(error(target, "Moria exit authorization exceeds its bound"));
    }
    let nonce = nonce()?;
    let deadline = Instant::now() + BUDGET;
    let mut child = OwnedHelper::spawn(command()?).map_err(|_| {
        error(
            target,
            "Moria exit helper could not start with a lifetime Job",
        )
    })?;
    let child_job = query_capability(job, &child).map_err(|_| {
        error(
            target,
            "Moria exit Job authorization could not be transferred",
        )
    })?;
    let members: Vec<_> = pinned
        .iter()
        .map(|member| serde_json::json!({"pid":member.handle.pid,"identity":member.identity}))
        .collect();
    let request = serde_json::json!({
        "version":1,"module":"returntomoria","nonce":nonce,"root":root,
        "target":target,"job":child_job as u64,"members":members,
        "remaining_ms":deadline.saturating_duration_since(Instant::now()).as_millis() as u64,
    })
    .to_string();
    if request.len() >= MAX_REQUEST {
        return Err(error(target, "Moria exit authorization exceeds its bound"));
    }
    // Validate again after helper creation and before transferring the single
    // authorization. The child independently pins and verifies every identity.
    verified_members(root, target, job, pinned, false)?;
    let stdin = child
        .process
        .stdin
        .take()
        .ok_or_else(|| error(target, "Moria exit private input is unavailable"))?;
    let mut writer = WindowsPipeWriter::new(stdin);
    writer
        .write_line(&request, &RuntimeStdinCancellation::default())
        .map_err(|_| error(target, "Moria exit authorization delivery failed; no retry"))?;
    drop(writer); // EOF is required before the helper accepts a complete request.
    let stdout = child
        .process
        .stdout
        .take()
        .ok_or_else(|| error(target, "Moria exit private reply is unavailable"))?;
    let output = unsafe { File::from_raw_handle(stdout.into_raw_handle()) };
    let reply = read_reply(&mut child, &output, deadline)
        .map_err(|_| error(target, "Moria exit helper result is unknown; no retry"))?;
    verified_members(root, target, job, pinned, true)?;
    let input_read = parse_reply(&reply, &nonce, root, target)?;
    eprintln!(
        "MORIA_SHUTDOWN phase=native_exit_observed input_read={input_read} native_tree_exit_confirmed=false"
    );
    Ok(())
}

fn keys(value: &Value, expected: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
    })
}

struct Request {
    nonce: String,
    root: u32,
    target: u32,
    job: OwnedWindowsHandle,
    pinned: Vec<Pinned>,
    deadline: Instant,
}

fn authorize(value: &Value) -> Option<Request> {
    if !keys(
        value,
        &[
            "version",
            "module",
            "nonce",
            "root",
            "target",
            "job",
            "members",
            "remaining_ms",
        ],
    ) || value["version"] != 1
        || value["module"] != "returntomoria"
    {
        return None;
    }
    let remaining_ms = value["remaining_ms"]
        .as_u64()
        .filter(|ms| (1..=12_000).contains(ms))?;
    let deadline = Instant::now() + Duration::from_millis(remaining_ms);
    let nonce = value["nonce"].as_str()?;
    if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let root = u32::try_from(value["root"].as_u64()?)
        .ok()
        .filter(|pid| *pid != 0)?;
    let target = u32::try_from(value["target"].as_u64()?)
        .ok()
        .filter(|pid| *pid != 0)?;
    let raw_job = usize::try_from(value["job"].as_u64()?)
        .ok()
        .filter(|raw| *raw != 0 && *raw != usize::MAX)?;
    let members = value["members"].as_array()?;
    if members.is_empty() || members.len() > MAX_MEMBERS {
        return None;
    }
    let mut records = Vec::new();
    let mut pinned = Vec::new();
    let mut seen = HashSet::new();
    // This process owns the transferred query handle, including rejection paths.
    let job = OwnedWindowsHandle::new(raw_job as *mut c_void);
    for member in members {
        if !keys(member, &["pid", "identity"])
            || !keys(&member["identity"], &["creation_time", "image_path"])
        {
            return None;
        }
        let pid = u32::try_from(member["pid"].as_u64()?)
            .ok()
            .filter(|pid| *pid != 0 && seen.insert(*pid))?;
        let identity: ProcessIdentity = serde_json::from_value(member["identity"].clone()).ok()?;
        if identity.creation_time == 0
            || identity.image_path.len() > 4096
            || identity.image_path.chars().any(char::is_control)
        {
            return None;
        }
        let handle = open_verified_windows_process(pid, &identity, 0).ok()??;
        if !belongs(&handle, job.as_raw()).ok()? {
            return None;
        }
        records.push(WindowsProcessRecord {
            process_id: pid,
            parent_process_id: root,
            name: String::new(),
            identity: Some(identity.clone()),
        });
        pinned.push(Pinned { handle, identity });
    }
    let owner = pinned.iter().find(|member| member.handle.pid == root)?;
    if select_shipping(root, &owner.identity, &records)?.process_id != target {
        return None;
    }
    Some(Request {
        nonce: nonce.into(),
        root,
        target,
        job,
        pinned,
        deadline,
    })
}

/// Early production entry; recognized invocations must not continue app startup.
pub fn run_if_requested() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(FLAG)) {
        return None;
    }
    Some(if args.next().is_none() {
        run_worker()
    } else {
        2
    })
}

/// Fixed host entry shared with explicitly selected, isolated libtest children.
pub fn run_worker() -> i32 {
    let mut raw_output = std::ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            GetStdHandle(-11i32 as u32),
            GetCurrentProcess(),
            &mut raw_output,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return 2;
    }
    let mut output = unsafe { File::from_raw_handle(raw_output) };
    let mut bytes = Vec::new();
    if std::io::stdin()
        .take(MAX_REQUEST as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > MAX_REQUEST
    {
        return 2;
    }
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return 2;
    };
    let Some(request) = authorize(&value) else {
        // Report an identified parent's pre-input refusal without echoing image
        // paths or treating malformed/unknown requests as dispatched commands.
        if let (Some(nonce), Some(root), Some(target)) = (
            value["nonce"].as_str().filter(|nonce| {
                nonce.len() == 32 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
            }),
            value["root"]
                .as_u64()
                .filter(|pid| *pid > 0 && *pid <= u32::MAX.into()),
            value["target"]
                .as_u64()
                .filter(|pid| *pid > 0 && *pid <= u32::MAX.into()),
        ) {
            let response = serde_json::json!({
                "nonce":nonce,"root":root,"target":target,"queued":false,
                "input_read":null,
                "error":"authorization rejected before console attachment",
            });
            return if writeln!(output, "\n{FRAME}{response}").is_ok() {
                0
            } else {
                2
            };
        }
        return 2;
    };
    // Detaching only this new sender cannot disturb the manager/operator console.
    unsafe { FreeConsole() };
    let result = deliver(
        request.root,
        request.target,
        request.job.as_raw(),
        &request.pinned,
        request.deadline,
    );
    let response = serde_json::json!({
        "nonce":request.nonce,"root":request.root,"target":request.target,
        "input_read":result.as_ref().ok().copied(),
        "queued":result.is_ok(),"error":result.err().map(|failure| failure.to_string()),
    });
    if writeln!(output, "\n{FRAME}{response}").is_err() {
        2
    } else {
        0
    }
}

#[cfg(test)]
#[path = "windows_moria_console_helper_tests.rs"]
mod tests;
