use super::*;

#[test]
fn carriage_returns_and_partial_chunks_report_before_eof() {
    let mut output = PreparationOutput::default();
    assert_eq!(
        output.push(0, b"Downloading update (1 of 4 KB)\r"),
        "Downloading update (1 of 4 KB)"
    );
    assert_eq!(output.push(1, b"network diagnostic"), "network diagnostic");
    output.push(0, b"Update complete, lau");
    assert!(!output.self_updated);
    output.push(0, b"nching...");
    assert!(output.self_updated);
    assert!(output.excerpt().contains("network diagnostic"));
}

#[test]
fn output_and_partial_lines_remain_bounded_and_keep_recent_diagnostics() {
    let mut output = PreparationOutput::default();
    for _ in 0..100 {
        output.push(0, &vec![b'a'; OUTPUT_CHUNK_BYTES]);
        output.push(0, b"\r");
    }
    output.push(1, &vec![b'b'; OUTPUT_CHUNK_BYTES * 2]);
    output.push(1, b"\rfinal network diagnostic");
    assert!(output.excerpt().len() <= EXCERPT_LIMIT_BYTES);
    assert!(output.excerpt().contains("final network diagnostic"));
    assert!(output.excerpt().contains("line truncated"));
}

#[test]
fn completion_marker_survives_chunk_boundaries_in_an_oversized_line() {
    let mut output = PreparationOutput::default();
    output.push(0, &vec![b'x'; OUTPUT_CHUNK_BYTES]);
    output.push(0, b"Update complete, lau");
    let mut next = b"nching".to_vec();
    next.extend(vec![b'x'; OUTPUT_CHUNK_BYTES - next.len()]);
    output.push(0, &next);
    assert!(output.self_updated);
}

