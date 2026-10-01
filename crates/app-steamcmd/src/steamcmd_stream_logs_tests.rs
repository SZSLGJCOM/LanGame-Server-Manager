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
