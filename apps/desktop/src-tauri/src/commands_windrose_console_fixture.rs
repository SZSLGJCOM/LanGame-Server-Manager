use super::{INTERRUPTED, Ordering};
use std::ffi::c_void;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

pub(super) fn create() {
    let class = wide("FConsoleWindow");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let descriptor = WindowClass {
        style: 0,
        proc: Some(window_proc),
        class_extra: 0,
        window_extra: 0,
        instance,
        icon: std::ptr::null_mut(),
        cursor: std::ptr::null_mut(),
        background: std::ptr::null_mut(),
        menu: std::ptr::null(),
        class: class.as_ptr(),
    };
    assert_ne!(unsafe { RegisterClassW(&descriptor) }, 0);
    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        )
    };
    assert!(!window.is_null());
    for (name, id) in [("Edit", 0x8804), ("Button", 0x8805)] {
        assert!(
            !unsafe {
                CreateWindowExW(
                    0,
                    wide(name).as_ptr(),
                    wide("").as_ptr(),
                    0x40000000,
                    0,
                    0,
                    0,
                    0,
                    window,
                    id as *mut c_void,
                    instance,
                    std::ptr::null_mut(),
                )
            }
            .is_null()
        );
    }
}
pub(super) fn pump() {
    let mut message: Message = unsafe { std::mem::zeroed() };
    while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, 1) } != 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
unsafe extern "system" fn window_proc(
    window: *mut c_void,
    message: u32,
    wp: usize,
    lp: isize,
) -> isize {
    if message == 0x111 && wp == 0x8805 {
        let mut text = [0u16; 1024];
        let edit = unsafe { GetDlgItem(window, 0x8804) };
        let length = unsafe { GetWindowTextW(edit, text.as_mut_ptr(), 1024) };
        assert_eq!(String::from_utf16_lossy(&text[..length as usize]), "quit");
        let root = std::path::PathBuf::from(
            std::env::var_os("LANGAME_WINDROSE_BOOTSTRAP_FIXTURE").unwrap(),
        );
        std::fs::write(root.join("bootstrap-command"), b"quit").unwrap();
        INTERRUPTED.store(true, Ordering::SeqCst);
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wp, lp) }
}
#[repr(C)]
struct WindowClass {
    style: u32,
    proc: Option<unsafe extern "system" fn(*mut c_void, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: *mut c_void,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: *mut c_void,
    menu: *const u16,
    class: *const u16,
}
#[repr(C)]
struct Message {
    window: *mut c_void,
    message: u32,
    wp: usize,
    lp: isize,
    time: u32,
    x: i32,
    y: i32,
    private: u32,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WindowClass) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> *mut c_void;
    fn PeekMessageW(
        message: *mut Message,
        window: *mut c_void,
        min: u32,
        max: u32,
        remove: u32,
    ) -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn DispatchMessageW(message: *const Message) -> isize;
    fn GetWindowTextW(window: *mut c_void, text: *mut u16, capacity: i32) -> i32;
    fn GetDlgItem(window: *mut c_void, id: i32) -> *mut c_void;
    fn DefWindowProcW(window: *mut c_void, message: u32, wp: usize, lp: isize) -> isize;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}
