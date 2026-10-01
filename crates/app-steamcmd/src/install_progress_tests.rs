use super::*;
use crate::steamcmd_stream::spawn_line_forwarder;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

#[test]
fn steam_update_states_report_real_download_bytes_and_reset_for_validation() {
    let mut output = SteamInstallOutput::new();
    let download = output
        .line(
            "Update state (0x61) downloading, progress: 25.00 (5368709120 / 21474836480)",
            String::new(),
        )
        .unwrap()
        .install_progress
        .unwrap();
    assert_eq!(download.phase, InstallPhase::Downloading);
    assert_eq!(download.percent, Some(25.0));
    assert_eq!(download.downloaded_bytes, Some(5_368_709_120));
    assert_eq!(download.total_bytes, Some(21_474_836_480));

    let generic = output
        .line("Waiting for the content server...", String::new())
        .unwrap()
        .install_progress
        .unwrap();
    assert_eq!(generic, download, "ordinary logs do not invent progress");

    let verifying = output
        .line(
            "Update state (0x81) validating, progress: 50.00 (10737418240 / 21474836480)",
            String::new(),
        )
        .unwrap()
        .install_progress
        .unwrap();
    assert_eq!(verifying.phase, InstallPhase::Verifying);
    assert_eq!(verifying.percent, Some(50.0));
    assert_eq!(verifying.downloaded_bytes, None);
    assert_eq!(verifying.total_bytes, None);

    let resumed = output
        .line(
            "Update state (0x61) downloading, progress: 1.00 (10 / 1000)",
            String::new(),
        )
        .unwrap()
        .install_progress
        .unwrap();
    assert_eq!(resumed.downloaded_bytes, Some(10));
    assert_eq!(resumed.percent, Some(1.0));
}

#[test]
fn steam_progress_rejects_invalid_counters_and_never_marks_unverified_output_ready() {
    for line in [
        "Update state (0x61) downloading, progress: NaN (200 / 100)",
        "Update state (0x61) downloading, progress: inf (1 / 0)",
        "Update state (0x61) downloading, progress: -1 (18446744073709551616 / 100)",
        "Update state (0x61) downloading, progress: 101 (2 / truncated",
    ] {
        let parsed = parse_steamcmd_install_progress(line).unwrap();
        assert_eq!(parsed.percent, None);
        assert_eq!(parsed.downloaded_bytes, None);
        assert_eq!(parsed.total_bytes, None);
    }
    assert!(parse_steamcmd_install_progress("unrelated progress: 77").is_none());
    let mut output = SteamInstallOutput::new();
    let progress = output
        .line("Success! App '123' fully installed.", String::new())
        .unwrap()
        .install_progress
        .unwrap();
    assert_eq!(progress.phase, InstallPhase::Verifying);
    assert_eq!(progress.percent, None);
}

#[test]
fn incomplete_native_percentage_never_becomes_a_reported_number() {
    let mut output = SteamInstallOutput::new();
    let previous = output
        .console_line(
            "Update state (0x61) downloading, progress: 25.0 (250 / 1000)",
            String::new(),
        )
        .unwrap()
        .install_progress;
    for suffix in ["9", "97.80", "97.80 (97 / 10", "97.80 (97 / 100"] {
        let line = format!("Update state (0x61) downloading, progress: {suffix}");
        assert!(output.console_line(&line, String::new()).is_none());
        assert_eq!(Some(output.progress.clone()), previous);
        let progress = parse_steamcmd_install_progress(&line).unwrap();
        assert_eq!(progress.phase, InstallPhase::Downloading);
        assert_eq!(progress.percent, None, "{line}");
        assert_eq!(progress.downloaded_bytes, None, "{line}");
        assert_eq!(progress.total_bytes, None, "{line}");
    }
    let complete = parse_steamcmd_install_progress(
        "Update state (0x61) downloading, progress: 97.80 (978 / 1000)",
    )
    .unwrap();
    assert_eq!(complete.percent, Some(97.8));
    assert_eq!(complete.downloaded_bytes, Some(978));
}

