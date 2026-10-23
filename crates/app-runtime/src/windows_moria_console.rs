//! Deliver the publisher's `exit` command to one owned Moria console.
//! This is a shutdown queue receipt; the supervisor still verifies whole-Job exit.

use super::*;
use std::ffi::c_void;
use windows_sys::Win32::System::JobObjects::IsProcessInJob;

const MAX_MEMBERS: usize = 64;
const MAX_EMPTY_READS: u32 = 32;
const SHIPPING: &str = "MoriaServer-Win64-Shipping.exe";

#[path = "windows_moria_console_helper.rs"]
mod helper;
pub use helper::{run_if_requested, run_worker};

#[path = "windows_moria_console_input.rs"]
mod input_reader;

#[path = "windows_moria_console_receipt.rs"]
mod receipt;
use receipt::{exit_response_received, require_empty_prompt};

fn error(pid: u32, message: impl Into<String>) -> RuntimeProcessError {
    RuntimeProcessError::WriteTrackedProcessStdin {
        pid,
        source: std::io::Error::other(message.into()),
    }
}

pub(super) fn request(
    child: &RuntimeChild,
    root_identity: &ProcessIdentity,
) -> Result<(), RuntimeProcessError> {
    let pid = child.id();
    let RuntimeChild::Windows(native) = child else {
        return Err(error(pid, "Moria exit requires the native launch owner"));
    };
    let job = native
        .job
        .as_ref()
        .ok_or_else(|| error(pid, "Moria exit requires an owned Job"))?;
    let records = collect_windows_process_tree(pid, root_identity)?;
    if records.len() > 4096 {
        return Err(error(
            pid,
            "Moria owned process discovery exceeds its bound",
        ));
    }
    let shipping = select_shipping(pid, root_identity, &records)
        .ok_or_else(|| error(pid, "Moria exit requires one exact owned Shipping image"))?;
    let shipping_pid = shipping.process_id;
    // Pinned handles plus Job membership authorize delivery. The ancestry/path
    // snapshot only selects candidates; it cannot grant process ownership.
    let mut pinned = Vec::new();
    for record in &records {
        let Some(identity) = &record.identity else {
            continue;
        };
        let Some(handle) = open_verified_windows_process(record.process_id, identity, 0)? else {
            continue;
        };
        if belongs(&handle, job.as_raw())? {
            pinned.push(Pinned {
                handle,
                identity: identity.clone(),
            });
        }
    }
    if !pinned.iter().any(|member| member.handle.pid == pid)
        || !pinned
            .iter()
            .any(|member| member.handle.pid == shipping_pid)
    {
        return Err(error(
            pid,
            "Moria launch or Shipping is outside the owned Job",
        ));
    }
    windows_console_control::with_windows_console_ctrl_lock(|| {
        helper::request(pid, shipping_pid, job.as_raw(), &pinned)
    })
}

fn select_shipping<'a>(
    root_pid: u32,
    root: &ProcessIdentity,
    records: &'a [WindowsProcessRecord],
) -> Option<&'a WindowsProcessRecord> {
    if root_pid == 0 || root.creation_time == 0 {
        return None;
    }
    let image = Path::new(&root.image_path);
    let name = image.file_name()?.to_str()?;
    let expected = if name.eq_ignore_ascii_case("MoriaServer.exe") {
        image.parent()?.join("Moria/Binaries/Win64").join(SHIPPING)
    } else if name.eq_ignore_ascii_case(SHIPPING) {
        image.to_path_buf()
    } else {
        return None;
    };
    let expected = normalized_process_image_path(&expected.to_string_lossy());
    let mut candidates = records.iter().filter(|record| {
        record.identity.as_ref().is_some_and(|identity| {
            record.process_id != 0
                && identity.creation_time >= root.creation_time
                && normalized_process_image_path(&identity.image_path) == expected
                && (!name.eq_ignore_ascii_case(SHIPPING)
                    || (record.process_id == root_pid && process_identities_match(root, identity)))
        })
    });
    let selected = candidates.next()?;
    candidates.next().is_none().then_some(selected)
}

struct Pinned {
    handle: WindowsProcessHandle,
    identity: ProcessIdentity,
}

