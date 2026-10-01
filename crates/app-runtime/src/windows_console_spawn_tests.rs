//! Real isolated processes verify that temporary sender protection cannot leak
//! into a concurrently created managed game. No storage or real backend is used.
use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::Instant;

const FIXTURE: &str = "windows_console_control::spawn_tests::isolated_spawn_inheritance_fixture";
const ROLE: &str = "LGSM_SPAWN_INHERITANCE_ROLE";
const ROOT: &str = "LGSM_SPAWN_INHERITANCE_ROOT";
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

struct Target {
    child: RuntimeChild,
    _desktop: Option<WindowsHiddenDesktop>,
    root: PathBuf,
    role: String,
}

impl Target {
    fn release_and_wait(&mut self) -> std::process::ExitStatus {
        fs::write(self.root.join(format!("{}.release", self.role)), b"release").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "synthetic target did not exit cooperatively"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        let _ = fs::write(self.root.join(format!("{}.release", self.role)), b"release");
        let deadline = Instant::now() + Duration::from_secs(3);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        // This is only an owned synthetic Job's bounded failure recovery.
        let _ = self.child.finish_process_tree();
    }
}

fn spawn_target(root: &Path, role: &str, conpty: bool) -> std::io::Result<Target> {
    let executable = std::env::current_exe()?;
    let args = [
        "--exact",
        FIXTURE,
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]
    .map(String::from);
    let environment = BTreeMap::from([
        (ROLE.into(), role.into()),
        (ROOT.into(), root.to_string_lossy().into_owned()),
    ]);
    let log = File::create(root.join(format!("{role}.log")))?;
    // Use the production private desktop, owned Job and CreateProcessW paths.
    let executable_text = executable.to_string_lossy();
    let plan = SpawnCommand {
        executable: &executable_text,
        args: &args,
        working_directory: root,
        environment: &environment,
    };
    let (child, desktop) = if conpty {
        crate::pseudo_console::spawn(&plan, log, true, None)?
    } else {
        spawn_hidden_desktop_process(&plan, log.try_clone()?, log, CREATE_NEW_CONSOLE, None)?
    };
    Ok(Target {
        child,
        _desktop: desktop,
        root: root.into(),
        role: role.into(),
    })
}

fn wait_for(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    path.exists()
}

