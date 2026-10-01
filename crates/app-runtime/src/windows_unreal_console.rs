use super::*;
use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::JobObjects::IsProcessInJob;

const EDIT_ID: i32 = 0x8804;
const RUN_ID: i32 = 0x8805;
const MAX_WINDOWS: usize = 4096;
const MAX_COMMAND_UNITS: usize = 1023;
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(3);

fn error(pid: u32, message: impl Into<String>) -> RuntimeProcessError {
    RuntimeProcessError::WriteTrackedProcessStdin {
        pid,
        source: std::io::Error::other(message.into()),
    }
}

/// The launch Job and private desktop, not process names or ancestry snapshots,
/// authorize targets. Every candidate process remains pinned through delivery.
pub(super) fn request(
    child: &RuntimeChild,
    root_identity: &ProcessIdentity,
    desktop: Option<&WindowsHiddenDesktop>,
    command: &str,
) -> Result<(), RuntimeProcessError> {
    let pid = child.id();
    if command.trim().is_empty()
        || command.chars().any(char::is_control)
        || command.encode_utf16().count() > MAX_COMMAND_UNITS
    {
        return Err(error(
            pid,
            "Unreal console requires one nonempty line of at most 1023 UTF-16 units",
        ));
    }
    let RuntimeChild::Windows(native) = child else {
        return Err(error(
            pid,
            "Unreal console requires the native launch owner",
        ));
    };
    let job = native
        .job
        .as_ref()
        .ok_or_else(|| error(pid, "Unreal console requires an owned Job"))?;
    let desktop = desktop.ok_or_else(|| error(pid, "Unreal console requires a private desktop"))?;
    let actual =
        query_windows_process_identity_from_handle(pid, native.process_handle as *mut c_void)?;
    if !process_identities_match(root_identity, &actual) {
        return Err(RuntimeProcessError::ProcessIdentityMismatch { pid });
    }
    let job_handle = job.as_raw() as usize;
    let desktop_handle = desktop.handle;
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("runtime-unreal-console".into())
            .spawn_scoped(scope, move || {
                deliver(pid, job_handle, desktop_handle, command)
            })
            .map_err(|cause| error(pid, format!("cannot start native console worker: {cause}")))?;
        worker
            .join()
            .map_err(|_| error(pid, "native console worker panicked"))?
    })
}

