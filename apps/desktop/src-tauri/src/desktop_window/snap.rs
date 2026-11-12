//! Maximize a completed title-bar drag at the top edge without resize borders.

use std::cell::Cell;
use std::io;

use tauri::WebviewWindow;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, HTCAPTION, IsIconic, IsZoomed, PostMessageW, SC_MAXIMIZE, WM_CANCELMODE,
    WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_MOVING, WM_NCDESTROY, WM_NCLBUTTONDOWN, WM_SYSCOMMAND,
};

const SUBCLASS_ID: usize = 1;
const TOP_EDGE_TOLERANCE: i64 = 8;

#[derive(Clone, Copy, Default)]
struct DragGesture {
    armed: bool,
    active: bool,
    moved: bool,
}

impl DragGesture {
    fn observe(&mut self, message: u32, hit: WPARAM, held: bool, escape: bool) -> bool {
        match message {
            WM_NCLBUTTONDOWN => {
                *self = Self {
                    armed: hit == HTCAPTION as usize && held,
                    ..Self::default()
                };
            }
            WM_ENTERSIZEMOVE => {
                self.active = std::mem::take(&mut self.armed);
                self.moved = false;
            }
            WM_MOVING => self.moved |= self.active,
            WM_CANCELMODE => *self = Self::default(),
            WM_EXITSIZEMOVE => {
                let completed = self.active && self.moved && !held && !escape;
                *self = Self::default();
                return completed;
            }
            _ => {}
        }
        false
    }
}

fn at_top_edge(cursor: POINT, monitor: RECT) -> bool {
    cursor.x >= monitor.left
        && cursor.x < monitor.right
        && (0..=TOP_EDGE_TOLERANCE).contains(&(i64::from(cursor.y) - i64::from(monitor.top)))
}

/// Queue the ordinary native maximize command, leaving the window style unchanged.
pub(crate) fn maximize_at_top_edge(hwnd: HWND, cursor: POINT) -> io::Result<bool> {
    // SAFETY: callers supply their live window handle; the monitor query only
    // reads the cursor's display and writes this initialized MONITORINFO.
    unsafe {
        if IsZoomed(hwnd) != 0 || IsIconic(hwnd) != 0 {
            return Ok(false);
        }
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return Err(io::Error::last_os_error());
        }
        if !at_top_edge(cursor, info.rcMonitor) {
            return Ok(false);
        }
        // Defer until Windows has returned from its move loop. Synchronous
        // maximization here could reenter the window's placement handler.
        if PostMessageW(hwnd, WM_SYSCOMMAND, SC_MAXIMIZE as usize, 0) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(true)
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        // SAFETY: install transfers this Box to this exact subclass. Remove
        // the callback before freeing it; recovered views get a new HWND/state.
        unsafe {
            RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
            drop(Box::from_raw(data as *mut Cell<DragGesture>));
            return DefSubclassProc(hwnd, message, wparam, lparam);
        }
    }
    if matches!(
        message,
        WM_NCLBUTTONDOWN | WM_ENTERSIZEMOVE | WM_MOVING | WM_CANCELMODE | WM_EXITSIZEMOVE
    ) {
        let held = matches!(message, WM_NCLBUTTONDOWN | WM_EXITSIZEMOVE)
            && super::drag::primary_button_held();
        // SAFETY: this only reads the Escape key's physical state.
        let escape =
            message == WM_EXITSIZEMOVE && unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE)) < 0 };
        let completed = {
            // SAFETY: this state is owned by the installed subclass and is
            // accessed only on its UI thread. No borrow crosses native dispatch.
            let state = unsafe { &*(data as *const Cell<DragGesture>) };
            let mut gesture = state.get();
            let completed = gesture.observe(message, wparam, held, escape);
            state.set(gesture);
            completed
        };
        if completed {
            let result = (|| {
                let mut cursor = POINT::default();
                // SAFETY: GetCursorPos writes one valid stack POINT.
                if unsafe { GetCursorPos(&mut cursor) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                maximize_at_top_edge(hwnd, cursor)
            })();
            if let Err(error) = result {
                eprintln!("maximize desktop after top-edge drag: {error}");
            }
        }
    }
    // SAFETY: preserve every other handler and the original window procedure.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// Must run on the window's UI thread, alongside the rest of window preparation.
pub(super) fn install(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|error| error.to_string())?.0.cast();
    install_on_ui_thread(hwnd)
}

