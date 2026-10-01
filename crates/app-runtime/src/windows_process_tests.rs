use super::*;
#[cfg(windows)]
use crate::windows_console_control::{
    WindowsConsoleControl, request_windows_console_ctrl_c_with_control,
    request_windows_console_ctrl_c_with_control_and_verifier, with_windows_console_ctrl_lock,
};
use std::sync::Arc;

#[cfg(windows)]
fn identity_inspection_error(code: i32) -> RuntimeProcessError {
    RuntimeProcessError::InspectProcess {
        pid: 42,
        source: std::io::Error::from_raw_os_error(code),
    }
}

#[cfg(windows)]
#[test]
fn process_identity_exit_race_accepts_only_a_confirmed_exit() {
    let checked = std::cell::Cell::new(false);
    let result = windows_process_identity::resolve_identity_after_exit_race(
        Err(identity_inspection_error(ERROR_ACCESS_DENIED)),
        || {
            checked.set(true);
            Ok(true)
        },
    );
    assert!(matches!(result, Ok(None)), "{result:?}");
    assert!(checked.get());
}

#[cfg(windows)]
#[test]
fn process_identity_exit_race_preserves_access_denied_while_running() {
    let checked = std::cell::Cell::new(false);
    let result = windows_process_identity::resolve_identity_after_exit_race(
        Err(identity_inspection_error(ERROR_ACCESS_DENIED)),
        || {
            checked.set(true);
            Ok(false)
        },
    );
    assert!(matches!(
        result,
        Err(RuntimeProcessError::InspectProcess { pid: 42, source })
            if source.raw_os_error() == Some(ERROR_ACCESS_DENIED)
    ));
    assert!(checked.get());
}

#[cfg(windows)]
#[test]
fn process_identity_exit_race_propagates_failed_exit_confirmation() {
    let result = windows_process_identity::resolve_identity_after_exit_race(
        Err(identity_inspection_error(ERROR_ACCESS_DENIED)),
        || Err(identity_inspection_error(ERROR_INVALID_HANDLE_FOR_TEST)),
    );
    assert!(matches!(
        result,
        Err(RuntimeProcessError::InspectProcess { pid: 42, source })
            if source.raw_os_error() == Some(ERROR_INVALID_HANDLE_FOR_TEST)
    ));
}

#[cfg(windows)]
#[test]
fn process_identity_exit_race_keeps_success_without_an_extra_wait() {
    let expected = ProcessIdentity {
        creation_time: 123,
        image_path: String::from("server.exe"),
    };
    let result =
        windows_process_identity::resolve_identity_after_exit_race(Ok(expected.clone()), || {
            panic!("successful identity lookup must not wait for exit")
        })
        .expect("successful identity");
    assert_eq!(result, Some(expected));
}

#[test]
fn delayed_workload_is_not_replaced_by_an_early_console_host() {
    let record = |pid, parent, name: &str| WindowsProcessRecord {
        process_id: pid,
        parent_process_id: parent,
        name: name.into(),
        identity: None,
    };
    let mut descendants = vec![record(2, 1, "conhost.exe")];
    assert_eq!(
        select_windows_workload_descendant(1, &descendants, None),
        None
    );
    descendants.push(record(3, 1, "SCUMServer.exe"));
    assert_eq!(
        select_windows_workload_descendant(1, &descendants, None),
        Some(3)
    );
    descendants.push(record(4, 3, "conhost.exe"));
    assert_eq!(
        select_windows_workload_descendant(1, &descendants, None),
        Some(3)
    );
    descendants.push(record(5, 3, "Server-Win64-Shipping.exe"));
    assert_eq!(
        select_windows_workload_descendant(1, &descendants, None),
        Some(5)
    );
}

