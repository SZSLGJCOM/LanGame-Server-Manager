//! Native output is reduced to safe phases before crossing the worker boundary.
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use super::SteamWorkshopAccountPhase as Phase;
#[cfg(windows)]
use super::{SteamWorkshopAccountInput, SteamWorkshopAccountProgress};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChallengeAction {
    NativePassword,
    NativeCode,
    RestartWithDeviceCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Challenge {
    revision: u64,
    phase: Phase,
    action: ChallengeAction,
}

struct PromptState {
    revision: u64,
    phase: Phase,
    pending: Option<Challenge>,
    last_prompt: Option<Phase>,
    login: Option<InteractiveLogin>,
    email_code_expected: bool,
    frozen: bool,
}

struct InteractiveLogin {
    account_name: String,
    started: bool,
    verified: bool,
    failed: bool,
    guard_code_selected: bool,
    mobile_after_code: bool,
    progress_observed: bool,
    prepared_password_available: bool,
    prepared_password_pending: bool,
    discard_prepared_password: bool,
    start_login_pending: bool,
    evidence: super::AccountLoginEvidence,
}

impl InteractiveLogin {
    fn new(account_name: String) -> Self {
        Self {
            account_name,
            started: false,
            verified: false,
            failed: false,
            guard_code_selected: false,
            mobile_after_code: false,
            progress_observed: false,
            prepared_password_available: false,
            prepared_password_pending: false,
            discard_prepared_password: false,
            start_login_pending: false,
            evidence: super::AccountLoginEvidence::default(),
        }
    }
}

impl Default for PromptState {
    fn default() -> Self {
        Self {
            revision: 0,
            phase: Phase::Connecting,
            pending: None,
            last_prompt: None,
            login: None,
            email_code_expected: false,
            frozen: false,
        }
    }
}

impl PromptState {
    fn for_account(account_name: String) -> Self {
        Self {
            login: Some(InteractiveLogin::new(account_name)),
            phase: Phase::PreparingSteamcmd,
            ..Self::default()
        }
    }

    fn observe_snapshot(&mut self, row: &str) {
        if self.frozen {
            return;
        }
        let lower = row.trim().to_ascii_lowercase();
        if lower.starts_with("please check your email for the message from steam,") {
            self.email_code_expected = true;
        }
        let mut authenticating = false;
        if let Some(login) = self.login.as_mut() {
            if login.started {
                login.evidence.observe_line(row);
                login.progress_observed |= lower.starts_with("steam>login ")
                    || [
                        "logging in",
                        "connecting ",
                        "password:",
                        "steam guard code:",
                        "two-factor code:",
                        "login failure:",
                        "error!",
                        "failed",
                    ]
                    .iter()
                    .any(|prefix| lower.starts_with(prefix));
                login.verified = login
                    .evidence
                    .authenticated_account_name()
                    .is_some_and(|account| account.eq_ignore_ascii_case(&login.account_name));
                let configuration_ready = lower.starts_with("waiting for client config...")
                    || lower.starts_with("waiting for user info...");
                authenticating = configuration_ready
                    || lower.starts_with("logging in user ") && lower.ends_with("...ok")
                    || login.verified;
                if configuration_ready || login.verified {
                    // Cached credentials can fail first and still open a real
                    // password prompt. Keep the prepared password until native
                    // login has progressed beyond credential authentication.
                    login.prepared_password_available = false;
                    login.prepared_password_pending = false;
                    login.discard_prepared_password = true;
                }
            }
            if lower == "steam>" {
                if login.started {
                    // Ignore a repeated startup prompt while ConPTY redraws
                    // the submitted row. A consumed command or real login
                    // progress distinguishes a later failed-command prompt.
                    login.failed |= login.progress_observed && !login.verified;
                } else {
                    login.started = true;
                    login.start_login_pending = true;
                    self.publish(Phase::Connecting);
                }
                return;
            }
        }
        if authenticating
            && matches!(
                self.phase,
                Phase::Connecting | Phase::MobileConfirmation | Phase::GuardCode
            )
        {
            // The first native attempt may be approved on the user's phone
            // before a replacement token is accepted. Its proof wins atomically
            // here; a later queued code must not be written into Steam>.
            self.pending = None;
            self.publish(Phase::Authenticating);
        }
        if is_login_retry(&lower) {
            if lower.contains("invalid password")
                && let Some(login) = self
                    .login
                    .as_mut()
                    .filter(|login| !login.prepared_password_available)
            {
                login.discard_prepared_password = true;
                login.prepared_password_available = false;
            }
            self.last_prompt = None;
            self.pending = None;
            self.publish(if self.login.is_some() {
                Phase::Authenticating
            } else {
                Phase::Connecting
            });
            return;
        }
        let Some(mut phase) = prompt_phase(&lower) else {
            return;
        };
        if phase == Phase::Password
            && let Some(login) = self
                .login
                .as_mut()
                .filter(|login| login.started && login.prepared_password_available)
        {
            login.prepared_password_available = false;
            login.prepared_password_pending = true;
            self.last_prompt = Some(phase);
            self.pending = None;
            return;
        }
        if phase == Phase::MobileConfirmation
            && let Some(login) = self.login.as_mut()
        {
            if login.guard_code_selected {
                login.failed = true;
                login.mobile_after_code = true;
            } else if self.last_prompt != Some(Phase::GuardCode) {
                self.publish_prompt(
                    Phase::GuardCode,
                    Some(ChallengeAction::RestartWithDeviceCode),
                );
            }
            return;
        }
        if phase == Phase::GuardCode {
            self.email_code_expected = false;
        } else if phase == Phase::SteamGuard && self.email_code_expected {
            phase = Phase::EmailCode;
        }
        // ConPTY publishes edited full rows. Prefix snapshots and the completed
        // row must never open the same password/Guard challenge a second time.
        if self.last_prompt == Some(phase) {
            return;
        }
        let action = match phase {
            Phase::Password => {
                self.email_code_expected = false;
                Some(ChallengeAction::NativePassword)
            }
            Phase::GuardCode | Phase::SteamGuard | Phase::EmailCode => {
                Some(ChallengeAction::NativeCode)
            }
            _ => None,
        };
        self.publish_prompt(phase, action);
    }

    fn publish_prompt(&mut self, phase: Phase, action: Option<ChallengeAction>) {
        self.last_prompt = Some(phase);
        self.publish(phase);
        self.pending = action.map(|action| Challenge {
            revision: self.revision,
            phase,
            action,
        });
    }

    fn publish(&mut self, phase: Phase) {
        self.revision += 1;
        self.phase = phase;
    }

    fn consume(&mut self, revision: u64) -> Option<Challenge> {
        let challenge = self
            .pending
            .filter(|challenge| challenge.revision == revision)?;
        if let Some(login) = self.login.as_mut() {
            match challenge.action {
                ChallengeAction::NativePassword => login.discard_prepared_password = false,
                ChallengeAction::RestartWithDeviceCode => {
                    // Freeze the old sink before its Job is closed. Approval
                    // arriving after this accepted code cannot verify a new job.
                    self.frozen = true;
                }
                ChallengeAction::NativeCode => {}
            }
        }
        self.pending = None;
        self.publish(
            if challenge.action == ChallengeAction::RestartWithDeviceCode {
                Phase::PreparingSteamcmd
            } else {
                Phase::Connecting
            },
        );
        Some(challenge)
    }

    fn restart_with_device_code(&mut self, account_name: String, password_available: bool) {
        let revision = self.revision;
        let mut login = InteractiveLogin::new(account_name);
        login.guard_code_selected = true;
        login.prepared_password_available = password_available;
        *self = Self {
            login: Some(login),
            revision,
            ..Self::default()
        };
        self.publish(Phase::PreparingSteamcmd);
    }
}

fn prompt_phase(row: &str) -> Option<Phase> {
    match row {
        "password:" => Some(Phase::Password),
        "steam guard:" | "steam guard code:" | "please enter your steam guard code:" => {
            Some(Phase::SteamGuard)
        }
        "two-factor code:"
        | "steam guard mobile authenticator code:"
        | "please enter the current code from your authenticator app:" => Some(Phase::GuardCode),
        "please confirm the login in the steam mobile app on your phone."
        | "please confirm the login in the steam mobile app on your phone..."
        | "please confirm this login in the steam mobile app on your phone."
        | "please confirm this login in the steam mobile app on your phone..." => {
            Some(Phase::MobileConfirmation)
        }
        _ => None,
    }
}

fn is_login_retry(row: &str) -> bool {
    // Suppressed password echo may leave SteamCMD's native failure on the
    // same screen row as its prompt, rather than starting a new output line.
    let row = ["password:", "steam guard code:", "two-factor code:"]
        .iter()
        .find_map(|prompt| row.strip_prefix(*prompt))
        .unwrap_or(row)
        .trim();
    (row.starts_with("login failure:")
        || row.starts_with("error! failed to log on:")
        || row.starts_with("failed"))
        && [
            "invalid password",
            "invalid login auth code",
            "two-factor code mismatch",
            "invalid authentication code",
            "incorrect steam guard code",
            "expired",
        ]
        .iter()
        .any(|reason| row.contains(reason))
}

/// The sink retains no transcript, echoed response, password, or code.
struct PromptSink(Arc<Mutex<PromptState>>);

impl Write for PromptSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| io::Error::other("Login prompt state is unavailable."))?;
        for row in String::from_utf8_lossy(bytes).lines() {
            state.observe_snapshot(row);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn valid_response(phase: Phase, value: &str) -> bool {
    if phase == Phase::GuardCode {
        return value.len() == 5 && value.bytes().all(|byte| byte.is_ascii_alphanumeric());
    }
    !value.is_empty()
        && value.len() <= 1024
        && !value.chars().any(char::is_control)
        && (phase == Phase::Password
            || (matches!(phase, Phase::SteamGuard | Phase::EmailCode)
                && value.len() <= 32
                && value.bytes().all(|byte| byte.is_ascii_alphanumeric())))
}

#[cfg(windows)]
#[path = "steamcmd_account_console.rs"]
mod native;

#[cfg(windows)]
pub(super) use native::run;

#[cfg(test)]
#[path = "steamcmd_account_session_tests.rs"]
mod tests;