fn install_on_ui_thread(hwnd: HWND) -> Result<(), String> {
    let mut existing = 0;
    // SAFETY: prepare calls this on the HWND's owning thread. Querying our
    // callback/ID pair avoids replacing its Box if preparation is repeated.
    if unsafe { GetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, &mut existing) } != 0 {
        return Ok(());
    }
    let state = Box::into_raw(Box::new(Cell::new(DragGesture::default())));
    // SAFETY: state remains allocated until WM_NCDESTROY or this failed install.
    if unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, state as usize) } == 0 {
        // SAFETY: ownership was not transferred on failure.
        unsafe { drop(Box::from_raw(state)) };
        return Err("Could not install desktop top-edge drag handler".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subclass_installation_is_idempotent_and_owned_by_the_window_lifetime() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_MAXIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
        };
        struct HiddenWindow(HWND);
        impl Drop for HiddenWindow {
            fn drop(&mut self) {
                // SAFETY: this test creates and destroys the HWND on one thread.
                unsafe { DestroyWindow(self.0) };
            }
        }
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        // SAFETY: STATIC is a built-in class and the string remains live for
        // the call. No WS_VISIBLE means this test never takes desktop focus.
        let window = HiddenWindow(unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPED | WS_SYSMENU | WS_MAXIMIZEBOX,
                0,
                0,
                100,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        });
        assert!(!window.0.is_null(), "{}", io::Error::last_os_error());
        install_on_ui_thread(window.0).unwrap();
        let mut first = 0;
        // SAFETY: these only read the test's live subclass reference data.
        assert_ne!(
            unsafe { GetWindowSubclass(window.0, Some(subclass_proc), SUBCLASS_ID, &mut first) },
            0
        );
        assert_ne!(first, 0);
        install_on_ui_thread(window.0).unwrap();
        let mut second = 0;
        assert_ne!(
            unsafe { GetWindowSubclass(window.0, Some(subclass_proc), SUBCLASS_ID, &mut second) },
            0
        );
        assert_eq!(
            first, second,
            "Repeated preparation must retain the same owned state"
        );
        // Drop sends WM_NCDESTROY through the real subclass and frees its state.
    }

    fn moving_caption() -> DragGesture {
        let mut gesture = DragGesture::default();
        assert!(!gesture.observe(WM_NCLBUTTONDOWN, HTCAPTION as usize, true, false));
        assert!(!gesture.observe(WM_ENTERSIZEMOVE, 0, true, false));
        assert!(!gesture.observe(WM_MOVING, 0, true, false));
        gesture
    }

    #[test]
    fn only_a_released_completed_caption_drag_can_snap_and_is_consumed_once() {
        let mut gesture = moving_caption();
        assert!(gesture.observe(WM_EXITSIZEMOVE, 0, false, false));
        assert!(!gesture.observe(WM_EXITSIZEMOVE, 0, false, false));
    }

    #[test]
    fn escape_cancellation_and_held_buttons_never_snap() {
        for (held, escape) in [(true, false), (true, true), (false, true)] {
            assert!(!moving_caption().observe(WM_EXITSIZEMOVE, 0, held, escape));
        }
        let mut cancelled = moving_caption();
        cancelled.observe(WM_CANCELMODE, 0, true, false);
        assert!(!cancelled.observe(WM_EXITSIZEMOVE, 0, false, false));
    }

    #[test]
    fn clicks_quick_releases_keyboard_moves_and_resize_loops_never_snap() {
        for (hit, held, moved) in [
            (HTCAPTION as usize, true, false),
            (HTCAPTION as usize, false, true),
            (10, true, true),
            (0, false, true),
        ] {
            let mut gesture = DragGesture::default();
            gesture.observe(WM_NCLBUTTONDOWN, hit, held, false);
            gesture.observe(WM_ENTERSIZEMOVE, 0, held, false);
            if moved {
                gesture.observe(WM_MOVING, 0, held, false);
            }
            assert!(!gesture.observe(WM_EXITSIZEMOVE, 0, false, false));
        }
    }

    #[test]
    fn top_edge_uses_monitor_coordinates_including_negative_and_stacked_displays() {
        for monitor in [
            RECT {
                left: 0,
                top: 0,
                right: 2560,
                bottom: 1440,
            },
            RECT {
                left: -1920,
                top: -1080,
                right: 0,
                bottom: 0,
            },
            RECT {
                left: 2560,
                top: 400,
                right: 6400,
                bottom: 2560,
            },
        ] {
            let x = monitor.left + (monitor.right - monitor.left) / 2;
            for offset in [0, 1, 8] {
                assert!(at_top_edge(
                    POINT {
                        x,
                        y: monitor.top + offset
                    },
                    monitor
                ));
            }
            for offset in [-1, 9, 100] {
                assert!(!at_top_edge(
                    POINT {
                        x,
                        y: monitor.top + offset
                    },
                    monitor
                ));
            }
            assert!(!at_top_edge(
                POINT {
                    x: monitor.left - 1,
                    y: monitor.top
                },
                monitor
            ));
            assert!(!at_top_edge(
                POINT {
                    x: monitor.right,
                    y: monitor.top
                },
                monitor
            ));
        }
    }
}
