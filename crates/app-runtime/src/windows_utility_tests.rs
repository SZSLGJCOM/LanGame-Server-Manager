use super::*;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;

const PROBE: &str = "windows_utility::tests::utility_child_probe";

struct Fixture {
    root: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = crate::test_support::unique_test_root();
        std::fs::create_dir(&root).unwrap();
        Self { root }
    }

    fn run(&self, mode: &str, timeout: Duration, limit: usize) -> io::Result<Output> {
        let executable = std::env::current_exe().unwrap();
        let args = ["--ignored", "--exact", PROBE, "--nocapture"].map(str::to_owned);
        capture(
            &SpawnCommand {
                executable: executable.to_str().unwrap(),
                args: &args,
                working_directory: &self.root,
                environment: &BTreeMap::from([("LGSM_UTILITY_PROBE_MODE".into(), mode.into())]),
            },
            timeout,
            limit,
        )
    }

    fn assert_exited(&self, file: &str) {
        let (pid, identity): (u32, app_core::ProcessIdentity) =
            serde_json::from_slice(&std::fs::read(self.root.join(file)).unwrap()).unwrap();
        assert!(
            !crate::process_matches_identity(pid, &identity).unwrap(),
            "captured utility process remained alive"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
#[ignore = "internal bounded utility process fixture"]
fn utility_child_probe() {
    let mode = std::env::var("LGSM_UTILITY_PROBE_MODE").unwrap();
    let pid = std::process::id();
    let identity = crate::inspect_process_identity(pid).unwrap().unwrap();
    let file = if mode == "leaf" {
        "leaf.json"
    } else {
        "root.json"
    };
    std::fs::write(file, serde_json::to_vec(&(pid, identity)).unwrap()).unwrap();
    match mode.as_str() {
        "echo" => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).unwrap();
            assert!(input.is_empty(), "utility stdin must receive EOF");
            std::io::stdout()
                .write_all("stdout:服务器😀".as_bytes())
                .unwrap();
            std::io::stderr().write_all(b"stderr:final").unwrap();
            std::process::exit(17);
        }
        "flood" => {
            let bytes = [b'x'; 16 * 1024];
            loop {
                if std::io::stdout().write_all(&bytes).is_err() {
                    return;
                }
            }
        }
        "dual_stream" => {
            std::io::stderr()
                .write_all(&vec![b'e'; 128 * 1024])
                .unwrap();
            std::io::stdout()
                .write_all(&vec![b'o'; 128 * 1024])
                .unwrap();
            std::io::stderr().write_all(b"stderr-complete").unwrap();
            std::io::stdout().write_all(b"stdout-complete").unwrap();
        }
        "descendant" => {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", PROBE, "--nocapture"])
                .env("LGSM_UTILITY_PROBE_MODE", "leaf")
                .creation_flags(crate::CREATE_NO_WINDOW)
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !Path::new("leaf.json").exists() {
                assert!(child.try_wait().unwrap().is_none());
                assert!(Instant::now() < deadline);
                std::thread::sleep(POLL_INTERVAL);
            }
            // Leave the descendant retaining stdout after the launcher exits.
            std::process::exit(0);
        }
        "system-shell" => {
            let script = std::env::current_dir().unwrap().join("probe.cmd");
            let (shell, arguments) = crate::build_spawn_command(
                &script,
                script.parent().unwrap(),
                &[String::from("expected")],
            )
            .unwrap();
            assert_eq!(
                Path::new(&shell),
                windows_system_directory().unwrap().join("cmd.exe")
            );
            assert_ne!(shell, std::env::var("COMSPEC").unwrap());
            assert_eq!(&arguments[..2], ["/D", "/C"]);
            let output = capture_windows_utility(
                Path::new(&shell),
                &["/D", "/C", script.to_str().unwrap(), "expected"],
                Duration::from_secs(10),
            )
            .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "expected");
        }
        "wait" | "leaf" => std::thread::sleep(Duration::from_secs(30)),
        _ => panic!("unknown utility fixture mode"),
    }
}

#[test]
fn managed_shell_ignores_poisoned_comspec_and_working_directory_executables() {
    let fixture = Fixture::new();
    std::fs::write(fixture.root.join("cmd.exe"), b"not an executable").unwrap();
    std::fs::write(fixture.root.join("probe.cmd"), b"@echo off\r\necho %~1\r\n").unwrap();
    let executable = std::env::current_exe().unwrap();
    let arguments = ["--ignored", "--exact", PROBE, "--nocapture"].map(str::to_owned);
    let output = capture(
        &SpawnCommand {
            executable: executable.to_str().unwrap(),
            args: &arguments,
            working_directory: &fixture.root,
            environment: &BTreeMap::from([
                ("LGSM_UTILITY_PROBE_MODE".into(), "system-shell".into()),
                (
                    "COMSPEC".into(),
                    fixture.root.join("cmd.exe").to_string_lossy().into_owned(),
                ),
            ]),
        },
        Duration::from_secs(15),
        OUTPUT_LIMIT,
    )
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    fixture.assert_exited("root.json");
}

#[test]
fn utility_capture_preserves_both_streams_eof_and_failure_exit_status() {
    let fixture = Fixture::new();
    let output = fixture
        .run("echo", Duration::from_secs(10), OUTPUT_LIMIT)
        .unwrap();
    assert_eq!(output.status.code(), Some(17));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("stdout:服务器😀")
    );
    assert_eq!(output.stderr, b"stderr:final");
    fixture.assert_exited("root.json");
}

#[test]
fn utility_capture_terminates_and_reaps_an_unresponsive_process() {
    let fixture = Fixture::new();
    let started = Instant::now();
    let error = fixture
        .run("wait", Duration::from_secs(3), OUTPUT_LIMIT)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(6));
    fixture.assert_exited("root.json");
}

#[test]
fn utility_capture_bounds_continuous_output_and_reaps_its_owner() {
    let fixture = Fixture::new();
    let error = fixture
        .run("flood", Duration::from_secs(10), 32 * 1024)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    fixture.assert_exited("root.json");
}

#[test]
fn utility_capture_drains_stderr_larger_than_the_pipe_before_stdout_completes() {
    let fixture = Fixture::new();
    let output = fixture
        .run("dual_stream", Duration::from_secs(10), OUTPUT_LIMIT)
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        output.stderr,
        [vec![b'e'; 128 * 1024], b"stderr-complete".to_vec()].concat()
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&("o".repeat(128 * 1024) + "stdout-complete")));
    fixture.assert_exited("root.json");
}

#[test]
fn utility_capture_deadline_includes_inherited_descendant_output_pipes() {
    let fixture = Fixture::new();
    let error = fixture
        .run("descendant", Duration::from_secs(3), OUTPUT_LIMIT)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    fixture.assert_exited("root.json");
    fixture.assert_exited("leaf.json");
}

#[test]
fn utility_capture_rejects_path_search_and_expired_deadlines_before_launch() {
    assert_eq!(
        capture_windows_utility(Path::new("netstat.exe"), &[], Duration::from_secs(1))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    let fixture = Fixture::new();
    assert_eq!(
        fixture
            .run("echo", Duration::ZERO, OUTPUT_LIMIT)
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    assert!(!fixture.root.join("root.json").exists());
}
