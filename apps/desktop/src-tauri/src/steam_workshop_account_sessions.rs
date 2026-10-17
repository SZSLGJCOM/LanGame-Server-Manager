//! Login state contains only allowlisted prompts. Secret input lives solely in
//! the bounded worker channel and is never part of a snapshot or run history.
use app_steamcmd::{
    SteamWorkshopAccountAuthenticationResult, SteamWorkshopAccountInput, SteamWorkshopAccountPhase,
    SteamWorkshopAccountProgress,
};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tokio::sync::mpsc;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum SteamAccountLoginTarget {
    Instance(String),
    Module(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LoginPhase {
    Ready,
    InputSubmitted,
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
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InputMode {
    Command,
    Secret,
    Disabled,
}

#[derive(Clone, Serialize)]
pub(crate) struct SteamWorkshopAccountLoginSnapshot {
    pub session_id: String,
    pub target: SteamAccountLoginTarget,
    pub revision: u64,
    pub accepted_input_revision: Option<u64>,
    pub phase: LoginPhase,
    pub input_mode: InputMode,
    pub events: Vec<LoginPhase>,
    pub account_name: Option<String>,
    pub steamcmd_root: Option<String>,
    pub error: Option<String>,
}

struct Session {
    snapshot: SteamWorkshopAccountLoginSnapshot,
    input: mpsc::Sender<SteamWorkshopAccountInput>,
    native_input_revision: Option<u64>,
    task: Option<tokio::task::JoinHandle<()>>,
    storage_root: std::path::PathBuf,
    server_download: Option<(String, std::path::PathBuf)>,
    server_download_revision: u64,
}

pub(crate) struct ServerAccountAuthorization {
    session_id: String,
    revision: u64,
}

#[derive(Default)]
pub(crate) struct SteamWorkshopAccountSessions {
    current: Mutex<Option<Session>>,
}

impl SteamWorkshopAccountSessions {
    pub(crate) fn begin(
        &self,
        target: &SteamAccountLoginTarget,
        session_id: &str,
        input: mpsc::Sender<SteamWorkshopAccountInput>,
        storage_root: &std::path::Path,
    ) -> Result<SteamWorkshopAccountLoginSnapshot, String> {
        let mut current = self.current.lock().map_err(|_| lock_error())?;
        if let Some(session) = current.as_ref()
            && (!terminal(session.snapshot.phase) || session.snapshot.session_id == session_id)
        {
            return Err(String::from(
                "A SteamCMD login session is already active or this ID has been used.",
            ));
        }
        let snapshot = SteamWorkshopAccountLoginSnapshot {
            session_id: session_id.into(),
            target: target.clone(),
            revision: 1,
            accepted_input_revision: None,
            phase: LoginPhase::Ready,
            input_mode: InputMode::Command,
            events: vec![LoginPhase::Ready],
            account_name: None,
            steamcmd_root: None,
            error: None,
        };
        *current = Some(Session {
            snapshot: snapshot.clone(),
            input,
            native_input_revision: None,
            task: None,
            storage_root: storage_root.to_owned(),
            server_download: None,
            server_download_revision: 0,
        });
        Ok(snapshot)
    }

    pub(crate) fn snapshot(
        &self,
        target: &SteamAccountLoginTarget,
        session_id: &str,
    ) -> Result<SteamWorkshopAccountLoginSnapshot, String> {
        let current = self.current.lock().map_err(|_| lock_error())?;
        Ok(bound_session(current.as_ref(), target, session_id)?
            .snapshot
            .clone())
    }

    pub(crate) fn attach_task(
        &self,
        session_id: &str,
        task: tokio::task::JoinHandle<()>,
    ) -> Result<(), String> {
        let mut current = self.current.lock().map_err(|_| lock_error())?;
        let session = current
            .as_mut()
            .filter(|session| session.snapshot.session_id == session_id)
            .ok_or_else(missing_session)?;
        session.task = Some(task);
        Ok(())
    }

    pub(crate) fn submit(
        &self,
        target: &SteamAccountLoginTarget,
        session_id: &str,
        revision: u64,
        input: String,
    ) -> Result<SteamWorkshopAccountLoginSnapshot, String> {
        let mut current = self.current.lock().map_err(|_| lock_error())?;
        bound_session(current.as_ref(), target, session_id)?;
        let session = current.as_mut().ok_or_else(missing_session)?;
        if session.snapshot.revision != revision
            || session.snapshot.input_mode == InputMode::Disabled
        {
            return Err(input_error(
                "steam_account_prompt_changed",
                "The SteamCMD login prompt has changed.",
            ));
        }
        let invalid = match session.snapshot.phase {
            LoginPhase::GuardCode => {
                input.is_empty()
                    || input.len() != 5
                    || !input.bytes().all(|byte| byte.is_ascii_alphanumeric())
            }
            LoginPhase::EmailCode | LoginPhase::SteamGuard => {
                input.is_empty()
                    || input.len() > 32
                    || !input.bytes().all(|byte| byte.is_ascii_alphanumeric())
            }
            _ => input.is_empty() || input.len() > 1024 || input.chars().any(char::is_control),
        };
        if invalid {
            return Err(input_error(
                "steam_account_input_invalid",
                "The response does not match the current SteamCMD input challenge.",
            ));
        }
        let input = if session.snapshot.input_mode == InputMode::Command {
            let account = account_from_command(&input)?;
            app_steamcmd::normalize_workshop_account_name(account)
                .map_err(|error| crate::commands::steamcmd_error_message(&error))?
        } else {
            input
        };
        let challenge_revision = if session.snapshot.input_mode == InputMode::Secret {
            Some(session.native_input_revision.ok_or_else(|| {
                input_error(
                    "steam_account_prompt_changed",
                    "The native SteamCMD input challenge is unavailable.",
                )
            })?)
        } else {
            None
        };
        session
            .input
            .try_send(SteamWorkshopAccountInput {
                challenge_revision,
                value: input,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => input_error(
                    "steam_account_input_pending",
                    "SteamCMD already has a response waiting for this prompt.",
                ),
                mpsc::error::TrySendError::Closed(_) => input_error(
                    "steam_account_input_closed",
                    "The SteamCMD input session is closed.",
                ),
            })?;
        // A receipt contains only the prompt identity. It lets the UI recover
        // a lost IPC response without retaining or submitting the secret again.
        session.snapshot.accepted_input_revision = Some(revision);
        session.native_input_revision = None;
        set_phase(
            &mut session.snapshot,
            LoginPhase::InputSubmitted,
            InputMode::Disabled,
        );
        Ok(session.snapshot.clone())
    }

    /// A selected username may use only the verified cache for this operation's
    /// target and current storage context. The installer verifies it again natively.
    pub(crate) fn authorize_account(
        &self,
        target: &SteamAccountLoginTarget,
        account: &str,
        steamcmd_root: &std::path::Path,
        storage_root: &std::path::Path,
    ) -> Result<ServerAccountAuthorization, String> {
        let current = self.current.lock().map_err(|_| lock_error())?;
        let valid = current.as_ref().is_some_and(|session| {
            let snapshot = &session.snapshot;
            snapshot.target == *target
                && session.storage_root == storage_root
                && snapshot.phase == LoginPhase::Authenticated
                && snapshot.account_name.as_deref() == Some(account)
                && snapshot.steamcmd_root.as_deref().map(std::path::Path::new)
                    == Some(steamcmd_root)
        });
        if !valid {
            return Err(missing_session());
        }
        let session = current.as_ref().ok_or_else(missing_session)?;
        Ok(ServerAccountAuthorization {
            session_id: session.snapshot.session_id.clone(),
            revision: session.server_download_revision,
        })
    }

    /// A successful explicit server acquisition grants a bounded fallback to
    /// that exact program, never to another instance's independent installation.
    pub(crate) fn record_server_download(
        &self,
        target: &SteamAccountLoginTarget,
        account: &str,
        authorization: &ServerAccountAuthorization,
        module_id: &str,
        program_root: std::path::PathBuf,
    ) {
        if let Ok(mut current) = self.current.lock()
            && let Some(session) = current.as_mut().filter(|session| {
                session.snapshot.target == *target
                    && session.snapshot.session_id == authorization.session_id
                    && session.server_download_revision == authorization.revision
                    && session.snapshot.phase == LoginPhase::Authenticated
                    && session.snapshot.account_name.as_deref() == Some(account)
            })
        {
            session.server_download = Some((module_id.to_owned(), program_root));
        }
    }

    pub(crate) fn clear_server_download(
        &self,
        target: &SteamAccountLoginTarget,
    ) -> Result<(), String> {
        let mut current = self.current.lock().map_err(|_| lock_error())?;
        if let Some(session) = current
            .as_mut()
            .filter(|session| session.snapshot.target == *target)
        {
            session.server_download = None;
            session.server_download_revision += 1;
        }
        Ok(())
    }

    pub(crate) fn server_download_account(
        &self,
        targets: &[SteamAccountLoginTarget],
        module_id: &str,
        program_root: &std::path::Path,
        steamcmd_root: &std::path::Path,
        storage_root: &std::path::Path,
    ) -> Result<Option<String>, String> {
        let current = self.current.lock().map_err(|_| lock_error())?;
        Ok(current
            .as_ref()
            .filter(|session| {
                targets.contains(&session.snapshot.target)
                    && session.snapshot.phase == LoginPhase::Authenticated
                    && session.storage_root == storage_root
                    && session
                        .snapshot
                        .steamcmd_root
                        .as_deref()
                        .map(std::path::Path::new)
                        == Some(steamcmd_root)
                    && session
                        .server_download
                        .as_ref()
                        .is_some_and(|(module, root)| module == module_id && root == program_root)
            })
            .and_then(|session| session.snapshot.account_name.clone()))
    }

    pub(crate) fn progress(&self, session_id: &str, progress: SteamWorkshopAccountProgress) {
        if let Ok(mut current) = self.current.lock()
            && let Some(session) = current.as_mut().filter(|session| {
                session.snapshot.session_id == session_id && !terminal(session.snapshot.phase)
            })
        {
            let (phase, mode) = match progress.phase {
                SteamWorkshopAccountPhase::Queued => (LoginPhase::Queued, InputMode::Disabled),
                SteamWorkshopAccountPhase::PreparingSteamcmd => {
                    (LoginPhase::PreparingSteamcmd, InputMode::Disabled)
                }
                SteamWorkshopAccountPhase::Connecting => {
                    (LoginPhase::Connecting, InputMode::Disabled)
                }
                SteamWorkshopAccountPhase::Authenticating => {
                    (LoginPhase::Authenticating, InputMode::Disabled)
                }
                SteamWorkshopAccountPhase::VerifyingCache => {
                    (LoginPhase::VerifyingCache, InputMode::Disabled)
                }
                SteamWorkshopAccountPhase::GuardCode => (LoginPhase::GuardCode, InputMode::Secret),
                SteamWorkshopAccountPhase::EmailCode => (LoginPhase::EmailCode, InputMode::Secret),
                SteamWorkshopAccountPhase::Password => (LoginPhase::Password, InputMode::Secret),
                SteamWorkshopAccountPhase::SteamGuard => {
                    (LoginPhase::SteamGuard, InputMode::Secret)
                }
                SteamWorkshopAccountPhase::MobileConfirmation => {
                    (LoginPhase::MobileConfirmation, InputMode::Disabled)
                }
                // Success is published only with the verified account result.
                SteamWorkshopAccountPhase::Authenticated => {
                    (LoginPhase::VerifyingCache, InputMode::Disabled)
                }
            };
            session.native_input_revision = if mode == InputMode::Secret {
                progress.challenge_revision
            } else {
                None
            };
            set_phase(&mut session.snapshot, phase, mode);
        }
    }

    pub(crate) fn finish(
        &self,
        session_id: &str,
        result: Result<SteamWorkshopAccountAuthenticationResult, String>,
    ) {
        if let Ok(mut current) = self.current.lock()
            && let Some(session) = current
                .as_mut()
                .filter(|session| session.snapshot.session_id == session_id)
        {
            session.native_input_revision = None;
            match result {
                Ok(result) => {
                    session.snapshot.account_name = Some(result.account_name);
                    session.snapshot.steamcmd_root = Some(result.steamcmd_root);
                    set_phase(
                        &mut session.snapshot,
                        LoginPhase::Authenticated,
                        InputMode::Disabled,
                    );
                }
                Err(error) => {
                    let phase = if error == "installation_cancelled" {
                        LoginPhase::Cancelled
                    } else {
                        LoginPhase::Failed
                    };
                    session.snapshot.error = Some(error);
                    set_phase(&mut session.snapshot, phase, InputMode::Disabled);
                }
            }
        }
    }
}

fn set_phase(snapshot: &mut SteamWorkshopAccountLoginSnapshot, phase: LoginPhase, mode: InputMode) {
    snapshot.revision += 1;
    snapshot.phase = phase;
    snapshot.input_mode = mode;
    if snapshot.events.last() != Some(&phase) {
        if snapshot.events.len() >= 32 {
            snapshot.events.remove(0);
        }
        snapshot.events.push(phase);
    }
}
fn terminal(phase: LoginPhase) -> bool {
    matches!(
        phase,
        LoginPhase::Authenticated | LoginPhase::Failed | LoginPhase::Cancelled
    )
}
fn lock_error() -> String {
    input_error(
        "steam_account_backend_unavailable",
        "SteamCMD login session lock is unavailable.",
    )
}
fn missing_session() -> String {
    input_error(
        "steam_account_session_unavailable",
        "The SteamCMD login session is no longer available.",
    )
}

fn input_error(code: &'static str, message: &'static str) -> String {
    serde_json::json!({"code": code, "message": message}).to_string()
}

fn account_from_command(input: &str) -> Result<&str, String> {
    let mut words = input.split_ascii_whitespace();
    let first = words.next();
    let second = words.next();
    match (first, second, words.next()) {
        (Some(command), Some(account), None) if command.eq_ignore_ascii_case("login") => {
            Ok(account)
        }
        (Some(account), None, None) if !account.eq_ignore_ascii_case("login") => Ok(account),
        _ => Err(input_error(
            "steam_account_command_invalid",
            "Enter an account name or login ACCOUNT, without a password argument.",
        )),
    }
}
fn bound_session<'a>(
    session: Option<&'a Session>,
    target: &SteamAccountLoginTarget,
    session_id: &str,
) -> Result<&'a Session, String> {
    session
        .filter(|session| {
            session.snapshot.session_id == session_id && session.snapshot.target == *target
        })
        .ok_or_else(missing_session)
}

#[cfg(test)]
#[path = "steam_workshop_account_sessions_tests.rs"]
mod tests;
