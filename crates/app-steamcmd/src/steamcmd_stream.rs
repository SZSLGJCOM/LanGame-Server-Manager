use super::install_progress::SteamInstallOutput;
use super::steamcmd_account::AccountLoginEvidence;
use super::steamcmd_content_evidence::ContentLogEvidence;
use super::steamcmd_content_evidence::InvocationLog;
use super::steamcmd_manifest_recovery::ManifestDenial;
use super::*;

#[path = "steamcmd_stream_logs.rs"]
mod logs;
use logs::{LogSource, StreamLogs};
#[path = "steamcmd_content_log.rs"]
mod content_log;
use content_log::WorkshopLogExcerpt;
pub(super) use content_log::read_steamcmd_content_log_excerpt;

#[derive(Debug)]
pub(super) struct StreamingCommandResult {
    pub(super) success: bool,
    pub(super) exit_code: Option<i32>,
    pub(super) excerpt: String,
    pub(super) content_log_excerpt: Option<String>,
    pub(super) workshop_log_excerpt: Option<String>,
    pub(super) workshop_download_failed: bool,
    pub(super) workshop_command_evidence: super::workshop_download::WorkshopEvidence,
    pub(super) app_command_evidence: super::steam_app_access::SteamAppCommandEvidence,
    pub(super) authenticated_account_name: Option<String>,
    pub(super) denied_manifests: Vec<ManifestDenial>,
}

#[derive(Default)]
struct CommandEvidence {
    workshop_download_failed: bool,
    workshop: super::workshop_download::WorkshopEvidence,
    app: super::steam_app_access::SteamAppCommandEvidence,
    account: AccountLoginEvidence,
}

struct OutputRecord {
    line: String,
    complete: bool,
    repeated: bool,
}

impl CommandEvidence {
    fn observe_line(&mut self, line: &str) {
        self.workshop_download_failed |=
            super::workshop_download::is_workshop_download_failure_line(line);
        self.workshop.observe_line(line);
        self.app.observe_line(line);
        self.account.observe_line(line);
    }
}

pub(super) async fn run_steamcmd_script_with_progress<F>(
    executable_path: &str,
    script_path: &Path,
    working_directory: &str,
    deadline: InstallDeadline,
    on_progress: F,
) -> Result<StreamingCommandResult, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
{
    let bridge = super::steamcmd_update_bridge::UpdateBridge::for_managed_root(
        Path::new(working_directory),
        deadline,
    )
    .await?;
    let result = run_steamcmd_script_with_source(
        executable_path,
        script_path,
        working_directory,
        deadline,
        on_progress,
        bridge.as_ref().map(|bridge| bridge.url.as_str()),
    )
    .await;
    if let Some(bridge) = bridge {
        bridge.close().await;
    }
    result
}

