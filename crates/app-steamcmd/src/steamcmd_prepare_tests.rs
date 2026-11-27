#![cfg(test)]

use super::*;

#[cfg(windows)]
#[path = "steamcmd_prepare_handoff_fixture.rs"]
mod fixture;

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
    use super::fixture::HandoffFixture;
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

    #[tokio::test]
    async fn successful_self_update_requires_fresh_successful_verification() {
        let fixture = HandoffFixture::new().await;
        let mut attempts = 0;
        let mut phases = Vec::new();
        let result = verify_with_command_factory(
            InstallDeadline::new("handoff verification", Duration::from_secs(15)),
            Duration::from_secs(3),
            &mut PrepareReporter::new(|progress| phases.push(progress.phase)),
            || {
                attempts += 1;
                if attempts == 1 {
                    fixture.command("update-ready")
                } else {
                    fixture.command("failure")
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
        let fixture = HandoffFixture::new().await;
        let mut attempts = 0;
        let result = verify_with_command_factory(
            InstallDeadline::new("one bounded handoff", Duration::from_secs(15)),
            Duration::from_secs(3),
            &mut PrepareReporter::new(|_| {}),
            || {
                attempts += 1;
                fixture.command("update-stall")
            },
        )
        .await;
        assert_eq!(attempts, 2);
        assert!(
            matches!(result, Err(SteamCmdError::SteamCmdPreparationStalled { output_excerpt, .. }) if output_excerpt.contains("Update complete, launching"))
        );
    }

    #[tokio::test]
    async fn self_update_handoff_waits_for_owned_tree_exit_before_verification() {
        let fixture = HandoffFixture::new().await;
        let mut attempts = 0;
        let started = std::time::Instant::now();
        let mut handoff_observed = false;
        let mut reporter = PrepareReporter::new(|progress: SteamCmdPrepareProgress| {
            if !handoff_observed
                && progress
                    .output_excerpt
                    .contains("Update complete, launching")
            {
                handoff_observed = true;
                eprintln!(
                    "native parent and child ready after {:?}",
                    started.elapsed()
                );
            }
        });
        // The second command checks its predecessor using a fixture file so the
        // same handoff code controls cleanup and the new process creation.
        let result = verify_with_command_factory(
            InstallDeadline::new("handoff cleanup", Duration::from_secs(15)),
            Duration::from_secs(3),
            &mut reporter,
            || {
                attempts += 1;
                if attempts == 1 {
                    fixture.command("parent")
                } else {
                    fixture.command("verify-cleanup")
                }
            },
        )
        .await;
        assert_eq!(attempts, 2);
        assert!(handoff_observed);
        assert!(result.is_ok(), "fresh verification failed: {result:?}");
    }

    #[tokio::test]
    async fn independent_verification_may_cleanly_complete_another_update() {
        let fixture = HandoffFixture::new().await;
        let mut attempts = 0;
        let result = verify_with_command_factory(
            InstallDeadline::new("clean independent verification", Duration::from_secs(15)),
            Duration::from_secs(3),
            &mut PrepareReporter::new(|_| {}),
            || {
                attempts += 1;
                fixture.command("update-ready")
            },
        )
        .await;
        assert_eq!(attempts, 2);
        assert!(result.is_ok());
    }
}
