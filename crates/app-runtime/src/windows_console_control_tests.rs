use super::*;
use std::os::windows::process::CommandExt;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

const FIXTURE_TEST: &str = "windows_console_control::tests::isolated_console_fixture";
const FIXTURE_ROLE: &str = "LGSM_CONSOLE_FIXTURE_ROLE";
const FIXTURE_ROOT: &str = "LGSM_CONSOLE_FIXTURE_ROOT";
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

#[test]
fn console_interrupt_errors_identify_the_failing_native_operation() {
    struct FailingControl {
        operation: &'static str,
        attached: std::cell::Cell<bool>,
    }
    impl WindowsConsoleControl for FailingControl {
        fn detach_console(&self) -> std::io::Result<()> {
            if self.attached.get() && self.operation == "FreeConsole" {
                return Err(std::io::Error::from_raw_os_error(6));
            }
            self.attached.set(false);
            Ok(())
        }
        fn attach_console(&self, _: u32) -> std::io::Result<()> {
            if self.operation == "AttachConsole" {
                return Err(std::io::Error::from_raw_os_error(6));
            }
            self.attached.set(true);
            Ok(())
        }
        fn set_ctrl_c_ignored(&self, ignored: bool) -> std::io::Result<()> {
            if ignored && self.operation == "SetConsoleCtrlHandler" {
                return Err(std::io::Error::from_raw_os_error(6));
            }
            Ok(())
        }
        fn generate_ctrl_c(&self) -> std::io::Result<()> {
            if self.operation == "GenerateConsoleCtrlEvent" {
                return Err(std::io::Error::from_raw_os_error(6));
            }
            Ok(())
        }
    }
    for operation in [
        "AttachConsole",
        "SetConsoleCtrlHandler",
        "GenerateConsoleCtrlEvent",
        "FreeConsole",
    ] {
        let error = request_windows_console_ctrl_c_with_control_and_verifier(
            &FailingControl {
                operation,
                attached: std::cell::Cell::new(false),
            },
            42,
            Duration::ZERO,
            || Ok(()),
        )
        .expect_err("native operation failure must be reported");
        assert!(
            error.to_string().contains(operation),
            "the failure must identify {operation}: {error}"
        );
        assert!(matches!(error,
            RuntimeProcessError::ConsoleInterruptProcess { source, .. }
                if source.raw_os_error() == Some(6)
        ));
    }
}

#[test]
fn descendant_console_search_requires_a_missing_console_at_attach() {
    for operation in [
        "AttachConsole",
        "GetConsoleProcessList",
        "GenerateConsoleCtrlEvent",
        "FreeConsole",
    ] {
        for code in [5, 6, 87] {
            let error = RuntimeProcessError::ConsoleInterruptProcess {
                pid: 42,
                operation,
                source: std::io::Error::from_raw_os_error(code),
            };
            assert_eq!(
                console_is_unavailable(&error),
                operation == "AttachConsole" && code == 6
            );
        }
    }
}

unsafe extern "system" fn record_interrupt(event: u32) -> i32 {
    if event == CTRL_C_EVENT {
        INTERRUPTED.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}

struct FixtureChild(Child);

impl Drop for FixtureChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            unsafe { WaitForSingleObject(self.0.as_raw_handle(), 1000) };
        }
    }
}