fn steamcmd_script_command(
    executable_path: &str,
    script_path: &Path,
    working_directory: &str,
    update_source: Option<&str>,
) -> Command {
    let mut command = Command::new(executable_path);
    apply_no_window(&mut command);
    if let Some(source) = update_source {
        command.args(["-overridepackageurl", source]);
    }
    // Scripts accept no interactive input. Native console detach/attach can
    // invalidate inherited stdin, so give this child an independent EOF source.
    command
        .arg("+runscript")
        .arg(script_path)
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

async fn run_steamcmd_script_with_source<F>(
    executable_path: &str,
    script_path: &Path,
    working_directory: &str,
    deadline: InstallDeadline,
    mut on_progress: F,
    update_source: Option<&str>,
) -> Result<StreamingCommandResult, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
{
    deadline.check_cancelled()?;
    let mut content_evidence = deadline
        .run(ContentLogEvidence::before_spawn(Path::new(
            working_directory,
        )))
        .await
        .map_err(|_| operation_timeout(deadline))?;
    let mut workshop_evidence_log = deadline
        .run(InvocationLog::before_spawn(
            Path::new(working_directory).join("logs/workshop_log.txt"),
        ))
        .await
        .map_err(|_| operation_timeout(deadline))?;
    let mut console_evidence_log = deadline
        .run(InvocationLog::before_spawn(
            Path::new(working_directory).join("logs/console_log.txt"),
        ))
        .await
        .map_err(|_| operation_timeout(deadline))?;
    let content_log_before = deadline
        .run(read_steamcmd_content_log_excerpt(Path::new(
            working_directory,
        )))
        .await
        .map_err(|_| operation_timeout(deadline))?;
    let workshop_log = deadline
        .run(WorkshopLogExcerpt::before_spawn(Path::new(
            working_directory,
        )))
        .await
        .map_err(|_| operation_timeout(deadline))?;
    let mut logs = deadline
        .run(StreamLogs::before_spawn(Path::new(working_directory)))
        .await
        .map_err(|_| operation_timeout(deadline))?
        .map_err(|source| SteamCmdError::SpawnCommand { source })?;
    let mut command = steamcmd_script_command(
        executable_path,
        script_path,
        working_directory,
        update_source,
    );
    let workshop_manifests = deadline
        .run(super::workshop_download::requested_item_manifests(
            script_path,
            Path::new(working_directory),
        ))
        .await
        .map_err(|_| operation_timeout(deadline))?;
    let (mut child, mut process_guard) = spawn_managed_command(&mut command).await?;

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_and_reap_child(&mut child, &mut process_guard, deadline).await?;
            return Err(SteamCmdError::SpawnCommand {
                source: std::io::Error::other("SteamCMD stdout pipe unavailable"),
            });
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate_and_reap_child(&mut child, &mut process_guard, deadline).await?;
            return Err(SteamCmdError::SpawnCommand {
                source: std::io::Error::other("SteamCMD stderr pipe unavailable"),
            });
        }
    };

    let (tx, mut rx) =
        mpsc::channel::<Result<OutputRecord, std::io::Error>>(STEAMCMD_OUTPUT_CHANNEL_CAPACITY);
    let record = |line, complete, repeated| OutputRecord {
        line,
        complete,
        repeated,
    };
    let mut stdout_forwarder = spawn_record_forwarder(stdout, tx.clone(), true, record);
    let mut stderr_forwarder = spawn_record_forwarder(stderr, tx.clone(), true, record);
    drop(tx);

    let mut excerpt_lines = VecDeque::new();
    let mut evidence = CommandEvidence::default();
    for (app, item, manifest) in workshop_manifests {
        evidence.workshop.seed_manifest(app, item, manifest);
    }
    let mut progress = SteamInstallOutput::new();
    let mut status = None;
    let mut channel_closed = false;
    let mut log_tick = tokio::time::interval(Duration::from_millis(250));
    log_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        if status.is_some() && channel_closed {
            let evidence_activity = publish_invocation_evidence(
                &mut content_evidence,
                &mut workshop_evidence_log,
                &mut console_evidence_log,
                &mut evidence,
                deadline,
            )
            .await?;
            match publish_log_progress(
                &mut logs,
                &mut progress,
                &mut excerpt_lines,
                &mut evidence,
                deadline,
                &mut on_progress,
            )
            .await
            {
                Ok(true) => continue,
                Ok(false) if evidence_activity => continue,
                Ok(false) => break,
                Err(error) => {
                    let cleanup =
                        terminate_and_reap_child(&mut child, &mut process_guard, deadline).await;
                    abort_line_forwarders(&mut stdout_forwarder, &mut stderr_forwarder).await;
                    cleanup?;
                    return Err(error);
                }
            }
        }

        tokio::select! {
            biased;
            _ = deadline.run(std::future::pending::<()>()) => {
                let cleanup = terminate_and_reap_child(&mut child, &mut process_guard, deadline).await;
                abort_line_forwarders(&mut stdout_forwarder, &mut stderr_forwarder).await;
                cleanup?;
                return Err(operation_timeout(deadline));
            }
            _ = log_tick.tick() => {
                if let Err(error) = publish_invocation_evidence(&mut content_evidence, &mut workshop_evidence_log, &mut console_evidence_log, &mut evidence, deadline).await {
                    let cleanup = terminate_and_reap_child(&mut child, &mut process_guard, deadline).await;
                    abort_line_forwarders(&mut stdout_forwarder, &mut stderr_forwarder).await;
                    cleanup?;
                    return Err(error);
                }
                if let Err(error) = publish_log_progress(&mut logs, &mut progress, &mut excerpt_lines, &mut evidence, deadline, &mut on_progress).await {
                    let cleanup = terminate_and_reap_child(&mut child, &mut process_guard, deadline).await;
                    abort_line_forwarders(&mut stdout_forwarder, &mut stderr_forwarder).await;
                    cleanup?;
                    return Err(error);
                }
            }
            maybe_line = rx.recv(), if !channel_closed => {
                match maybe_line {
                    Some(Ok(record)) => {
                        let trimmed = record.line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if record.complete { evidence.observe_line(trimmed); }
                        if record.repeated { continue; }
                        push_excerpt_line(&mut excerpt_lines, trimmed);
                        if let Some(update) = progress.stdout_line(trimmed, join_excerpt_lines(&excerpt_lines)) {
                            on_progress(update);
                        }
                    }
                    Some(Err(source)) => {
                        let cleanup = terminate_and_reap_child(&mut child, &mut process_guard, deadline).await;
                        abort_line_forwarders(&mut stdout_forwarder, &mut stderr_forwarder).await;
                        cleanup?;
                        return Err(SteamCmdError::SpawnCommand { source });
                    }
                    None => {
                        channel_closed = true;
                    }
                }
            }
            result = child.wait(), if status.is_none() => {
                match result {
                    Ok(exit_status) => status = Some(exit_status),
                    Err(source) => {
                        let cleanup = terminate_and_reap_child(&mut child, &mut process_guard, deadline).await;
                        abort_line_forwarders(&mut stdout_forwarder, &mut stderr_forwarder).await;
                        cleanup?;
                        return Err(SteamCmdError::SpawnCommand { source });
                    }
                }
            }
        }
    }

    if let Err(error) = join_line_forwarders(&mut stdout_forwarder, &mut stderr_forwarder).await {
        terminate_and_reap_child(&mut child, &mut process_guard, deadline).await?;
        return Err(error);
    }
    if let Err(error) = process_guard.wait_until_empty(deadline).await {
        terminate_and_reap_child(&mut child, &mut process_guard, deadline).await?;
        return Err(error);
    }

    let success = status
        .map(|exit_status| exit_status.success())
        .unwrap_or(false);
    let exit_code = status.and_then(|exit_status| exit_status.code());
    let (denied_manifests, retained_content_excerpt) = deadline
        .run(content_evidence.finish_with_excerpt())
        .await
        .map_err(|_| operation_timeout(deadline))?;
    let content_log_after = deadline
        .run(read_steamcmd_content_log_excerpt(Path::new(
            working_directory,
        )))
        .await
        .map_err(|_| operation_timeout(deadline))?;
    let workshop_log_excerpt = deadline
        .run(workshop_log.finish())
        .await
        .map_err(|_| operation_timeout(deadline))?;
    let mut content_log_excerpt =
        updated_content_log_excerpt(content_log_before.as_deref(), content_log_after.as_deref());
    if let Some(retained) = retained_content_excerpt {
        let tail = content_log_excerpt.get_or_insert_with(String::new);
        if !tail.contains(&retained) {
            *tail = format!("[retained manifest permission evidence]\n{retained}\n{tail}");
        }
    }
    evidence.workshop.finish_capture();
    Ok(StreamingCommandResult {
        success,
        exit_code,
        excerpt: join_excerpt_lines(&excerpt_lines),
        content_log_excerpt,
        workshop_log_excerpt,
        workshop_download_failed: evidence.workshop_download_failed,
        workshop_command_evidence: evidence.workshop,
        app_command_evidence: evidence.app,
        authenticated_account_name: evidence
            .account
            .authenticated_account_name()
            .map(String::from),
        denied_manifests,
    })
}

