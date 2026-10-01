use super::*;
use std::io::Read;
use std::os::windows::io::IntoRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};

#[path = "control_tests.rs"]
mod control_tests;

const ROOT_ENV: &str = "LANGAME_ELEVATED_FIXTURE_ROOT";
const HELPER_TEST: &str = "elevated_launcher::tests::helper_probe";
const WORKLOAD_TEST: &str = "elevated_launcher::tests::workload_probe";
const GRANDCHILD_TEST: &str = "elevated_launcher::tests::grandchild_probe";
const PARENT_TEST: &str = "elevated_launcher::tests::parent_probe";

fn probe_command(test: &str, directory: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", test, "--nocapture"])
        .env(ROOT_ENV, directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(crate::CREATE_NO_WINDOW);
    command
}

// Only a #[cfg(test)] entry can bypass UAC. It exercises the production pipe,
// authentication, launch Job and parent-death loop with this test executable.
#[test]
#[ignore = "internal native fixture, run by elevated launcher lifecycle tests"]
fn helper_probe() {
    let nonce = std::env::var("LANGAME_ELEVATED_FIXTURE_NONCE").unwrap();
    let parent = std::env::var("LANGAME_ELEVATED_FIXTURE_PARENT")
        .unwrap()
        .parse()
        .unwrap();
    let creation = std::env::var("LANGAME_ELEVATED_FIXTURE_CREATION")
        .unwrap()
        .parse()
        .unwrap();
    let code = run_helper(&nonce, parent, creation).unwrap();
    std::process::exit(code);
}

#[test]
#[ignore = "internal native fixture, run by elevated launcher lifecycle tests"]
fn workload_probe() {
    let root = std::path::PathBuf::from(std::env::var_os(ROOT_ENV).unwrap());
    std::fs::write(root.join("workload.pid"), std::process::id().to_string()).unwrap();
    std::fs::write(
        root.join("args.json"),
        serde_json::to_vec(&std::env::args().collect::<Vec<_>>()).unwrap(),
    )
    .unwrap();
    // The descendant inherits membership in the Job retained by the helper.
    let child = OwnedWindowsHandle::new(
        probe_command(GRANDCHILD_TEST, &root)
            .spawn()
            .unwrap()
            .into_raw_handle(),
    );
    let _grandchild = wait_process(&root.join("grandchild.pid"));
    if std::env::var_os("LANGAME_ELEVATED_FIXTURE_ROOT_EXITS").is_some() {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !root.join("allow-root-exit").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(pipe::POLL);
        }
        drop(child);
        return;
    }
    let mut byte = [0];
    let _ = std::io::stdin().read_exact(&mut byte);
    std::thread::sleep(Duration::from_secs(20));
}

#[test]
#[ignore = "internal native fixture, run by elevated launcher lifecycle tests"]
fn grandchild_probe() {
    let root = std::path::PathBuf::from(std::env::var_os(ROOT_ENV).unwrap());
    std::fs::write(root.join("grandchild.pid"), std::process::id().to_string()).unwrap();
    std::thread::sleep(Duration::from_secs(20));
}

#[test]
#[ignore = "internal native fixture, run by elevated launcher lifecycle tests"]
fn parent_probe() {
    let root = std::path::PathBuf::from(std::env::var_os(ROOT_ENV).unwrap());
    let child = launch_fixture(&root, false);
    std::fs::write(root.join("helper.pid"), child.id().to_string()).unwrap();
    let mut byte = [0];
    let _ = std::io::stdin().read_exact(&mut byte);
    drop(child);
}

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path = crate::test_support::unique_test_root();
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn helper_command(nonce: &str, directory: &Path) -> Command {
    let parent = crate::query_windows_process_identity_from_handle(std::process::id(), unsafe {
        crate::GetCurrentProcess()
    })
    .unwrap();
    let mut command = probe_command(HELPER_TEST, directory);
    command
        .env("LANGAME_ELEVATED_FIXTURE_NONCE", nonce)
        .env(
            "LANGAME_ELEVATED_FIXTURE_PARENT",
            std::process::id().to_string(),
        )
        .env(
            "LANGAME_ELEVATED_FIXTURE_CREATION",
            parent.creation_time.to_string(),
        );
    command
}

fn handle(child: &Child) -> OwnedWindowsHandle {
    let raw = unsafe {
        crate::OpenProcess(
            crate::SYNCHRONIZE | crate::PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            child.id(),
        )
    };
    assert!(!raw.is_null());
    OwnedWindowsHandle::new(raw)
}

