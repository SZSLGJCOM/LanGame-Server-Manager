use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::install_process::{
    AbortOnDropTask, ChildProcessGuard, ManagedCommandSpawnError, spawn_managed_command_classified,
    terminate_and_reap_preparation_tree,
};
use super::steamcmd_bootstrap_log::BootstrapLog;
use super::steamcmd_prepare::{PrepareReporter, SteamCmdPrepareProgress};
use super::steamcmd_stream::{
    abort_line_forwarders, finish_bounded_output_line, format_command_failure_excerpt,
    join_line_forwarders,
};
use super::{InstallDeadline, STEAMCMD_OUTPUT_LINE_LIMIT_BYTES, SteamCmdError};

const EXCERPT_LIMIT_BYTES: usize = 16 * 1024;
const OUTPUT_CHANNEL_CAPACITY: usize = 16;
const OUTPUT_CHUNK_BYTES: usize = 4 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PreparationAttempt {
    Verified,
    SelfUpdateHandoff,
}

struct OutputChunk {
    stream: usize,
    bytes: Vec<u8>,
}

fn forward_output<R: AsyncRead + Unpin + Send + 'static>(
    mut reader: R,
    stream: usize,
    sender: mpsc::Sender<std::io::Result<OutputChunk>>,
) -> AbortOnDropTask<()> {
    AbortOnDropTask::new(tokio::spawn(async move {
        let mut bytes = [0_u8; OUTPUT_CHUNK_BYTES];
        loop {
            match reader.read(&mut bytes).await {
                Ok(0) => break,
                Ok(length) => {
                    if sender
                        .send(Ok(OutputChunk {
                            stream,
                            bytes: bytes[..length].to_vec(),
                        }))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error)).await;
                    break;
                }
            }
        }
    }))
}

