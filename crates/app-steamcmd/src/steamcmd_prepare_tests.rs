use super::*;

#[test]
fn update_bytes_only_come_from_valid_steamcmd_download_counts() {
    assert_eq!(
        update_download_bytes("[ 12%] Downloading update (512 of 43472 KB)..."),
        Some((512 * 1024, 43472 * 1024))
    );
    assert_eq!(
        update_download_bytes("Downloading update (8 of 4 KB)"),
        None
    );
    assert_eq!(
        update_download_bytes("Downloading update (1 of 0 KB)"),
        None
    );
    assert_eq!(
        update_download_bytes("Checking for available updates..."),
        None
    );
    assert_eq!(
        update_download_bytes("Downloading update (38,216 of 43,472 KB)..."),
        Some((38216 * 1024, 43472 * 1024))
    );
    assert_eq!(
        update_download_bytes("Downloading update (43,472 of 43,472 KB)..."),
        Some((43472 * 1024, 43472 * 1024))
    );
    assert_eq!(
        update_download_bytes("Downloading update (3,82 of 43,472 KB)..."),
        None
    );
    assert_eq!(
        update_download_bytes("Downloading update (+3 of 43,472 KB)..."),
        None
    );
}

#[test]
fn chinese_bootstrap_log_reports_actual_download_bytes_and_updating_phase() {
    let line = "[2026-09-20 11:27:58] 正在下载更新 (已下载 397，共 10,673 KB)...";
    let mut reporter = PrepareReporter::new(|_| {});
    reporter.stage(SteamCmdPreparePhase::Verifying, "Checking SteamCMD");
    reporter.output(line.into(), line.into());
    assert_eq!(reporter.current.phase, SteamCmdPreparePhase::Updating);
    assert_eq!(reporter.current.downloaded_bytes, Some(397 * 1024));
    assert_eq!(reporter.current.total_bytes, Some(10673 * 1024));
    // Windows SteamCMD truncates console lines; the complete bootstrap log
    // supplies the byte counts without inventing a percentage from that text.
    assert_eq!(
        update_download_bytes("[ 4%] 正在下载更新 (已下载 477，共"),
        None
    );
    assert_eq!(
        update_download_bytes("正在下载更新 (已下载 397，共 10,67 KB)..."),
        None
    );
    assert_eq!(
        update_download_bytes("正在下载更新 (已下载 11,000，共 10,673 KB)..."),
        None
    );
}

#[test]
fn extracting_and_installing_do_not_reuse_download_completion_bytes() {
    let mut reporter = PrepareReporter::new(|_| {});
    for next in [
        "Extracting package...",
        "Installing update...",
        "Update complete, launching...",
        "正在解压软件包...",
        "正在安装更新...",
        "更新完成，正在启动...",
    ] {
        reporter.output(
            "Downloading update (43,472 of 43,472 KB)...".into(),
            String::new(),
        );
        assert_eq!(reporter.current.downloaded_bytes, Some(43472 * 1024));
        reporter.output(next.into(), String::new());
        assert!(reporter.current.downloaded_bytes.is_none());
        assert!(reporter.current.total_bytes.is_none());
    }
}

#[cfg(windows)]
mod windows {
    use super::*;

