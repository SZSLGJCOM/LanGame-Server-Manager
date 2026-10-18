use super::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "langame-stream-logs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(path.join("logs")).unwrap();
        Self(path)
    }

    fn append(&self, name: &str, bytes: &[u8]) {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.0.join("logs").join(name))
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn console_and_bootstrap_publish_only_new_lines_and_deduplicate_completion() {
    let fixture = Fixture::new();
    fixture.append("console_log.txt", b"old installation progress\n");
    fixture.append("bootstrap_log.txt", b"old bootstrap\n");
    let mut logs = StreamLogs::before_spawn(&fixture.0).await.unwrap();
    assert!(logs.poll().await.unwrap().lines.is_empty());
    fixture.append(
        "console_log.txt",
        b"[2026-09-20 15:38:45] Update state (0x61) downloading, progress: 25.0 (25 / 100)",
    );
    let update = logs.poll().await.unwrap();
    assert_eq!(update.lines.len(), 1);
    assert_eq!(update.lines[0].0, LogSource::Console);
    assert!(update.lines[0].1.ends_with("(25 / 100)"));
    assert!(logs.poll().await.unwrap().lines.is_empty());
    fixture.append("console_log.txt", b"\r\n");
    assert!(logs.poll().await.unwrap().lines.is_empty());
    fixture.append(
        "bootstrap_log.txt",
        "正在下载更新 (已下载 397，共 10,673 KB)...\n".as_bytes(),
    );
    assert_eq!(logs.poll().await.unwrap().lines[0].0, LogSource::Bootstrap);
    fs::write(fixture.0.join("logs/console_log.txt"), b"replaced\n").unwrap();
    assert_eq!(
        logs.poll().await.unwrap().lines,
        [(LogSource::Console, String::from("replaced"))]
    );
}

#[tokio::test]
async fn unterminated_log_waits_are_live_utf8_bounded_and_resettable() {
    let fixture = Fixture::new();
    let mut logs = StreamLogs::before_spawn(&fixture.0).await.unwrap();
    let wait = "Connecting anonymously to Steam Public...";
    fixture.append("console_log.txt", wait.as_bytes());
    assert_eq!(
        logs.poll().await.unwrap().lines,
        [(LogSource::Console, wait.to_owned())]
    );
    fixture.append("console_log.txt", &[0xe4, 0xb8]);
    assert!(logs.poll().await.unwrap().lines.is_empty());
    fixture.append("console_log.txt", &[0xad]);
    assert_eq!(
        logs.poll().await.unwrap().lines,
        [(LogSource::Console, format!("{wait}中"))]
    );
    fixture.append("console_log.txt", b"\n");
    assert!(logs.poll().await.unwrap().lines.is_empty());
    // The same new record is still observable after a file reset.
    fs::write(fixture.0.join("logs/console_log.txt"), wait.as_bytes()).unwrap();
    assert_eq!(
        logs.poll().await.unwrap().lines,
        [(LogSource::Console, wait.to_owned())]
    );
    fixture.append(
        "console_log.txt",
        &vec![b'x'; STEAMCMD_OUTPUT_LINE_LIMIT_BYTES + 32],
    );
    let mut snapshots = Vec::new();
    loop {
        let update = logs.poll().await.unwrap();
        snapshots.extend(update.lines);
        if !update.activity {
            break;
        }
    }
    assert!(!snapshots.is_empty());
    assert!(
        snapshots
            .iter()
            .all(|(_, line)| line.len() <= STEAMCMD_OUTPUT_LINE_LIMIT_BYTES)
    );
    assert!(
        snapshots
            .last()
            .unwrap()
            .1
            .ends_with(crate::STEAMCMD_OUTPUT_LINE_TRUNCATED_SUFFIX)
    );
    fixture.append("console_log.txt", b"\r\nWaiting for user info...");
    assert_eq!(
        logs.poll().await.unwrap().lines,
        [(LogSource::Console, String::from("Waiting for user info..."))]
    );
}