fn packet(directory: &Path, stdout: &File) -> serde_json::Value {
    json!({"executable": std::env::current_exe().unwrap(),
        "args": ["--ignored", "--exact", WORKLOAD_TEST, "--nocapture", "--skip", "literal & | % ! \"quotes\" C:\\save path\\"],
        "directory": directory, "stdout": stdout.as_raw_handle() as usize,
        "stderr": stdout.as_raw_handle() as usize, "background": true})
}

fn launch_fixture(directory: &Path, root_exits: bool) -> RuntimeChild {
    let nonce = pipe::nonce().unwrap();
    let channel = pipe::Channel::server(&nonce).unwrap();
    let mut command = helper_command(&nonce, directory);
    if root_exits {
        command.env("LANGAME_ELEVATED_FIXTURE_ROOT_EXITS", "1");
    }
    let helper = OwnedWindowsHandle::new(command.spawn().unwrap().into_raw_handle());
    let stdout = File::options()
        .create(true)
        .append(true)
        .open(directory.join("output.log"))
        .unwrap();
    complete_start(channel, helper, &packet(directory, &stdout)).unwrap()
}

fn wait_process(path: &Path) -> WindowsProcessHandle {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(value) = std::fs::read_to_string(path)
            && let Ok(pid) = value.parse()
            && let Some(process) =
                WindowsProcessHandle::open(pid, crate::PROCESS_TERMINATE).unwrap()
        {
            return process;
        }
        assert!(
            Instant::now() < deadline,
            "fixture did not publish {}",
            path.display()
        );
        std::thread::sleep(pipe::POLL);
    }
}

#[test]
fn elevated_owner_stop_reaps_native_root_and_descendant() {
    let directory = Directory::new();
    let unrelated = crate::process_exit_target::tests::Fixture::new();
    let mut child = launch_fixture(&directory.0, false);
    let root = wait_process(&directory.0.join("workload.pid"));
    let descendant = wait_process(&directory.0.join("grandchild.pid"));
    let args: Vec<String> =
        serde_json::from_slice(&std::fs::read(directory.0.join("args.json")).unwrap()).unwrap();
    assert_eq!(
        args.last().unwrap(),
        "literal & | % ! \"quotes\" C:\\save path\\"
    );
    assert!(child.has_owned_process_tree());
    assert_eq!(child.owned_process_tree_is_running().unwrap(), Some(true));
    child.finish_process_tree().unwrap();
    assert!(root.wait_for_exit(0).unwrap());
    assert!(descendant.wait_for_exit(0).unwrap());
    assert_eq!(child.owned_process_tree_is_running().unwrap(), Some(false));
    child.finish_process_tree().unwrap();
    assert!(unrelated.capture().is_running().unwrap());
}

#[test]
fn elevated_owner_retains_descendants_after_workload_root_exits() {
    let directory = Directory::new();
    let mut child = launch_fixture(&directory.0, true);
    let root = wait_process(&directory.0.join("workload.pid"));
    let descendant = wait_process(&directory.0.join("grandchild.pid"));
    std::fs::write(directory.0.join("allow-root-exit"), b"ready").unwrap();
    assert!(root.wait_for_exit(5000).unwrap());
    assert!(descendant.is_running().unwrap());
    assert_eq!(child.owned_process_tree_is_running().unwrap(), Some(true));
    child.finish_process_tree().unwrap();
    assert!(descendant.wait_for_exit(0).unwrap());
}

#[test]
fn elevated_owner_disconnect_reaps_tree_without_blocking_drop() {
    let directory = Directory::new();
    let child = launch_fixture(&directory.0, false);
    let root = wait_process(&directory.0.join("workload.pid"));
    let descendant = wait_process(&directory.0.join("grandchild.pid"));
    let helper = WindowsProcessHandle::open(child.id(), 0).unwrap().unwrap();
    drop(child);
    assert!(helper.wait_for_exit(5000).unwrap());
    assert!(root.wait_for_exit(0).unwrap());
    assert!(descendant.wait_for_exit(0).unwrap());
}

