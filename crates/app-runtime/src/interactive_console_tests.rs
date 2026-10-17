use super::*;

#[cfg(not(windows))]
#[test]
fn unsupported_hosts_do_not_start_a_process() {
    let error = EphemeralConsole::spawn(Path::new("/missing"), &[], Path::new("/"), Vec::new())
        .err()
        .expect("non-Windows console must be unsupported");
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    #[derive(Clone, Default)]
    struct MemoryOutput(Arc<Mutex<Vec<u8>>>);

    impl MemoryOutput {
        fn contains(&self, text: &str) -> bool {
            String::from_utf8_lossy(&self.0.lock().unwrap()).contains(text)
        }
    }

    impl Write for MemoryOutput {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let mut output = self.0.lock().unwrap();
            if bytes.len() > (64 * 1024usize).saturating_sub(output.len()) {
                return Err(io::Error::other("synthetic memory output limit"));
            }
            output.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = crate::test_support::unique_test_root();
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn spawn(&self, script: &str, output: impl Write + Send + 'static) -> EphemeralConsole {
            let executable = crate::windows_system_directory()
                .unwrap()
                .join("WindowsPowerShell/v1.0/powershell.exe");
            let script = format!(
                "$ErrorActionPreference='Stop'; [Console]::InputEncoding=[Text.UTF8Encoding]::new($false); [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); {script}"
            );
            let arguments = [
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ]
            .map(str::to_owned);
            EphemeralConsole::spawn(&executable, &arguments, &self.0, output).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn await_text(console: &mut EphemeralConsole, output: &MemoryOutput, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !output.contains(text) {
            // Root exit can precede the reader's delivery of its final frame.
            // Keep the owner alive while waiting for that bounded output drain.
            let _ = console.try_wait().unwrap();
            assert!(
                Instant::now() < deadline,
                "fixture did not produce expected output"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn process_handle(console: &EphemeralConsole) -> crate::OwnedWindowsHandle {
        let Some(crate::RuntimeChild::Windows(child)) = console.child.as_ref() else {
            panic!("expected owned Windows process");
        };
        crate::duplicate_inheritable_handle(child.process_handle as *mut std::ffi::c_void).unwrap()
    }

    #[test]
    fn unterminated_prompt_accepts_console_input_and_retains_exit_code_without_files() {
        fn assert_send<T: Send>() {}
        assert_send::<EphemeralConsole>();
        let fixture = Fixture::new();
        let output = MemoryOutput::default();
        let mut console = fixture.spawn(
            "[Console]::Write('redirected:'+ [Console]::IsInputRedirected+':'+[Console]::IsOutputRedirected+' Password: '); $line=[Console]::ReadLine(); if($line -cne 'synthetic-玩家'){exit 9}; [Console]::WriteLine('accepted-length:'+$line.Length); exit 23",
            output.clone(),
        );
        await_text(&mut console, &output, "Password:");
        assert!(output.contains("redirected:False:False"));
        console.write_line("synthetic-玩家").unwrap();
        await_text(&mut console, &output, "accepted-length:12");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(code) = console.try_wait().unwrap() {
                assert_eq!(code, 23);
                break;
            }
            assert!(Instant::now() < deadline, "fixture did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        console.terminate_and_wait(Duration::from_secs(5)).unwrap();
        assert_eq!(console.try_wait().unwrap(), Some(23));
        assert_eq!(
            console.write_line("later").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 0);
    }

    #[test]
    fn exhausted_cleanup_budget_keeps_ownership_and_retry_reaps_the_complete_job() {
        let fixture = Fixture::new();
        let output = MemoryOutput::default();
        let mut console = fixture.spawn(
            "$p=Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList '-NoProfile -NonInteractive -Command Start-Sleep -Seconds 120' -WindowStyle Hidden -PassThru; [Console]::WriteLine('descendant:'+$p.Id); Start-Sleep -Seconds 120",
            output.clone(),
        );
        await_text(&mut console, &output, "descendant:");
        let root = process_handle(&console);
        let Some(crate::RuntimeChild::Windows(child)) = console.child.as_ref() else {
            unreachable!()
        };
        assert!(child.job.as_ref().unwrap().active_process_count().unwrap() >= 2);
        assert_eq!(
            console
                .terminate_and_wait(Duration::ZERO)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(console.child.is_some());
        assert!(console.try_wait().unwrap().is_none());
        console.terminate_and_wait(Duration::from_secs(5)).unwrap();
        assert!(console.child.is_none());
        assert_eq!(
            unsafe { crate::WaitForSingleObject(root.as_raw(), 0) },
            crate::WAIT_OBJECT_0
        );
    }

    #[test]
    fn dropping_a_live_console_terminates_its_owned_process() {
        let fixture = Fixture::new();
        let output = MemoryOutput::default();
        let mut console = fixture.spawn(
            "[Console]::Write('ready'); Start-Sleep -Seconds 120",
            output.clone(),
        );
        await_text(&mut console, &output, "ready");
        let process = process_handle(&console);
        let started = Instant::now();
        drop(console);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(
            unsafe { crate::WaitForSingleObject(process.as_raw(), 0) },
            crate::WAIT_OBJECT_0
        );
    }

    #[test]
    fn empty_challenge_response_is_supported_but_multiline_input_is_rejected() {
        let fixture = Fixture::new();
        let output = MemoryOutput::default();
        let mut console = fixture.spawn(
            "[Console]::Write('continue:'); $line=[Console]::ReadLine(); if($line.Length -ne 0){exit 9}; [Console]::WriteLine('empty-accepted'); Start-Sleep -Seconds 120",
            output.clone(),
        );
        await_text(&mut console, &output, "continue:");
        for invalid in ["one\ntwo", "one\rtwo", "one\0two"] {
            assert_eq!(
                console.write_line(invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        console.write_line("").unwrap();
        await_text(&mut console, &output, "empty-accepted");
        console.terminate_and_wait(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn the_console_is_not_visible_and_uses_a_private_desktop() {
        let fixture = Fixture::new();
        let output = MemoryOutput::default();
        let mut console = fixture.spawn(r#"
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class ConsoleProbe {
 [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr handle);
 [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
 [DllImport("user32.dll")] public static extern IntPtr GetThreadDesktop(uint thread);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool GetUserObjectInformationW(IntPtr handle, int index, StringBuilder buffer, uint length, out uint needed);
}
'@
$name=[Text.StringBuilder]::new(256); $needed=0;
if(-not [ConsoleProbe]::GetUserObjectInformationW([ConsoleProbe]::GetThreadDesktop([ConsoleProbe]::GetCurrentThreadId()),2,$name,512,[ref]$needed)){exit 9}
[Console]::WriteLine('desktop:'+$name.ToString());
[Console]::WriteLine('console-visible:'+ [ConsoleProbe]::IsWindowVisible([ConsoleProbe]::GetConsoleWindow()));
Start-Sleep -Seconds 120
"#, output.clone());
        let desktop_name = console.desktop.as_ref().unwrap().name.clone();
        assert!(!desktop_name.eq_ignore_ascii_case("default"));
        await_text(&mut console, &output, &format!("desktop:{desktop_name}"));
        await_text(&mut console, &output, "console-visible:False");
        console.terminate_and_wait(Duration::from_secs(5)).unwrap();
    }

    struct RejectOutput;
    impl Write for RejectOutput {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic-sensitive-sink-error"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn output_failure_is_visible_without_returning_sensitive_sink_errors() {
        let fixture = Fixture::new();
        let mut console = fixture.spawn(
            "[Console]::Write('ready'); Start-Sleep -Seconds 120",
            RejectOutput,
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        let error = loop {
            match console.try_wait() {
                Err(error) => break error,
                Ok(_) => {
                    assert!(Instant::now() < deadline, "output failure was not reported");
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };
        assert!(error.to_string().contains("output is incomplete"));
        assert!(!error.to_string().contains("synthetic-sensitive"));
        console.terminate_and_wait(Duration::from_secs(5)).unwrap();
    }
}