fn belongs(handle: &WindowsProcessHandle, job: *mut c_void) -> Result<bool, RuntimeProcessError> {
    let mut member = 0;
    if unsafe { IsProcessInJob(handle.raw, job, &mut member) } == 0 {
        return Err(error(
            handle.pid,
            "Moria Job membership could not be verified",
        ));
    }
    Ok(member != 0)
}

fn member_set_valid(
    members: &[u32],
    sender: u32,
    target: u32,
    allowed: &[u32],
    require_target: bool,
) -> bool {
    !members.is_empty()
        && members.len() <= MAX_MEMBERS
        && members.contains(&sender)
        && (!require_target || members.contains(&target))
        && members
            .iter()
            .all(|pid| *pid == sender || allowed.contains(pid))
}

fn verified_members(
    root: u32,
    target: u32,
    job: *mut c_void,
    pinned: &[Pinned],
    allow_exited: bool,
) -> Result<Vec<u32>, RuntimeProcessError> {
    let mut allowed = Vec::with_capacity(pinned.len());
    for member in pinned {
        if !member.handle.is_running()? {
            if !allow_exited && (member.handle.pid == root || member.handle.pid == target) {
                return Err(error(
                    target,
                    "Moria process exited before console delivery",
                ));
            }
            continue;
        }
        // This retained kernel handle was identity-checked when pinned. It
        // cannot change to a reused PID. Image-path queries become unavailable
        // during teardown before the same handle is signaled; they add no
        // ownership evidence here. Keep checking live Job membership instead.
        if !belongs(&member.handle, job)? {
            return Err(RuntimeProcessError::ProcessIdentityMismatch {
                pid: member.handle.pid,
            });
        }
        allowed.push(member.handle.pid);
    }
    Ok(allowed)
}

fn verify(
    root: u32,
    target: u32,
    job: *mut c_void,
    pinned: &[Pinned],
    allow_exited: bool,
) -> Result<bool, RuntimeProcessError> {
    let allowed = verified_members(root, target, job, pinned, allow_exited)?;
    let mut members = [0u32; MAX_MEMBERS];
    let count =
        unsafe { GetConsoleProcessList(members.as_mut_ptr(), members.len() as u32) } as usize;
    if count > members.len()
        || !member_set_valid(
            &members[..count],
            std::process::id(),
            target,
            &allowed,
            !allow_exited,
        )
    {
        return Err(error(
            target,
            "Moria console contains an unowned or changed process",
        ));
    }
    Ok(members[..count].contains(&target))
}

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        close_handle(self.0);
    }
}
struct ConsoleSession {
    original: [*mut c_void; 3],
    ignored_ctrl_c: bool,
}
impl ConsoleSession {
    fn attach(pid: u32) -> Result<Self, RuntimeProcessError> {
        let original = [-10i32, -11, -12].map(|id| unsafe { GetStdHandle(id as u32) });
        // Only the private sender reaches this API, after detaching its own
        // initial console. The manager never attaches or detaches its console.
        if unsafe { AttachConsole(pid) } == 0 {
            return Err(error(
                pid,
                format!(
                    "Moria console attach failed: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        let mut session = Self {
            original,
            ignored_ctrl_c: false,
        };
        if unsafe { SetConsoleCtrlHandler(std::ptr::null_mut(), 1) } == 0 {
            return Err(error(pid, "Cannot protect the console command sender"));
        }
        session.ignored_ctrl_c = true;
        Ok(session)
    }
}
impl Drop for ConsoleSession {
    fn drop(&mut self) {
        unsafe {
            FreeConsole();
            for (id, handle) in [-10i32, -11, -12].into_iter().zip(self.original) {
                SetStdHandle(id as u32, handle);
            }
            if self.ignored_ctrl_c {
                SetConsoleCtrlHandler(std::ptr::null_mut(), 0);
            }
        }
    }
}
struct QueryLock(Handle);
impl Drop for QueryLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0.0);
        }
    }
}

