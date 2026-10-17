//! Account names and allowlisted login phases are the only public login data.
//! Native prompts and credential input stay inside the ephemeral console owner.
use serde::Serialize;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{AppSettings, InstallCancellation, InstallLockContext, SteamCmdError};

#[cfg(windows)]
use super::install_process::run_command_capture;
#[cfg(windows)]
use super::{InstallDeadline, SteamCmdStatus, acquire_steamcmd_operation, steamcmd_script_root};
#[cfg(windows)]
use std::io::Write;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(windows)]
use tokio::process::Command;

#[cfg(windows)]
pub(super) struct AuthenticationLease {
    // Abort the endpoint before releasing the native runtime lease on failure
    // or an abandoned authentication worker.
    bridge: Option<super::steamcmd_update_bridge::UpdateBridge>,
    guard: super::SteamCmdLifecycleGuard,
}

#[cfg(windows)]
impl AuthenticationLease {
    fn update_source(&self) -> Option<&str> {
        self.bridge.as_ref().map(|bridge| bridge.url.as_str())
    }

    async fn close(mut self) {
        if let Some(bridge) = self.bridge.take() {
            bridge.close().await;
        }
        drop(self.guard);
    }
}

#[cfg(any(windows, test))]
#[path = "steamcmd_account_session.rs"]
mod session;

#[cfg(windows)]
const AUTHENTICATION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
#[cfg(windows)]
const CACHE_VERIFICATION_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SteamWorkshopAccountPhase {
    Queued,
    PreparingSteamcmd,
    Connecting,
    Authenticating,
    VerifyingCache,
    GuardCode,
    EmailCode,
    Password,
    SteamGuard,
    MobileConfirmation,
    Authenticated,
}

// Credential values must never gain diagnostic or serialization support.
pub struct SteamWorkshopAccountInput {
    pub challenge_revision: Option<u64>,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteamWorkshopAccountProgress {
    pub phase: SteamWorkshopAccountPhase,
    pub challenge_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SteamWorkshopAccountAuthenticationResult {
    pub account_name: String,
    pub steamcmd_root: String,
}

pub fn normalize_workshop_account_name(value: &str) -> Result<String, SteamCmdError> {
    if value.is_empty()
        || value.len() > 64
        || value.eq_ignore_ascii_case("anonymous")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'@' | b'-'))
    {
        return Err(SteamCmdError::InvalidSteamAccountName);
    }
    Ok(value.to_ascii_lowercase())
}

pub(super) fn workshop_login_line(account_name: Option<&str>) -> String {
    format!("login {}", account_name.unwrap_or("anonymous"))
}

/// Owns the SteamCMD lock until the console has been reaped and a fresh cached
/// login has verified the selected identity. The budget includes lock queuing.
pub async fn authenticate_workshop_account_with_progress<F>(
    lock_context: &InstallLockContext,
    settings: &AppSettings,
    account_name: &str,
    cancellation: &InstallCancellation,
    input: &mut mpsc::Receiver<SteamWorkshopAccountInput>,
    budget: Duration,
    on_progress: F,
) -> Result<SteamWorkshopAccountAuthenticationResult, SteamCmdError>
where
    F: FnMut(SteamWorkshopAccountProgress),
{
    let account_name = normalize_workshop_account_name(account_name)?;
    #[cfg(not(windows))]
    {
        let _ = (
            lock_context,
            settings,
            account_name,
            cancellation,
            input,
            budget,
            on_progress,
        );
        Err(SteamCmdError::SteamAccountInteractiveUnsupported)
    }
    #[cfg(windows)]
    cancellation
        .scope(async {
            let deadline = InstallDeadline::new(
                "Steam account authentication",
                budget.min(AUTHENTICATION_TIMEOUT),
            );
            let mut on_progress = on_progress;
            on_progress(SteamWorkshopAccountProgress {
                phase: SteamWorkshopAccountPhase::Queued,
                challenge_revision: None,
            });
            let guard = acquire_steamcmd_operation(lock_context, settings, deadline).await?;
            on_progress(SteamWorkshopAccountProgress {
                phase: SteamWorkshopAccountPhase::PreparingSteamcmd,
                challenge_revision: None,
            });
            let steamcmd = guard
                .require_ready_with_progress(settings, deadline, |_| {})
                .await?;
            let bridge = super::steamcmd_update_bridge::UpdateBridge::for_managed_root(
                std::path::Path::new(&steamcmd.root),
                deadline,
            )
            .await?;
            let lease = session::run(
                &steamcmd,
                &account_name,
                AuthenticationLease { bridge, guard },
                cancellation,
                input,
                deadline,
                &mut on_progress,
            )
            .await?;
            on_progress(SteamWorkshopAccountProgress {
                phase: SteamWorkshopAccountPhase::VerifyingCache,
                challenge_revision: None,
            });
            let verified =
                verify_cached_login(&steamcmd, &account_name, deadline, lease.update_source())
                    .await;
            lease.close().await;
            verified?;
            on_progress(SteamWorkshopAccountProgress {
                phase: SteamWorkshopAccountPhase::Authenticated,
                challenge_revision: None,
            });
            Ok(SteamWorkshopAccountAuthenticationResult {
                account_name,
                steamcmd_root: steamcmd.root,
            })
        })
        .await
}