async fn publish_invocation_evidence(
    content: &mut ContentLogEvidence,
    workshop: &mut InvocationLog,
    console: &mut InvocationLog,
    evidence: &mut CommandEvidence,
    deadline: InstallDeadline,
) -> Result<bool, SteamCmdError> {
    deadline
        .run(async {
            let content = content.poll().await;
            let workshop = workshop.poll().await;
            let console = console.poll().await;
            for line in content.lines.iter().chain(&workshop.lines) {
                evidence.workshop.observe_line(line);
            }
            for line in &console.lines {
                evidence.observe_line(line);
            }
            content.activity || workshop.activity || console.activity
        })
        .await
        .map_err(|_| operation_timeout(deadline))
}

async fn publish_log_progress<F: FnMut(InstallProgressUpdate)>(
    logs: &mut StreamLogs,
    progress: &mut SteamInstallOutput,
    excerpt: &mut VecDeque<String>,
    _evidence: &mut CommandEvidence,
    deadline: InstallDeadline,
    on_progress: &mut F,
) -> Result<bool, SteamCmdError> {
    let update = deadline
        .run(logs.poll())
        .await
        .map_err(|_| operation_timeout(deadline))?
        .map_err(|source| SteamCmdError::SpawnCommand { source })?;
    for (source, line) in update.lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        push_excerpt_line(excerpt, line);
        let excerpt = join_excerpt_lines(excerpt);
        let update = match source {
            LogSource::Bootstrap => progress.bootstrap_line(line, excerpt),
            LogSource::Console => progress.console_line(line, excerpt),
        };
        if let Some(update) = update {
            on_progress(update);
        }
    }
    Ok(update.activity)
}