#[test]
fn elevated_native_workload_requires_its_image_instead_of_a_crash_reporter() {
    let expected = r"D:\servers\SCUM\SCUMServer.exe";
    let record = |pid, parent, name: &str, image: &str| WindowsProcessRecord {
        process_id: pid,
        parent_process_id: parent,
        name: name.into(),
        identity: Some(ProcessIdentity {
            creation_time: 100 + u64::from(pid),
            image_path: image.into(),
        }),
    };
    let mut descendants = vec![record(
        4,
        3,
        "crashpad_handler.exe",
        r"D:\servers\SCUM\crashpad_handler.exe",
    )];
    assert_eq!(
        select_windows_workload_descendant(1, &descendants, Some(expected)),
        None
    );
    descendants.push(record(
        3,
        1,
        "SCUMServer.exe",
        "d:/servers/scum/scumserver.exe",
    ));
    descendants.push(record(5, 3, "SCUMServer.exe", r"D:\other\SCUMServer.exe"));
    assert_eq!(
        select_windows_workload_descendant(1, &descendants, Some(expected)),
        Some(3)
    );
}

#[cfg(windows)]
const ERROR_INVALID_HANDLE_FOR_TEST: i32 = 6;
#[cfg(windows)]
const WINDOWS_HANDLE_PROBE_FILE: &str = "unlisted-handle.txt";

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetHandleInformation(handle: *mut std::ffi::c_void, flags: *mut u32) -> i32;
    fn GetFileInformationByHandle(
        handle: *mut std::ffi::c_void,
        information: *mut TestByHandleFileInformation,
    ) -> i32;
}

