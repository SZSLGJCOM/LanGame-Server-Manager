use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const CONSOLE_TEST: &str = "elevated_launcher::tests::control_tests::console_probe";
const DESCENDANT_TEST: &str = "elevated_launcher::tests::control_tests::descendant_probe";
static INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetConsoleProcessList(processes: *mut u32, capacity: u32) -> u32;
}

fn console_members() -> u32 {
    let mut members = [0_u32; 16];
    unsafe { GetConsoleProcessList(members.as_mut_ptr(), members.len() as u32) }
}

unsafe extern "system" fn record_interrupt(event: u32) -> i32 {
    if event == crate::CTRL_C_EVENT {
        INTERRUPTS.fetch_add(1, Ordering::SeqCst);
        1
    } else {
        0
    }
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "elevated control fixture timed out"
        );
        std::thread::sleep(pipe::POLL);
    }
}

#[test]
#[ignore = "owned native console fixture, started only by elevated control tests"]
fn console_probe() {
    let root = std::path::PathBuf::from(std::env::var_os(ROOT_ENV).unwrap());
    // The helper retains the Job that owns these descendants after this root exits.
    if std::env::var_os("LANGAME_ELEVATED_FIXTURE_NO_CONSOLE").is_some() {
        assert_ne!(unsafe { crate::FreeConsole() }, 0);
        std::fs::write(
            root.join("detached-console-members"),
            console_members().to_string(),
        )
        .unwrap();
        std::fs::write(root.join("workload.pid"), std::process::id().to_string()).unwrap();
        let _descendant = OwnedWindowsHandle::new(
            probe_command(DESCENDANT_TEST, &root)
                .spawn()
                .unwrap()
                .into_raw_handle(),
        );
        wait_until(|| root.join("allow-root-exit").exists());
        return;
    }
    assert_ne!(
        unsafe { crate::SetConsoleCtrlHandler(std::ptr::null_mut(), 0) },
        0
    );
    assert_ne!(
        unsafe { crate::SetConsoleCtrlHandler(record_interrupt as *const () as *mut _, 1) },
        0
    );
    std::fs::write(root.join("workload.pid"), std::process::id().to_string()).unwrap();
    let _descendant = OwnedWindowsHandle::new(
        probe_command(DESCENDANT_TEST, &root)
            .spawn()
            .unwrap()
            .into_raw_handle(),
    );
    wait_until(|| INTERRUPTS.load(Ordering::SeqCst) != 0);
    std::fs::write(
        root.join("interrupted"),
        INTERRUPTS.load(Ordering::SeqCst).to_string(),
    )
    .unwrap();
    wait_until(|| root.join("allow-root-exit").exists());
    println!("native graceful final output");
}

#[test]
#[ignore = "owned native descendant fixture, started only by elevated control tests"]
fn descendant_probe() {
    let root = std::path::PathBuf::from(std::env::var_os(ROOT_ENV).unwrap());
    if std::env::var_os("LANGAME_ELEVATED_FIXTURE_NO_CONSOLE").is_some() && console_members() != 0 {
        assert_ne!(unsafe { crate::FreeConsole() }, 0);
    }
    std::fs::write(
        root.join("descendant-console-members"),
        console_members().to_string(),
    )
    .unwrap();
    std::fs::write(root.join("grandchild.pid"), std::process::id().to_string()).unwrap();
    wait_until(|| root.join("allow-descendant-exit").exists());
}

fn launch_console(directory: &Path, detached: bool) -> RuntimeChild {
    let nonce = pipe::nonce().unwrap();
    let channel = pipe::Channel::server(&nonce).unwrap();
    let mut command = helper_command(&nonce, directory);
    if detached {
        command.env("LANGAME_ELEVATED_FIXTURE_NO_CONSOLE", "1");
    }
    let helper = OwnedWindowsHandle::new(command.spawn().unwrap().into_raw_handle());
    let stdout = File::options()
        .create(true)
        .append(true)
        .open(directory.join("output.log"))
        .unwrap();
    let mut launch = packet(directory, &stdout);
    launch["args"] = json!(["--ignored", "--exact", CONSOLE_TEST, "--nocapture"]);
    complete_start(channel, helper, &launch).unwrap()
}