#[cfg(windows)]
#[tokio::test]
async fn console_file_updates_reach_progress_while_the_process_is_still_running() {
    use crate::steamcmd_stream::run_steamcmd_script_with_source;
    use crate::{InstallDeadline, ps_literal};
    use app_core::InstallPhase;
    use std::time::Duration;

    let fixture = Fixture::new();
    fixture.append(
        "console_log.txt",
        b"old Update state (0x61) downloading, progress: 99 (99 / 100)\n",
    );
    let gate = fixture.0.join("observed-download");
    let verified = fixture.0.join("observed-verification");
    let script = fixture.0.join("writer.ps1");
    fs::write(&script, format!(
        concat!(
            "[IO.File]::AppendAllText({log}, '[2026-09-20 15:38:45] Update state (0x61) downloading, progress: 25.0 (25 / 100)');\n",
            "while (-not [IO.File]::Exists({gate})) {{ Start-Sleep -Milliseconds 10 }};\n",
            "[IO.File]::AppendAllText({log}, [Environment]::NewLine + '[2026-09-20 15:39:41] Update state (0x81) verifying update, progress: 50.0 (50 / 100)' + [Environment]::NewLine);\n",
            "while (-not [IO.File]::Exists({verified})) {{ Start-Sleep -Milliseconds 10 }};\n"
        ),
        log = ps_literal(&fixture.0.join("logs/console_log.txt")),
        gate = ps_literal(&gate), verified = ps_literal(&verified),
    )).unwrap();
    let launcher = fixture.0.join("steamcmd.cmd");
    fs::write(&launcher, format!(
        "@echo off\r\npowershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\"\r\n", script.display(),
    )).unwrap();
    let mut received = Vec::new();
    let result = run_steamcmd_script_with_source(
        launcher.to_str().unwrap(),
        &fixture.0.join("ignored.txt"),
        fixture.0.to_str().unwrap(),
        InstallDeadline::new("console log progress fixture", Duration::from_secs(20)),
        |update| {
            let progress = update.install_progress.unwrap();
            if progress.phase == InstallPhase::Downloading {
                assert_eq!(progress.downloaded_bytes, Some(25));
                fs::write(&gate, b"observed before exit").unwrap();
            } else if progress.phase == InstallPhase::Verifying {
                assert_eq!(progress.downloaded_bytes, None);
                fs::write(&verified, b"observed before exit").unwrap();
            }
            received.push(progress.phase);
        },
        None,
    )
    .await
    .unwrap();
    assert!(result.success);
    assert_eq!(
        received,
        [InstallPhase::Downloading, InstallPhase::Verifying]
    );
}