#[cfg(windows)]
#[derive(Default)]
#[repr(C)]
struct TestByHandleFileInformation {
    file_attributes: u32,
    creation_time: FileTime,
    last_access_time: FileTime,
    last_write_time: FileTime,
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[cfg(windows)]
#[derive(Default)]
struct RecordingConsoleState {
    events: Vec<&'static str>,
    active_sessions: usize,
    maximum_active_sessions: usize,
}

#[cfg(windows)]
#[derive(Clone)]
struct RecordingConsoleControl {
    state: Arc<std::sync::Mutex<RecordingConsoleState>>,
    fail_generate: bool,
}

#[cfg(windows)]
impl RecordingConsoleControl {
    fn new(fail_generate: bool) -> Self {
        Self {
            state: Arc::new(std::sync::Mutex::new(RecordingConsoleState::default())),
            fail_generate,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, RecordingConsoleState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(windows)]
impl WindowsConsoleControl for RecordingConsoleControl {
    fn detach_console(&self) -> Result<(), std::io::Error> {
        let mut state = self.state();
        if state.active_sessions == 0 {
            state.events.push("detach-initial");
        } else {
            state.events.push("detach-cleanup");
            state.active_sessions -= 1;
        }
        Ok(())
    }

    fn attach_console(&self, _pid: u32) -> Result<(), std::io::Error> {
        let mut state = self.state();
        state.events.push("attach");
        state.active_sessions += 1;
        state.maximum_active_sessions = state.maximum_active_sessions.max(state.active_sessions);
        Ok(())
    }

    fn set_ctrl_c_ignored(&self, ignored: bool) -> Result<(), std::io::Error> {
        self.state()
            .events
            .push(if ignored { "ignore-on" } else { "ignore-off" });
        Ok(())
    }

    fn generate_ctrl_c(&self) -> Result<(), std::io::Error> {
        self.state().events.push("generate");
        if self.fail_generate {
            Err(std::io::Error::other("injected console event failure"))
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
#[test]
fn windows_console_ctrl_sessions_are_process_serialized() {
    let control = RecordingConsoleControl::new(false);
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();

    for pid in [11, 12] {
        let control = control.clone();
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            with_windows_console_ctrl_lock(|| {
                request_windows_console_ctrl_c_with_control(
                    &control,
                    pid,
                    Duration::from_millis(25),
                )
            })
        }));
    }

    barrier.wait();
    for thread in threads {
        thread.join().expect("console control worker").unwrap();
    }

    let state = control.state();
    assert_eq!(state.maximum_active_sessions, 1);
    assert_eq!(state.active_sessions, 0);
    assert_eq!(
        state.events,
        [
            "detach-initial",
            "attach",
            "ignore-on",
            "generate",
            "detach-cleanup",
            "ignore-off",
            "detach-initial",
            "attach",
            "ignore-on",
            "generate",
            "detach-cleanup",
            "ignore-off",
        ]
    );
}

#[cfg(windows)]
#[test]
fn windows_console_ctrl_session_cleans_up_after_generation_failure() {
    let control = RecordingConsoleControl::new(true);

    let error = with_windows_console_ctrl_lock(|| {
        request_windows_console_ctrl_c_with_control(&control, 42, Duration::ZERO)
    })
    .expect_err("injected generation failure");

    assert_eq!(error.kind(), std::io::ErrorKind::Other);
    let state = control.state();
    assert_eq!(state.active_sessions, 0);
    assert_eq!(
        state.events,
        [
            "detach-initial",
            "attach",
            "ignore-on",
            "generate",
            "detach-cleanup",
            "ignore-off",
        ]
    );
}

#[cfg(windows)]
#[test]
fn windows_console_ctrl_rechecks_identity_after_attach_before_signalling() {
    let control = RecordingConsoleControl::new(false);

    let error = with_windows_console_ctrl_lock(|| {
        request_windows_console_ctrl_c_with_control_and_verifier(
            &control,
            42,
            Duration::ZERO,
            || Err(RuntimeProcessError::ProcessIdentityMismatch { pid: 42 }),
        )
    })
    .expect_err("changed identity must block console signalling");

    assert!(matches!(
        error,
        RuntimeProcessError::ProcessIdentityMismatch { pid: 42 }
    ));
    let state = control.state();
    assert_eq!(state.active_sessions, 0);
    assert_eq!(state.events, ["detach-initial", "attach", "detach-cleanup"]);
}

#[test]
fn same_pid_with_a_different_start_identity_is_not_the_recorded_process() {
    let recorded = ProcessIdentity {
        creation_time: 100,
        image_path: String::from("C:/Servers/GameServer.exe"),
    };
    let reused_pid = ProcessIdentity {
        creation_time: 101,
        image_path: recorded.image_path.clone(),
    };

    assert!(!process_identities_match(&recorded, &reused_pid));
}

#[cfg(windows)]
#[test]
fn performance_policy_skips_a_reused_root_pid() {
    let pid = std::process::id();
    let mut mismatched_identity = inspect_process_identity(pid)
        .expect("inspect current process")
        .expect("current process is running");
    mismatched_identity.creation_time = mismatched_identity.creation_time.saturating_sub(1);
    let policy = RuntimePerformancePolicy::default();

    let application = apply_runtime_performance_policy(pid, &mismatched_identity, &policy);

    assert_eq!(application.targeted_process_count, 0);
    assert_eq!(application.priority_applied_count, 0);
    assert_eq!(application.affinity_applied_count, 0);
    assert_eq!(application.warnings.len(), 1);
    assert!(application.warnings[0].contains("identity does not match"));
}

#[cfg(windows)]
#[test]
fn current_process_identity_is_stable_across_inspections() {
    let pid = std::process::id();
    let first = inspect_process_identity(pid)
        .expect("inspect current process")
        .expect("current process is running");
    let second = inspect_process_identity(pid)
        .expect("inspect current process again")
        .expect("current process remains running");

    assert!(process_identities_match(&first, &second));
    assert!(!first.image_path.is_empty());
}

#[cfg(windows)]
#[test]
fn background_managed_terminal_uses_real_hidden_console() {
    assert_eq!(
        background_creation_flags(false, &ProcessHostSurface::ManagedTerminal),
        CREATE_NEW_CONSOLE
    );
    assert_eq!(
        background_creation_flags(true, &ProcessHostSurface::ManagedTerminal),
        CREATE_NEW_CONSOLE
    );
    assert_eq!(
        background_creation_flags(false, &ProcessHostSurface::ManagedNativeWindow),
        CREATE_NO_WINDOW | DETACHED_PROCESS
    );
}

#[cfg(windows)]
#[test]
fn hidden_desktop_spawn_target_uses_interactive_window_station() {
    assert_eq!(
        hidden_desktop_spawn_target("LanGameHidden-1234-0"),
        "WinSta0\\LanGameHidden-1234-0"
    );
    assert_eq!(
        hidden_desktop_spawn_target("CustomSta\\LanGameHidden-1234-0"),
        "CustomSta\\LanGameHidden-1234-0"
    );
}

#[cfg(windows)]
#[test]
fn hidden_desktop_spawn_inherits_only_allowlisted_stdio_handles() {
    let probe_directory = WindowsHandleProbeDirectory::create();
    let extra_file = File::create(probe_directory.path.join("extra.txt")).expect("extra file");
    let unlisted_handle =
        duplicate_inheritable_handle(extra_file.as_raw_handle() as *mut _).expect("extra handle");
    let mut handle_flags = 0;
    assert_ne!(
        unsafe { GetHandleInformation(unlisted_handle.as_raw(), &mut handle_flags) },
        0,
        "probe handle must be valid"
    );
    assert_ne!(handle_flags & HANDLE_FLAG_INHERIT, 0);
    let expected_identity =
        handle_file_identity_for_test(unlisted_handle.as_raw()).expect("probe file identity");
    fs::write(
        probe_directory.path.join(WINDOWS_HANDLE_PROBE_FILE),
        format!(
            "{}\n{}\n{}",
            unlisted_handle.as_raw() as usize,
            expected_identity.0,
            expected_identity.1
        ),
    )
    .expect("probe handle value");

    let executable = std::env::current_exe().expect("current test executable");
    let arguments = vec![
        String::from("--exact"),
        String::from("windows_process_tests::windows_unlisted_handle_inheritance_probe"),
        String::from("--nocapture"),
    ];
    let stdout = File::create(probe_directory.path.join("stdout.log")).expect("stdout log");
    let stderr = File::create(probe_directory.path.join("stderr.log")).expect("stderr log");
    let (mut child, hidden_desktop) = spawn_hidden_desktop_process(
        &SpawnCommand {
            executable: &executable.to_string_lossy(),
            args: &arguments,
            working_directory: &probe_directory.path,
            environment: &BTreeMap::new(),
        },
        stdout,
        stderr,
        CREATE_NO_WINDOW,
        None,
    )
    .expect("spawn inheritance probe");

    assert_eq!(child.wait_code().expect("wait for probe"), Some(0));
    drop(hidden_desktop);
    let probe_output =
        fs::read_to_string(probe_directory.path.join("stdout.log")).expect("read probe output");
    assert!(
        probe_output.contains("windows_process_tests::windows_unlisted_handle_inheritance_probe"),
        "child exited without executing the handle inheritance probe"
    );
}

#[cfg(windows)]
#[test]
fn windows_unlisted_handle_inheritance_probe() {
    let probe_path = std::env::current_dir()
        .expect("current directory")
        .join(WINDOWS_HANDLE_PROBE_FILE);
    if !probe_path.is_file() {
        return;
    }

    let probe = fs::read_to_string(probe_path).expect("probe handle value");
    let mut values = probe.lines();
    let handle_value = values
        .next()
        .expect("probe handle")
        .parse::<usize>()
        .expect("numeric probe handle");
    let expected_volume = values
        .next()
        .expect("probe volume")
        .parse::<u32>()
        .expect("numeric probe volume");
    let expected_file_index = values
        .next()
        .expect("probe file index")
        .parse::<u64>()
        .expect("numeric probe file index");
    let mut flags = 0;
    let raw_handle = handle_value as *mut std::ffi::c_void;
    let result = unsafe { GetHandleInformation(raw_handle, &mut flags) };
    let error = std::io::Error::last_os_error();

    if result == 0 {
        assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_HANDLE_FOR_TEST));
        return;
    }

