use std::ffi::c_void;
use std::time::{Duration, Instant};

use app_core::ProcessIdentity;

use super::super::{
    NativePlayerConsoleError as Error,
    frame::{Capture, Screen},
};

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

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        crate::close_handle(self.0);
    }
}
struct Attachment;
impl Drop for Attachment {
    fn drop(&mut self) {
        unsafe {
            crate::FreeConsole();
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

pub(super) fn capture(
    pid: u32,
    identity: &ProcessIdentity,
    nonce: &str,
    timeout: Duration,
) -> Result<String, Error> {
    let deadline = Instant::now() + timeout;
    let root = crate::open_verified_windows_process(pid, identity, 0)
        .map_err(|_| Error::ProcessUnavailable)?
        .ok_or(Error::ProcessUnavailable)?;
    let (pid, identity) = console_process(pid, identity)?;
    let process = crate::open_verified_windows_process(pid, &identity, 0)
        .map_err(|_| Error::ProcessUnavailable)?
        .ok_or(Error::ProcessUnavailable)?;
    let name: Vec<_> = format!(
        "Local\\LanGame-MoriaConsole-{pid}-{}",
        identity.creation_time
    )
    .encode_utf16()
    .chain([0])
    .collect();
    let mutex = Handle(unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) });
    if mutex.0.is_null() {
        return Err(Error::Io);
    }
    match unsafe { crate::WaitForSingleObject(mutex.0, 0) } {
        0 | 0x80 => {}
        258 => return Err(Error::Timeout),
        _ => return Err(Error::Io),
    }
    let _lock = QueryLock(mutex);
    unsafe {
        crate::FreeConsole();
    }
    if unsafe { crate::AttachConsole(pid) } == 0 {
        return Err(Error::Io);
    }
    let _attachment = Attachment;
    if unsafe { crate::SetConsoleCtrlHandler(std::ptr::null_mut(), 1) } == 0 {
        return Err(Error::Io);
    }
    verify_console(pid, &process)?;
    let input = open_console("CONIN$", 0xc0000000)?;
    let output = open_console("CONOUT$", 0x80000000)?;
    let initial = read_screen(output.0)?;
    // Never append a query to an operator's unfinished command or consume their
    // queued keystrokes. A prompt and an empty input queue are required.
    let last = initial.rows.last().ok_or(Error::Incomplete)?;
    if String::from_utf16(&last[..initial.cursor_x]).map_err(|_| Error::Incomplete)? != "> " {
        return Err(Error::Incomplete);
    }
    let mut pending = 0;
    if unsafe { GetNumberOfConsoleInputEvents(input.0, &mut pending) } == 0 || pending != 0 {
        return Err(Error::Incomplete);
    }
    let mut capture = Capture::new(nonce, &initial)?;
    let commands =
        format!("LGM_PLAYER_QUERY_BEGIN_{nonce}\rplayers\rLGM_PLAYER_QUERY_END_{nonce}\r");
    let events: Vec<_> = commands
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
                    ..KeyEvent::default()
                },
            })
        })
        .collect();
    let mut written = 0;
    if !root.is_running().map_err(|_| Error::ProcessUnavailable)? {
        return Err(Error::ProcessUnavailable);
    }
    verify_console(pid, &process)?;
    if unsafe { WriteConsoleInputW(input.0, events.as_ptr(), events.len() as u32, &mut written) }
        == 0
        || written as usize != events.len()
    {
        return Err(Error::Io);
    }
    while Instant::now() < deadline {
        if !root.is_running().map_err(|_| Error::ProcessUnavailable)? {
            return Err(Error::ProcessUnavailable);
        }
        verify_console(pid, &process)?;
        if let Some(text) = capture.observe(&read_screen(output.0)?)? {
            if !root.is_running().map_err(|_| Error::ProcessUnavailable)? {
                return Err(Error::ProcessUnavailable);
            }
            verify_console(pid, &process)?;
            return Ok(text);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(Error::Timeout)
}

fn console_process(pid: u32, identity: &ProcessIdentity) -> Result<(u32, ProcessIdentity), Error> {
    let image = std::path::Path::new(&identity.image_path);
    if image
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case("MoriaServer-Win64-Shipping.exe"))
    {
        return Ok((pid, identity.clone()));
    }
    let expected_child = image
        .parent()
        .ok_or(Error::ProcessUnavailable)?
        .join("Moria/Binaries/Win64/MoriaServer-Win64-Shipping.exe");
    let expected_child = crate::normalized_process_image_path(&expected_child.to_string_lossy());
    // The registered primary is the Unreal bootstrap executable. Resolve only
    // its creation-time-verified descendants and the exact installed child path;
    // an unrelated instance with the same filename is never a candidate.
    let descendants = crate::load_windows_process_descendants(pid, identity)
        .map_err(|_| Error::ProcessUnavailable)?;
    let mut candidates = descendants.into_iter().filter_map(|process| {
        process
            .identity
            .filter(|identity| {
                crate::normalized_process_image_path(&identity.image_path) == expected_child
            })
            .map(|identity| (process.process_id, identity))
    });
    let process = candidates.next().ok_or(Error::ProcessUnavailable)?;
    if candidates.next().is_some() {
        return Err(Error::ProcessUnavailable);
    }
    Ok(process)
}