fn open_console(pid: u32, name: &str, access: u32) -> Result<Handle, RuntimeProcessError> {
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            3,
            std::ptr::null_mut(),
            3,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle as isize == -1 {
        return Err(error(pid, "Moria console handle is unavailable"));
    }
    Ok(Handle(handle))
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Coord {
    x: i16,
    y: i16,
}
#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i16,
    top: i16,
    right: i16,
    bottom: i16,
}
#[repr(C)]
#[derive(Default)]
struct BufferInfo {
    size: Coord,
    cursor: Coord,
    attributes: u16,
    window: Rect,
    maximum: Coord,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyEvent {
    down: i32,
    repeat: u16,
    key: u16,
    scan: u16,
    character: u16,
    state: u32,
}
#[repr(C)]
#[derive(Default)]
struct InputRecord {
    kind: u16,
    event: KeyEvent,
}
const _: () = assert!(std::mem::size_of::<KeyEvent>() == 16);
const _: () = assert!(std::mem::size_of::<InputRecord>() == 20);
const _: () = assert!(std::mem::align_of::<InputRecord>() == 4);
const _: () = assert!(std::mem::offset_of!(InputRecord, event) == 4);
const _: () = assert!(std::mem::size_of::<BufferInfo>() == 22);

fn write_exit_once(
    pid: u32,
    write: impl FnOnce(&[InputRecord]) -> Result<u32, std::io::Error>,
) -> Result<(), RuntimeProcessError> {
    write_fixed_input_once(pid, "exit\r", write)
}

fn write_fixed_input_once(
    pid: u32,
    text: &'static str,
    write: impl FnOnce(&[InputRecord]) -> Result<u32, std::io::Error>,
) -> Result<(), RuntimeProcessError> {
    let events: Vec<_> = text
        .encode_utf16()
        .flat_map(|character| {
            [true, false].map(|down| InputRecord {
                kind: 1,
                event: KeyEvent {
                    down: i32::from(down),
                    repeat: 1,
                    key: if character == 13 {
                        13
                    } else {
                        (character as u8).to_ascii_uppercase() as u16
                    },
                    character,
                    ..Default::default()
                },
            })
        })
        .collect();
    match write(&events) {
        Ok(written) if written as usize == events.len() => Ok(()),
        _ => Err(error(
            pid,
            "Moria console input delivery failed or is uncertain; no retry",
        )),
    }
}

fn deliver(
    root: u32,
    target: u32,
    job: *mut c_void,
    pinned: &[Pinned],
    deadline: std::time::Instant,
) -> Result<&'static str, RuntimeProcessError> {
    let target_identity = &pinned
        .iter()
        .find(|member| member.handle.pid == target)
        .ok_or_else(|| error(target, "Moria Shipping owner is unavailable"))?
        .identity;
    if std::time::Instant::now() >= deadline {
        return Err(error(
            target,
            "Moria exit deadline expired before console attachment",
        ));
    }
    // Serialize with the separate read-only player helper without extending its
    // authority. A busy player query makes shutdown fail before any input write.
    let name: Vec<u16> = format!(
        "Local\\LanGame-MoriaConsole-{target}-{}",
        target_identity.creation_time
    )
    .encode_utf16()
    .chain([0])
    .collect();
    let mutex = Handle(unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) });
    if mutex.0.is_null() {
        return Err(error(target, "Moria console lock is unavailable"));
    }
    if !matches!(unsafe { WaitForSingleObject(mutex.0, 0) }, 0 | 0x80) {
        return Err(error(target, "Moria console is already in use"));
    }
    let _lock = QueryLock(mutex);
    let _session = ConsoleSession::attach(target)?;
    let input = open_console(target, "CONIN$", 0xc0000000)?;
    let output = open_console(target, "CONOUT$", 0x80000000)?;
    require_empty_prompt(target, &input, &output)?;
    verify(root, target, job, pinned, false)?;
    let reader = input_reader::InputReader::find(target, target_identity.creation_time, deadline)?;
    verify(root, target, job, pinned, false)?;
    let before = require_empty_prompt(target, &input, &output)?;
    if std::time::Instant::now() >= deadline {
        return Err(error(
            target,
            "Moria exit deadline expired before input delivery",
        ));
    }
    write_exit_once(target, |events| {
        let mut written = 0;
        if unsafe {
            WriteConsoleInputW(input.0, events.as_ptr(), events.len() as u32, &mut written)
        } == 0
        {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(written)
        }
    })?;
    // Exit may already have completed. Retained handles distinguish that from
    // PID reuse; any remaining console member must still belong to this Job.
    verify(root, target, job, pinned, true)?;
    if let Some(reader) = reader {
        let mut response_seen = false;
        let mut last_woken: Option<receipt::Prompt> = None;
        let mut wake_count = 0_u32;
        let mut next_wake = std::time::Instant::now();
        loop {
            let target_handle = &pinned
                .iter()
                .find(|member| member.handle.pid == target)
                .ok_or_else(|| error(target, "Moria Shipping owner is unavailable"))?
                .handle;
            if !target_handle.is_running()? {
                return Ok("native_process_exited");
            }
            if !reader.running()? {
                // The input thread ended naturally. Continue observing native
                // shutdown through the supervisor's complete Job deadline.
                return Ok("native_exit_queued");
            }
            if !verify(root, target, job, pinned, true)? {
                // Native teardown detached the target. The remaining sender
                // console cannot provide further game input or output evidence.
                return Ok("native_exit_queued");
            }
            let prompt = if wake_count < MAX_EMPTY_READS && std::time::Instant::now() >= next_wake {
                match &last_woken {
                    Some(previous) => receipt::stable_prompt(target, &input, &output)?
                        .filter(|prompt| prompt.follows_empty_read(previous)),
                    None => exit_response_received(target, &input, &output, &before)?,
                }
            } else {
                None
            };
            response_seen |= prompt.is_some();
            if let Some(prompt) = prompt
                && reader.pending()?
            {
                if !verify(root, target, job, pinned, true)? {
                    // The native console detached. Never write to the sender's
                    // remaining console; the supervisor observes the owned Job.
                    return Ok("native_exit_queued");
                }
                if require_empty_prompt(target, &input, &output)? != prompt {
                    return Err(error(target, "Moria prompt changed before empty input"));
                }
                if std::time::Instant::now() >= deadline {
                    return Err(error(target, "Moria empty input deadline expired"));
                }
                // A new empty native read loops through the game's own Stop
                // flag without dispatching another command. Do not cancel the
                // read: some Windows versions report success with stale input.
                write_fixed_input_once(target, "\r", |events| {
                    let mut written = 0;
                    if unsafe {
                        WriteConsoleInputW(
                            input.0,
                            events.as_ptr(),
                            events.len() as u32,
                            &mut written,
                        )
                    } == 0
                    {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(written)
                    }
                })?;
                last_woken = Some(prompt);
                wake_count += 1;
                if wake_count < MAX_EMPTY_READS {
                    next_wake = std::time::Instant::now()
                        + deadline.saturating_duration_since(std::time::Instant::now())
                            / (MAX_EMPTY_READS - wake_count);
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(error(
                    target,
                    format!(
                        "Moria native exit did not complete (native_response_seen={response_seen}, empty_reads={wake_count}); exit is not replayed"
                    ),
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    // A native console receipt is independent of any build-specific memory
    // layout. The lifecycle caller still waits for the complete owned Job;
    // queued input is neither a save confirmation nor proof of process exit.
    Ok("native_exit_queued")
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(id: u32) -> *mut c_void;
    fn SetStdHandle(id: u32, handle: *mut c_void) -> i32;
    fn GetConsoleProcessList(pids: *mut u32, count: u32) -> u32;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *mut c_void,
        disposition: u32,
        flags: u32,
        template: *mut c_void,
    ) -> *mut c_void;
    fn GetConsoleScreenBufferInfo(handle: *mut c_void, info: *mut BufferInfo) -> i32;
    fn ReadConsoleOutputCharacterW(
        handle: *mut c_void,
        buffer: *mut u16,
        length: u32,
        start: Coord,
        read: *mut u32,
    ) -> i32;
    fn WriteConsoleInputW(
        handle: *mut c_void,
        events: *const InputRecord,
        length: u32,
        written: *mut u32,
    ) -> i32;
    fn GetNumberOfConsoleInputEvents(handle: *mut c_void, count: *mut u32) -> i32;
    fn CreateMutexW(security: *mut c_void, owner: i32, name: *const u16) -> *mut c_void;
    fn ReleaseMutex(handle: *mut c_void) -> i32;
}

#[cfg(test)]
#[path = "windows_moria_console_tests.rs"]
mod tests;