#[cfg(windows)]
#[tokio::test]
async fn workshop_failure_survives_excerpt_eviction_and_exposes_only_new_workshop_logs() {
    use crate::steamcmd_stream::run_steamcmd_script_with_source;
    use crate::{InstallDeadline, ps_literal};
    use std::time::Duration;

    const FAILURE: &str = "[2026-10-17 13:50:44] [AppID 1623730] Update canceled: Failed to initialize depot 1623730, manifest 3622555758966187100 (Missing decryption key) (Missing decryption key)";
    for source in ["stdout", "console", "none"] {
        let fixture = Fixture::new();
        fixture.append(
            "console_log.txt",
            b"[2026-10-16 13:50:44] ERROR! Download item 1111111111 failed (Failure).\n",
        );
        fixture.append("workshop_log.txt", b"historical Workshop error\n");
        let console = ps_literal(&fixture.0.join("logs/console_log.txt"));
        let native_failure = match source {
            "stdout" => String::from(
                "[Console]::Out.WriteLine('ERROR! Download item 3816899969 failed (Failure).');",
            ),
            "console" => format!(
                "[IO.File]::AppendAllText({console}, '[2026-10-17 13:50:44] ERROR! Download item 3816899969 failed (Failure).' + [Environment]::NewLine);"
            ),
            _ => String::new(),
        };
        let noise = if source == "console" {
            format!(
                "[IO.File]::AppendAllText({console}, ('later diagnostic ' + $_) + [Environment]::NewLine);"
            )
        } else {
            String::from("[Console]::Out.WriteLine('later diagnostic ' + $_);")
        };
        let script = fixture.0.join("writer.ps1");
        fs::write(
            &script,
            format!(
                concat!(
                    "$ErrorActionPreference = 'Stop';\n",
                    "[IO.File]::AppendAllText({workshop}, '{failure}' + [Environment]::NewLine);\n",
                    "{native_failure}\n",
                    "1..80 | ForEach-Object {{ {noise} }};\n",
                    "exit 0\n"
                ),
                workshop = ps_literal(&fixture.0.join("logs/workshop_log.txt")),
                failure = FAILURE,
                native_failure = native_failure,
                noise = noise,
            ),
        )
        .unwrap();
        let launcher = fixture.0.join("steamcmd.cmd");
        fs::write(
            &launcher,
            format!(
                "@echo off\r\npowershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\"\r\n",
                script.display(),
            ),
        )
        .unwrap();
        let result = run_steamcmd_script_with_source(
            launcher.to_str().unwrap(),
            &fixture.0.join("ignored.txt"),
            fixture.0.to_str().unwrap(),
            InstallDeadline::new("Workshop diagnostics fixture", Duration::from_secs(20)),
            |_| {},
            None,
        )
        .await
        .unwrap();
        assert!(result.success, "{source}: {}", result.excerpt);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(
            result.workshop_download_failed,
            source != "none",
            "{source}"
        );
        assert!(!result.excerpt.contains("ERROR! Download item"), "{source}");
        assert_eq!(result.workshop_log_excerpt.as_deref(), Some(FAILURE));
    }
}