    #[tokio::test]
    async fn corrupt_existing_executable_falls_back_to_installation_sources() {
        let temporary = std::env::temp_dir().join(format!(
            "langame-corrupt-steamcmd-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = prepare_configured_steamcmd_root(&temporary).unwrap();
        let executable = root.join("steamcmd.exe");
        std::fs::write(&executable, b"damaged executable fixture").unwrap();
        let root_string = root.to_string_lossy().into_owned();
        let executable_string = executable.to_string_lossy().into_owned();
        let status = SteamCmdStatus {
            root: root_string.clone(),
            executable_path: executable_string.clone(),
            executable_exists: true,
            ready: false,
            configured_root: root_string,
            configured_executable_path: executable_string,
            source: SteamCmdSource::Configured,
            ownership: crate::SteamCmdOwnership::Managed,
            can_uninstall: true,
        };
        let mut progress = Vec::new();
        let result = verify_existing_installation(
            &status,
            InstallDeadline::new("corrupt executable fallback", Duration::from_secs(10)),
            &mut PrepareReporter::new(|update| progress.push(update)),
        )
        .await;
        std::fs::remove_dir_all(&root).unwrap();

        assert!(!result.expect("a pre-spawn failure allows the next installation source"));
        assert!(
            progress
                .iter()
                .any(|update| update.phase == SteamCmdPreparePhase::Inspecting
                    && update.detail.contains("checking installation sources")
                    && update.detail.contains("no process was created"))
        );
        assert!(
            !progress
                .iter()
                .any(|update| update.phase == SteamCmdPreparePhase::Ready)
        );
    }

    fn command(script: &str) -> Command {
        let mut command = Command::new("powershell");
        apply_no_window(&mut command);
        command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        command
    }

    #[tokio::test]
    async fn successful_self_update_requires_fresh_successful_verification() {
        let mut attempts = 0;
        let mut phases = Vec::new();
        let result = verify_with_command_factory(
            InstallDeadline::new("handoff verification", Duration::from_secs(15)),
            Duration::from_secs(3),
            &mut PrepareReporter::new(|progress| phases.push(progress.phase)),
            || {
                attempts += 1;
                if attempts == 1 {
                    command("[Console]::WriteLine('Update complete, launching...'); [Console]::WriteLine('Steam Console Client (c) Valve Corporation - version fixture'); [Console]::WriteLine('Loading Steam API...OK'); exit 0")
                } else {
                    command(
                        "[Console]::Error.WriteLine('fresh runtime failed validation'); exit 17",
                    )
                }
            },
        )
        .await;
        assert_eq!(attempts, 2);
        assert!(phases.contains(&SteamCmdPreparePhase::Verifying));
        assert!(!phases.contains(&SteamCmdPreparePhase::Ready));
        assert!(
            matches!(result, Err(SteamCmdError::PrepareSteamCmd { output_excerpt }) if output_excerpt.contains("fresh runtime failed validation"))
        );
    }

    #[tokio::test]
    async fn stalled_self_update_handoff_is_allowed_only_once() {
        let mut attempts = 0;
        let result = verify_with_command_factory(
            InstallDeadline::new("one bounded handoff", Duration::from_secs(15)),
            Duration::from_secs(3), &mut PrepareReporter::new(|_| {}),
            || {
                attempts += 1;
                command("[Console]::WriteLine('Update complete, launching...'); Start-Sleep -Seconds 10")
            },
        ).await;
        assert_eq!(attempts, 2);
        assert!(
            matches!(result, Err(SteamCmdError::SteamCmdPreparationStalled { output_excerpt, .. }) if output_excerpt.contains("Update complete, launching"))
        );
    }

    #[tokio::test]
    async fn self_update_handoff_waits_for_owned_tree_exit_before_verification() {
        let mut attempts = 0;
        let mut reporter = PrepareReporter::new(|_| {});
        // The second command checks its predecessor using a fixture file so the
        // same handoff code controls cleanup and the new process creation.
        let pid_path = std::env::temp_dir().join(format!(
            "langame-handoff-pid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let child_script = pid_path.with_extension("ps1");
        std::fs::write(&child_script, format!(
            "[IO.File]::WriteAllText({}, [string]$PID); [Console]::WriteLine('restarted child'); Start-Sleep -Seconds 10",
            ps_literal(&pid_path),
        )).unwrap();
        let parent_script = format!(
            concat!(
                "$info=New-Object Diagnostics.ProcessStartInfo; $info.FileName='powershell.exe'; ",
                "$info.Arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"' + {child} + '\"'; ",
                "$info.UseShellExecute=$false; $info.CreateNoWindow=$true; ",
                "$child=[Diagnostics.Process]::Start($info); ",
                "while (-not [IO.File]::Exists({pid})) {{ Start-Sleep -Milliseconds 20 }}; ",
                "[Console]::WriteLine('Update complete, launching...'); exit 0"
            ),
            child = ps_literal(&child_script),
            pid = ps_literal(&pid_path)
        );
        let result = verify_with_command_factory(
            InstallDeadline::new("handoff cleanup", Duration::from_secs(15)), Duration::from_secs(3), &mut reporter,
            || {
                attempts += 1;
                if attempts == 1 {
                    command(&parent_script)
                } else {
                    command(&format!("$oldPid=[int][IO.File]::ReadAllText({}); if (Get-Process -Id $oldPid -ErrorAction SilentlyContinue) {{ exit 19 }}; [Console]::WriteLine('Steam Console Client (c) Valve Corporation - version fixture'); [Console]::WriteLine('Loading Steam API...OK'); exit 0", ps_literal(&pid_path)))
                }
            },
        ).await;
        let _ = std::fs::remove_file(&pid_path);
        let _ = std::fs::remove_file(&child_script);
        assert_eq!(attempts, 2);
        assert!(result.is_ok(), "fresh verification failed: {result:?}");
    }

    #[tokio::test]
    async fn independent_verification_may_cleanly_complete_another_update() {
        let mut attempts = 0;
        let result = verify_with_command_factory(
            InstallDeadline::new("clean independent verification", Duration::from_secs(15)),
            Duration::from_secs(3),
            &mut PrepareReporter::new(|_| {}),
            || {
                attempts += 1;
                command("[Console]::WriteLine('Update complete, launching...'); [Console]::WriteLine('Steam Console Client (c) Valve Corporation - version fixture'); [Console]::WriteLine('Loading Steam API...OK'); exit 0")
            },
        )
        .await;
        assert_eq!(attempts, 2);
        assert!(result.is_ok());
    }
}
