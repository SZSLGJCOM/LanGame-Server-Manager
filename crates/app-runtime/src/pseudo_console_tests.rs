use super::*;
use std::path::Path;

#[test]
fn idle_nonblocking_output_pipe_is_not_eof() {
    let (read, _write) = create_pipe().unwrap();
    set_nonblocking(read.as_raw()).unwrap();
    let pipe = unsafe { File::from_raw_handle(read.into_raw()) };
    assert_eq!(
        read_pipe(&pipe, &mut [0; 8]).unwrap_err().raw_os_error(),
        Some(ERROR_NO_DATA)
    );
}

#[test]
fn full_terminal_input_pipe_times_out_and_rejects_future_commands() {
    let (_read, write) = create_pipe().unwrap();
    set_nonblocking(write.as_raw()).unwrap();
    let mut pipe = unsafe { File::from_raw_handle(write.into_raw()) };
    let fill = [b'x'; 4096];
    while pipe.write(&fill).unwrap() != 0 {}
    let mut input = PseudoConsoleInput {
        pipe: Some(pipe),
        state: Arc::new(TerminalState::default()),
    };
    let started = Instant::now();
    let error = input
        .write_line_with_budget(
            "clientlist",
            Duration::from_millis(40),
            &RuntimeStdinCancellation::default(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(input.pipe.is_none());
    assert!(!input.state.output_failed.load(Ordering::Acquire));
    assert_eq!(
        input.write_line("exit").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[test]
fn terminal_input_rejects_control_sequences_before_writing() {
    let (_read, write) = create_pipe().unwrap();
    set_nonblocking(write.as_raw()).unwrap();
    let mut input = PseudoConsoleInput {
        pipe: Some(unsafe { File::from_raw_handle(write.into_raw()) }),
        state: Arc::new(TerminalState::default()),
    };
    for command in ["clientlist\nexit", "\x1b[1A", "\0", &"x".repeat(4097)] {
        assert_eq!(
            input.write_line(command).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert!(!input.state.output_failed.load(Ordering::Acquire));
}

#[test]
fn cancelled_terminal_dispatch_writes_nothing_and_preserves_healthy_input() {
    let (read, write) = create_pipe().unwrap();
    set_nonblocking(read.as_raw()).unwrap();
    set_nonblocking(write.as_raw()).unwrap();
    let read = unsafe { File::from_raw_handle(read.into_raw()) };
    let mut input = RuntimeStdin::PseudoConsole(PseudoConsoleInput {
        pipe: Some(unsafe { File::from_raw_handle(write.into_raw()) }),
        state: Arc::new(TerminalState::default()),
    });
    let cancellation = RuntimeStdinCancellation::default();
    cancellation.cancel();
    assert_eq!(
        input
            .write_stdin_line_with_cancellation("cancelled", &cancellation)
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
    let mut buffer = [0; 32];
    assert_eq!(
        read_pipe(&read, &mut buffer).unwrap_err().raw_os_error(),
        Some(ERROR_NO_DATA)
    );
    input.write_stdin_line("accepted").unwrap();
    let count = read_pipe(&read, &mut buffer).unwrap();
    assert_eq!(&buffer[..count], b"accepted\r");
}

#[test]
fn terminal_cancellation_after_partial_write_closes_input_without_disabling_output() {
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    assert_ne!(
        unsafe { crate::CreatePipe(&mut read, &mut write, std::ptr::null_mut(), 4096) },
        0
    );
    let read = unsafe { File::from_raw_handle(read) };
    let write = unsafe { File::from_raw_handle(write) };
    set_nonblocking(write.as_raw_handle()).unwrap();
    let state = Arc::new(TerminalState::default());
    let mut input = PseudoConsoleInput {
        pipe: Some(write),
        state: Arc::clone(&state),
    };
    let cancellation = RuntimeStdinCancellation::default();
    let canceller = cancellation.clone();
    let observer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut accepted = 0;
        loop {
            let ok = unsafe {
                PeekNamedPipe(
                    read.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut accepted,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || accepted > 0 || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // Do not consume any bytes: the 4097-byte command cannot fit in this
        // 4096-byte pipe, so cancellation necessarily interrupts a partial write.
        canceller.cancel();
        (read, accepted)
    });
    let result = input.write_line_with_cancellation(&"x".repeat(4096), &cancellation);
    let (mut read, accepted) = observer.join().unwrap();
    assert!(accepted > 0 && accepted < 4097);
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert!(input.pipe.is_none());
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut read, &mut bytes).unwrap();
    assert_eq!(bytes, vec![b'x'; accepted as usize]);
    assert_eq!(
        input.write_line("later").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert!(!state.output_failed.load(Ordering::Acquire));
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn PeekNamedPipe(
        pipe: *mut c_void,
        buffer: *mut c_void,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        left: *mut u32,
    ) -> i32;
}

#[test]
fn terminal_input_failure_preserves_subsequent_diagnostic_output() {
    let (read, write) = create_pipe().unwrap();
    drop(read);
    let state = Arc::new(TerminalState::default());
    let mut input = PseudoConsoleInput {
        pipe: Some(unsafe { File::from_raw_handle(write.into_raw()) }),
        state: Arc::clone(&state),
    };
    assert!(input.write_line("clientlist").is_err());
    assert!(input.pipe.is_none());

    let (read, write) = create_pipe().unwrap();
    let mut write = unsafe { File::from_raw_handle(write.into_raw()) };
    write
        .write_all(b"diagnostic after input failure\r\n")
        .unwrap();
    drop(write);
    let mut transcript = Vec::new();
    drain_output(
        unsafe { File::from_raw_handle(read.into_raw()) },
        &mut transcript,
        Arc::clone(&state),
    );
    assert_eq!(transcript, b"diagnostic after input failure\n");
    assert!(!state.output_failed.load(Ordering::Acquire));
}

#[test]
fn terminal_output_failure_does_not_disable_server_stop_commands() {
    let (read, write) = create_pipe().unwrap();
    set_nonblocking(read.as_raw()).unwrap();
    set_nonblocking(write.as_raw()).unwrap();
    let state = Arc::new(TerminalState::default());
    state.output_failed.store(true, Ordering::Release);
    let read = unsafe { File::from_raw_handle(read.into_raw()) };
    let mut input = PseudoConsoleInput {
        pipe: Some(unsafe { File::from_raw_handle(write.into_raw()) }),
        state,
    };
    input.write_line("exit").unwrap();
    let mut buffer = [0; 32];
    let count = read_pipe(&read, &mut buffer).unwrap();
    assert_eq!(&buffer[..count], b"exit\r");
}

struct ProbeDirectory(std::path::PathBuf);

impl ProbeDirectory {
    fn create() -> Self {
        let path = crate::test_support::unique_test_root();
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.join("terminal.log"));
        let _ = std::fs::remove_dir(&self.0);
    }
}

#[test]
fn terminal_eof_preserves_the_final_unterminated_line() {
    let root = ProbeDirectory::create();
    let log_path = root.0.join("terminal.log");
    let (read, write) = create_pipe().unwrap();
    let mut write = unsafe { File::from_raw_handle(write.into_raw()) };
    write.write_all(b"first\r\nlast message").unwrap();
    drop(write);
    let state = Arc::new(TerminalState::default());
    drain_output(
        unsafe { File::from_raw_handle(read.into_raw()) },
        File::create(&log_path).unwrap(),
        Arc::clone(&state),
    );

    assert!(!state.output_failed.load(Ordering::Acquire));
    assert_eq!(
        std::fs::read_to_string(log_path).unwrap(),
        "first\nlast message\n"
    );
}

#[test]
fn terminal_close_drains_asynchronous_exit_output_until_eof() {
    let root = ProbeDirectory::create();
    let log_path = root.0.join("terminal.log");
    let (read, write) = create_pipe().unwrap();
    set_nonblocking(read.as_raw()).unwrap();
    let mut write = unsafe { File::from_raw_handle(write.into_raw()) };
    let state = Arc::new(TerminalState::default());
    state.closed.store(true, Ordering::Release);
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(40));
        write.write_all(b"saved before exit\r\n")
    });
    drain_output(
        unsafe { File::from_raw_handle(read.into_raw()) },
        File::create(&log_path).unwrap(),
        Arc::clone(&state),
    );
    let write_result = writer.join().unwrap();

    assert_eq!(
        std::fs::read_to_string(log_path).unwrap(),
        "saved before exit\n"
    );
    assert!(write_result.is_ok());
    assert!(!state.output_failed.load(Ordering::Acquire));
}

#[test]
fn terminal_close_marks_an_unclosed_output_channel_as_failed() {
    let root = ProbeDirectory::create();
    let log_path = root.0.join("terminal.log");
    let (read, write) = create_pipe().unwrap();
    set_nonblocking(read.as_raw()).unwrap();
    let mut write = unsafe { File::from_raw_handle(write.into_raw()) };
    write.write_all(b"last known partial row").unwrap();
    let state = Arc::new(TerminalState::default());
    state.closed.store(true, Ordering::Release);
    let started = Instant::now();
    drain_output(
        unsafe { File::from_raw_handle(read.into_raw()) },
        File::create(&log_path).unwrap(),
        Arc::clone(&state),
    );

    assert!(state.output_failed.load(Ordering::Acquire));
    assert!(started.elapsed() < Duration::from_secs(10));
    let output = std::fs::read_to_string(log_path).unwrap();
    assert!(output.starts_with("last known partial row\n"), "{output}");
    assert!(
        output.contains("[LanGame] Terminal output capture stopped:"),
        "{output}"
    );
    assert!(output.contains("before EOF"), "{output}");
}

#[test]
fn pseudo_console_provides_real_console_input_output_and_owned_cleanup() {
    let root = ProbeDirectory::create();
    let log_path = root.0.join("terminal.log");
    let log = File::create(&log_path).unwrap();
    let system = std::env::var_os("SystemRoot").unwrap();
    let executable = Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let args = ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "[Console]::WriteLine('ready:'+[Console]::IsInputRedirected+':'+[Console]::IsOutputRedirected); $line=[Console]::ReadLine(); [Console]::WriteLine('reply:'+$line); Start-Sleep -Seconds 120"].map(str::to_owned);
    let command = crate::SpawnCommand {
        executable: executable.to_str().unwrap(),
        args: &args,
        working_directory: &root.0,
        environment: &Default::default(),
    };
    let (mut child, desktop) = spawn(&command, log, false, None).unwrap();
    assert!(desktop.is_none());
    let state = match &child {
        RuntimeChild::Windows(child) => Arc::clone(&child.terminal.as_ref().unwrap().state),
        _ => panic!("expected managed Windows console"),
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let read_log = || std::fs::read_to_string(&log_path).unwrap();
    while !read_log().contains("ready:False:False\n") {
        assert!(
            !state.output_failed.load(Ordering::Acquire),
            "terminal transcript failed"
        );
        assert!(
            Instant::now() < deadline,
            "console never became ready: {}",
            read_log()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut input = child.take_stdin().unwrap();
    input.write_stdin_line("terminal-玩家").unwrap();
    while !read_log().contains("reply:terminal-玩家\n") {
        assert!(
            !state.output_failed.load(Ordering::Acquire),
            "terminal transcript failed"
        );
        assert!(
            Instant::now() < deadline,
            "console did not accept Unicode input: {}",
            read_log()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let process = match &child {
        RuntimeChild::Windows(child) => {
            crate::duplicate_inheritable_handle(child.process_handle as *mut c_void).unwrap()
        }
        _ => unreachable!(),
    };
    let started = Instant::now();
    drop(child);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(state.closed.load(Ordering::Acquire));
    assert_eq!(
        unsafe { crate::WaitForSingleObject(process.as_raw(), 3000) },
        crate::WAIT_OBJECT_0
    );
    assert_eq!(
        input.write_stdin_line("later").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert!(!read_log().contains('\u{1b}'));
}

#[test]
fn pseudo_console_failed_spawn_cleans_up_reader_and_handles() {
    let root = ProbeDirectory::create();
    let started = Instant::now();
    assert!(
        spawn(
            &crate::SpawnCommand {
                executable: root.0.join("missing-executable.exe").to_str().unwrap(),
                args: &[],
                working_directory: &root.0,
                environment: &Default::default(),
            },
            File::create(root.0.join("terminal.log")).unwrap(),
            true,
            None
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn background_pseudo_console_keeps_native_windows_on_private_desktop() {
    let root = ProbeDirectory::create();
    let log_path = root.0.join("terminal.log");
    let system = std::env::var_os("SystemRoot").unwrap();
    let executable = Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let script = r#"
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class WindowProbe {
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr CreateWindowExW(uint ex, string cls, string title, uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern IntPtr GetThreadDesktop(uint thread);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern bool GetUserObjectInformationW(IntPtr handle, int index, StringBuilder text, uint length, out uint needed);
    public static string Desktop(IntPtr window) {
        uint process, needed;
        var thread = GetWindowThreadProcessId(window, out process);
        var text = new StringBuilder(256);
        if (!GetUserObjectInformationW(GetThreadDesktop(thread), 2, text, 512, out needed)) throw new Win32Exception();
        return text.ToString();
    }
}
'@
$window = [WindowProbe]::CreateWindowExW(0, 'STATIC', 'LanGame ConPTY isolation probe', 0, 0, 0, 10, 10, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero)
if ($window -eq [IntPtr]::Zero) { throw 'CreateWindowExW failed' }
try {
    [Console]::WriteLine('window-desktop:' + [WindowProbe]::Desktop($window))
    [Console]::WriteLine('reply:' + [Console]::ReadLine())
} finally { [void][WindowProbe]::DestroyWindow($window) }
"#;
    let plan = app_core::LaunchPlan {
        environment: Default::default(),
        instance_id: "console-desktop-probe".into(),
        instance_name: "Console desktop probe".into(),
        module_id: "console-desktop-probe".into(),
        install_root: root.0.to_string_lossy().into_owned(),
        install_state: app_core::InstallState::Installed,
        uses_private_runtime: true,
        working_directory: root.0.to_string_lossy().into_owned(),
        executable_path: executable.to_string_lossy().into_owned(),
        executable_exists: true,
        ready_to_launch: true,
        validation_issues: Vec::new(),
        args: [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ]
        .map(str::to_owned)
        .to_vec(),
        command_line: String::new(),
        window_policy: app_core::ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: app_core::ProcessHostSurface::ManagedPseudoConsole,
        host_notes: None,
        performance_policy: Default::default(),
        performance_preview: Default::default(),
    };
    let mut spawned = crate::spawn_launch_plan(&plan, &log_path).unwrap();
    let desktop_name = &spawned
        .hidden_desktop
        .as_ref()
        .expect("background ConPTY must retain its private desktop")
        .name;
    let expected = format!("window-desktop:{desktop_name}\n");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !std::fs::read_to_string(&log_path)
        .unwrap()
        .contains(&expected)
    {
        assert!(
            Instant::now() < deadline,
            "native window is not on the managed desktop: {}",
            std::fs::read_to_string(&log_path).unwrap()
        );
        assert!(
            spawned
                .child
                .as_mut()
                .unwrap()
                .try_wait()
                .unwrap()
                .is_none()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let child = spawned.child.as_mut().unwrap();
    let mut input = child.take_stdin().unwrap();
    input.write_stdin_line("isolated-玩家").unwrap();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "isolated console failed: {status}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "isolated console did not exit normally"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(spawned);
    assert!(
        std::fs::read_to_string(&log_path)
            .unwrap()
            .contains("reply:isolated-玩家\n")
    );
    assert_eq!(
        input.write_stdin_line("later").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[test]
#[ignore = "requires BAROTRAUMA_CONSOLE_PROBE_DIR pointing to an isolated prepared server with a langame-console-probe marker"]
fn barotrauma_isolated_production_console_accepts_correlated_empty_lists_and_exit() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("BAROTRAUMA_CONSOLE_PROBE_DIR").expect("isolated server directory"),
    );
    // Keep the ordinary absolute launch path, matching build_launch_plan.
    // .NET's XML URI loader in this server rejects canonicalize's \\?\ prefix.
    assert!(directory.is_absolute());
    assert!(
        directory.join("langame-console-probe").is_file(),
        "explicit isolated-server marker is required"
    );
    let executable = directory.join("DedicatedServer.exe");
    assert!(executable.is_file());
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let log_path = directory.join(format!("conpty-runtime-probe-{nonce:x}.log"));
    let plan = app_core::LaunchPlan {
        environment: Default::default(),
        instance_id: "barotrauma-console-probe".into(),
        instance_name: "Isolated console probe".into(),
        module_id: "barotrauma".into(),
        install_root: directory.to_string_lossy().into_owned(),
        install_state: app_core::InstallState::Installed,
        uses_private_runtime: true,
        working_directory: directory.to_string_lossy().into_owned(),
        executable_path: executable.to_string_lossy().into_owned(),
        executable_exists: true,
        ready_to_launch: true,
        validation_issues: Vec::new(),
        args: [
            "-public",
            "false",
            "-upnp",
            "false",
            "-port",
            "37015",
            "-queryport",
            "37016",
            "-name",
            "LanGame isolated terminal validation",
        ]
        .map(str::to_owned)
        .to_vec(),
        command_line: String::new(),
        window_policy: app_core::ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: false,
        host_surface: app_core::ProcessHostSurface::ManagedPseudoConsole,
        host_notes: None,
        performance_policy: app_core::RuntimePerformancePolicy::default(),
        performance_preview: app_core::RuntimePerformancePolicyPreview::default(),
    };
    let mut spawned = crate::spawn_launch_plan(&plan, &log_path).unwrap();
    let read_log = || std::fs::read_to_string(&log_path).unwrap();
    let child = spawned.child.as_mut().unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    while !read_log().contains("Selected shuttle:") {
        assert!(
            child.try_wait().unwrap().is_none(),
            "server exited before ready"
        );
        assert!(Instant::now() < deadline, "server did not become ready");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut input = child.take_stdin().unwrap();
    for index in 0..2 {
        let command = format!("clientlist LGM_PLAYER_QUERY_{:032x}", nonce + index);
        input.write_stdin_line(&command).unwrap();
        let expected = format!("{command}\n***************\n***************\n");
        let response_deadline = Instant::now() + Duration::from_secs(5);
        while !read_log().contains(&expected) {
            assert!(
                Instant::now() < response_deadline,
                "correlated empty response missing"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    input.write_stdin_line("exit").unwrap();
    let exit_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(0));
            break;
        }
        assert!(Instant::now() < exit_deadline, "server did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(spawned);
    assert!(!read_log().contains('\u{1b}'));
    assert!(!read_log().contains("input will be ignored"));
    println!(
        "Validated isolated ConPTY clientlist nonce framing and exit; transcript: {}",
        log_path.display()
    );
}
