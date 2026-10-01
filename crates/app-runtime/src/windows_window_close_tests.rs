use super::*;
use std::os::windows::process::CommandExt;
use std::sync::OnceLock;
use std::time::Instant;

fn window(handle: usize, pid: u32, class_name: &str) -> WindowTarget {
    WindowTarget {
        handle,
        pid,
        thread_id: 7,
        class_name: class_name.into(),
        has_owner: false,
    }
}

#[test]
fn window_close_filters_foreign_helper_and_owned_popup_windows() {
    let owned = HashSet::from([42]);
    let mut popup = window(6, 42, "Dialog");
    popup.has_owner = true;
    let windows = vec![
        window(1, 42, "Unity.BatchModeWindow"),
        window(2, 43, "Unity.BatchModeWindow"),
        window(3, 42, "IME"),
        window(4, 42, "MSCTFIME UI"),
        window(5, 42, "ConsoleWindowClass"),
        popup,
    ];
    let mut posted = Vec::new();
    assert_eq!(
        dispatch_close(
            &windows,
            &owned,
            |_| Ok(true),
            |window| {
                posted.push(window.handle);
                Ok(())
            }
        )
        .unwrap(),
        1
    );
    assert_eq!(posted, [1]);
    assert_eq!(
        dispatch_close(
            &windows[1..],
            &owned,
            |_| panic!("ineligible window reached verifier"),
            |_| panic!("helper/foreign window received close")
        )
        .unwrap(),
        0
    );
}

#[test]
fn window_close_rejects_changed_process_identity_before_delivery() {
    let windows = [window(1, 42, "Unity.BatchModeWindow")];
    let error = dispatch_close(
        &windows,
        &HashSet::from([42]),
        |_| Err(RuntimeProcessError::ProcessIdentityMismatch { pid: 42 }),
        |_| panic!("identity mismatch must not post WM_CLOSE"),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RuntimeProcessError::ProcessIdentityMismatch { pid: 42 }
    ));
    assert_eq!(
        dispatch_close(
            &windows,
            &HashSet::from([42]),
            |_| Ok(false),
            |_| panic!("reused HWND must not receive WM_CLOSE")
        )
        .unwrap(),
        0
    );
}

#[test]
fn window_close_propagates_message_delivery_failure() {
    let error = dispatch_close(
        &[window(1, 42, "Unity.BatchModeWindow")],
        &HashSet::from([42]),
        |_| Ok(true),
        |_| {
            Err(close_error(
                42,
                "PostMessageW(WM_CLOSE)",
                std::io::Error::from_raw_os_error(5),
            ))
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, RuntimeProcessError::WindowCloseProcess { source, .. } if source.raw_os_error() == Some(5))
    );
}

const FIXTURE_TEST: &str = "windows_window_close::tests::isolated_window_close_fixture";
const FIXTURE_ROOT: &str = "LGSM_WINDOW_CLOSE_FIXTURE_ROOT";
const FIXTURE_ROLE: &str = "LGSM_WINDOW_CLOSE_FIXTURE_ROLE";
static WINDOW_OUTPUT: OnceLock<PathBuf> = OnceLock::new();

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "isolated window fixture timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct FixtureChild(Child);
impl Drop for FixtureChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            unsafe {
                WaitForSingleObject(self.0.as_raw_handle(), 1000);
            }
        }
    }
}

