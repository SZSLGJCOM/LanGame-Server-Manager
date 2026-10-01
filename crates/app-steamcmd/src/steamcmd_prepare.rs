use std::path::PathBuf;
use std::time::Duration;

use app_core::AppSettings;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[cfg(test)]
use super::ps_literal;
use super::steamcmd_prepare_process::{PreparationAttempt, run_preparation_command};
use super::{
    InstallDeadline, SteamCmdError, SteamCmdSource, SteamCmdStatus, acquire_steamcmd_operation,
    apply_no_window, configured_steamcmd_root, managed_steamcmd_status,
    prepare_configured_steamcmd_root, steamcmd_status, validate_steamcmd_ownership,
};

pub(super) const STEAMCMD_PREPARE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const STEAMCMD_OUTPUT_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SteamCmdPreparePhase {
    Queued,
    Inspecting,
    Downloading,
    Extracting,
    Updating,
    Verifying,
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamCmdPrepareProgress {
    pub phase: SteamCmdPreparePhase,
    pub detail: String,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub output_excerpt: String,
}

pub async fn ensure_steamcmd_installed(
    settings: &AppSettings,
) -> Result<SteamCmdStatus, SteamCmdError> {
    ensure_steamcmd_installed_with_progress(settings, |_| {}).await
}

pub async fn ensure_steamcmd_installed_with_progress<F>(
    settings: &AppSettings,
    on_progress: F,
) -> Result<SteamCmdStatus, SteamCmdError>
where
    F: FnMut(SteamCmdPrepareProgress),
{
    ensure_steamcmd_installed_with_progress_and_cancellation(
        settings,
        &super::InstallCancellation::new(),
        on_progress,
    )
    .await
}

pub async fn ensure_steamcmd_installed_with_progress_and_cancellation<F>(
    settings: &AppSettings,
    cancellation: &super::InstallCancellation,
    on_progress: F,
) -> Result<SteamCmdStatus, SteamCmdError>
where
    F: FnMut(SteamCmdPrepareProgress),
{
    cancellation
        .scope(async {
            let deadline = InstallDeadline::new("SteamCMD preparation", STEAMCMD_PREPARE_TIMEOUT);
            let mut reporter = PrepareReporter::new(on_progress);
            reporter.stage(
                SteamCmdPreparePhase::Queued,
                "Waiting for the installation lock...",
            );
            let _operation = acquire_steamcmd_operation(settings, deadline).await?;
            prepare_locked(settings, deadline, &mut reporter).await
        })
        .await
}

async fn prepare_locked<F: FnMut(SteamCmdPrepareProgress)>(
    settings: &AppSettings,
    deadline: InstallDeadline,
    reporter: &mut PrepareReporter<F>,
) -> Result<SteamCmdStatus, SteamCmdError> {
    reporter.stage(
        SteamCmdPreparePhase::Inspecting,
        "Inspecting the SteamCMD installation...",
    );
    let current = managed_steamcmd_status(settings);
    if current.ownership == super::SteamCmdOwnership::Managed {
        return prepare_managed_installation(settings, deadline, reporter).await;
    }
    let discovered = steamcmd_status(settings);
    for status in [
        Some(current),
        matches!(discovered.source, SteamCmdSource::Discovered).then_some(discovered),
    ]
    .into_iter()
    .flatten()
    {
        if !status.executable_exists {
            continue;
        }
        super::steamcmd_readiness::invalidate(&status)?;
        if verify_existing_installation(&status, deadline, reporter).await? {
            deadline.check_cancelled()?;
            return reporter.ready(status);
        }
    }

    prepare_managed_installation(settings, deadline, reporter).await
}

async fn prepare_managed_installation<F: FnMut(SteamCmdPrepareProgress)>(
    settings: &AppSettings,
    deadline: InstallDeadline,
    reporter: &mut PrepareReporter<F>,
) -> Result<SteamCmdStatus, SteamCmdError> {
    let root = prepare_configured_steamcmd_root(&configured_steamcmd_root(settings))?;
    super::steamcmd_readiness::invalidate(&managed_steamcmd_status(settings))?;
    let update =
        super::steamcmd_update_cache::prepare_update(&root, deadline, reporter, false).await?;
    let result = apply_prepared_update(settings, update, deadline, reporter).await;
    if matches!(&result, Err(SteamCmdError::PrepareSteamCmd { .. })) {
        // Capability detection is not an integrity check. If a damaged modern
        // EXE cannot run, seed the verified bootstrapper once and try again.
        // Cancellation, timeout and unconfirmed process cleanup never retry.
        reporter.stage(
            SteamCmdPreparePhase::Extracting,
            "Repairing the SteamCMD bootstrapper...",
        );
        let update =
            super::steamcmd_update_cache::prepare_update(&root, deadline, reporter, true).await?;
        apply_prepared_update(settings, update, deadline, reporter).await?;
    } else {
        result?;
    }
    deadline.check_cancelled()?;
    validate_steamcmd_ownership(&root)?;
    reporter.ready(managed_steamcmd_status(settings))
}

async fn apply_prepared_update<F: FnMut(SteamCmdPrepareProgress)>(
    settings: &AppSettings,
    update: super::steamcmd_update_cache::PreparedUpdate,
    deadline: InstallDeadline,
    reporter: &mut PrepareReporter<F>,
) -> Result<(), SteamCmdError> {
    deadline.check_cancelled()?;
    let bridge = super::steamcmd_update_bridge::UpdateBridge::start(update.manifest).await?;
    reporter.stage(
        SteamCmdPreparePhase::Updating,
        "Applying SteamCMD updates...",
    );
    let result = warm_up_steamcmd_with_source(
        &managed_steamcmd_status(settings),
        deadline,
        reporter,
        Some(&bridge.url),
    )
    .await;
    bridge.close().await;
    result
}

async fn verify_existing_installation<F: FnMut(SteamCmdPrepareProgress)>(
    status: &SteamCmdStatus,
    deadline: InstallDeadline,
    reporter: &mut PrepareReporter<F>,
) -> Result<bool, SteamCmdError> {
    reporter.stage(
        SteamCmdPreparePhase::Verifying,
        "Checking SteamCMD with +quit...",
    );
    match warm_up_steamcmd(status, deadline, reporter).await {
        Ok(()) => Ok(true),
        Err(error @ SteamCmdError::PrepareSteamCmd { .. }) => {
            reporter.stage(
                SteamCmdPreparePhase::Inspecting,
                &format!("SteamCMD validation failed; checking installation sources: {error}"),
            );
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

async fn warm_up_steamcmd<F: FnMut(SteamCmdPrepareProgress)>(
    status: &SteamCmdStatus,
    deadline: InstallDeadline,
    reporter: &mut PrepareReporter<F>,
) -> Result<(), SteamCmdError> {
    warm_up_steamcmd_with_source(status, deadline, reporter, None).await
}

async fn warm_up_steamcmd_with_source<F: FnMut(SteamCmdPrepareProgress)>(
    status: &SteamCmdStatus,
    deadline: InstallDeadline,
    reporter: &mut PrepareReporter<F>,
    source: Option<&str>,
) -> Result<(), SteamCmdError> {
    verify_with_command_factory(deadline, STEAMCMD_OUTPUT_IDLE_TIMEOUT, reporter, || {
        let mut command = Command::new(&status.executable_path);
        apply_no_window(&mut command);
        if let Some(source) = source {
            command.args(["-overridepackageurl", source]);
        }
        command
            .arg("+quit")
            .current_dir(PathBuf::from(&status.root));
        command
    })
    .await
}

async fn verify_with_command_factory<F, C>(
    deadline: InstallDeadline,
    idle_timeout: Duration,
    reporter: &mut PrepareReporter<F>,
    mut command: C,
) -> Result<(), SteamCmdError>
where
    F: FnMut(SteamCmdPrepareProgress),
    C: FnMut() -> Command,
{
    for attempt in 0..2 {
        match run_preparation_command(command(), deadline, idle_timeout, attempt == 0, reporter).await? {
            PreparationAttempt::Verified => return Ok(()),
            PreparationAttempt::SelfUpdateHandoff => reporter.stage(
                SteamCmdPreparePhase::Verifying,
                "SteamCMD self-update completed; starting a fresh +quit process to verify the updated runtime...",
            ),
        }
    }
    Err(SteamCmdError::PrepareSteamCmd {
        output_excerpt: reporter.current.output_excerpt.clone(),
    })
}

pub(super) struct PrepareReporter<F> {
    on_progress: F,
    pub(super) current: SteamCmdPrepareProgress,
}

impl<F: FnMut(SteamCmdPrepareProgress)> PrepareReporter<F> {
    pub(super) fn new(on_progress: F) -> Self {
        Self {
            on_progress,
            current: SteamCmdPrepareProgress {
                phase: SteamCmdPreparePhase::Queued,
                detail: String::new(),
                downloaded_bytes: None,
                total_bytes: None,
                output_excerpt: String::new(),
            },
        }
    }

    pub(super) fn stage(&mut self, phase: SteamCmdPreparePhase, detail: &str) {
        self.current.phase = phase;
        self.current.detail = detail.to_owned();
        self.current.downloaded_bytes = None;
        self.current.total_bytes = None;
        (self.on_progress)(self.current.clone());
    }

    pub(super) fn download(&mut self, downloaded: u64, total: Option<u64>) {
        self.current.downloaded_bytes = Some(downloaded);
        self.current.total_bytes = total;
        (self.on_progress)(self.current.clone());
    }

    pub(super) fn output(&mut self, line: String, excerpt: String) {
        let lower = line.to_ascii_lowercase();
        let applying_update = [
            "extracting package",
            "installing update",
            "update complete, launching",
            "正在解压软件包",
            "正在提取软件包",
            "正在安装更新",
            "更新完成",
        ]
        .iter()
        .any(|marker| lower.contains(marker));
        if lower.contains("downloading update") || lower.contains("正在下载更新") || applying_update
        {
            self.current.phase = SteamCmdPreparePhase::Updating;
        }
        if let Some((downloaded, total)) = update_download_bytes(&line) {
            self.current.downloaded_bytes = Some(downloaded);
            self.current.total_bytes = Some(total);
        } else if applying_update {
            self.current.downloaded_bytes = None;
            self.current.total_bytes = None;
        }
        self.current.detail = line;
        self.current.output_excerpt = excerpt;
        (self.on_progress)(self.current.clone());
    }

    fn ready(&mut self, mut status: SteamCmdStatus) -> Result<SteamCmdStatus, SteamCmdError> {
        super::steamcmd_readiness::record_verified(&status)?;
        status.ready = true;
        self.stage(SteamCmdPreparePhase::Ready, "SteamCMD is ready.");
        Ok(status)
    }
}

fn update_download_bytes(line: &str) -> Option<(u64, u64)> {
    let (body, separator) = if let Some((_, body)) = line.split_once("Downloading update (") {
        (body, " of ")
    } else {
        (line.split_once("正在下载更新 (已下载 ")?.1, "，共 ")
    };
    let (downloaded, total) = body.split_once(" KB)")?.0.split_once(separator)?;
    let downloaded = parse_download_count(downloaded)?.checked_mul(1024)?;
    let total = parse_download_count(total)?.checked_mul(1024)?;
    (downloaded <= total && total > 0).then_some((downloaded, total))
}

fn parse_download_count(value: &str) -> Option<u64> {
    let value = value.trim();
    let mut groups = value.split(',');
    let first = groups.next()?;
    if first.is_empty() || !first.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if value.contains(',')
        && (first.len() > 3
            || groups
                .any(|group| group.len() != 3 || !group.bytes().all(|byte| byte.is_ascii_digit())))
    {
        return None;
    }
    value.replace(',', "").parse().ok()
}

#[cfg(test)]
#[path = "steamcmd_prepare_tests.rs"]
mod tests;
