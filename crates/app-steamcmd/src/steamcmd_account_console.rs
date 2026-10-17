//! Owns the protected native console, private input, and process cleanup.
use super::super::{AuthenticationLease, workshop_login_line};
use super::*;
use app_runtime::EphemeralConsole;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

#[cfg(test)]
use super::super::super::SteamCmdLifecycleGuard;
use super::super::super::{InstallCancellation, InstallDeadline, SteamCmdError, SteamCmdStatus};

const POLL_INTERVAL: Duration = Duration::from_millis(25);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct LoginLaunch {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub directory: PathBuf,
}

struct CancelOnDrop(Option<InstallCancellation>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(cancellation) = &self.0 {
            cancellation.cancel();
        }
    }
}

pub(crate) async fn run(
    steamcmd: &SteamCmdStatus,
    account_name: &str,
    lease: AuthenticationLease,
    cancellation: &InstallCancellation,
    input: &mut mpsc::Receiver<SteamWorkshopAccountInput>,
    deadline: InstallDeadline,
    callback: &mut impl FnMut(SteamWorkshopAccountProgress),
) -> Result<AuthenticationLease, SteamCmdError> {
    let mut arguments = Vec::new();
    if let Some(source) = lease.update_source() {
        arguments.extend([String::from("-overridepackageurl"), source.to_owned()]);
    }
    arguments.extend([
        String::from("+@NoPromptForPassword"),
        String::from("0"),
        String::from("+@ShutdownOnFailedCommand"),
        String::from("1"),
    ]);
    let launch = LoginLaunch {
        executable: PathBuf::from(&steamcmd.executable_path),
        directory: PathBuf::from(&steamcmd.root),
        arguments,
    };
    run_launch_owned(
        launch,
        lease,
        Some(account_name.to_owned()),
        cancellation,
        input,
        deadline,
        callback,
    )
    .await
}

#[cfg(test)]
pub(super) async fn run_launch(
    launch: LoginLaunch,
    guard: SteamCmdLifecycleGuard,
    cancellation: &InstallCancellation,
    input: &mut mpsc::Receiver<SteamWorkshopAccountInput>,
    deadline: InstallDeadline,
    callback: &mut impl FnMut(SteamWorkshopAccountProgress),
) -> Result<SteamCmdLifecycleGuard, SteamCmdError> {
    run_launch_owned(
        launch,
        AuthenticationLease {
            bridge: None,
            guard,
        },
        None,
        cancellation,
        input,
        deadline,
        callback,
    )
    .await
    .map(|lease| lease.guard)
}

async fn run_launch_owned(
    launch: LoginLaunch,
    lease: AuthenticationLease,
    account_name: Option<String>,
    cancellation: &InstallCancellation,
    input: &mut mpsc::Receiver<SteamWorkshopAccountInput>,
    deadline: InstallDeadline,
    callback: &mut impl FnMut(SteamWorkshopAccountProgress),
) -> Result<AuthenticationLease, SteamCmdError> {
    deadline.check_cancelled()?;
    let state = Arc::new(Mutex::new(
        account_name
            .map(PromptState::for_account)
            .unwrap_or_default(),
    ));
    let (responses, receiver) = mpsc::channel(1);
    let worker_state = state.clone();
    let worker_cancellation = cancellation.clone();
    // On future drop, the worker still owns both the console and SteamCMD
    // lock and cooperatively kills/reaps before either is released.
    let mut cancel_on_drop = CancelOnDrop(Some(cancellation.clone()));
    let mut worker = tokio::task::spawn_blocking(move || {
        run_owned_console(
            launch,
            lease,
            &worker_cancellation,
            receiver,
            &worker_state,
            deadline,
        )
    });
    let mut observed_revision = 0;
    let mut forwarded_revision = None;
    let result = loop {
        let snapshot = state
            .lock()
            .ok()
            .map(|state| (state.revision, state.phase, state.pending));
        let Some(snapshot) = snapshot else {
            cancellation.cancel();
            break worker
                .await
                .map_err(|_| authentication_failed("The protected login worker failed."))?;
        };
        if snapshot.0 != observed_revision {
            callback(SteamWorkshopAccountProgress {
                phase: snapshot.1,
                challenge_revision: snapshot.2.map(|challenge| challenge.revision),
            });
            observed_revision = snapshot.0;
        }
        let challenge = snapshot
            .2
            .filter(|challenge| Some(challenge.revision) != forwarded_revision);
        tokio::select! {
            result = &mut worker => {
                break result.map_err(|_| authentication_failed("The protected login worker failed."))?;
            }
            response = input.recv(), if challenge.is_some() => {
                let Some(response) = response else {
                    cancellation.cancel();
                    break worker.await.map_err(|_| authentication_failed("The protected login worker failed."))?;
                };
                let revision = challenge.map(|challenge| challenge.revision);
                if response.challenge_revision != revision {
                    // Keep the identity assigned when the UI accepted this
                    // input; never relabel it for a newer native challenge.
                    continue;
                }
                forwarded_revision = revision;
                if responses.try_send(response).is_err() {
                    cancellation.cancel();
                }
            }
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }
    };
    // Do not cancel a successful session's subsequent cached-login verifier.
    cancel_on_drop.0 = None;
    result
}