#[cfg(windows)]
mod windows {
    use std::fs;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::{apply_no_window, ps_literal, run_powershell};
    use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "langame-prepare-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn command(script: &str) -> Command {
        let mut command = Command::new("powershell");
        apply_no_window(&mut command);
        command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        command
    }

    fn deadline() -> InstallDeadline {
        InstallDeadline::new("preparation fixture", Duration::from_secs(15))
    }

    #[tokio::test]
    async fn fresh_bootstrap_log_progress_keeps_silent_process_alive_without_replaying_history() {
        let root = TestDirectory::new();
        let logs = root.0.join("logs");
        fs::create_dir_all(&logs).unwrap();
        let log_path = logs.join("bootstrap_log.txt");
        fs::write(&log_path, b"historical Update complete, launching...\n").unwrap();
        let line = "[2026-09-20 11:27:58] 正在下载更新 (已下载 397，共 10,673 KB)...";
        let script = format!(
            "$encoding=New-Object Text.UTF8Encoding($false); for ($i=0; $i -lt 8; $i++) {{ [IO.File]::AppendAllText({}, '{}'+[Environment]::NewLine, $encoding); Start-Sleep -Milliseconds 500 }}; [Console]::WriteLine('Steam Console Client (c) Valve Corporation - version fixture'); [Console]::WriteLine('Loading Steam API...OK'); exit 0",
            ps_literal(&log_path),
            line,
        );
        let mut command = command(&script);
        command.current_dir(&root.0);
        let mut progress = Vec::new();
        let outcome = run_preparation_command(
            command,
            deadline(),
            Duration::from_secs(3),
            true,
            &mut PrepareReporter::new(|update| progress.push(update)),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PreparationAttempt::Verified);
        assert!(progress.iter().any(|update| update.detail == line
            && update.downloaded_bytes == Some(397 * 1024)
            && update.total_bytes == Some(10673 * 1024)));
        assert!(
            progress
                .iter()
                .all(|update| !update.output_excerpt.contains("historical"))
        );
    }

    #[tokio::test]
    async fn completed_process_drains_new_bootstrap_log_before_deciding_handoff() {
        let root = TestDirectory::new();
        let logs = root.0.join("logs");
        fs::create_dir_all(&logs).unwrap();
        let log_path = logs.join("bootstrap_log.txt");
        let mut command = command(&format!(
            "[IO.File]::WriteAllText({}, 'Update complete, launching...'+[Environment]::NewLine); exit 0",
            ps_literal(&log_path),
        ));
        command.current_dir(&root.0);
        let outcome = run_preparation_command(
            command,
            deadline(),
            Duration::from_secs(3),
            true,
            &mut PrepareReporter::new(|_| {}),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PreparationAttempt::SelfUpdateHandoff);
    }

    async fn assert_process_gone(pid_path: &Path) {
        let pid = fs::read_to_string(pid_path)
            .unwrap()
            .parse::<u32>()
            .unwrap();
        let check = run_powershell(
            &format!("if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 1 }}"),
            None,
            deadline(),
        )
        .await
        .unwrap();
        assert!(
            check.status.success(),
            "owned process {pid} survived preparation cleanup"
        );
    }

    #[tokio::test]
    async fn stdout_and_stderr_progress_unblocks_a_running_process_without_newlines() {
        let root = TestDirectory::new();
        let gate = root.0.join("progress-observed");
        let gate_literal = ps_literal(&gate);
        let script = format!(
            concat!(
                "[Console]::Write('partial stdout'); [Console]::Out.Flush(); ",
                "[Console]::Error.Write('partial stderr'); [Console]::Error.Flush(); ",
                "$end=[DateTime]::UtcNow.AddSeconds(10); ",
                "while (-not [IO.File]::Exists({gate}) -and [DateTime]::UtcNow -lt $end) {{ Start-Sleep -Milliseconds 20 }}; ",
                "if (-not [IO.File]::Exists({gate})) {{ exit 8 }}; [Console]::WriteLine('Steam Console Client (c) Valve Corporation - version fixture'); [Console]::WriteLine('Loading Steam API...OK'); exit 0"
            ),
            gate = gate_literal
        );
        let mut reporter = PrepareReporter::new(|progress: SteamCmdPrepareProgress| {
            if progress.output_excerpt.contains("partial stdout")
                && progress.output_excerpt.contains("partial stderr")
            {
                fs::write(&gate, b"observed while running").unwrap();
            }
        });
        let outcome = run_preparation_command(
            command(&script),
            deadline(),
            Duration::from_secs(3),
            false,
            &mut reporter,
        )
        .await
        .unwrap();
        assert_eq!(outcome, PreparationAttempt::Verified);
        assert!(gate.exists());
    }

    #[tokio::test]
    async fn idle_timeout_retains_both_streams_and_reaps_the_process() {
        let root = TestDirectory::new();
        let pid_path = root.0.join("pid");
        let script = format!(
            "[IO.File]::WriteAllText({}, [string]$PID); [Console]::Write('last stdout'); [Console]::Error.Write('last stderr'); Start-Sleep -Seconds 10",
            ps_literal(&pid_path)
        );
        let error = run_preparation_command(
            command(&script),
            deadline(),
            Duration::from_secs(3),
            false,
            &mut PrepareReporter::new(|_| {}),
        )
        .await
        .unwrap_err();
        match error {
            SteamCmdError::SteamCmdPreparationStalled { output_excerpt, .. } => {
                assert!(output_excerpt.contains("last stdout"));
                assert!(output_excerpt.contains("last stderr"));
            }
            error => panic!("expected idle timeout, got {error}"),
        }
        assert_process_gone(&pid_path).await;
    }

    #[tokio::test]
    async fn total_timeout_retains_output_and_reaps_the_process() {
        let root = TestDirectory::new();
        let output_path = root.0.join("output-flushed");
        let release_path = root.0.join("release");
        let completed_path = root.0.join("completed");
        let script = format!(
            concat!(
                "[Console]::Write('deadline diagnostic'); [Console]::Out.Flush(); ",
                "[IO.File]::WriteAllText({}, 'flushed'); ",
                "while (-not [IO.File]::Exists({})) {{ Start-Sleep -Milliseconds 10 }}; ",
                "[IO.File]::WriteAllText({}, 'completed')"
            ),
            ps_literal(&output_path),
            ps_literal(&release_path),
            ps_literal(&completed_path),
        );
        let mut command = command(&script);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut child, mut guard) = crate::spawn_managed_command(&mut command).await.unwrap();
        const SYNCHRONIZE: u32 = 0x00100000;
        // Keep the actual process object so PID reuse cannot satisfy cleanup.
        let raw = unsafe { OpenProcess(SYNCHRONIZE, 0, child.id().unwrap()) };
        if raw.is_null() {
            let error = std::io::Error::last_os_error();
            terminate_and_reap_preparation_tree(&mut child, &mut guard)
                .await
                .unwrap();
            panic!("open owned fixture process: {error}");
        }
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        // PowerShell cold startup has a separate bounded handshake. The output
        // is already in the real pipe before testing the unchanged 3s budget.
        let ready = tokio::time::timeout(Duration::from_secs(20), async {
            while !output_path.exists() {
                if child.try_wait()?.is_some() {
                    return Err(std::io::Error::other("fixture exited before readiness"));
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok::<_, std::io::Error>(())
        })
        .await;
        if !matches!(ready, Ok(Ok(()))) {
            terminate_and_reap_preparation_tree(&mut child, &mut guard)
                .await
                .unwrap();
            panic!("fixture must finish its output handshake: {ready:?}");
        }
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) },
            WAIT_TIMEOUT
        );
        let total = InstallDeadline::new("preparation total", Duration::from_secs(3));
        let error = capture_preparation_process(
            child,
            guard,
            None,
            total,
            Duration::from_secs(10),
            false,
            &mut PrepareReporter::new(|_| {}),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&error, SteamCmdError::SteamCmdPreparationTimedOut { timeout_seconds: 3, output_excerpt } if output_excerpt.contains("deadline diagnostic")),
            "expected timeout with emitted diagnostics, got {error:?}"
        );
        assert!(Instant::now() >= total.expires_at());
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) },
            WAIT_OBJECT_0
        );
        assert!(!completed_path.exists());
    }

    #[tokio::test]
    async fn expired_total_deadline_rejects_preparation_before_starting_a_process() {
        let root = TestDirectory::new();
        // An attempted spawn would produce a different error for this absent
        // executable. The entry point must reject the elapsed budget first.
        let command = Command::new(root.0.join("must-not-start.exe"));
        let error = run_preparation_command(
            command,
            InstallDeadline::new("preparation total", Duration::ZERO),
            Duration::from_secs(10),
            false,
            &mut PrepareReporter::new(|_| {}),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                error,
                SteamCmdError::OperationTimedOut {
                    operation: "preparation total",
                    timeout_seconds: 0,
                }
            ),
            "expected exhausted budget before spawn, got {error:?}"
        );
    }

    #[tokio::test]
    async fn idle_timeout_reaps_descendant_holding_output_after_parent_exit() {
        let root = TestDirectory::new();
        let pid_path = root.0.join("child-pid");
        let child_script = root.0.join("child.ps1");
        fs::write(&child_script, format!("[IO.File]::WriteAllText({}, [string]$PID); [Console]::Write('inherited pipe diagnostic'); Start-Sleep -Seconds 10", ps_literal(&pid_path))).unwrap();
        let script = format!(
            concat!(
                "$info=New-Object Diagnostics.ProcessStartInfo; $info.FileName='powershell.exe'; ",
                "$info.Arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"' + {child} + '\"'; ",
                "$info.UseShellExecute=$false; $info.CreateNoWindow=$true; ",
                "$child=[Diagnostics.Process]::Start($info); ",
                "while (-not [IO.File]::Exists({pid})) {{ Start-Sleep -Milliseconds 20 }}; exit 0"
            ),
            child = ps_literal(&child_script),
            pid = ps_literal(&pid_path)
        );
        let error = run_preparation_command(
            command(&script),
            deadline(),
            Duration::from_secs(3),
            false,
            &mut PrepareReporter::new(|_| {}),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            SteamCmdError::SteamCmdPreparationStalled { .. }
        ));
        assert_process_gone(&pid_path).await;
    }

    #[tokio::test]
    async fn cancellation_drops_readers_and_terminates_owned_parent_and_descendant() {
        cancellation_fixture(false).await;
    }

    #[tokio::test]
    async fn explicit_stop_returns_only_after_the_owned_tree_has_exited() {
        cancellation_fixture(true).await;
    }

    async fn cancellation_fixture(explicit: bool) {
        let root = TestDirectory::new();
        let parent_pid_path = root.0.join("parent-pid");
        let child_pid_path = root.0.join("child-pid");
        let child_script = root.0.join("child.ps1");
        fs::write(
            &child_script,
            format!(
                "[IO.File]::WriteAllText({}, [string]$PID); Start-Sleep -Seconds 10",
                ps_literal(&child_pid_path)
            ),
        )
        .unwrap();
        let script = format!(
            concat!(
                "[IO.File]::WriteAllText({parent_pid}, [string]$PID); ",
                "$info=New-Object Diagnostics.ProcessStartInfo; $info.FileName='powershell.exe'; ",
                "$info.Arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"' + {child} + '\"'; ",
                "$info.UseShellExecute=$false; $info.CreateNoWindow=$true; ",
                "$child=[Diagnostics.Process]::Start($info); ",
                "while (-not [IO.File]::Exists({child_pid})) {{ Start-Sleep -Milliseconds 20 }}; ",
                "[Console]::WriteLine('fixture running'); Start-Sleep -Seconds 10"
            ),
            parent_pid = ps_literal(&parent_pid_path),
            child = ps_literal(&child_script),
            child_pid = ps_literal(&child_pid_path)
        );
        let (sender, started) = tokio::sync::oneshot::channel();
        let cancellation = crate::InstallCancellation::new();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut sender = Some(sender);
            let mut reporter = PrepareReporter::new(move |progress: SteamCmdPrepareProgress| {
                if progress.output_excerpt.contains("fixture running")
                    && let Some(sender) = sender.take()
                {
                    let _ = sender.send(());
                }
            });
            task_cancellation
                .scope(run_preparation_command(
                    command(&script),
                    deadline(),
                    Duration::from_secs(10),
                    false,
                    &mut reporter,
                ))
                .await
        });
        tokio::time::timeout(Duration::from_secs(10), started)
            .await
            .unwrap()
            .unwrap();
        if explicit {
            cancellation.cancel();
            assert!(matches!(
                task.await.unwrap(),
                Err(SteamCmdError::InstallCancelled { .. })
            ));
        } else {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        }
        assert_process_gone(&parent_pid_path).await;
        assert_process_gone(&child_pid_path).await;
    }

    #[tokio::test]
    async fn successful_bootstrap_exit_without_console_api_readiness_is_rejected() {
        let error = run_preparation_command(
            command("[Console]::WriteLine('Verification complete'); exit 0"),
            deadline(),
            Duration::from_secs(3),
            false,
            &mut PrepareReporter::new(|_| {}),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, SteamCmdError::PrepareSteamCmd { output_excerpt } if output_excerpt.contains("Bootstrap verification alone is insufficient"))
        );
    }
}
