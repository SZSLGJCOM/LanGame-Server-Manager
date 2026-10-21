//! Deliver the publisher's `exit` command to one owned Moria console.
//! This is a shutdown queue receipt; the supervisor still verifies whole-Job exit.

use super::*;
use std::ffi::c_void;
use windows_sys::Win32::System::JobObjects::IsProcessInJob;

const MAX_MEMBERS: usize = 64;
const SHIPPING: &str = "MoriaServer-Win64-Shipping.exe";

#[path = "windows_moria_console_helper.rs"]
mod helper;
pub use helper::{run_if_requested, run_worker};

#[path = "windows_moria_console_shutdown.rs"]
mod shutdown;

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
        if !belongs(&member.handle, job)?
            || !process_identities_match(&member.identity, &member.handle.identity()?)
        {
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
) -> Result<(), RuntimeProcessError> {
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
    Ok(())
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

fn require_empty_prompt(
    pid: u32,
    input: &Handle,
    output: &Handle,
) -> Result<(), RuntimeProcessError> {
    let mut pending = 0;
    let mut before = BufferInfo::default();
    if unsafe { GetNumberOfConsoleInputEvents(input.0, &mut pending) } == 0
        || pending != 0
        || unsafe { GetConsoleScreenBufferInfo(output.0, &mut before) } == 0
        || before.cursor.x != 2
        || before.cursor.y < 0
    {
        return Err(error(
            pid,
            "Moria console has pending input or no complete prompt",
        ));
    }
    // Read only the two prompt cells, never console history or join credentials.
    let mut prompt = [0u16; 2];
    let mut read = 0;
    let mut after = BufferInfo::default();
    if unsafe {
        ReadConsoleOutputCharacterW(
            output.0,
            prompt.as_mut_ptr(),
            2,
            Coord {
                x: 0,
                y: before.cursor.y,
            },
            &mut read,
        )
    } == 0
        || read != 2
        || prompt != [b'>' as u16, b' ' as u16]
        || unsafe { GetConsoleScreenBufferInfo(output.0, &mut after) } == 0
        || before.cursor != after.cursor
        || before.size != after.size
    {
        return Err(error(pid, "Moria console prompt changed before delivery"));
    }
    Ok(())
}

fn write_exit_once(
    pid: u32,
    write: impl FnOnce(&[InputRecord]) -> Result<u32, std::io::Error>,
) -> Result<(), RuntimeProcessError> {
    let events: Vec<_> = "exit\r"
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
            "Moria exit input delivery failed or is uncertain; no retry",
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
    let shutdown = shutdown::ShutdownRead::prepare(target, target_identity, job, deadline)?;
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
    require_empty_prompt(target, &input, &output)?;
    shutdown.before_write()?;
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
    shutdown.finish()
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