    if let Ok(actual_identity) = handle_file_identity_for_test(raw_handle) {
        assert_ne!(
            actual_identity,
            (expected_volume, expected_file_index),
            "unlisted inheritable file handle reached child"
        );
    }
}

#[cfg(windows)]
fn handle_file_identity_for_test(
    handle: *mut std::ffi::c_void,
) -> Result<(u32, u64), std::io::Error> {
    let mut information = TestByHandleFileInformation::default();
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((
        information.volume_serial_number,
        ((information.file_index_high as u64) << 32) | information.file_index_low as u64,
    ))
}

#[cfg(windows)]
struct WindowsHandleProbeDirectory {
    path: PathBuf,
}

#[cfg(windows)]
impl WindowsHandleProbeDirectory {
    fn create() -> Self {
        let path = crate::test_support::unique_test_root();
        fs::create_dir(&path).expect("create handle probe directory");
        Self { path }
    }
}

#[cfg(windows)]
impl Drop for WindowsHandleProbeDirectory {
    fn drop(&mut self) {
        for name in [
            WINDOWS_HANDLE_PROBE_FILE,
            "extra.txt",
            "stdout.log",
            "stderr.log",
        ] {
            let _ = fs::remove_file(self.path.join(name));
        }
        let _ = fs::remove_dir(&self.path);
    }
}

#[cfg(windows)]
#[test]
fn build_spawn_command_uses_script_name_when_batch_is_in_working_directory() {
    let executable =
        Path::new(r"D:\Example\LanGame Server Manager\instances\demo\config\StartServer.bat");
    let working_directory = Path::new(r"D:\Example\LanGame Server Manager\instances\demo\config");
    let args = vec![String::from(
        r"D:\Example\LanGame Server Manager\instances\demo\data",
    )];

    let (spawn_command, spawn_args) =
        build_spawn_command(executable, working_directory, &args).unwrap();

    assert!(spawn_command.to_ascii_lowercase().ends_with("cmd.exe"));
    assert_eq!(spawn_args[0], "/D");
    assert_eq!(spawn_args[1], "/C");
    assert_eq!(spawn_args[2], "StartServer.bat");
    assert_eq!(spawn_args[3], args[0]);
}

#[cfg(windows)]
#[test]
fn managed_batch_launch_rejects_shell_control_instead_of_interpreting_arguments() {
    let working_directory = Path::new(r"D:\Example");
    let executable = working_directory.join("StartServer.cmd");
    for value in [
        "safe&exit",
        "%COMSPEC%",
        "value!expanded!",
        "first\nsecond",
        "a|b",
        "\"quoted\"",
    ] {
        let error =
            build_spawn_command(&executable, working_directory, &[value.into()]).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{value}");
        let unsafe_entry = working_directory.join(format!("server{value}.cmd"));
        assert!(build_spawn_command(&unsafe_entry, working_directory, &[]).is_err());
    }
    let native = working_directory.join("server.exe");
    let (_, arguments) = build_spawn_command(&native, working_directory, &["a&b".into()]).unwrap();
    assert_eq!(arguments, ["a&b"]);
}

#[cfg(windows)]
#[test]
fn cmd_launch_with_c_flag_is_script_entrypoint() {
    let executable = Path::new(r"C:\Windows\System32\cmd.exe");
    let cmd_with_c = vec![String::from("/C"), String::from("start")];
    let cmd_with_k = vec![String::from("/K"), String::from("echo")];
    let cmd_without_switch = vec![String::from("echo"), String::from("hello")];

    assert!(is_script_entrypoint(executable, &cmd_with_c));
    assert!(is_script_entrypoint(executable, &cmd_with_k));
    assert!(!is_script_entrypoint(executable, &cmd_without_switch));
}

#[cfg(windows)]
#[test]
fn build_command_line_keeps_batch_preview_readable() {
    let executable =
        Path::new(r"D:\Example\LanGame Server Manager\instances\demo\config\StartServer.bat");
    let args = vec![String::from(
        r"D:\Example\LanGame Server Manager\instances\demo\data",
    )];

    let command_line = build_command_line(executable, &args);

    assert!(command_line.to_ascii_lowercase().contains("cmd.exe"));
    assert!(command_line.contains("/C"));
    assert!(command_line.contains(&format!("\"{}\"", executable.to_string_lossy())));
    assert!(command_line.ends_with(&format!("\"{}\"", args[0])));
}

#[test]
fn windows_argument_quoting_preserves_quotes_and_trailing_backslashes() {
    assert_eq!(
        quote_command_segment(r#"value with "quotes""#),
        r#""value with \"quotes\"""#
    );
    assert_eq!(
        quote_command_segment(r"C:\save path\"),
        r#""C:\save path\\""#
    );
}