fn child_command(root: &Path, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", FIXTURE_TEST, "--ignored", "--nocapture"])
        .env(FIXTURE_ROLE, role)
        .env(FIXTURE_ROOT, root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "isolated console fixture timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "child entry: started only inside an owned hidden console by the regression test"]
fn isolated_console_fixture() {
    let Ok(role) = std::env::var(FIXTURE_ROLE) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os(FIXTURE_ROOT).expect("fixture root"));
    if role == "launcher" || role == "launcher-shared" {
        let mut target_command = child_command(&root, "target");
        if role == "launcher" {
            target_command.creation_flags(CREATE_NEW_CONSOLE);
        }
        let mut target = FixtureChild(target_command.spawn().unwrap());
        assert_ne!(unsafe { FreeConsole() }, 0);
        fs::write(root.join("launcher.ready"), b"ready").unwrap();
        wait_until(|| target.0.try_wait().unwrap().is_some());
        assert!(target.0.try_wait().unwrap().unwrap().success());
        return;
    }
    if role == "target" || role == "sentinel" {
        assert_ne!(unsafe { SetConsoleCtrlHandler(std::ptr::null_mut(), 0) }, 0);
        assert_ne!(
            unsafe { SetConsoleCtrlHandler(record_interrupt as *const () as *mut _, 1) },
            0
        );
        fs::write(root.join(format!("{role}.ready")), b"ready").unwrap();
        wait_until(|| INTERRUPTED.load(Ordering::SeqCst));
        fs::write(root.join(format!("{role}.interrupted")), b"interrupted").unwrap();
        return;
    }
    assert!(["private", "shared", "detached-launcher", "detached-shared"].contains(&role.as_str()));
    // Both peers are descendants of this synthetic controller. The controller
    // itself was created with CREATE_NEW_CONSOLE on a private hidden desktop;
    // none of these processes can inherit the Cargo/user console.
    let mut sentinel = FixtureChild(child_command(&root, "sentinel").spawn().unwrap());
    let mut target_command = child_command(
        &root,
        match role.as_str() {
            "detached-launcher" => "launcher",
            "detached-shared" => "launcher-shared",
            _ => "target",
        },
    );
    let private = role == "private" || role == "detached-launcher";
    if private {
        target_command.creation_flags(CREATE_NEW_CONSOLE);
    }
    let mut target = FixtureChild(target_command.spawn().unwrap());
    wait_until(|| root.join("target.ready").exists() && root.join("sentinel.ready").exists());
    if role.starts_with("detached-") {
        wait_until(|| root.join("launcher.ready").exists());
    }
    let identity = inspect_process_identity(target.0.id()).unwrap().unwrap();
    let result = request_windows_console_ctrl_c(target.0.id(), &identity);
    if private {
        result.expect("private workload console must receive Ctrl+C");
        wait_until(|| target.0.try_wait().unwrap().is_some());
        assert!(target.0.try_wait().unwrap().unwrap().success());
        assert!(root.join("target.interrupted").exists());
    } else {
        let error = result.expect_err("shared console broadcast must be refused");
        assert!(matches!(
            error,
            RuntimeProcessError::ConsoleInterruptProcess { source, .. }
                if source.kind() == std::io::ErrorKind::PermissionDenied
                    && source.to_string().contains("outside the managed workload")
        ));
        assert!(target.0.try_wait().unwrap().is_none());
        assert!(!root.join("target.interrupted").exists());
    }
    assert!(sentinel.0.try_wait().unwrap().is_none());
    assert!(!root.join("sentinel.interrupted").exists());
    fs::write(root.join("verified"), b"verified").unwrap();
}

#[test]
fn windows_console_interrupt_isolates_sibling_processes() {
    let root = crate::test_support::unique_test_root();
    fs::create_dir_all(&root).unwrap();
    for role in ["shared", "private", "detached-launcher", "detached-shared"] {
        let fixture = root.join(role);
        fs::create_dir(&fixture).unwrap();
        let executable = std::env::current_exe().unwrap();
        let args = ["--exact", FIXTURE_TEST, "--ignored", "--nocapture"].map(String::from);
        let environment = BTreeMap::from([
            (FIXTURE_ROLE.into(), role.into()),
            (FIXTURE_ROOT.into(), fixture.to_string_lossy().into_owned()),
        ]);
        let log = File::create(fixture.join("fixture.log")).unwrap();
        let (mut child, _desktop) = spawn_hidden_desktop_process(
            &SpawnCommand {
                executable: &executable.to_string_lossy(),
                args: &args,
                working_directory: &fixture,
                environment: &environment,
            },
            log.try_clone().unwrap(),
            log,
            CREATE_NEW_CONSOLE,
            None,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.finish_process_tree().unwrap();
                panic!("isolated {role} console fixture timed out");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        child.finish_process_tree().unwrap();
        let output = fs::read_to_string(fixture.join("fixture.log")).unwrap();
        assert!(status.success(), "{role} console fixture failed: {output}");
        assert!(fixture.join("verified").exists());
    }
    fs::remove_dir_all(root).unwrap();
}
