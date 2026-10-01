use app_core::{InstallPhase, InstallProgress};

use super::{SteamCmdPreparePhase, SteamCmdPrepareProgress};

#[derive(Debug, Clone)]
pub struct InstallProgressUpdate {
    pub progress_percent: f32,
    pub detail: String,
    pub output_excerpt: String,
    pub install_progress: Option<InstallProgress>,
}

impl InstallProgressUpdate {
    pub fn stage(phase: InstallPhase, detail: impl Into<String>) -> Self {
        let percent = (phase == InstallPhase::Ready).then_some(100.0);
        Self {
            progress_percent: percent.unwrap_or(0.0),
            detail: detail.into(),
            output_excerpt: String::new(),
            install_progress: Some(InstallProgress {
                phase,
                downloaded_bytes: None,
                total_bytes: None,
                percent,
                elapsed_seconds: 0,
            }),
        }
    }

    pub fn download(detail: impl Into<String>, downloaded: u64, total: Option<u64>) -> Self {
        let total = total.filter(|total| *total > 0 && downloaded <= *total);
        let percent = total.map(|total| ((downloaded as f64 / total as f64) * 100.0) as f32);
        let mut update = Self::stage(InstallPhase::Downloading, detail);
        if let Some(progress) = update.install_progress.as_mut() {
            progress.downloaded_bytes = Some(downloaded);
            progress.total_bytes = total;
            progress.percent = percent;
        }
        update.progress_percent = percent.unwrap_or(0.0);
        update
    }

    pub fn with_output(mut self, output: impl Into<String>) -> Self {
        self.output_excerpt = output.into();
        self
    }

    pub(super) fn preparing_steamcmd(update: SteamCmdPrepareProgress) -> Self {
        let phase = match update.phase {
            SteamCmdPreparePhase::Queued => InstallPhase::Queued,
            SteamCmdPreparePhase::Inspecting | SteamCmdPreparePhase::Ready => {
                InstallPhase::Preparing
            }
            SteamCmdPreparePhase::Downloading => InstallPhase::Downloading,
            SteamCmdPreparePhase::Extracting => InstallPhase::Extracting,
            SteamCmdPreparePhase::Updating if update.downloaded_bytes.is_some() => {
                InstallPhase::Downloading
            }
            SteamCmdPreparePhase::Updating => InstallPhase::Installing,
            SteamCmdPreparePhase::Verifying => InstallPhase::Verifying,
        };
        let detail = format!("SteamCMD: {}", update.detail);
        let result = if phase == InstallPhase::Downloading
            && let Some(downloaded) = update.downloaded_bytes
        {
            Self::download(detail, downloaded, update.total_bytes)
        } else {
            Self::stage(phase, detail)
        };
        result.with_output(update.output_excerpt)
    }
}

/// SteamCMD reports progress for individual update states, not the whole install.
pub(super) struct SteamInstallOutput {
    progress: InstallProgress,
    bootstrap: super::steamcmd_prepare::PrepareReporter<fn(SteamCmdPrepareProgress)>,
    runtime_started: bool,
    console_selected: bool,
}

impl SteamInstallOutput {
    pub(super) fn new() -> Self {
        let mut bootstrap =
            super::steamcmd_prepare::PrepareReporter::new((|_| {}) as fn(SteamCmdPrepareProgress));
        bootstrap.current.phase = SteamCmdPreparePhase::Verifying;
        Self {
            progress: InstallProgress {
                phase: InstallPhase::Preparing,
                downloaded_bytes: None,
                total_bytes: None,
                percent: None,
                elapsed_seconds: 0,
            },
            bootstrap,
            runtime_started: false,
            console_selected: false,
        }
    }

    pub(super) fn stdout_line(
        &mut self,
        line: &str,
        excerpt: String,
    ) -> Option<InstallProgressUpdate> {
        // The native console log is flushed while redirected stdout may arrive
        // much later. Its first meaningful runtime step owns progress, including
        // login waits before any download counter has been reported.
        if self.console_selected {
            return None;
        }
        self.line(line, excerpt)
    }

    pub(super) fn console_line(
        &mut self,
        line: &str,
        excerpt: String,
    ) -> Option<InstallProgressUpdate> {
        let runtime_activity = !is_runtime_banner(console_message(line));
        let update = self.line(line, excerpt);
        self.console_selected |= runtime_activity && update.is_some();
        update
    }