fn fixture_command(root: &Path, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", FIXTURE_TEST, "--ignored", "--nocapture"])
        .env(FIXTURE_ROOT, root)
        .env(FIXTURE_ROLE, role)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

#[test]
#[ignore = "child entry: synthetic windows inside an owned private desktop only"]
fn isolated_window_close_fixture() {
    let Ok(role) = std::env::var(FIXTURE_ROLE) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os(FIXTURE_ROOT).unwrap());
    if role == "launcher" {
        let mut target = FixtureChild(fixture_command(&root, "target").spawn().unwrap());
        let mut sentinel = FixtureChild(fixture_command(&root, "sentinel").spawn().unwrap());
        fs::write(
            root.join("pids.json"),
            serde_json::to_vec(&[target.0.id(), sentinel.0.id()]).unwrap(),
        )
        .unwrap();
        wait_until(|| {
            target.0.try_wait().unwrap().is_some() && sentinel.0.try_wait().unwrap().is_some()
        });
        assert!(target.0.try_wait().unwrap().unwrap().success());
        assert!(sentinel.0.try_wait().unwrap().unwrap().success());
        return;
    }
    assert!(["target", "sentinel"].contains(&role.as_str()));
    WINDOW_OUTPUT
        .set(root.join(format!("{role}.closed")))
        .unwrap();
    let class = wide_null("Unity.BatchModeWindow");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let descriptor = WindowClass {
        style: 0,
        window_proc: Some(fixture_window_proc),
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
    fs::write(root.join(format!("{role}.ready")), b"ready").unwrap();
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

#[test]
fn window_close_hidden_desktop_closes_verified_target_and_preserves_sibling() {
    let root = crate::test_support::unique_test_root();
    fs::create_dir_all(&root).unwrap();
    let executable = std::env::current_exe().unwrap();
    let args = ["--exact", FIXTURE_TEST, "--ignored", "--nocapture"].map(String::from);
    let environment = BTreeMap::from([
        (FIXTURE_ROOT.into(), root.to_string_lossy().into_owned()),
        (FIXTURE_ROLE.into(), "launcher".into()),
    ]);
    let log = File::create(root.join("fixture.log")).unwrap();
    let (mut launcher, desktop) = spawn_hidden_desktop_process(
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
    wait_until(|| {
        root.join("target.ready").exists()
            && root.join("sentinel.ready").exists()
            && root.join("pids.json").exists()
    });
    let pids: [u32; 2] =
        serde_json::from_slice(&fs::read(root.join("pids.json")).unwrap()).unwrap();
    let target = inspect_process_identity(pids[0]).unwrap().unwrap();
    let sentinel = inspect_process_identity(pids[1]).unwrap().unwrap();
    let launcher_identity = inspect_process_identity(launcher.id()).unwrap().unwrap();
    // Even real children are not implicitly authorized window targets. Toolhelp
    // ancestry may refer to a recycled PID; require a pinned target identity.
    let launcher_close =
        request_windows_window_close(launcher.id(), &launcher_identity, desktop.as_ref());
    assert!(matches!(
        launcher_close,
        Err(RuntimeProcessError::WindowCloseProcess { operation: "window selection", source, .. })
            if source.kind() == std::io::ErrorKind::NotFound
    ));
    assert!(process_matches_identity(pids[0], &target).unwrap());
    assert!(process_matches_identity(pids[1], &sentinel).unwrap());
    assert!(!root.join("target.closed").exists());
    assert!(!root.join("sentinel.closed").exists());
    let mut stale = target.clone();
    stale.creation_time += 1;
    assert!(matches!(
        request_windows_window_close(pids[0], &stale, desktop.as_ref()),
        Err(RuntimeProcessError::ProcessIdentityMismatch { .. })
    ));
    assert!(!root.join("target.closed").exists());
    let original_desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    request_windows_window_close(pids[0], &target, desktop.as_ref()).unwrap();
    assert_eq!(
        unsafe { GetThreadDesktop(GetCurrentThreadId()) },
        original_desktop
    );
    wait_until(|| {
        root.join("target.closed").exists() && inspect_process_identity(pids[0]).unwrap().is_none()
    });
    assert!(process_matches_identity(pids[1], &sentinel).unwrap());
    assert!(!root.join("sentinel.closed").exists());
    request_windows_window_close(pids[1], &sentinel, desktop.as_ref()).unwrap();
    wait_until(|| launcher.try_wait().unwrap().is_some());
    assert!(launcher.try_wait().unwrap().unwrap().success());
    assert!(root.join("sentinel.closed").exists());
    launcher.finish_process_tree().unwrap();
    drop(launcher);
    drop(desktop);
    fs::remove_dir_all(root).unwrap();
}

unsafe extern "system" fn fixture_window_proc(
    window: *mut c_void,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if message == WM_CLOSE {
        if let Some(path) = WINDOW_OUTPUT.get() {
            let _ = fs::write(path, b"WM_CLOSE processed before exit");
        }
        unsafe {
            DestroyWindow(window);
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
        ex_style: u32,
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
        parameter: *mut c_void,
    ) -> *mut c_void;
    fn GetMessageW(message: *mut WindowMessage, window: *mut c_void, min: u32, max: u32) -> i32;
    fn TranslateMessage(message: *const WindowMessage) -> i32;
    fn DispatchMessageW(message: *const WindowMessage) -> isize;
    fn DestroyWindow(window: *mut c_void) -> i32;
    fn PostQuitMessage(code: i32);
    fn DefWindowProcW(window: *mut c_void, message: u32, wparam: usize, lparam: isize) -> isize;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}