#[test]
fn elevated_console_control_uses_owned_helper_and_waits_for_natural_tree_exit() {
    let directory = Directory::new();
    let mut unrelated = crate::process_exit_target::tests::Fixture::new();
    let child = launch_console(&directory.0, false);
    let helper = WindowsProcessHandle::open(child.id(), 0).unwrap().unwrap();
    let root = wait_process(&directory.0.join("workload.pid"));
    let descendant = wait_process(&directory.0.join("grandchild.pid"));
    let mut supervisor = crate::RuntimeSupervisor::default();
    supervisor.insert_running(
        app_core::InstanceSummary {
            id: "elevated-control".into(),
            name: "Elevated control".into(),
            module_id: "fixture".into(),
            status: app_core::InstanceStatus::Running,
            active_process_count: 1,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        None,
        vec![crate::ManagedProcess {
            run_id: 1,
            process_key: "main".into(),
            display_name: "Owned fixture".into(),
            pid: root.pid,
            process_identity: root.identity().unwrap(),
            root_process_identity: helper.identity().unwrap(),
            log_path: "output.log".into(),
            is_primary: true,
            uses_script_entrypoint: false,
            performance_policy: app_core::RuntimePerformancePolicy::default(),
            last_performance_refresh: None,
            last_performance_target_count: None,
            last_performance_application: None,
            child: Some(child),
            hidden_desktop: None,
        }],
    );
    supervisor
        .request_console_interrupt("elevated-control", None)
        .unwrap();
    wait_until(|| directory.0.join("interrupted").exists());
    assert_eq!(
        std::fs::read_to_string(directory.0.join("interrupted")).unwrap(),
        "1"
    );
    assert!(root.is_running().unwrap());
    assert!(descendant.is_running().unwrap());
    assert!(unrelated.capture().is_running().unwrap());
    let mut owner = supervisor
        .take_running_for_stop("elevated-control")
        .unwrap();
    let child = owner.processes[0].child.as_mut().unwrap();
    std::fs::write(directory.0.join("allow-root-exit"), b"exit").unwrap();
    assert!(root.wait_for_exit(5000).unwrap());
    assert!(
        helper.is_running().unwrap(),
        "root exit must retain the descendant owner"
    );
    assert_eq!(child.owned_process_tree_is_running().unwrap(), Some(true));
    std::fs::write(directory.0.join("allow-descendant-exit"), b"exit").unwrap();
    assert!(descendant.wait_for_exit(5000).unwrap());
    assert!(helper.wait_for_exit(5000).unwrap());
    assert_eq!(child.owned_process_tree_is_running().unwrap(), Some(false));
    assert_eq!(child.try_wait().unwrap().unwrap().code(), Some(0));
    assert!(
        std::fs::read_to_string(directory.0.join("output.log"))
            .unwrap()
            .contains("native graceful final output")
    );
    unrelated.exit_normally();
}

#[test]
fn elevated_console_control_failure_preserves_descendant_and_owner() {
    let directory = Directory::new();
    let mut child = launch_fixture(&directory.0, true);
    let root = wait_process(&directory.0.join("workload.pid"));
    let descendant = wait_process(&directory.0.join("grandchild.pid"));
    std::fs::write(directory.0.join("allow-root-exit"), b"exit").unwrap();
    assert!(root.wait_for_exit(5000).unwrap());
    let RuntimeChild::Windows(native) = &mut child else {
        unreachable!()
    };
    let error = native
        .elevated
        .as_mut()
        .unwrap()
        .request_console_interrupt(native.process_handle)
        .unwrap_err();
    assert!(
        error.to_string().contains("process"),
        "native cause retained: {error}"
    );
    assert!(descendant.is_running().unwrap());
    assert_eq!(child.owned_process_tree_is_running().unwrap(), Some(true));
    child.finish_process_tree().unwrap(); // Explicit cleanup remains a separate protocol.
    assert!(descendant.wait_for_exit(0).unwrap());
}

#[test]
fn elevated_console_absence_returns_native_error_without_reaping_live_workload() {
    let directory = Directory::new();
    let mut child = launch_console(&directory.0, true);
    let root = wait_process(&directory.0.join("workload.pid"));
    let descendant = wait_process(&directory.0.join("grandchild.pid"));
    assert_eq!(
        std::fs::read_to_string(directory.0.join("detached-console-members")).unwrap(),
        "0"
    );
    assert_eq!(
        std::fs::read_to_string(directory.0.join("descendant-console-members")).unwrap(),
        "0"
    );
    let RuntimeChild::Windows(native) = &mut child else {
        unreachable!()
    };
    let error = native
        .elevated
        .as_mut()
        .unwrap()
        .request_console_interrupt(native.process_handle)
        .unwrap_err();
    assert!(
        error.to_string().contains("AttachConsole"),
        "native cause retained: {error}"
    );
    assert!(
        error.to_string().contains("os error 6"),
        "native code retained: {error}"
    );
    assert!(root.is_running().unwrap());
    assert!(descendant.is_running().unwrap());
    assert_eq!(child.owned_process_tree_is_running().unwrap(), Some(true));
    std::fs::write(directory.0.join("allow-root-exit"), b"exit").unwrap();
    std::fs::write(directory.0.join("allow-descendant-exit"), b"exit").unwrap();
    assert!(root.wait_for_exit(5000).unwrap());
    assert!(descendant.wait_for_exit(5000).unwrap());
    wait_until(|| child.owned_process_tree_is_running().unwrap() == Some(false));
}

fn channel_pair() -> (ElevatedGuard, pipe::Channel) {
    let nonce = pipe::nonce().unwrap();
    let server = pipe::Channel::server(&nonce).unwrap();
    let client = pipe::Channel::connect(&nonce).unwrap();
    server
        .accept(Instant::now() + Duration::from_secs(1))
        .unwrap();
    (
        ElevatedGuard {
            channel: server,
            confirmed: false,
            interrupt_pending: true,
            receipts: control::ReceiptReader::default(),
        },
        client,
    )
}

#[test]
fn elevated_console_pending_receipt_survives_partial_frame_without_rebroadcast() {
    let (mut guard, client) = channel_pair();
    let payload = serde_json::to_vec(&json!({"interrupted": true})).unwrap();
    let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
    frame.extend(payload);
    for split in [2, 7] {
        client
            .write_all(&frame[..split], Instant::now() + Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            guard
                .receipts
                .receive(&guard.channel, Instant::now() + Duration::from_millis(30))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        client
            .write_all(&frame[split..], Instant::now() + Duration::from_secs(1))
            .unwrap();
        guard.request_console_interrupt(0).unwrap(); // No process query or duplicate C while pending.
        assert!(!guard.interrupt_pending);
        let mut bytes = [0; 8];
        assert_eq!(client.read_available(&mut bytes).unwrap(), 0);
        guard.interrupt_pending = true;
    }
    client
        .send(
            &json!({"interrupted": false, "error": "native failure"}),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    client
        .send(
            &json!({"stopped": true}),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    guard
        .confirm_stopped(Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert!(guard.confirmed);
    assert!(!guard.interrupt_pending);
}
