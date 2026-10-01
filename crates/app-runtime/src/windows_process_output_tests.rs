use super::*;
use std::sync::Mutex;

struct MemorySink(Arc<Mutex<Vec<u8>>>);

impl Write for MemorySink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn output_owner_drains_eof_and_preserves_split_unicode_and_final_bytes() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (owner, mut pipe) =
        ManagedProcessOutput::new(Box::new(MemorySink(Arc::clone(&bytes)))).unwrap();
    let expected = "服务器输出\n".repeat(16384) + "final-without-newline";
    for chunk in expected.as_bytes().chunks(23) {
        pipe.write_all(chunk).unwrap();
    }
    drop(pipe);
    drop(owner);
    assert_eq!(*bytes.lock().unwrap(), expected.as_bytes());
}

struct FailedSink(Arc<AtomicBool>);
impl Write for FailedSink {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        self.0.store(true, Ordering::Release);
        Err(io::Error::other("simulated full disk"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn failed_sink_still_drains_server_output_without_an_unbounded_queue() {
    let failed = Arc::new(AtomicBool::new(false));
    let (owner, mut pipe) =
        ManagedProcessOutput::new(Box::new(FailedSink(Arc::clone(&failed)))).unwrap();
    pipe.write_all(&vec![b'x'; 1024 * 1024]).unwrap();
    drop(pipe);
    drop(owner);
    assert!(failed.load(Ordering::Acquire));
}

#[test]
fn output_owner_does_not_wait_forever_for_an_idle_retained_writer() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (owner, pipe) =
        ManagedProcessOutput::new(Box::new(MemorySink(Arc::clone(&bytes)))).unwrap();
    let started = Instant::now();
    drop(owner);
    assert!(started.elapsed() < Duration::from_secs(4));
    drop(pipe);
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("[LanGame] Terminal output capture stopped:"),
        "{output}"
    );
    assert!(output.contains("before EOF"), "{output}");
}

#[test]
fn interrupted_output_preserves_known_bytes_and_reports_its_incomplete_tail() {
    for timeout in [false, true] {
        let closing = Arc::new(AtomicBool::new(false));
        let prefix = b"ready\nlast known bytes without newline";
        let mut first_read = true;
        let mut output = Vec::new();
        drain_output_with_reader(
            |buffer| {
                if first_read {
                    first_read = false;
                    buffer[..prefix.len()].copy_from_slice(prefix);
                    closing.store(timeout, Ordering::Release);
                    return Ok(prefix.len());
                }
                assert!(!timeout, "an expired drain must not read again");
                Err(io::Error::other("synthetic non-EOF pipe failure"))
            },
            &mut output,
            Arc::clone(&closing),
            Duration::ZERO,
        );
        assert!(output.starts_with(prefix));
        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output
                .matches("[LanGame] Terminal output capture stopped:")
                .count(),
            1
        );
        assert!(output.contains("Output capture is incomplete"), "{output}");
        assert!(
            output.contains(if timeout {
                "before EOF"
            } else {
                "synthetic non-EOF pipe failure"
            }),
            "{output}"
        );
    }
}

const OUTPUT_PROBE: &str = "windows_process_output::tests::output_child_probe";

#[test]
#[ignore = "internal managed output probe"]
fn output_child_probe() {
    let release = std::path::PathBuf::from(std::env::var_os("LGSM_OUTPUT_RELEASE_PATH").unwrap());
    std::io::stdout()
        .write_all("stdout:服务器😀\n".as_bytes())
        .unwrap();
    std::io::stderr().write_all(b"stderr:ready\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !release.exists() && Instant::now() < deadline {
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[test]
fn launch_routes_both_output_streams_through_the_owned_sink() {
    use app_core::{InstallState, LaunchPlan, ProcessHostSurface, ProcessWindowPolicy};
    for host_surface in [
        ProcessHostSurface::ManagedTerminal,
        ProcessHostSurface::ManagedPseudoConsole,
    ] {
        let root = crate::test_support::unique_test_root();
        std::fs::create_dir(&root).unwrap();
        let release = root.join("release");
        let log = root.join("console.log");
        let plan = LaunchPlan {
            instance_id: "output-probe".into(),
            instance_name: "Output probe".into(),
            module_id: "demo".into(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            uses_private_runtime: false,
            working_directory: root.to_string_lossy().into_owned(),
            executable_path: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: vec![],
            args: ["--ignored", "--exact", OUTPUT_PROBE, "--nocapture"]
                .map(str::to_owned)
                .to_vec(),
            environment: [(
                String::from("LGSM_OUTPUT_RELEASE_PATH"),
                release.to_string_lossy().into_owned(),
            )]
            .into(),
            command_line: String::new(),
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface,
            host_notes: None,
            performance_policy: Default::default(),
            performance_preview: Default::default(),
        };
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let mut spawned = crate::spawn_launch_plan_with_log_writer(
            &plan,
            &log,
            Box::new(MemorySink(Arc::clone(&bytes))),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let output = String::from_utf8_lossy(&bytes.lock().unwrap()).into_owned();
            if output.contains("stdout:服务器😀") && output.contains("stderr:ready") {
                break;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        std::fs::write(&release, b"exit").unwrap();
        spawned.child.as_mut().unwrap().wait_code().unwrap();
        drop(spawned);
        let output = String::from_utf8_lossy(&bytes.lock().unwrap()).into_owned();
        let direct_log_exists = log.exists();
        std::fs::remove_dir_all(root).unwrap();
        assert!(output.contains("stdout:服务器😀"), "{output}");
        assert!(output.contains("stderr:ready"), "{output}");
        assert!(
            !direct_log_exists,
            "the child must not open a second log file outside its sink"
        );
    }
}