pub(super) async fn run_preparation_command<F: FnMut(SteamCmdPrepareProgress)>(
    mut command: Command,
    deadline: InstallDeadline,
    idle_timeout: Duration,
    allow_handoff: bool,
    reporter: &mut PrepareReporter<F>,
) -> Result<PreparationAttempt, SteamCmdError> {
    deadline.check_cancelled()?;
    // Snapshot before spawning: previous attempts' logs cannot count as fresh
    // activity or prove that this process completed a self-update.
    let bootstrap = if let Some(root) = command.as_std().get_current_dir() {
        Some(
            deadline
                .run(BootstrapLog::before_spawn(root))
                .await
                .map_err(|_| super::operation_timeout(deadline))?
                .map_err(|source| SteamCmdError::SteamCmdPreparationLogRead {
                    source,
                    output_excerpt: String::new(),
                })?,
        )
    } else {
        None
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    deadline.check_cancelled()?;
    let (child, guard) = spawn_managed_command_classified(&mut command)
        .await
        .map_err(|error| match error {
            ManagedCommandSpawnError::NotStarted(source) => SteamCmdError::PrepareSteamCmd {
                output_excerpt: format!(
                    "SteamCMD could not start; no process was created: {source}"
                ),
            },
            ManagedCommandSpawnError::ProcessManagement(error) => error,
        })?;
    capture_preparation_process(
        child,
        guard,
        bootstrap,
        deadline,
        idle_timeout,
        allow_handoff,
        reporter,
    )
    .await
}

async fn capture_preparation_process<F: FnMut(SteamCmdPrepareProgress)>(
    mut child: Child,
    mut guard: ChildProcessGuard,
    mut bootstrap: Option<BootstrapLog>,
    deadline: InstallDeadline,
    idle_timeout: Duration,
    allow_handoff: bool,
    reporter: &mut PrepareReporter<F>,
) -> Result<PreparationAttempt, SteamCmdError> {
    let cancellation = super::InstallCancellation::current().unwrap_or_default();
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        terminate_and_reap_preparation_tree(&mut child, &mut guard).await?;
        return Err(SteamCmdError::SpawnCommand {
            source: std::io::Error::other("SteamCMD preparation output pipes are unavailable"),
        });
    };
    let (sender, mut receiver) = mpsc::channel(OUTPUT_CHANNEL_CAPACITY);
    let mut stdout = forward_output(stdout, 0, sender.clone());
    let mut stderr = forward_output(stderr, 1, sender);
    let mut output = PreparationOutput::default();
    let mut runtime = super::steamcmd_runtime_evidence::RuntimeEvidence::default();
    let mut last_output = Instant::now();
    let mut status = None;
    let mut closed = false;
    let mut log_poll = tokio::time::interval(Duration::from_millis(250));
    log_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let result = loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break Err(super::operation_timeout(deadline)),
            _ = tokio::time::sleep_until(deadline.expires_at()) => break Err(SteamCmdError::SteamCmdPreparationTimedOut {
                timeout_seconds: deadline.timeout().as_secs(), output_excerpt: output.excerpt(),
            }),
            _ = tokio::time::sleep_until(last_output + idle_timeout) => {
                // A completed bootstrap may leave its restarted client waiting on
                // the network or inherited pipes. One fresh owned +quit process
                // must verify it; the completion marker is never readiness.
                if output.self_updated && allow_handoff {
                    break Ok(PreparationAttempt::SelfUpdateHandoff);
                }
                break Err(SteamCmdError::SteamCmdPreparationStalled {
                    timeout_seconds: idle_timeout.as_secs(), output_excerpt: output.excerpt(),
                });
            }
            next = receiver.recv(), if !closed => match next {
                Some(Ok(chunk)) => {
                    last_output = Instant::now();
                    runtime.push(chunk.stream, &chunk.bytes);
                    let line = output.push(chunk.stream, &chunk.bytes);
                    if !line.is_empty() { reporter.output(line, output.excerpt()); }
                }
                Some(Err(source)) => break Err(SteamCmdError::PrepareSteamCmd {
                    output_excerpt: format!("SteamCMD output read failed: {source}\n{}", output.excerpt()),
                }),
                None => closed = true,
            },
            exit = child.wait(), if status.is_none() => match exit {
                Ok(exit) => status = Some(exit),
                Err(source) => break Err(SteamCmdError::PrepareSteamCmd {
                    output_excerpt: format!("SteamCMD process wait failed: {source}\n{}", output.excerpt()),
                }),
            },
            _ = log_poll.tick(), if bootstrap.is_some() => {
                if let Some(log) = bootstrap.as_mut()
                    && let Err(error) = read_bootstrap_progress(log, &mut output, &mut last_output, idle_timeout, deadline, reporter, false).await
                {
                    break Err(error);
                }
            },
            empty = guard.wait_until_empty(deadline), if status.is_some() && closed => {
                if let Err(error) = empty {
                    if matches!(error, SteamCmdError::InstallCancelled { .. }) { break Err(error); }
                    break Err(SteamCmdError::PrepareSteamCmd { output_excerpt: format!("{error}\n{}", output.excerpt()) });
                }
                if let Some(log) = bootstrap.as_mut() {
                    match read_bootstrap_progress(log, &mut output, &mut last_output, idle_timeout, deadline, reporter, true).await {
                        Ok(true) => continue,
                        Ok(false) => {},
                        Err(error) => break Err(error),
                    }
                }
                if output.self_updated && allow_handoff {
                    break Ok(PreparationAttempt::SelfUpdateHandoff);
                }
                break match status {
                    Some(exit) if exit.success() && runtime.ready() => Ok(PreparationAttempt::Verified),
                    Some(exit) if exit.success() => Err(SteamCmdError::PrepareSteamCmd {
                        output_excerpt: format!("SteamCMD exited without proving the console runtime and Steam API initialized. Bootstrap verification alone is insufficient.\n{}", output.excerpt()),
                    }),
                    exit => Err(SteamCmdError::PrepareSteamCmd { output_excerpt: format_command_failure_excerpt(
                        exit.and_then(|exit| exit.code()), &output.excerpt(),
                    ) }),
                };
            }
        }
    };

    if !matches!(result, Ok(PreparationAttempt::Verified)) {
        let cleanup = terminate_and_reap_preparation_tree(&mut child, &mut guard).await;
        abort_line_forwarders(&mut stdout, &mut stderr).await;
        if let Err(error) = cleanup {
            return Err(SteamCmdError::SteamCmdPreparationCleanupFailed {
                output_excerpt: format!(
                    "Failed to stop the previous SteamCMD process tree: {error}\n{}",
                    output.excerpt(),
                ),
            });
        }
    } else {
        join_line_forwarders(&mut stdout, &mut stderr).await?;
    }
    result
}