#[cfg(test)]
pub(super) fn spawn_line_forwarder<R>(
    reader: R,
    sender: mpsc::Sender<Result<String, std::io::Error>>,
) -> AbortOnDropTask<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    spawn_record_forwarder(reader, sender, false, |line, _, _| line)
}

fn spawn_record_forwarder<R, T, F>(
    mut reader: R,
    sender: mpsc::Sender<Result<T, std::io::Error>>,
    include_completed_snapshots: bool,
    record: F,
) -> AbortOnDropTask<()>
where
    R: AsyncRead + Unpin + Send + 'static,
    T: Send + 'static,
    F: Fn(String, bool, bool) -> T + Send + 'static,
{
    AbortOnDropTask::new(tokio::spawn(async move {
        let mut read_buffer = [0_u8; STEAMCMD_OUTPUT_READ_CHUNK_BYTES];
        let mut line_buffer = Vec::with_capacity(STEAMCMD_OUTPUT_LINE_LIMIT_BYTES);
        let mut line_truncated = false;
        let mut last_snapshot = String::new();
        let mut snapshot_tick = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_millis(250),
            Duration::from_millis(250),
        );
        snapshot_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let read = match tokio::select! {
                read = reader.read(&mut read_buffer) => read,
                _ = snapshot_tick.tick() => {
                    let complete = match std::str::from_utf8(&line_buffer) {
                        Err(error) if error.error_len().is_none() => error.valid_up_to(),
                        _ => line_buffer.len(),
                    };
                    let snapshot = finish_bounded_output_line(&line_buffer[..complete], line_truncated);
                    if !snapshot.is_empty() && snapshot != last_snapshot {
                        if sender.send(Ok(record(snapshot.clone(), false, false))).await.is_err() { return; }
                        last_snapshot = snapshot;
                    }
                    continue;
                }
            } {
                Ok(read) => read,
                Err(source) => {
                    let _ = sender.send(Err(source)).await;
                    return;
                }
            };
            if read == 0 {
                if (!line_buffer.is_empty() || line_truncated)
                    && (include_completed_snapshots
                        || finish_bounded_output_line(&line_buffer, line_truncated)
                            != last_snapshot)
                    && sender
                        .send(Ok(record(
                            finish_bounded_output_line(&line_buffer, line_truncated),
                            true,
                            finish_bounded_output_line(&line_buffer, line_truncated)
                                == last_snapshot,
                        )))
                        .await
                        .is_err()
                {
                    return;
                }
                return;
            }

            for &byte in &read_buffer[..read] {
                if matches!(byte, b'\n' | b'\r') {
                    if line_buffer.is_empty() && !line_truncated {
                        continue;
                    }
                    let line = finish_bounded_output_line(&line_buffer, line_truncated);
                    line_buffer.clear();
                    line_truncated = false;
                    let already_sent = line == last_snapshot;
                    last_snapshot.clear();
                    if (include_completed_snapshots || !already_sent)
                        && sender
                            .send(Ok(record(line, true, already_sent)))
                            .await
                            .is_err()
                    {
                        return;
                    }
                } else if line_buffer.len() < STEAMCMD_OUTPUT_LINE_LIMIT_BYTES {
                    line_buffer.push(byte);
                } else {
                    line_truncated = true;
                }
            }
        }
    }))
}