fn verify_console(pid: u32, process: &crate::WindowsProcessHandle) -> Result<(), Error> {
    if !process
        .is_running()
        .map_err(|_| Error::ProcessUnavailable)?
    {
        return Err(Error::ProcessUnavailable);
    }
    let mut pids = [0_u32; 64];
    let count = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) } as usize;
    if count == 0 || count > pids.len() || !pids[..count].contains(&pid) {
        return Err(Error::ProcessUnavailable);
    }
    Ok(())
}

fn open_console(name: &str, access: u32) -> Result<Handle, Error> {
    let name: Vec<_> = name.encode_utf16().chain([0]).collect();
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
        Err(Error::Io)
    } else {
        Ok(Handle(handle))
    }
}

fn read_screen(handle: *mut c_void) -> Result<Screen, Error> {
    let mut before = BufferInfo::default();
    if unsafe { GetConsoleScreenBufferInfo(handle, &mut before) } == 0 {
        return Err(Error::Io);
    }
    if before.size.x < 80
        || before.size.x > 512
        || before.size.y < 16
        || before.cursor.x < 0
        || before.cursor.x >= before.size.x
        || before.cursor.y < 0
        || before.cursor.y >= before.size.y
    {
        return Err(Error::CaptureLimit);
    }
    // Only the most recent 256 physical rows can participate in this response;
    // never copy the entire accumulated console history or its join password.
    let start = (before.cursor.y - 255).max(0);
    let length = usize::from(before.size.x as u16) * (before.cursor.y - start + 1) as usize;
    let mut cells = vec![0_u16; length];
    let mut count = 0;
    if unsafe {
        ReadConsoleOutputCharacterW(
            handle,
            cells.as_mut_ptr(),
            length as u32,
            Coord { x: 0, y: start },
            &mut count,
        )
    } == 0
        || count as usize != length
    {
        return Err(Error::Incomplete);
    }
    let mut after = BufferInfo::default();
    if unsafe { GetConsoleScreenBufferInfo(handle, &mut after) } == 0 {
        return Err(Error::Io);
    }
    if before.size != after.size || before.cursor != after.cursor {
        return Err(Error::Incomplete);
    }
    Ok(Screen {
        width: before.size.x as usize,
        height: before.size.y as usize,
        cursor_x: before.cursor.x as usize,
        rows: cells
            .chunks_exact(before.size.x as usize)
            .map(<[u16]>::to_vec)
            .collect(),
    })
}

#[link(name = "kernel32")]
unsafe extern "system" {
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
    fn GetConsoleProcessList(pids: *mut u32, count: u32) -> u32;
    fn CreateMutexW(security: *mut c_void, owner: i32, name: *const u16) -> *mut c_void;
    fn ReleaseMutex(handle: *mut c_void) -> i32;
    pub(super) fn GetStdHandle(handle: u32) -> *mut c_void;
    pub(super) fn GetFileType(handle: *mut c_void) -> u32;
    pub(super) fn PeekNamedPipe(
        pipe: *mut c_void,
        buffer: *mut c_void,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        remaining: *mut u32,
    ) -> i32;
    pub(super) fn ReadFile(
        handle: *mut c_void,
        buffer: *mut c_void,
        count: u32,
        read: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_console_ffi_matches_windows_console_abi() {
        assert_eq!(std::mem::size_of::<Coord>(), 4);
        assert_eq!(std::mem::size_of::<Rect>(), 8);
        assert_eq!(std::mem::size_of::<BufferInfo>(), 22);
        assert_eq!(std::mem::offset_of!(BufferInfo, window), 10);
        assert_eq!(std::mem::size_of::<KeyEvent>(), 16);
        assert_eq!(std::mem::offset_of!(KeyEvent, character), 10);
        assert_eq!(std::mem::size_of::<InputRecord>(), 20);
        assert_eq!(std::mem::offset_of!(InputRecord, event), 4);
    }
}