#[cfg(windows)]
async fn verify_cached_login(
    steamcmd: &SteamCmdStatus,
    account_name: &str,
    deadline: InstallDeadline,
    update_source: Option<&str>,
) -> Result<(), SteamCmdError> {
    let script = AccountLoginScript::new(account_name)?;
    let mut command = Command::new(&steamcmd.executable_path);
    if let Some(source) = update_source {
        command.args(["-overridepackageurl", source]);
    }
    command
        .arg("+runscript")
        .arg(&script.path)
        .current_dir(&steamcmd.root);
    let output =
        run_command_capture(command, deadline.limited_to(CACHE_VERIFICATION_TIMEOUT)).await?;
    let excerpt = super::output_excerpt(&output.stdout, &output.stderr);
    let mut evidence = AccountLoginEvidence::default();
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .chain(String::from_utf8_lossy(&output.stderr).lines())
    {
        evidence.observe_line(line);
    }
    verify_cached_account(
        account_name,
        output.status.success(),
        evidence.authenticated_account_name(),
        &super::steamcmd_stream::format_command_failure_excerpt(output.status.code(), &excerpt),
    )
}

#[cfg(windows)]
struct AccountLoginScript {
    path: PathBuf,
}

#[cfg(windows)]
impl AccountLoginScript {
    fn new(account_name: &str) -> Result<Self, SteamCmdError> {
        let root = steamcmd_script_root();
        std::fs::create_dir_all(&root).map_err(|source| SteamCmdError::CreatePath {
            path: root.clone(),
            source,
        })?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let path = root.join(format!(
            "steamcmd-account-{}-{stamp}.txt",
            std::process::id()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| SteamCmdError::WriteScript {
                path: path.clone(),
                source,
            })?;
        let script = Self { path };
        let contents = format!(
            "@ShutdownOnFailedCommand 1\n@NoPromptForPassword 1\n{}\nquit\n",
            workshop_login_line(Some(account_name))
        );
        file.write_all(contents.as_bytes())
            .map_err(|source| SteamCmdError::WriteScript {
                path: script.path.clone(),
                source,
            })?;
        Ok(script)
    }
}

#[cfg(windows)]
impl Drop for AccountLoginScript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Default)]
pub(super) struct AccountLoginEvidence {
    account_name: Option<String>,
    waiting_for_user_info: bool,
    verified: bool,
}

impl AccountLoginEvidence {
    pub(super) fn observe_line(&mut self, line: &str) {
        let message = line
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
            .filter(|(stamp, _)| {
                stamp.len() == 19 && stamp.as_bytes()[4] == b'-' && stamp.as_bytes()[10] == b' '
            })
            .map_or(line, |(_, message)| message)
            .trim();
        let lower = message.to_ascii_lowercase();
        if let Some(remainder) = lower.strip_prefix("logging in user ") {
            let account_name = if let Some(quote) = remainder
                .chars()
                .next()
                .filter(|quote| matches!(*quote, '\'' | '"'))
            {
                remainder[1..].split(quote).next()
            } else {
                remainder.split_whitespace().next()
            };
            self.account_name =
                account_name.and_then(|name| normalize_workshop_account_name(name).ok());
            self.waiting_for_user_info = false;
            self.verified = false;
        } else if lower.starts_with("connecting anonymously")
            || lower.contains("login failure")
            || lower.contains("failed to log on")
            || lower.contains("failed to login")
            || lower.starts_with("error! failed to log")
        {
            self.account_name = None;
            self.waiting_for_user_info = false;
            self.verified = false;
        } else if let Some(result) = lower.strip_prefix("waiting for user info...") {
            self.waiting_for_user_info = result.trim().is_empty();
            self.verified = result.trim() == "ok";
        } else if self.waiting_for_user_info {
            if lower == "ok" {
                self.waiting_for_user_info = false;
                self.verified = true;
            } else if lower.contains("failed") {
                self.waiting_for_user_info = false;
                self.verified = false;
            }
        }
    }

    pub(super) fn authenticated_account_name(&self) -> Option<&str> {
        self.verified
            .then_some(self.account_name.as_deref())
            .flatten()
    }
}

pub(super) fn verify_authenticated_account(
    expected_account: &str,
    authenticated_account: Option<&str>,
    output_excerpt: &str,
) -> Result<(), SteamCmdError> {
    if authenticated_account.is_some_and(|name| name.eq_ignore_ascii_case(expected_account)) {
        return Ok(());
    }
    Err(SteamCmdError::SteamAccountAuthenticationFailed {
        output_excerpt: format!(
            "SteamCMD did not verify a cached login for the selected account. Complete SteamCMD login again; the download was not switched to anonymous.\n{output_excerpt}"
        ),
    })
}

#[cfg(any(windows, test))]
fn verify_cached_account(
    expected_account: &str,
    process_success: bool,
    authenticated_account: Option<&str>,
    output_excerpt: &str,
) -> Result<(), SteamCmdError> {
    // This process only logs in and quits. A download process, in contrast,
    // can fail after login, and its exit status belongs to Workshop validation.
    verify_authenticated_account(
        expected_account,
        process_success.then_some(authenticated_account).flatten(),
        output_excerpt,
    )
}

#[cfg(test)]
#[path = "steamcmd_account_tests.rs"]
mod tests;

#[cfg(all(test, windows))]
#[path = "steamcmd_account_bridge_tests.rs"]
mod bridge_tests;

#[cfg(all(test, windows))]
#[path = "steamcmd_account_code_probe_tests.rs"]
mod code_probe_tests;