#[cfg(windows)]
#[tokio::test]
async fn decisive_workshop_evidence_survives_full_stream_and_large_log_rollover() {
    use crate::steamcmd_stream::run_steamcmd_script_with_source;
    use crate::{InstallDeadline, SteamCmdError, ps_literal, workshop_download};
    use std::time::Duration;

    const APP: u32 = 1_623_730;
    const DENIED: &str = "CDepotDownloadMgr::BYldRequestDepotManifest(App: 1623730, Depot: 1623730, Manifest: 8195362111486240380, branch: public): Failed to get manifest request code, 'Access Denied'";
    const REQUEST: &str = "[AppID 1623730] Download item 3817722715 requested by app";
    const MANIFEST: &str = "[AppID 1623730] Detected workshop change (latest from server) : changed cached item 3817722715, new manifest 8195362111486240380";
    const RESULT: &str = "[AppID 1623730] Download item 3817722715 result : No Connection";
    for mode in [
        "stdout",
        "console",
        "key",
        "manifest",
        "cached-manifest",
        "partial-stdout",
        "partial-console",
    ] {
        let fixture = Fixture::new();
        let console = ps_literal(&fixture.0.join("logs/console_log.txt"));
        let workshop = ps_literal(&fixture.0.join("logs/workshop_log.txt"));
        let content = ps_literal(&fixture.0.join("logs/content_log.txt"));
        let body = match mode {
            "partial-stdout" => "[Console]::Out.Write('ERROR! Download item 3817722715 failed (Access Denied)'); [Console]::Out.Flush(); Start-Sleep -Milliseconds 600; [Console]::Out.WriteLine(' while writing local cache'); 1..100 | ForEach-Object { [Console]::Out.WriteLine($noise) };".to_owned(),
            "partial-console" => format!("[IO.File]::AppendAllText({console}, 'ERROR! Download item 3817722715 failed (Access Denied)'); Start-Sleep -Milliseconds 600; [IO.File]::AppendAllText({console}, ' while writing local cache' + $nl + ($noise + $nl) * 100);"),
            "stdout" => "[Console]::Out.WriteLine('ERROR! Download item 3817722715 failed (No subscription).'); 1..100 | ForEach-Object { [Console]::Out.WriteLine($noise) };".to_owned(),
            "console" => format!("[IO.File]::AppendAllText({console}, 'ERROR! Download item 3817722715 failed (Access Denied)' + $nl + ($noise + $nl) * 100);"),
            "key" => format!("[IO.File]::AppendAllText({workshop}, '[AppID 1623730] Update canceled: Missing decryption key' + $nl + ($noise + $nl) * 100);"),
            _ => format!(
                "[IO.File]::AppendAllText({workshop}, '{REQUEST}' + $nl + ($noise + $nl) * 100 + '{mapping}' + $nl + ($noise + $nl) * 100 + '{RESULT}' + $nl + ($noise + $nl) * 100); [IO.File]::AppendAllText({content}, '{denial}' + $nl + ($noise + $nl) * {count});",
                mapping = if mode == "manifest" { MANIFEST } else { "unchanged item metadata reused" },
                denial = DENIED.replace("'", "''"),
                count = if mode == "manifest" { 5000 } else { 100 },
            ),
        };
        let install_script = fixture.0.join("download.txt");
        fs::write(&install_script, format!("force_install_dir \"{}\"\nlogin anonymous\nworkshop_download_item {APP} 3817722715 validate\nquit\n", fixture.0.display())).unwrap();
        if mode == "cached-manifest" {
            fs::create_dir_all(fixture.0.join("steamapps/workshop")).unwrap();
            fs::write(fixture.0.join("steamapps/workshop/appworkshop_1623730.acf"),
                "\"AppWorkshop\" { \"appid\" \"1623730\" \"WorkshopItemDetails\" { \"3817722715\" { \"manifest\" \"8195362111486240380\" } } }").unwrap();
        }
        let writer = fixture.0.join("writer.ps1");
        fs::write(&writer, format!("$ErrorActionPreference = 'Stop'; $nl = [Environment]::NewLine; $noise = 'later diagnostic ' + ('x' * 1024); {body}\nexit 0\n")).unwrap();
        let launcher = fixture.0.join("steamcmd.cmd");
        fs::write(&launcher, format!("@echo off\r\npowershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\"\r\n", writer.display())).unwrap();
        let result = run_steamcmd_script_with_source(
            launcher.to_str().unwrap(),
            &install_script,
            fixture.0.to_str().unwrap(),
            InstallDeadline::new("independent Workshop evidence", Duration::from_secs(30)),
            |_| {},
            None,
        )
        .await
        .unwrap();
        assert!(result.success, "{mode}: {}", result.excerpt);
        assert_eq!(
            result.workshop_download_failed,
            matches!(
                mode,
                "stdout" | "console" | "partial-stdout" | "partial-console"
            )
        );
        let error = workshop_download::verify_command_result(APP, None, &result).unwrap_err();
        if mode.starts_with("partial-") {
            assert!(
                matches!(error, SteamCmdError::SteamCmdCommandFailed { .. }),
                "{mode}: {error}"
            );
            continue;
        }
        if mode == "key" {
            assert!(
                matches!(error, SteamCmdError::WorkshopDecryptionKeyMissing { .. }),
                "{mode}: {error}"
            );
        } else {
            assert!(
                matches!(error, SteamCmdError::WorkshopAccountRequired { .. }),
                "{mode}: {error}"
            );
        }
        let diagnostic = error.to_string();
        assert!(
            diagnostic.contains(if mode == "key" {
                "Missing decryption key"
            } else if mode == "stdout" {
                "No subscription"
            } else {
                "Access Denied"
            }),
            "{mode}: {diagnostic}"
        );
        if mode.contains("manifest") {
            assert!(
                diagnostic.contains("3817722715") && diagnostic.contains("8195362111486240380")
            );
        }
    }
}