#[test]
fn preparation_progress_preserves_download_bytes_without_finishing_game_install() {
    let mut prepare = SteamCmdPrepareProgress {
        phase: SteamCmdPreparePhase::Downloading,
        detail: String::from("Downloading runtime..."),
        downloaded_bytes: Some(25),
        total_bytes: Some(100),
        output_excerpt: String::from("runtime output"),
    };
    let update = InstallProgressUpdate::preparing_steamcmd(prepare.clone());
    assert_eq!(update.install_progress.unwrap().percent, Some(25.0));
    assert_eq!(update.output_excerpt, "runtime output");
    prepare.phase = SteamCmdPreparePhase::Updating;
    let updating = InstallProgressUpdate::preparing_steamcmd(prepare.clone())
        .install_progress
        .unwrap();
    assert_eq!(updating.phase, InstallPhase::Downloading);
    assert_eq!(updating.downloaded_bytes, Some(25));
    assert_eq!(updating.total_bytes, Some(100));
    assert_eq!(updating.percent, Some(25.0));
    prepare.downloaded_bytes = None;
    prepare.total_bytes = None;
    let installing = InstallProgressUpdate::preparing_steamcmd(prepare.clone())
        .install_progress
        .unwrap();
    assert_eq!(installing.phase, InstallPhase::Installing);
    assert_eq!(installing.percent, None);
    prepare.phase = SteamCmdPreparePhase::Ready;
    let progress = InstallProgressUpdate::preparing_steamcmd(prepare)
        .install_progress
        .unwrap();
    assert_eq!(progress.phase, InstallPhase::Preparing);
    assert_eq!(progress.percent, None);
    assert_eq!(progress.downloaded_bytes, None);
}

#[test]
fn http_progress_unknown_length_remains_indeterminate_and_stages_clear_counters() {
    let unknown = InstallProgressUpdate::download("Download", 128, None)
        .install_progress
        .unwrap();
    assert_eq!(unknown.downloaded_bytes, Some(128));
    assert_eq!(unknown.total_bytes, None);
    assert_eq!(unknown.percent, None);
    let complete = InstallProgressUpdate::download("Download", 256, Some(256))
        .install_progress
        .unwrap();
    assert_eq!(complete.percent, Some(100.0));
    assert_eq!(complete.phase, InstallPhase::Downloading);
    let extracting = InstallProgressUpdate::stage(InstallPhase::Extracting, "Extracting")
        .install_progress
        .unwrap();
    assert_eq!(extracting.downloaded_bytes, None);
    assert_eq!(extracting.percent, None);
}

#[tokio::test]
async fn carriage_return_progress_arrives_before_stdout_closes() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    let (sender, mut receiver) = mpsc::channel(4);
    let mut forwarder = spawn_line_forwarder(reader, sender);
    writer
        .write_all(b"Update state (0x61) downloading, progress: 10.0 (10 / 100)\r")
        .await
        .unwrap();
    let line = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut output = SteamInstallOutput::new();
    assert_eq!(
        output
            .line(&line, String::new())
            .unwrap()
            .install_progress
            .unwrap()
            .downloaded_bytes,
        Some(10)
    );
    writer
        .write_all(b"\nUpdate state (0x81) validating, progress: 20.0 (20 / 100)\r")
        .await
        .unwrap();
    let line = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        output
            .line(&line, String::new())
            .unwrap()
            .install_progress
            .unwrap()
            .phase,
        InstallPhase::Verifying
    );
    writer.shutdown().await.unwrap();
    forwarder.join().await.unwrap();
    assert!(receiver.recv().await.is_none());
}

#[tokio::test]
async fn unterminated_waiting_output_is_live_complete_utf8_and_not_duplicated() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    let (sender, mut receiver) = mpsc::channel(4);
    let mut forwarder = spawn_line_forwarder(reader, sender);
    writer
        .write_all(b"Connecting anonymously to Steam Public...")
        .await
        .unwrap();
    async fn receive(receiver: &mut mpsc::Receiver<Result<String, std::io::Error>>) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }
    assert_eq!(
        receive(&mut receiver).await,
        "Connecting anonymously to Steam Public..."
    );
    writer.write_all(&[b' ', 0xe4, 0xb8]).await.unwrap();
    assert_eq!(
        receive(&mut receiver).await,
        "Connecting anonymously to Steam Public... "
    );
    writer.write_all(&[0xad]).await.unwrap();
    assert_eq!(
        receive(&mut receiver).await,
        "Connecting anonymously to Steam Public... 中"
    );
    writer.write_all(b"\r\n").await.unwrap();
    writer.shutdown().await.unwrap();
    forwarder.join().await.unwrap();
    assert!(
        receiver.recv().await.is_none(),
        "completed snapshots are not repeated"
    );
}