pub(super) fn finish_bounded_output_line(bytes: &[u8], input_truncated: bool) -> String {
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    let mut line = String::from_utf8_lossy(bytes).into_owned();
    let truncated = input_truncated || line.len() > STEAMCMD_OUTPUT_LINE_LIMIT_BYTES;
    let content_limit = if truncated {
        STEAMCMD_OUTPUT_LINE_LIMIT_BYTES - STEAMCMD_OUTPUT_LINE_TRUNCATED_SUFFIX.len()
    } else {
        STEAMCMD_OUTPUT_LINE_LIMIT_BYTES
    };
    if line.len() > content_limit {
        let mut boundary = content_limit;
        while !line.is_char_boundary(boundary) {
            boundary -= 1;
        }
        line.truncate(boundary);
    }
    if truncated {
        line.push_str(STEAMCMD_OUTPUT_LINE_TRUNCATED_SUFFIX);
    }
    line
}

pub(super) async fn abort_line_forwarders(
    stdout: &mut AbortOnDropTask<()>,
    stderr: &mut AbortOnDropTask<()>,
) {
    stdout.abort();
    stderr.abort();
    stdout.abort_and_join().await;
    stderr.abort_and_join().await;
}

pub(super) async fn join_line_forwarders(
    stdout: &mut AbortOnDropTask<()>,
    stderr: &mut AbortOnDropTask<()>,
) -> Result<(), SteamCmdError> {
    let stdout_result = stdout.join().await;
    let stderr_result = stderr.join().await;
    stdout_result
        .and(stderr_result)
        .map_err(|source| SteamCmdError::SpawnCommand {
            source: std::io::Error::other(format!("SteamCMD output reader failed: {source}")),
        })
}

pub(super) async fn sleep_with_deadline(
    duration: Duration,
    deadline: InstallDeadline,
) -> Result<(), SteamCmdError> {
    deadline
        .run(tokio::time::sleep(duration))
        .await
        .map_err(|InstallDeadlineElapsed| operation_timeout(deadline))
}

fn push_excerpt_line(lines: &mut VecDeque<String>, line: &str) {
    if lines.len() >= 40 {
        lines.pop_front();
    }
    lines.push_back(line.to_string());
}

fn join_excerpt_lines(lines: &VecDeque<String>) -> String {
    lines.iter().cloned().collect::<Vec<_>>().join(
        "
",
    )
}

pub(super) fn steamcmd_output_is_retryable_file_lock(excerpt: &str) -> bool {
    let lower = excerpt.to_ascii_lowercase();
    lower.contains("file locked") || (lower.contains("state is 0x602") && lower.contains("locked"))
}

