use super::install_progress::SteamInstallOutput;
use super::*;

#[path = "steamcmd_stream_logs.rs"]
mod logs;
use logs::{LogSource, StreamLogs};
#[path = "steamcmd_content_log.rs"]
mod content_log;
pub(super) use content_log::read_steamcmd_content_log_excerpt;

#[derive(Debug)]
pub(super) struct StreamingCommandResult {
    pub(super) success: bool,
    pub(super) exit_code: Option<i32>,
    pub(super) excerpt: String,
    pub(super) content_log_excerpt: Option<String>,
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
    command
        .arg("+runscript")
        .arg(script_path)
        .current_dir(working_directory)
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
    let content_log_before = deadline
        .run(read_steamcmd_content_log_excerpt(Path::new(
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
        mpsc::channel::<Result<String, std::io::Error>>(STEAMCMD_OUTPUT_CHANNEL_CAPACITY);
    let mut stdout_forwarder = spawn_line_forwarder(stdout, tx.clone());
    let mut stderr_forwarder = spawn_line_forwarder(stderr, tx.clone());
    drop(tx);

    let mut excerpt_lines = VecDeque::new();
    let mut progress = SteamInstallOutput::new();
    let mut status = None;
    let mut channel_closed = false;
    let mut log_tick = tokio::time::interval(Duration::from_millis(250));
    log_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        if status.is_some() && channel_closed {
            match publish_log_progress(
                &mut logs,
                &mut progress,
                &mut excerpt_lines,
                deadline,
                &mut on_progress,
            )
            .await
            {
                Ok(true) => continue,
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
                if let Err(error) = publish_log_progress(&mut logs, &mut progress, &mut excerpt_lines, deadline, &mut on_progress).await {
                    let cleanup = terminate_and_reap_child(&mut child, &mut process_guard, deadline).await;
                    abort_line_forwarders(&mut stdout_forwarder, &mut stderr_forwarder).await;
                    cleanup?;
                    return Err(error);
                }
            }
            maybe_line = rx.recv(), if !channel_closed => {
                match maybe_line {
                    Some(Ok(line)) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
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
    let content_log_after = deadline
        .run(read_steamcmd_content_log_excerpt(Path::new(
            working_directory,
        )))
        .await
        .map_err(|_| operation_timeout(deadline))?;
    Ok(StreamingCommandResult {
        success,
        exit_code,
        excerpt: join_excerpt_lines(&excerpt_lines),
        content_log_excerpt: updated_content_log_excerpt(
            content_log_before.as_deref(),
            content_log_after.as_deref(),
        ),
    })
}

async fn publish_log_progress<F: FnMut(InstallProgressUpdate)>(
    logs: &mut StreamLogs,
    progress: &mut SteamInstallOutput,
    excerpt: &mut VecDeque<String>,
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

pub(super) fn spawn_line_forwarder<R>(
    mut reader: R,
    sender: mpsc::Sender<Result<String, std::io::Error>>,
) -> AbortOnDropTask<()>
where
    R: AsyncRead + Unpin + Send + 'static,
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
                        if sender.send(Ok(snapshot.clone())).await.is_err() { return; }
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
                    && finish_bounded_output_line(&line_buffer, line_truncated) != last_snapshot
                    && sender
                        .send(Ok(finish_bounded_output_line(&line_buffer, line_truncated)))
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
                    if !already_sent && sender.send(Ok(line)).await.is_err() {
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
}
