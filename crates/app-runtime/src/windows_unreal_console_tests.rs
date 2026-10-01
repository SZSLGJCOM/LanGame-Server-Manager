use super::*;
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const ENTRY: &str = "windows_unreal_console::tests::unreal_console_fixture";
const ENV_ROOT: &str = "LGSM_UNREAL_CONSOLE_FIXTURE";
const ENV_MODE: &str = "LGSM_UNREAL_CONSOLE_MODE";
static OUTPUT: OnceLock<PathBuf> = OnceLock::new();
static MODE: OnceLock<String> = OnceLock::new();

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "Unreal console fixture timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Fixture {
    child: RuntimeChild,
    desktop: Option<WindowsHiddenDesktop>,
    identity: ProcessIdentity,
    root: PathBuf,
}
impl Fixture {
    fn start(mode: &str) -> Self {
        let root = crate::test_support::unique_test_root();
        fs::create_dir_all(&root).unwrap();
        let executable = std::env::current_exe().unwrap();
        let args = ["--exact", ENTRY, "--ignored", "--nocapture"].map(String::from);
        let environment = BTreeMap::from([
            (ENV_ROOT.into(), root.to_string_lossy().into_owned()),
            (ENV_MODE.into(), mode.into()),
        ]);
        let log = File::create(root.join("fixture.log")).unwrap();
        let (child, desktop) = spawn_hidden_desktop_process(
            &SpawnCommand {
                executable: &executable.to_string_lossy(),
                args: &args,
                working_directory: &root,
                environment: &environment,
            },
            log.try_clone().unwrap(),
            log,
            DETACHED_PROCESS,
            None,
        )
        .unwrap();
        let identity = inspect_process_identity(child.id()).unwrap().unwrap();
        let fixture = Self {
            child,
            desktop,
            identity,
            root,
        };
        wait_until(|| fixture.root.join("ready").exists());
        fixture
    }
    fn send(&self, command: &str) -> Result<(), RuntimeProcessError> {
        request(&self.child, &self.identity, self.desktop.as_ref(), command)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only synthetic disposable test children may be forcibly cleaned up.
        self.child.finish_process_tree().unwrap();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn unreal_console_delivers_exact_text_and_quit_to_owned_private_window() {
    let mut fixture = Fixture::start("normal");
    let mut stale = fixture.identity.clone();
    stale.creation_time += 1;
    assert!(request(&fixture.child, &stale, fixture.desktop.as_ref(), "quit").is_err());
    assert!(request(&fixture.child, &fixture.identity, None, "quit").is_err());
    for invalid in ["", "a\nb", "a\0b", &"x".repeat(1024)] {
        assert!(fixture.send(invalid).is_err());
    }
    assert!(!fixture.root.join("commands").exists());
    fixture.send("stat fps").unwrap();
    wait_until(|| fixture.root.join("commands").exists());
    assert_eq!(
        fs::read_to_string(fixture.root.join("commands")).unwrap(),
        "stat fps\n"
    );
    fixture.send("stat fps").unwrap();
    assert_eq!(
        fs::read_to_string(fixture.root.join("commands")).unwrap(),
        "stat fps\nstat fps\n"
    );
    fixture.send("quit").unwrap();
    wait_until(|| fixture.child.try_wait().unwrap().is_some());
    assert!(fixture.child.try_wait().unwrap().unwrap().success());
}

#[test]
fn unreal_console_rejects_ambiguous_windows_and_wrong_controls() {
    for mode in ["ambiguous", "wrong-control"] {
        let fixture = Fixture::start(mode);
        assert!(fixture.send("quit").is_err());
        assert!(!fixture.root.join("commands").exists());
        assert!(process_matches_identity(fixture.child.id(), &fixture.identity).unwrap());
    }
}

#[test]
fn unreal_console_delivery_timeout_is_bounded_and_never_retried() {
    let mut fixture = Fixture::start("hang");
    let started = Instant::now();
    assert!(fixture.send("quit").is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
    wait_until(|| fixture.child.try_wait().unwrap().is_some());
    assert_eq!(
        fs::read_to_string(fixture.root.join("commands")).unwrap(),
        "quit\n"
    );
}

#[test]
fn unreal_console_selects_owned_descendant_and_rejects_other_job_desktop() {
    let mut target = Fixture::start("launcher");
    let mut other = Fixture::start("normal");
    assert!(
        request(
            &target.child,
            &target.identity,
            other.desktop.as_ref(),
            "quit"
        )
        .is_err()
    );
    assert!(!other.root.join("commands").exists());
    target.send("quit").unwrap();
    wait_until(|| target.child.try_wait().unwrap().is_some());
    assert!(target.child.try_wait().unwrap().unwrap().success());
    assert!(process_matches_identity(other.child.id(), &other.identity).unwrap());
    assert!(!other.root.join("commands").exists());
    other.send("quit").unwrap();
    wait_until(|| other.child.try_wait().unwrap().is_some());
}

#[test]
#[ignore = "synthetic child entry, never launches a game"]
fn unreal_console_fixture() {
    let Some(root) = std::env::var_os(ENV_ROOT) else {
        return;
    };
    let root = PathBuf::from(root);
    let mode = std::env::var(ENV_MODE).unwrap();
    if mode == "launcher" {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", ENTRY, "--ignored", "--nocapture"])
            .env(ENV_MODE, "normal")
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(child.wait().unwrap().success());
        return;
    }
    OUTPUT.set(root.join("commands")).unwrap();
    MODE.set(mode.clone()).unwrap();
    let class = wide_null("FConsoleWindow");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let descriptor = WindowClass {
        style: 0,
        window_proc: Some(window_proc),
        class_extra: 0,
        window_extra: 0,
        instance,
        icon: std::ptr::null_mut(),
        cursor: std::ptr::null_mut(),
        background: std::ptr::null_mut(),
        menu_name: std::ptr::null(),
        class_name: class.as_ptr(),
    };
    assert_ne!(unsafe { RegisterClassW(&descriptor) }, 0);
    for _ in 0..if mode == "ambiguous" { 2 } else { 1 } {
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
        for (name, id) in [
            ("Edit", 0x8804),
            (
                "Button",
                if mode == "wrong-control" {
                    0x8806
                } else {
                    0x8805
                },
            ),
        ] {
            let child_class = wide_null(name);
            assert!(
                !unsafe {
                    CreateWindowExW(
                        0,
                        child_class.as_ptr(),
                        wide_null("").as_ptr(),
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
    fs::write(root.join("ready"), b"ready").unwrap();
    let mut message: WindowMessage = unsafe { std::mem::zeroed() };
    loop {
        let received = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        assert!(received >= 0);
        if received == 0 {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: *mut c_void,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if message == 0x111 && wparam == 0x8805 {
        let edit = unsafe { GetDlgItem(window, 0x8804) };
        let mut text = [0u16; 1024];
        let length = unsafe { GetWindowTextW(edit, text.as_mut_ptr(), 1024) };
        let command = String::from_utf16_lossy(&text[..length as usize]);
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(OUTPUT.get().unwrap())
            .unwrap();
        writeln!(file, "{command}").unwrap();
        if MODE.get().unwrap() == "hang" {
            std::thread::sleep(Duration::from_secs(6));
        }
        if command == "quit" {
            unsafe {
                DestroyWindow(window);
            }
        }
        return 0;
    }
    if message == 2 {
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
#[repr(C)]
struct WindowClass {
    style: u32,
    window_proc: Option<unsafe extern "system" fn(*mut c_void, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: *mut c_void,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: *mut c_void,
    menu_name: *const u16,
    class_name: *const u16,
}
#[repr(C)]
struct WindowMessage {
    window: *mut c_void,
    message: u32,
    wparam: usize,
    lparam: isize,
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
        width: i32,
        height: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> *mut c_void;
    fn GetMessageW(message: *mut WindowMessage, window: *mut c_void, min: u32, max: u32) -> i32;
    fn TranslateMessage(message: *const WindowMessage) -> i32;
    fn DispatchMessageW(message: *const WindowMessage) -> isize;
    fn GetWindowTextW(window: *mut c_void, text: *mut u16, capacity: i32) -> i32;
    fn GetDlgItem(window: *mut c_void, id: i32) -> *mut c_void;
    fn DestroyWindow(window: *mut c_void) -> i32;
    fn PostQuitMessage(code: i32);
    fn DefWindowProcW(window: *mut c_void, message: u32, wp: usize, lp: isize) -> isize;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}