pub(super) fn steamcmd_failure_context(
    excerpt: &str,
    exit_code: Option<i32>,
    content_log_excerpt: Option<&str>,
) -> String {
    let excerpt = format_command_failure_excerpt(exit_code, excerpt);

    let Some(content_log_excerpt) = content_log_excerpt.map(str::trim) else {
        return excerpt;
    };
    if content_log_excerpt.is_empty() {
        return excerpt;
    }

    if excerpt.contains(content_log_excerpt) {
        return excerpt;
    }

    format!("{excerpt}\n\n[steamcmd content log]\n{content_log_excerpt}")
}

pub(super) fn updated_content_log_excerpt(
    before: Option<&str>,
    after: Option<&str>,
) -> Option<String> {
    let after = after.map(str::trim).filter(|value| !value.is_empty())?;
    if before.map(str::trim) == Some(after) {
        return None;
    }

    Some(after.to_string())
}

pub(super) fn format_command_failure_excerpt(exit_code: Option<i32>, excerpt: &str) -> String {
    let exit_line = match exit_code {
        Some(code) => format!("Command exited with code {code}."),
        None => String::from("Command terminated without an exit code."),
    };

    let trimmed = excerpt.trim();
    if trimmed.is_empty() {
        exit_line
    } else {
        format!("{exit_line}\n{trimmed}")
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn managed_script_launch_uses_bootstrap_override_before_native_script_arguments() {
        let source = "http://127.0.0.1:12345";
        let script = Path::new("installation script.txt");
        let managed = steamcmd_script_command("steamcmd.exe", script, ".", Some(source));
        let arguments = managed
            .as_std()
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            [
                "-overridepackageurl",
                source,
                "+runscript",
                "installation script.txt"
            ]
        );
        let external = steamcmd_script_command("steamcmd.exe", script, ".", None);
        let arguments = external
            .as_std()
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(arguments, ["+runscript", "installation script.txt"]);
    }

    #[tokio::test]
    async fn partial_permission_preview_waits_for_the_complete_native_result() {
        use tokio::io::AsyncWriteExt;
        let (mut writer, reader) = tokio::io::duplex(1024);
        let (sender, mut receiver) = mpsc::channel(8);
        let mut forwarder =
            spawn_record_forwarder(reader, sender, true, |line, complete, repeated| {
                OutputRecord {
                    line,
                    complete,
                    repeated,
                }
            });
        writer
            .write_all(b"ERROR! Download item 3817722715 failed (Access Denied)")
            .await
            .unwrap();
        let preview = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!preview.complete);
        writer
            .write_all(b" while writing local cache\n")
            .await
            .unwrap();
        drop(writer);
        let mut evidence = CommandEvidence::default();
        while let Some(record) = receiver.recv().await {
            let record = record.unwrap();
            if record.complete {
                evidence.observe_line(&record.line);
            }
        }
        forwarder.join().await.unwrap();
        assert!(evidence.workshop_download_failed);
        assert!(!evidence.workshop.permission_denial(1_623_730));
    }

    #[tokio::test]
    async fn completed_identical_preview_still_reaches_decisive_evidence() {
        use tokio::io::AsyncWriteExt;
        let (mut writer, reader) = tokio::io::duplex(1024);
        let (sender, mut receiver) = mpsc::channel(8);
        let mut forwarder =
            spawn_record_forwarder(reader, sender, true, |line, complete, repeated| {
                OutputRecord {
                    line,
                    complete,
                    repeated,
                }
            });
        writer
            .write_all(b"ERROR! Download item 3817722715 failed (Access Denied).")
            .await
            .unwrap();
        let preview = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!preview.complete);
        writer.write_all(b"\n").await.unwrap();
        drop(writer);
        let complete = receiver.recv().await.unwrap().unwrap();
        assert!(complete.complete && complete.repeated);
        let mut evidence = CommandEvidence::default();
        evidence.observe_line(&complete.line);
        assert!(evidence.workshop.permission_denial(1_623_730));
        forwarder.join().await.unwrap();
    }
}

#[cfg(all(test, windows))]
#[path = "steamcmd_stream_spawn_tests.rs"]
mod spawn_tests;