#[test]
fn console_log_timestamps_and_late_stdout_do_not_regress_real_progress() {
    let mut output = SteamInstallOutput::new();
    let download = output.console_line(
        "[2026-09-20 15:38:45]  Update state (0x61) downloading, progress: 97.80 (2607090956 / 2665758135)",
        String::new(),
    ).unwrap().install_progress.unwrap();
    assert_eq!(download.phase, InstallPhase::Downloading);
    assert_eq!(download.downloaded_bytes, Some(2_607_090_956));
    let verifying = output.console_line(
        "[2026-09-20 15:39:41]  Update state (0x81) verifying update, progress: 48.94 (1304694626 / 2665758135)",
        String::new(),
    ).unwrap().install_progress.unwrap();
    assert_eq!(verifying.phase, InstallPhase::Verifying);
    assert_eq!(verifying.percent, Some(48.94));
    assert_eq!(verifying.downloaded_bytes, None);
    assert!(
        output
            .stdout_line(
                "Update state (0x61) downloading, progress: 10.0 (10 / 100)",
                String::new()
            )
            .is_none()
    );
    assert!(
        output
            .bootstrap_line("Downloading update (10 of 100 KB)", String::new())
            .is_none()
    );
    let completed = output
        .console_line(
            "[2026-09-20 15:39:43] Success! App '728470' fully installed.",
            String::new(),
        )
        .unwrap()
        .install_progress
        .unwrap();
    assert_eq!(completed.phase, InstallPhase::Verifying);
    assert_eq!(completed.percent, None);
}

#[test]
fn runscript_bootstrap_bytes_clear_when_the_console_runtime_starts() {
    let mut output = SteamInstallOutput::new();
    let downloading = output
        .bootstrap_line("正在下载更新 (已下载 397，共 10,673 KB)...", String::new())
        .unwrap()
        .install_progress
        .unwrap();
    assert_eq!(downloading.phase, InstallPhase::Downloading);
    assert_eq!(downloading.downloaded_bytes, Some(397 * 1024));
    let console = output
        .console_line("[2026-09-20 15:36:37] Loading Steam API...", String::new())
        .unwrap()
        .install_progress
        .unwrap();
    assert_eq!(console.phase, InstallPhase::Preparing);
    assert_eq!(console.downloaded_bytes, None);
    assert_eq!(console.total_bytes, None);
    assert_eq!(console.percent, None);
    assert!(
        output
            .console_line(
                "[2026-09-20 15:36:38] Loading Steam API...OK",
                String::new()
            )
            .is_none()
    );
    assert!(
        output
            .stdout_line("Connecting anonymously to Steam Public...", String::new())
            .is_some()
    );
    assert!(parse_steamcmd_install_progress("Update state (0x61) down").is_none());
}

#[test]
fn native_wait_owns_the_step_before_download_and_noise_cannot_replace_it() {
    let mut output = SteamInstallOutput::new();
    assert!(
        output
            .stdout_line("Connecting anonymously to Steam Public...", String::new())
            .is_some()
    );
    let waiting = output
        .console_line(
            "[2026-09-20 15:57:53] Waiting for user info...",
            String::new(),
        )
        .unwrap();
    assert_eq!(
        waiting.detail,
        "[2026-09-20 15:57:53] Waiting for user info..."
    );
    assert_eq!(
        waiting.install_progress.unwrap().phase,
        InstallPhase::Preparing
    );
    assert!(
        output
            .stdout_line("Connecting anonymously to Steam Public...OK", String::new())
            .is_none()
    );
    for noise in [
        "Steam Console Client (c) Valve Corporation - version 1788292693",
        "Loading Steam API...OK",
        "@ShutdownOnFailedCommand 1",
        "@NoPromptForPassword 1",
        "force_install_dir game",
        "OK",
        "Redirecting stderr to stderr.txt",
        "quit",
    ] {
        assert!(
            output.console_line(noise, String::new()).is_none(),
            "{noise}"
        );
    }
    for (line, phase) in [
        (
            "Update state (0x81) validating, progress: 10.0 (10 / 100)",
            InstallPhase::Verifying,
        ),
        (
            "Update state (0x61) downloading, progress: 25.0 (25 / 100)",
            InstallPhase::Downloading,
        ),
        (
            "Update state (0x81) verifying update, progress: 50.0 (50 / 100)",
            InstallPhase::Verifying,
        ),
    ] {
        let update = output
            .console_line(line, String::from("full diagnostics retained"))
            .unwrap();
        assert_eq!(update.install_progress.unwrap().phase, phase);
        assert_eq!(update.output_excerpt, "full diagnostics retained");
    }
}

#[test]
fn runtime_boundary_publishes_once_and_execution_commands_are_meaningful() {
    let mut output = SteamInstallOutput::new();
    let banner = "Steam Console Client (c) Valve Corporation - version 1788292693";
    assert!(output.console_line(banner, String::new()).is_some());
    assert!(output.console_line(banner, String::new()).is_none());
    // An initialization banner alone must not suppress live pipe fallback.
    assert!(
        output
            .stdout_line("Waiting for client config...", String::new())
            .is_some()
    );
    for command in ["login anonymous", "app_update 2857200 validate"] {
        let update = output.console_line(command, String::new()).unwrap();
        assert_eq!(update.detail, command);
        assert_eq!(
            update.install_progress.unwrap().phase,
            InstallPhase::Preparing
        );
    }
    assert!(
        output
            .stdout_line("Waiting for client config...OK", String::new())
            .is_none()
    );
}