    pub(super) fn line(&mut self, line: &str, excerpt: String) -> Option<InstallProgressUpdate> {
        let message = console_message(line);
        let lower = message.to_ascii_lowercase();
        if lower.contains("update state (")
            && !lower
                .split_once("progress:")
                .is_some_and(|(_, values)| values.contains(')'))
        {
            // A growing line must not erase the last complete measurement.
            return None;
        }
        if let Some(progress) = parse_steamcmd_install_progress(line) {
            self.progress = progress;
            self.runtime_started = true;
        } else if message.starts_with("Success! App '") {
            self.runtime_started = true;
            self.progress.phase = InstallPhase::Verifying;
            self.progress.downloaded_bytes = None;
            self.progress.total_bytes = None;
            self.progress.percent = None;
        } else if is_runtime_activity(message)
            || (is_runtime_banner(message) && !self.runtime_started)
        {
            // Publish the runtime boundary once so consumers discard bootstrap
            // counters, while later banners cannot replace the current wait.
            self.begin_runtime();
        } else {
            // Banners, logging paths, configuration echoes and standalone OK lines
            // remain in the output excerpt, but cannot replace a real wait.
            return None;
        }
        Some(InstallProgressUpdate {
            progress_percent: self.progress.percent.unwrap_or(0.0),
            detail: line.to_owned(),
            output_excerpt: excerpt,
            install_progress: Some(self.progress.clone()),
        })
    }

    fn begin_runtime(&mut self) {
        if !self.runtime_started {
            self.progress.phase = InstallPhase::Preparing;
            self.progress.downloaded_bytes = None;
            self.progress.total_bytes = None;
            self.progress.percent = None;
        }
        self.runtime_started = true;
    }

    pub(super) fn bootstrap_line(
        &mut self,
        line: &str,
        excerpt: String,
    ) -> Option<InstallProgressUpdate> {
        if self.runtime_started || self.console_selected {
            return None;
        }
        self.bootstrap.output(line.to_owned(), excerpt);
        let update = InstallProgressUpdate::preparing_steamcmd(self.bootstrap.current.clone());
        if let Some(progress) = update.install_progress.as_ref() {
            self.progress = progress.clone();
        }
        Some(update)
    }
}

fn is_runtime_banner(message: &str) -> bool {
    message.starts_with("Steam Console Client") || message.starts_with("Loading Steam API")
}

fn is_runtime_activity(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower == "login anonymous"
        || lower
            .strip_prefix("app_update ")
            .and_then(|args| args.split_whitespace().next())
            .is_some_and(|id| id.parse::<u32>().is_ok())
        || [
            "connecting anonymously",
            "connecting to steam",
            "logging in",
            "waiting for ",
            "loading app info",
            "requesting app info",
            "downloading item ",
            "success. downloaded item ",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn console_message(line: &str) -> &str {
    line.strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .filter(|(stamp, _)| {
            stamp.len() == 19 && stamp.as_bytes()[4] == b'-' && stamp.as_bytes()[10] == b' '
        })
        .map_or(line, |(_, message)| message.trim_start())
}

fn parse_steamcmd_install_progress(line: &str) -> Option<InstallProgress> {
    let lower = line.to_ascii_lowercase();
    let state = lower
        .split_once("update state (")?
        .1
        .split_once(')')?
        .1
        .trim();
    // An unterminated console snapshot may end in the middle of the state name.
    let phase_name = state.split_once(',')?.0.trim();
    let phase = match phase_name {
        "downloading" => InstallPhase::Downloading,
        name if name.starts_with("validating") || name.starts_with("verifying") => {
            InstallPhase::Verifying
        }
        "extracting" | "unpacking" => InstallPhase::Extracting,
        "preallocating" | "waiting" | "queued" => InstallPhase::Preparing,
        _ => InstallPhase::Installing,
    };
    let numbers = state
        .split_once("progress:")
        .map(|(_, numbers)| numbers.trim());
    let percent = numbers
        // Partial snapshots may stop in the middle of a percentage token.
        // Native progress includes a counter pair; wait for its closing ')'.
        .and_then(|numbers| {
            let (percent, counters) = numbers.split_once('(')?;
            counters.split_once(')')?;
            Some(percent.trim())
        })
        .and_then(|number| number.trim_end_matches('%').parse::<f32>().ok())
        .filter(|value| value.is_finite() && (0.0..=100.0).contains(value));
    let bytes = numbers.and_then(|numbers| {
        let pair = numbers.split_once('(')?.1.split_once(')')?.0;
        let (downloaded, total) = pair.split_once('/')?;
        let downloaded = downloaded.trim().parse::<u64>().ok()?;
        let total = total.trim().parse::<u64>().ok()?;
        (total > 0 && downloaded <= total).then_some((downloaded, total))
    });
    // Validation counters measure local file processing, not network traffic.
    let bytes = (phase == InstallPhase::Downloading)
        .then_some(bytes)
        .flatten();
    Some(InstallProgress {
        phase,
        downloaded_bytes: bytes.map(|(downloaded, _)| downloaded),
        total_bytes: bytes.map(|(_, total)| total),
        percent,
        elapsed_seconds: 0,
    })
}

#[cfg(test)]
#[path = "install_progress_tests.rs"]
mod tests;