unsafe extern "system" fn record_interrupt(event: u32) -> i32 {
    if event == CTRL_C_EVENT {
        INTERRUPTED.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}

fn game(root: &Path, role: &str) {
    // Never clear NULL/FALSE in the game: that would conceal inheritance.
    assert_ne!(
        unsafe { SetConsoleCtrlHandler(record_interrupt as *const () as *mut _, 1) },
        0
    );
    fs::write(root.join(format!("{role}.ready")), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !INTERRUPTED.load(Ordering::SeqCst) && !root.join(format!("{role}.release")).exists() {
        assert!(
            Instant::now() < deadline,
            "synthetic controller disappeared"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let observed: &[u8] = if INTERRUPTED.load(Ordering::SeqCst) {
        b"true"
    } else {
        b"false"
    };
    fs::write(root.join(format!("{role}.result")), observed).unwrap();
}

struct GatedControl {
    ignored: mpsc::Sender<()>,
    resume: mpsc::Receiver<()>,
}

impl WindowsConsoleControl for GatedControl {
    fn detach_console(&self) -> std::io::Result<()> {
        NativeWindowsConsoleControl.detach_console()
    }
    fn attach_console(&self, pid: u32) -> std::io::Result<()> {
        NativeWindowsConsoleControl.attach_console(pid)
    }
    fn generate_ctrl_c(&self) -> std::io::Result<()> {
        NativeWindowsConsoleControl.generate_ctrl_c()
    }
    fn set_ctrl_c_ignored(&self, ignored: bool) -> std::io::Result<()> {
        NativeWindowsConsoleControl.set_ctrl_c_ignored(ignored)?;
        if ignored {
            let _ = self.ignored.send(());
            // The barrier widens only the real native ignore interval. Always
            // return success after setting it so the production RAII restores it.
            let _ = self.resume.recv_timeout(Duration::from_secs(5));
        }
        Ok(())
    }
}

fn observe_interrupt(target: &mut Target) -> serde_json::Value {
    assert!(wait_for(
        &target.root.join(format!("{}.ready", target.role)),
        Duration::from_secs(3)
    ));
    let pid = target.child.id();
    let identity = inspect_process_identity(pid).unwrap().unwrap();
    let dispatch = request_windows_console_ctrl_c(pid, &identity);
    let result = target.root.join(format!("{}.result", target.role));
    let _ = wait_for(&result, Duration::from_secs(1));
    let exit = target.release_and_wait();
    serde_json::json!({
        "interrupt_sent": dispatch.is_ok(),
        "observed_ctrl_c": fs::read_to_string(result).unwrap() == "true",
        "exit_code": exit.code(),
    })
}

fn controller(root: &Path, conpty: bool) {
    // Normalize only this disposable controller before the controlled race.
    assert_ne!(unsafe { SetConsoleCtrlHandler(std::ptr::null_mut(), 0) }, 0);
    let mut a = spawn_target(root, "a", false).unwrap();
    assert!(wait_for(&root.join("a.ready"), Duration::from_secs(3)));
    let a_pid = a.child.id();
    let a_identity = inspect_process_identity(a_pid).unwrap().unwrap();
    let (ignored_tx, ignored_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let sender = std::thread::spawn(move || {
        with_windows_console_ctrl_lock(|| {
            request_windows_console_ctrl_c_with_control_and_verifier(
                &GatedControl {
                    ignored: ignored_tx,
                    resume: resume_rx,
                },
                a_pid,
                Duration::from_millis(250),
                || verify_console_ownership(a_pid, &a_identity, a_pid),
            )
        })
    });
    ignored_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (attempted_tx, attempted_rx) = mpsc::channel();
    let (created_tx, created_rx) = mpsc::channel();
    let child_root = root.to_path_buf();
    let creator = std::thread::spawn(move || {
        attempted_tx.send(()).unwrap();
        let result = spawn_target(&child_root, "b", conpty);
        let _ = created_tx.send(result.is_ok());
        result
    });
    attempted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    // Process creation must wait until the process-wide Ctrl+C ignore state
    // is restored. The receipt records which ordering was observed.
    let created_while_ignored = match created_rx.recv_timeout(Duration::from_secs(1)) {
        Ok(created) => {
            assert!(created, "concurrent process creation failed");
            true
        }
        Err(mpsc::RecvTimeoutError::Timeout) => false,
        Err(error) => panic!("concurrent creator disappeared: {error}"),
    };
    resume_tx.send(()).unwrap();
    sender.join().unwrap().unwrap();
    let mut b = creator.join().unwrap().unwrap();
    let a_exit = a.release_and_wait();
    let b_receipt = observe_interrupt(&mut b);
    // An independently created post-cleanup child is the receiver/OS positive
    // control; success cannot be manufactured by resetting B's ignore bit.
    let mut c = spawn_target(root, "c", conpty).unwrap();
    let c_receipt = observe_interrupt(&mut c);
    fs::write(
        root.join("receipt.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "created_while_ignored": created_while_ignored,
            "a_exit_code": a_exit.code(), "concurrent_child": b_receipt,
            "post_restore_control": c_receipt,
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "child entry: isolated controller and synthetic owned games only"]
fn isolated_spawn_inheritance_fixture() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os(ROOT).expect("explicit fixture root"));
    assert_eq!(
        fs::read(root.join("fixture-marker")).unwrap(),
        b"console-spawn-inheritance"
    );
    match role.as_str() {
        "controller" => controller(&root, false),
        "controller-conpty" => controller(&root, true),
        "a" | "b" | "c" => game(&root, &role),
        _ => panic!("unknown fixture role"),
    }
}

#[test]
fn concurrent_managed_spawn_does_not_inherit_sender_ctrl_c_ignore() {
    assert_no_inherited_ignore(false);
}

#[test]
fn concurrent_conpty_spawn_does_not_inherit_sender_ctrl_c_ignore() {
    assert_no_inherited_ignore(true);
}

#[test]
fn managed_spawn_lock_preserves_native_creation_errors() {
    let root = crate::test_support::unique_test_root();
    fs::create_dir(&root).unwrap();
    let missing = root.join("missing-synthetic-executable.exe");
    let executable = missing.to_string_lossy();
    let environment = BTreeMap::new();
    let plan = SpawnCommand {
        executable: &executable,
        args: &[],
        working_directory: &root,
        environment: &environment,
    };
    for conpty in [false, true] {
        let log =
            File::create(root.join(if conpty { "conpty.log" } else { "native.log" })).unwrap();
        let result = if conpty {
            crate::pseudo_console::spawn(&plan, log, true, None)
        } else {
            spawn_hidden_desktop_process(
                &plan,
                log.try_clone().unwrap(),
                log,
                CREATE_NEW_CONSOLE,
                None,
            )
        };
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("missing executable unexpectedly started"),
        };
        assert_eq!(
            error.raw_os_error(),
            Some(2),
            "creation must preserve ERROR_FILE_NOT_FOUND: {error}"
        );
    }
    let canonical = fs::canonicalize(&root).unwrap();
    assert_eq!(
        canonical.parent(),
        Some(fs::canonicalize(std::env::temp_dir()).unwrap().as_path())
    );
    fs::remove_dir_all(canonical).unwrap();
}

fn assert_no_inherited_ignore(conpty: bool) {
    let root = crate::test_support::unique_test_root();
    fs::create_dir(&root).unwrap();
    fs::write(root.join("fixture-marker"), b"console-spawn-inheritance").unwrap();
    let role = if conpty {
        "controller-conpty"
    } else {
        "controller"
    };
    let mut controller = spawn_target(&root, role, false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(25);
    let status = loop {
        if let Some(status) = controller.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "isolated concurrent console fixture timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = fs::read_to_string(root.join(format!("{role}.log"))).unwrap();
    assert!(status.success(), "isolated controller failed: {output}");
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("receipt.json")).unwrap()).unwrap();
    drop(controller);
    let canonical = fs::canonicalize(&root).unwrap();
    assert_eq!(
        canonical.parent(),
        Some(fs::canonicalize(std::env::temp_dir()).unwrap().as_path())
    );
    fs::remove_dir_all(canonical).unwrap();
    // Intended red failure is after every synthetic process and Job is closed.
    assert_eq!(receipt["a_exit_code"], 0, "{receipt}");
    assert_eq!(
        receipt["created_while_ignored"], false,
        "creation must wait for sender restoration: {receipt}"
    );
    for role in ["post_restore_control", "concurrent_child"] {
        assert_eq!(receipt[role]["interrupt_sent"], true, "{receipt}");
        assert_eq!(receipt[role]["exit_code"], 0, "{receipt}");
        assert_eq!(receipt[role]["observed_ctrl_c"], true, "{receipt}");
    }
}
