use super::*;
use std::ffi::c_void;

const WM_CLOSE: u32 = 0x0010;
const GW_OWNER: u32 = 4;
const MAX_WINDOWS: usize = 4096;

#[derive(Clone, Debug)]
struct WindowTarget {
    handle: usize,
    pid: u32,
    thread_id: u32,
    class_name: String,
    has_owner: bool,
}

fn close_error(pid: u32, operation: &'static str, source: std::io::Error) -> RuntimeProcessError {
    RuntimeProcessError::WindowCloseProcess {
        pid,
        operation,
        source,
    }
}

fn eligible_window(window: &WindowTarget, owned: &HashSet<u32>) -> bool {
    window.handle != 0
        && window.handle != 0xffff
        && window.thread_id != 0
        && owned.contains(&window.pid)
        && !window.has_owner
        && !window.class_name.is_empty()
        && ![
            "IME",
            "MSCTFIME UI",
            "ConsoleWindowClass",
            "PseudoConsoleWindow",
        ]
        .iter()
        .any(|name| window.class_name.eq_ignore_ascii_case(name))
}

/// Queues WM_CLOSE only for the explicitly tracked process identity. Snapshot
/// ancestry cannot authorize a PID whose process may have changed since capture.
/// Success means delivery was queued, never that a game saved or exited; the
/// instance stop lifecycle separately waits for its complete owned tree.
pub(super) fn request_windows_window_close(
    root_pid: u32,
    root_identity: &ProcessIdentity,
    desktop: Option<&WindowsHiddenDesktop>,
) -> Result<(), RuntimeProcessError> {
    let desktop_handle = desktop.map(|item| item.handle);
    // User32 HWND inspection/posting across a private desktop is not reliable
    // from the manager's desktop. Never switch an existing UI/runtime thread.
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("runtime-window-close".into())
            .spawn_scoped(scope, move || {
                request_on_window_thread(root_pid, root_identity, desktop_handle)
            })
            .map_err(|source| close_error(root_pid, "spawn window close thread", source))?;
        worker.join().map_err(|_| {
            close_error(
                root_pid,
                "window close thread",
                std::io::Error::other("window close worker panicked"),
            )
        })?
    })
}

fn request_on_window_thread(
    root_pid: u32,
    root_identity: &ProcessIdentity,
    desktop: Option<usize>,
) -> Result<(), RuntimeProcessError> {
    if root_pid == std::process::id() {
        return Err(close_error(
            root_pid,
            "ownership verification",
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "cannot close runtime manager windows",
            ),
        ));
    }
    // Keep the process handle open through delivery: the verified PID cannot be
    // recycled into an unrelated process while its handle remains owned here.
    let handle = open_verified_windows_process(root_pid, root_identity, 0)?
        .ok_or(RuntimeProcessError::ProcessIdentityUnavailable { pid: root_pid })?;
    let owned = HashSet::from([root_pid]);
    let mut posted = 0;
    for target_desktop in desktop.into_iter().map(Some).chain(std::iter::once(None)) {
        let mut session = DesktopSession::enter(target_desktop)
            .map_err(|source| close_error(root_pid, "SetThreadDesktop", source))?;
        let windows = enumerate_windows(target_desktop)
            .map_err(|source| close_error(root_pid, "EnumDesktopWindows", source))?;
        posted += dispatch_close(
            &windows,
            &owned,
            |window| {
                if !handle.is_running()? {
                    return Ok(false);
                }
                if !process_identities_match(root_identity, &handle.identity()?) {
                    return Err(RuntimeProcessError::ProcessIdentityMismatch { pid: root_pid });
                }
                // HWND reuse can occur independently of PID reuse. Verify thread,
                // owner and class immediately before posting the fixed message.
                let actual = inspect_window(window.handle);
                Ok(actual.pid == window.pid
                    && actual.thread_id == window.thread_id
                    && actual.class_name == window.class_name
                    && eligible_window(&actual, &owned))
            },
            |window| {
                if unsafe { PostMessageW(window.handle as *mut c_void, WM_CLOSE, 0, 0) } == 0 {
                    Err(close_error(
                        window.pid,
                        "PostMessageW(WM_CLOSE)",
                        std::io::Error::last_os_error(),
                    ))
                } else {
                    Ok(())
                }
            },
        )?;
        session
            .restore()
            .map_err(|source| close_error(root_pid, "SetThreadDesktop(restore)", source))?;
    }
    if posted == 0 {
        return Err(close_error(
            root_pid,
            "window selection",
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no verified application window accepts WM_CLOSE",
            ),
        ));
    }
    Ok(())
}