#[test]
fn elevated_helper_reaps_tree_after_real_parent_termination() {
    let directory = Directory::new();
    let mut parent = probe_command(PARENT_TEST, &directory.0).spawn().unwrap();
    let helper = wait_process(&directory.0.join("helper.pid"));
    let root = wait_process(&directory.0.join("workload.pid"));
    let descendant = wait_process(&directory.0.join("grandchild.pid"));
    parent.kill().unwrap();
    assert!(helper.wait_for_exit(5000).unwrap());
    assert!(root.wait_for_exit(0).unwrap());
    assert!(descendant.wait_for_exit(0).unwrap());
    parent.wait().unwrap();
}

#[test]
fn elevated_request_rejects_malformed_or_nonabsolute_launches() {
    let directory = Directory::new();
    let stdout = File::create(directory.0.join("output.log")).unwrap();
    let valid = packet(&directory.0, &stdout);
    assert!(request::LaunchRequest::parse(&valid).is_ok());
    for (key, value) in [
        ("executable", json!("relative.exe")),
        ("directory", json!("relative")),
        ("stdout", json!(0)),
        ("args", json!(["bad\u{0}argument"])),
        ("background", json!("yes")),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert!(request::LaunchRequest::parse(&invalid).is_err(), "{key}");
    }
    let mut extra = valid;
    extra["command"] = json!("unexpected");
    assert!(request::LaunchRequest::parse(&extra).is_err());
}

#[test]
fn elevated_helper_rejects_parent_creation_mismatch_before_launch() {
    let directory = Directory::new();
    let nonce = pipe::nonce().unwrap();
    let channel = pipe::Channel::server(&nonce).unwrap();
    let mut helper = helper_command(&nonce, &directory.0)
        .env("LANGAME_ELEVATED_FIXTURE_CREATION", "1")
        .spawn()
        .unwrap();
    let process = handle(&helper);
    assert_eq!(
        unsafe { crate::WaitForSingleObject(process.as_raw(), 5000) },
        crate::WAIT_OBJECT_0
    );
    assert!(!helper.wait().unwrap().success());
    assert!(!directory.0.join("workload.pid").exists());
    drop(channel);
}

#[test]
fn elevated_pipe_has_exclusive_namespace_and_bounded_frames() {
    let nonce = pipe::nonce().unwrap();
    let server = pipe::Channel::server(&nonce).unwrap();
    assert!(pipe::Channel::server(&nonce).is_err());
    assert!(pipe::Channel::server("invalid\\remote").is_err());
    let client = pipe::Channel::connect(&nonce).unwrap();
    server
        .accept(Instant::now() + Duration::from_secs(1))
        .unwrap();
    client
        .write_all(
            &(pipe::MAX_FRAME as u32 + 1).to_le_bytes(),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(
        server
            .receive(Instant::now() + Duration::from_secs(1))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        server.receive(Instant::now()).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
}

#[test]
fn elevated_failure_diagnostic_retains_cause_and_tolerates_disconnected_stderr() {
    struct BrokenWriter;
    impl io::Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "fixture stderr disconnected",
            ))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let error = io::Error::new(io::ErrorKind::PermissionDenied, "fixture token rejected");
    let mut output = Vec::new();
    report_failure(&mut output, &error);
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("fixture token rejected")
    );
    report_failure(&mut BrokenWriter, &error);
}

#[test]
fn elevated_identity_failure_preserves_cleanup_error_without_waiting_on_live_process() {
    let directory = Directory::new();
    let mut process = probe_command(GRANDCHILD_TEST, &directory.0)
        .spawn()
        .unwrap();
    let process_id = process.id();
    let process_handle = handle(&process).into_raw() as usize; // Query/synchronize only.
    let mut child = RuntimeChild::Windows(WindowsSpawnedChild {
        process_handle,
        process_id,
        stdin: None,
        terminal: None,
        job: None,
        output: None,
        elevated: None,
    });
    let error = crate::runtime_launch::cleanup_failed_launch(
        &mut child,
        crate::RuntimeProcessError::ProcessIdentityUnavailable { pid: process_id },
    );
    match error {
        crate::RuntimeProcessError::FailedLaunchCleanup {
            launch_error,
            source,
        } => {
            assert!(
                matches!(*launch_error, crate::RuntimeProcessError::ProcessIdentityUnavailable { pid } if pid == process_id)
            );
            assert_eq!(
                source.raw_os_error(),
                Some(5),
                "termination access denial must be retained"
            );
        }
        error => panic!("expected both launch and cleanup errors: {error}"),
    }
    assert!(
        child.try_wait().unwrap().is_none(),
        "failure must not be reported as process exit"
    );
    process.kill().unwrap();
    process.wait().unwrap();
}