#[derive(Clone, PartialEq, Eq)]
struct Window {
    handle: usize,
    pid: u32,
    thread: u32,
    class: String,
    parent: usize,
}
fn inspect(handle: usize) -> Window {
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(handle as *mut c_void, &mut pid) };
    let mut class = [0u16; 256];
    let len = unsafe { GetClassNameW(handle as *mut c_void, class.as_mut_ptr(), 256) };
    Window {
        handle,
        pid,
        thread,
        class: String::from_utf16_lossy(&class[..len.clamp(0, 255) as usize]),
        parent: unsafe { GetParent(handle as *mut c_void) } as usize,
    }
}
struct Enumeration {
    windows: Vec<Window>,
    overflow: bool,
}
unsafe extern "system" fn collect(window: *mut c_void, context: isize) -> i32 {
    let state = unsafe { &mut *(context as *mut Enumeration) };
    if state.windows.len() == MAX_WINDOWS {
        state.overflow = true;
        return 0;
    }
    state.windows.push(inspect(window as usize));
    1
}
fn member(handle: &WindowsProcessHandle, job: usize) -> Result<bool, RuntimeProcessError> {
    let mut belongs = 0;
    if unsafe { IsProcessInJob(handle.raw, job as *mut c_void, &mut belongs) } == 0 {
        return Err(error(
            handle.pid,
            format!(
                "cannot verify Job membership: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    Ok(belongs != 0 && handle.is_running()?)
}
fn control(window: &Window, id: i32, class: &str) -> Result<Window, RuntimeProcessError> {
    let handle = unsafe { GetDlgItem(window.handle as *mut c_void, id) } as usize;
    let child = inspect(handle);
    if handle == 0
        || child.pid != window.pid
        || child.thread != window.thread
        || child.parent != window.handle
        || child.class != class
    {
        return Err(error(window.pid, "native console control identity differs"));
    }
    Ok(child)
}
fn verify(
    window: &Window,
    edit: &Window,
    button: &Window,
    process: &WindowsProcessHandle,
    identity: &ProcessIdentity,
    job: usize,
) -> Result<(), RuntimeProcessError> {
    if !member(process, job)?
        || !process_identities_match(identity, &process.identity()?)
        || inspect(window.handle) != *window
        || control(window, EDIT_ID, "Edit")? != *edit
        || control(window, RUN_ID, "Button")? != *button
    {
        return Err(error(
            window.pid,
            "native console target changed before delivery",
        ));
    }
    Ok(())
}
fn send(
    window: usize,
    message: u32,
    wp: usize,
    lp: isize,
    deadline: Instant,
    pid: u32,
) -> Result<usize, RuntimeProcessError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(error(pid, "native console delivery deadline elapsed"));
    }
    let mut result = 0;
    let sent = unsafe {
        SendMessageTimeoutW(
            window as *mut c_void,
            message,
            wp,
            lp,
            0x0001 | 0x0002 | 0x0020,
            remaining.as_millis().clamp(1, u32::MAX as u128) as u32,
            &mut result,
        )
    };
    if sent == 0 {
        return Err(error(
            pid,
            format!(
                "native console delivery failed or is uncertain; no retry: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    Ok(result)
}
fn deliver(
    root_pid: u32,
    job: usize,
    desktop: usize,
    command: &str,
) -> Result<(), RuntimeProcessError> {
    // Use a fresh worker so the application's UI never changes desktop.
    if unsafe { SetThreadDesktop(desktop as *mut c_void) } == 0 {
        return Err(error(
            root_pid,
            format!(
                "cannot enter private desktop: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let mut enumeration = Enumeration {
        windows: Vec::new(),
        overflow: false,
    };
    let ok = unsafe {
        EnumDesktopWindows(
            desktop as *mut c_void,
            Some(collect),
            &mut enumeration as *mut _ as isize,
        )
    };
    if ok == 0 || enumeration.overflow {
        return Err(error(
            root_pid,
            "private desktop enumeration failed or exceeded its bound",
        ));
    }
    let mut targets = Vec::new();
    for window in enumeration.windows {
        if window.class != "FConsoleWindow"
            || window.parent != 0
            || window.pid == 0
            || window.thread == 0
        {
            continue;
        }
        let Some(process) = WindowsProcessHandle::open(window.pid, 0)? else {
            continue;
        };
        if !member(&process, job)? {
            continue;
        }
        let identity = process.identity()?;
        targets.push((window, process, identity));
    }
    if targets.len() != 1 {
        return Err(error(
            root_pid,
            format!("expected one owned native console; found {}", targets.len()),
        ));
    }
    let (window, process, identity) = targets.pop().unwrap();
    let edit = control(&window, EDIT_ID, "Edit")?;
    let button = control(&window, RUN_ID, "Button")?;
    let deadline = Instant::now() + DELIVERY_TIMEOUT;
    verify(&window, &edit, &button, &process, &identity, job)?;
    let text: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    if send(
        edit.handle,
        0x000C,
        0,
        text.as_ptr() as isize,
        deadline,
        window.pid,
    )? == 0
    {
        return Err(error(window.pid, "native console rejected command text"));
    }
    let mut accepted = [0u16; MAX_COMMAND_UNITS + 1];
    let count = send(
        edit.handle,
        0x000D,
        accepted.len(),
        accepted.as_mut_ptr() as isize,
        deadline,
        window.pid,
    )?;
    if count != text.len() - 1 || accepted.get(..count) != Some(&text[..text.len() - 1]) {
        return Err(error(
            window.pid,
            "native console did not accept the complete command",
        ));
    }
    verify(&window, &edit, &button, &process, &identity, job)?;
    // One BN_CLICKED notification invokes Unreal's game-thread command queue.
    // WM_CLOSE and console signals use different exit paths and are not fallback.
    send(
        window.handle,
        0x0111,
        RUN_ID as usize,
        button.handle as isize,
        deadline,
        window.pid,
    )?;
    Ok(())
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetThreadDesktop(desktop: *mut c_void) -> i32;
    fn EnumDesktopWindows(
        desktop: *mut c_void,
        callback: Option<unsafe extern "system" fn(*mut c_void, isize) -> i32>,
        context: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
    fn GetClassNameW(window: *mut c_void, class: *mut u16, count: i32) -> i32;
    fn GetParent(window: *mut c_void) -> *mut c_void;
    fn GetDlgItem(window: *mut c_void, id: i32) -> *mut c_void;
    fn SendMessageTimeoutW(
        window: *mut c_void,
        message: u32,
        wp: usize,
        lp: isize,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> isize;
}

#[cfg(test)]
#[path = "windows_unreal_console_tests.rs"]
mod tests;