async fn read_bootstrap_progress<F: FnMut(SteamCmdPrepareProgress)>(
    log: &mut BootstrapLog,
    output: &mut PreparationOutput,
    last_output: &mut Instant,
    idle_timeout: Duration,
    deadline: InstallDeadline,
    reporter: &mut PrepareReporter<F>,
    flush_partial: bool,
) -> Result<bool, SteamCmdError> {
    let expires = deadline.expires_at().min(*last_output + idle_timeout);
    let update = match deadline
        .run(tokio::time::timeout_at(expires, log.poll()))
        .await
        .map_err(|_| super::operation_timeout(deadline))?
    {
        Ok(Ok(update)) => update,
        Ok(Err(source)) => {
            return Err(SteamCmdError::SteamCmdPreparationLogRead {
                source,
                output_excerpt: output.excerpt(),
            });
        }
        Err(_) if Instant::now() >= deadline.expires_at() => {
            return Err(SteamCmdError::SteamCmdPreparationTimedOut {
                timeout_seconds: deadline.timeout().as_secs(),
                output_excerpt: output.excerpt(),
            });
        }
        Err(_) => {
            return Err(SteamCmdError::SteamCmdPreparationStalled {
                timeout_seconds: idle_timeout.as_secs(),
                output_excerpt: output.excerpt(),
            });
        }
    };
    if update.reset {
        output.pending[2].clear();
        output.marker_tail[2].clear();
        output.truncated[2] = false;
    }
    if update.activity {
        *last_output = Instant::now();
    }
    // Files are not console buffers: publish each complete CR/LF record, so a
    // trailing status message cannot hide preceding real download byte counts.
    for segment in update
        .bytes
        .split_inclusive(|byte| *byte == b'\r' || *byte == b'\n')
    {
        let line = output.push(2, segment);
        if segment
            .last()
            .is_some_and(|byte| *byte == b'\r' || *byte == b'\n')
            && !line.is_empty()
        {
            reporter.output(line, output.excerpt());
        }
    }
    if flush_partial && !output.pending[2].is_empty() {
        reporter.output(output.line(2), output.excerpt());
    }
    Ok(update.activity)
}

#[derive(Default)]
struct PreparationOutput {
    pending: [VecDeque<u8>; 3],
    marker_tail: [Vec<u8>; 3],
    truncated: [bool; 3],
    lines: VecDeque<String>,
    retained_bytes: usize,
    self_updated: bool,
}

impl PreparationOutput {
    fn push(&mut self, stream: usize, bytes: &[u8]) -> String {
        const MARKER: &[u8] = b"Update complete, launching";
        if !self.self_updated {
            let mut scan = std::mem::take(&mut self.marker_tail[stream]);
            scan.extend_from_slice(bytes);
            self.self_updated = scan.windows(MARKER.len()).any(|window| window == MARKER);
            self.marker_tail[stream] = scan[scan.len().saturating_sub(MARKER.len() - 1)..].to_vec();
        }
        let mut latest = String::new();
        for &byte in bytes {
            if byte == b'\r' || byte == b'\n' {
                if !self.pending[stream].is_empty() || self.truncated[stream] {
                    latest = self.line(stream);
                    self.retained_bytes += latest.len() + 1;
                    self.lines.push_back(latest.clone());
                    self.pending[stream].clear();
                    self.truncated[stream] = false;
                    while self.retained_bytes > EXCERPT_LIMIT_BYTES / 2 || self.lines.len() > 40 {
                        if let Some(line) = self.lines.pop_front() {
                            self.retained_bytes -= line.len() + 1;
                        }
                    }
                }
            } else {
                if self.pending[stream].len() == STEAMCMD_OUTPUT_LINE_LIMIT_BYTES {
                    self.pending[stream].pop_front();
                    self.truncated[stream] = true;
                }
                self.pending[stream].push_back(byte);
            }
        }
        if !self.pending[stream].is_empty() {
            latest = self.line(stream);
        }
        latest
    }

    fn line(&self, stream: usize) -> String {
        let bytes = self.pending[stream].iter().copied().collect::<Vec<_>>();
        if self.truncated[stream] {
            const PREFIX: &str = "[line truncated] ";
            let line = String::from_utf8_lossy(&bytes);
            let mut start = line
                .len()
                .saturating_sub(STEAMCMD_OUTPUT_LINE_LIMIT_BYTES - PREFIX.len());
            while !line.is_char_boundary(start) {
                start += 1;
            }
            format!("{PREFIX}{}", &line[start..])
        } else {
            finish_bounded_output_line(&bytes, false)
        }
    }

    fn excerpt(&self) -> String {
        let mut lines = self.lines.iter().cloned().collect::<Vec<_>>();
        for stream in 0..self.pending.len() {
            if !self.pending[stream].is_empty() {
                lines.push(self.line(stream));
            }
        }
        let mut excerpt = lines.join("\n");
        if excerpt.len() > EXCERPT_LIMIT_BYTES {
            let mut start = excerpt.len() - EXCERPT_LIMIT_BYTES;
            while !excerpt.is_char_boundary(start) {
                start += 1;
            }
            excerpt.drain(..start);
        }
        excerpt
    }
}

#[cfg(test)]
#[path = "steamcmd_prepare_process_tests.rs"]
mod tests;