fn run_owned_console(
    launch: LoginLaunch,
    lease: AuthenticationLease,
    cancellation: &InstallCancellation,
    mut input: mpsc::Receiver<SteamWorkshopAccountInput>,
    state: &Arc<Mutex<PromptState>>,
    deadline: InstallDeadline,
) -> Result<AuthenticationLease, SteamCmdError> {
    if cancellation.is_cancelled() || tokio::time::Instant::now() >= deadline.expires_at() {
        return Err(stop_error(cancellation, deadline));
    }
    let account_name = state
        .lock()
        .map_err(|_| authentication_failed("The login prompt state became unavailable."))?
        .login
        .as_ref()
        .map(|login| login.account_name.clone());
    let mut console = EphemeralConsole::spawn(
        &launch.executable,
        &launch.arguments,
        &launch.directory,
        PromptSink(state.clone()),
    )
    .map_err(|_| authentication_failed("SteamCMD could not start its protected login console."))?;
    let mut quit_sent = false;
    // This secret is owned only by the console worker, never shared with
    // the output sink or serialized account session state.
    let mut prepared_password: Option<String> = None;
    let mut restart_code: Option<String> = None;
    let mut restarted = false;
    let outcome = loop {
        if cancellation.is_cancelled() || tokio::time::Instant::now() >= deadline.expires_at() {
            break Err(stop_error(cancellation, deadline));
        }
        let (
            login_failed,
            login_verified,
            production_login,
            mobile_after_code,
            password_pending,
            discard_password,
            start_login,
        ) = match state.lock() {
            Ok(mut state) => state
                .login
                .as_mut()
                .map(|login| {
                    (
                        login.failed,
                        login.verified,
                        true,
                        login.mobile_after_code,
                        std::mem::take(&mut login.prepared_password_pending),
                        login.discard_prepared_password,
                        std::mem::take(&mut login.start_login_pending),
                    )
                })
                .unwrap_or((false, false, false, false, false, false, false)),
            Err(_) => {
                break Err(authentication_failed(
                    "The login prompt state became unavailable.",
                ));
            }
        };
        if login_failed {
            break Err(authentication_failed(if mobile_after_code {
                "Steam did not complete code verification for this attempt. Retry with a current Steam Guard code."
            } else {
                "SteamCMD returned to its command prompt before the selected account was authenticated."
            }));
        }
        if password_pending {
            let Some(password) = prepared_password.as_deref() else {
                break Err(authentication_failed(
                    "The prepared login password is no longer available.",
                ));
            };
            if console.write_line(password).is_err() {
                break Err(authentication_failed(
                    "SteamCMD could not receive the login response.",
                ));
            }
        } else if discard_password {
            // A cached login can skip its password prompt altogether.
            // Discard the unused value as soon as native auth proves this.
            // This does not verify the supplied password: the selected native
            // identity and subsequent fresh cache login authenticate the session.
            prepared_password = None;
        }
        if start_login {
            let Some(account) = account_name.as_deref() else {
                break Err(authentication_failed(
                    "The selected login account is unavailable.",
                ));
            };
            let line = match restart_code.take() {
                Some(code) => format!("login {account} \"\" {code}"),
                None if !restarted => workshop_login_line(Some(account)),
                None => {
                    break Err(authentication_failed(
                        "The replacement login challenge is no longer available.",
                    ));
                }
            };
            if console.write_line(&line).is_err() {
                break Err(authentication_failed(
                    "SteamCMD could not receive the login response.",
                ));
            }
        }
        if login_verified && !quit_sent {
            if console.write_line("quit").is_err() {
                break Err(authentication_failed(
                    "SteamCMD could not close its verified login session.",
                ));
            }
            quit_sent = true;
        }
        match console.try_wait() {
            Ok(Some(0)) if !production_login || login_verified => break Ok(()),
            Ok(Some(0)) => {
                break Err(authentication_failed(
                    "SteamCMD exited before the selected account identity was verified.",
                ));
            }
            Ok(Some(_)) => {
                break Err(authentication_failed(
                    "SteamCMD exited before account authentication completed.",
                ));
            }
            Err(_) => {
                break Err(authentication_failed(
                    "The protected SteamCMD login console became unavailable.",
                ));
            }
            Ok(None) => {}
        }
        match input.try_recv() {
            Ok(response) => {
                let challenge = match state.lock() {
                    Ok(mut state) => response
                        .challenge_revision
                        .and_then(|revision| state.consume(revision)),
                    Err(_) => {
                        break Err(authentication_failed(
                            "The login prompt state became unavailable.",
                        ));
                    }
                };
                if let Some(challenge) = challenge {
                    if !valid_response(challenge.phase, &response.value) {
                        break Err(authentication_failed(
                            "The login response must be one valid password or Steam Guard code.",
                        ));
                    }
                    match challenge.action {
                        ChallengeAction::RestartWithDeviceCode => {
                            let Some(account) = account_name.as_ref().filter(|_| !restarted) else {
                                break Err(authentication_failed(
                                    "Steam could not complete code verification for this attempt.",
                                ));
                            };
                            if console.terminate_and_wait(CLEANUP_TIMEOUT).is_err() {
                                break Err(SteamCmdError::InstallProcessCleanupFailed {
                                    operation: deadline.operation().to_owned(),
                                    detail: String::from(
                                        "The protected SteamCMD login process could not be fully reaped.",
                                    ),
                                });
                            }
                            if cancellation.is_cancelled()
                                || tokio::time::Instant::now() >= deadline.expires_at()
                            {
                                break Err(stop_error(cancellation, deadline));
                            }
                            match state.lock() {
                                Ok(mut state) => state.restart_with_device_code(
                                    account.clone(),
                                    prepared_password.is_some(),
                                ),
                                Err(_) => {
                                    break Err(authentication_failed(
                                        "The login prompt state became unavailable.",
                                    ));
                                }
                            }
                            restart_code = Some(response.value);
                            console = match EphemeralConsole::spawn(
                                &launch.executable,
                                &launch.arguments,
                                &launch.directory,
                                PromptSink(state.clone()),
                            ) {
                                Ok(console) => console,
                                Err(_) => {
                                    break Err(authentication_failed(
                                        "SteamCMD could not start its protected login console.",
                                    ));
                                }
                            };
                            restarted = true;
                        }
                        ChallengeAction::NativePassword => {
                            if console.write_line(&response.value).is_err() {
                                break Err(authentication_failed(
                                    "SteamCMD could not receive the login response.",
                                ));
                            }
                            if account_name.is_some() {
                                prepared_password = Some(response.value);
                            }
                        }
                        ChallengeAction::NativeCode => {
                            if console.write_line(&response.value).is_err() {
                                break Err(authentication_failed(
                                    "SteamCMD could not receive the login response.",
                                ));
                            }
                        }
                    }
                }
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                break Err(stop_error(cancellation, deadline));
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    drop(prepared_password);
    drop(restart_code);
    let cleanup = console.terminate_and_wait(CLEANUP_TIMEOUT);
    // Drop performs the Job close fallback before the lifecycle lock can
    // release, including the explicit cleanup failure path.
    drop(console);
    cleanup.map_err(|_| SteamCmdError::InstallProcessCleanupFailed {
        operation: deadline.operation().to_owned(),
        detail: String::from("The protected SteamCMD login process could not be fully reaped."),
    })?;
    outcome?;
    Ok(lease)
}

fn stop_error(cancellation: &InstallCancellation, deadline: InstallDeadline) -> SteamCmdError {
    if cancellation.is_cancelled() {
        SteamCmdError::InstallCancelled {
            operation: deadline.operation().to_owned(),
        }
    } else {
        SteamCmdError::OperationTimedOut {
            operation: deadline.operation(),
            timeout_seconds: deadline.timeout().as_secs(),
        }
    }
}

fn authentication_failed(reason: &str) -> SteamCmdError {
    SteamCmdError::SteamAccountAuthenticationFailed {
        output_excerpt: reason.to_owned(),
    }
}