fn dispatch_close(
    windows: &[WindowTarget],
    owned: &HashSet<u32>,
    mut verify: impl FnMut(&WindowTarget) -> Result<bool, RuntimeProcessError>,
    mut post: impl FnMut(&WindowTarget) -> Result<(), RuntimeProcessError>,
) -> Result<usize, RuntimeProcessError> {
    let mut posted = 0;
    for window in windows
        .iter()
        .filter(|window| eligible_window(window, owned))
    {
        if verify(window)? {
            post(window)?;
            posted += 1;
        }
    }
    Ok(posted)
}

struct DesktopSession {
    original: usize,
    switched: bool,
}

impl DesktopSession {
    fn enter(target: Option<usize>) -> std::io::Result<Self> {
        let original = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
        if original.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let switched = target.is_some_and(|target| target != original as usize);
        if switched && unsafe { SetThreadDesktop(target.unwrap_or(0) as *mut c_void) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            original: original as usize,
            switched,
        })
    }
    fn restore(&mut self) -> std::io::Result<()> {
        if self.switched {
            if unsafe { SetThreadDesktop(self.original as *mut c_void) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            self.switched = false;
        }
        Ok(())
    }
}

impl Drop for DesktopSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

struct Enumeration {
    windows: Vec<WindowTarget>,
    overflow: bool,
}

fn enumerate_windows(desktop: Option<usize>) -> std::io::Result<Vec<WindowTarget>> {
    let mut context = Enumeration {
        windows: Vec::new(),
        overflow: false,
    };
    unsafe {
        SetLastError(0);
    }
    let success = unsafe {
        EnumDesktopWindows(
            desktop.unwrap_or(0) as *mut c_void,
            Some(collect_window),
            (&mut context as *mut Enumeration) as isize,
        )
    };
    if context.overflow {
        return Err(std::io::Error::other(
            "desktop window enumeration exceeds close bound",
        ));
    }
    if success == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(0) {
            return Err(error);
        }
    }
    Ok(context.windows)
}

unsafe extern "system" fn collect_window(window: *mut c_void, parameter: isize) -> i32 {
    let context = unsafe { &mut *(parameter as *mut Enumeration) };
    if context.windows.len() == MAX_WINDOWS {
        context.overflow = true;
        return 0;
    }
    context.windows.push(inspect_window(window as usize));
    1
}

fn inspect_window(handle: usize) -> WindowTarget {
    let window = handle as *mut c_void;
    let mut pid = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(window, &mut pid) };
    let mut class = [0u16; 256];
    let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    WindowTarget {
        handle,
        pid,
        thread_id,
        class_name: String::from_utf16_lossy(&class[..length.max(0) as usize]),
        has_owner: !unsafe { GetWindow(window, GW_OWNER) }.is_null(),
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetThreadDesktop(thread_id: u32) -> *mut c_void;
    fn SetThreadDesktop(desktop: *mut c_void) -> i32;
    fn EnumDesktopWindows(
        desktop: *mut c_void,
        callback: Option<unsafe extern "system" fn(*mut c_void, isize) -> i32>,
        parameter: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
    fn GetClassNameW(window: *mut c_void, class: *mut u16, capacity: i32) -> i32;
    fn GetWindow(window: *mut c_void, command: u32) -> *mut c_void;
    fn PostMessageW(window: *mut c_void, message: u32, wparam: usize, lparam: isize) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn SetLastError(error: u32);
}

#[cfg(test)]
#[path = "windows_window_close_tests.rs"]
mod tests;
