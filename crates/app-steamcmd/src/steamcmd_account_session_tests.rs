use super::*;

#[test]
fn partial_and_repeated_prompt_snapshots_open_one_challenge() {
    let mut state = PromptState::default();
    state.observe_snapshot("Pass");
    assert!(state.pending.is_none());
    state.observe_snapshot("Password: ");
    let first = state.pending.unwrap();
    assert_eq!(first.phase, Phase::Password);
    state.observe_snapshot("Password:");
    assert_eq!(state.pending.unwrap().revision, first.revision);
    assert_eq!(
        state
            .consume(first.revision)
            .map(|challenge| challenge.phase),
        Some(Phase::Password)
    );
    state.observe_snapshot("Password:");
    state.observe_snapshot("Password: fixture-password");
    assert!(state.pending.is_none());
    assert_eq!(state.consume(first.revision), None);
    assert_eq!(state.phase, Phase::Connecting);
}

#[test]
fn a_native_failure_rearms_the_same_prompt_but_rejects_stale_answers() {
    for (prompt, failure, phase) in [
        (
            "password:",
            "ERROR! Failed to log on: Invalid Password",
            Phase::Password,
        ),
        (
            "password:",
            "password: FAILED (Invalid Password)",
            Phase::Password,
        ),
        (
            "Steam Guard code:",
            "Login Failure: Invalid Login Auth Code",
            Phase::SteamGuard,
        ),
        (
            "Two-factor code:",
            "FAILED (Two-factor code mismatch)",
            Phase::GuardCode,
        ),
    ] {
        let mut state = PromptState::default();
        state.observe_snapshot(prompt);
        let first = state.pending.unwrap();
        assert_eq!(
            state
                .consume(first.revision)
                .map(|challenge| challenge.phase),
            Some(phase)
        );
        state.observe_snapshot(failure);
        state.observe_snapshot(prompt);
        let retry = state.pending.unwrap();
        assert!(retry.revision > first.revision);
        assert_eq!(state.consume(first.revision), None);
        assert_eq!(state.pending.unwrap().revision, retry.revision);
        assert_eq!(
            state
                .consume(retry.revision)
                .map(|challenge| challenge.phase),
            Some(phase)
        );
    }
}

#[test]
fn mobile_confirmation_never_opens_a_secret_input_challenge() {
    let mut state = PromptState::default();
    state.observe_snapshot("Please confirm the login in the Steam Mobile app on your phone.");
    assert_eq!(state.phase, Phase::MobileConfirmation);
    assert!(state.pending.is_none());
    let revision = state.revision;
    state.observe_snapshot("Please confirm the login in the Steam Mobile app on your phone.");
    state.observe_snapshot("Waiting for confirmation...");
    assert_eq!(state.revision, revision);
}

#[test]
fn only_exact_allowlisted_native_prompts_reach_the_shared_state() {
    let state = Arc::new(Mutex::new(PromptState::default()));
    let mut sink = PromptSink(state.clone());
    for row in [
        "Loading Steam API...OK",
        "fixture-password",
        "ABCDE",
        "Password: fixture-password",
        "Steam Guard code: ABCDE",
        "synthetic-response says password:",
        "unknown private output",
    ] {
        sink.write_all(row.as_bytes()).unwrap();
        sink.flush().unwrap();
    }
    assert_eq!(state.lock().unwrap().revision, 0);
    sink.write_all(b"Steam Guard code:\n").unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.phase, Phase::SteamGuard);
    assert_eq!(state.revision, 1);
}

#[test]
fn credential_responses_are_single_lines_and_guard_codes_are_bounded() {
    assert!(!valid_response(Phase::GuardCode, ""));
    assert!(valid_response(Phase::GuardCode, "7Q9XW"));
    for response in ["1234", "123456", "has space", "中文", "a\nBCD", "ab;cd"] {
        assert!(!valid_response(Phase::GuardCode, response));
    }
    assert!(valid_response(
        Phase::Password,
        "synthetic unicode 玩家 password"
    ));
    assert!(valid_response(Phase::SteamGuard, "A1B2C"));
    assert!(valid_response(Phase::EmailCode, "MAIL7"));
    for response in ["", "line\nquit", "line\r", "line\0", "line\t"] {
        assert!(!valid_response(Phase::Password, response));
    }
    assert!(!valid_response(Phase::Password, &"a".repeat(1025)));
    for response in ["has space", "中文", "abc;quit"] {
        assert!(!valid_response(Phase::SteamGuard, response));
    }
    assert!(!valid_response(Phase::SteamGuard, &"a".repeat(33)));
    assert!(!valid_response(Phase::Connecting, "input"));
}

#[test]
fn production_login_starts_at_the_native_command_prompt_and_only_requests_real_credentials() {
    let mut state = PromptState::for_account("selected_account".into());
    state.observe_snapshot("Steam");
    assert!(state.pending.is_none());
    state.observe_snapshot("Steam>");
    assert_eq!(state.phase, Phase::Connecting);
    assert!(state.pending.is_none());
    assert!(state.login.as_ref().unwrap().start_login_pending);
    state.observe_snapshot("Steam> ");
    assert!(!state.login.as_ref().unwrap().failed);
    state.observe_snapshot("Password:");
    let password = state.pending.unwrap();
    assert_eq!(password.action, ChallengeAction::NativePassword);
    assert_eq!(
        state.consume(password.revision).unwrap().phase,
        Phase::Password
    );
    assert!(state.pending.is_none());
    assert_eq!(state.phase, Phase::Connecting);
    state.observe_snapshot("Two-factor code:");
    let code = state.pending.unwrap();
    assert_eq!(code.phase, Phase::GuardCode);
    assert_eq!(code.action, ChallengeAction::NativeCode);
    assert!(code.revision > password.revision);
    assert_eq!(state.consume(password.revision), None);
}

fn native_login() -> PromptState {
    let mut state = PromptState::for_account("selected_account".into());
    state.observe_snapshot("Steam>");
    state.login.as_mut().unwrap().start_login_pending = false;
    state
}

fn password_restart() -> PromptState {
    let mut state = native_login();
    state.restart_with_device_code("selected_account".into(), true);
    state.observe_snapshot("Steam>");
    state.login.as_mut().unwrap().start_login_pending = false;
    state
}

#[test]
fn retained_password_survives_invalid_cache_and_is_not_reused_for_a_retry() {
    let mut state = password_restart();
    state.observe_snapshot("Logging in using cached credentials.");
    state.observe_snapshot("Logging in user 'selected_account' to Steam Public...");
    state.observe_snapshot("Warning: invalid cached credentials.");
    assert!(state.login.as_ref().unwrap().prepared_password_available);
    state.observe_snapshot("Password:");
    assert!(state.pending.is_none());
    let login = state.login.as_mut().unwrap();
    assert!(std::mem::take(&mut login.prepared_password_pending));
    assert!(!login.prepared_password_available);
    state.observe_snapshot("Password:");
    assert!(!state.login.as_ref().unwrap().prepared_password_pending);
    state.observe_snapshot("Login Failure: Invalid Password");
    assert!(state.login.as_ref().unwrap().discard_prepared_password);
    state.observe_snapshot("Password:");
    assert_eq!(
        state.pending.unwrap().action,
        ChallengeAction::NativePassword
    );
    assert!(!state.login.as_ref().unwrap().prepared_password_pending);
}

#[test]
fn verified_cached_login_discards_the_unused_retained_password() {
    let mut state = password_restart();
    state.observe_snapshot("Logging in user 'selected_account' to Steam Public...OK");
    assert!(state.login.as_ref().unwrap().prepared_password_available);
    assert_eq!(state.phase, Phase::Authenticating);
    state.observe_snapshot("Waiting for client config...");
    assert!(!state.login.as_ref().unwrap().prepared_password_available);
    assert!(state.login.as_ref().unwrap().discard_prepared_password);
    assert!(!state.login.as_ref().unwrap().prepared_password_pending);
    assert!(state.pending.is_none());
    state.observe_snapshot("Waiting for user info...OK");
    assert!(state.login.as_ref().unwrap().verified);
}

#[test]
fn email_description_does_not_open_input_until_the_real_prompt_and_never_crosses_attempts() {
    let mut state = native_login();
    state.observe_snapshot(
        "Please check your email for the message from Steam, and enter the Steam Guard",
    );
    assert!(state.email_code_expected);
    assert!(state.pending.is_none());
    state.observe_snapshot("code from that message.");
    assert!(state.pending.is_none());
    state.observe_snapshot("Steam Guard code:");
    let email = state.pending.unwrap();
    assert_eq!(email.phase, Phase::EmailCode);
    assert_eq!(email.action, ChallengeAction::NativeCode);
    state.consume(email.revision).unwrap();
    state.observe_snapshot("Login Failure: Invalid Login Auth Code");
    state.observe_snapshot("Steam Guard code:");
    assert_eq!(state.pending.unwrap().phase, Phase::EmailCode);
    state.restart_with_device_code("selected_account".into(), true);
    assert!(!state.email_code_expected);
    state.observe_snapshot("Steam Guard code:");
    assert_eq!(state.pending.unwrap().phase, Phase::SteamGuard);
    state.email_code_expected = true;
    state.observe_snapshot("Two-factor code:");
    assert_eq!(state.pending.unwrap().phase, Phase::GuardCode);
    assert!(!state.email_code_expected);
}

#[test]
fn accepted_mobile_replacement_freezes_old_proof_and_preserves_monotonic_challenges() {
    let mut state = native_login();
    state.observe_snapshot("Please confirm the login in the Steam Mobile app on your phone.");
    let first = state.pending.unwrap();
    assert_eq!(first.action, ChallengeAction::RestartWithDeviceCode);
    state.consume(first.revision).unwrap();
    assert!(state.frozen);
    state.observe_snapshot("Logging in user 'selected_account' to Steam Public...OK");
    state.observe_snapshot("Waiting for user info...OK");
    assert!(!state.login.as_ref().unwrap().verified);
    state.restart_with_device_code("selected_account".into(), true);
    assert!(!state.frozen);
    assert!(state.revision > first.revision);
    assert_eq!(state.consume(first.revision), None);
    assert!(
        state
            .login
            .as_ref()
            .unwrap()
            .evidence
            .authenticated_account_name()
            .is_none()
    );
    state.observe_snapshot("Steam>");
    state.observe_snapshot("Steam Guard code:");
    let next = state.pending.unwrap();
    assert!(next.revision > first.revision);
    assert_eq!(state.consume(first.revision), None);
    assert_eq!(state.pending.unwrap().revision, next.revision);
}

#[test]
fn production_login_must_verify_identity_before_returning_to_the_command_prompt() {
    let mut redraw = PromptState::for_account("selected_account".into());
    redraw.login.as_mut().unwrap().started = true;
    redraw.observe_snapshot("Steam>");
    assert!(!redraw.login.as_ref().unwrap().failed);
    redraw.observe_snapshot("Steam>login selected_account");
    redraw.observe_snapshot("Steam>");
    assert!(redraw.login.as_ref().unwrap().failed);
    for account in ["selected_account", "other_account"] {
        let mut state = PromptState::for_account("selected_account".into());
        state.login.as_mut().unwrap().started = true;
        state.observe_snapshot(&format!("Logging in user '{account}' to Steam Public...OK"));
        state.observe_snapshot("Waiting for user info...OK");
        state.observe_snapshot("Steam>");
        let login = state.login.as_ref().unwrap();
        assert_eq!(login.verified, account == "selected_account");
        assert_eq!(login.failed, account != "selected_account");
        assert!(state.pending.is_none());
    }
    let mut state = PromptState::for_account("selected_account".into());
    state.login.as_mut().unwrap().started = true;
    state.observe_snapshot("Login Failure: Invalid Password");
    state.observe_snapshot("Steam>");
    assert!(state.login.as_ref().unwrap().failed);
    assert!(state.pending.is_none());
}

#[test]
fn selecting_a_guard_code_cannot_silently_fall_back_to_phone_confirmation() {
    for selected in [true, false] {
        let mut state = PromptState::for_account("selected_account".into());
        let login = state.login.as_mut().unwrap();
        login.started = true;
        login.guard_code_selected = selected;
        state.observe_snapshot("Please confirm the login in the Steam Mobile app on your phone.");
        assert_eq!(state.login.as_ref().unwrap().failed, selected);
        assert_eq!(state.login.as_ref().unwrap().mobile_after_code, selected);
        assert_eq!(state.phase == Phase::GuardCode, !selected);
        assert_eq!(state.pending.is_none(), selected);
    }
}

#[test]
fn mobile_approval_before_a_code_is_accepted_closes_the_code_challenge() {
    let mut state = native_login();
    state.observe_snapshot("Please confirm the login in the Steam Mobile app on your phone.");
    assert_eq!(state.phase, Phase::GuardCode);
    let code = state.pending.unwrap();
    state.observe_snapshot("Logging in user 'selected_account' to Steam Public...");
    assert_eq!(state.phase, Phase::GuardCode);
    state.observe_snapshot("Waiting for client config...OK");
    assert_eq!(state.phase, Phase::Authenticating);
    assert!(state.pending.is_none());
    assert_eq!(state.consume(code.revision), None);
    assert!(!state.login.as_ref().unwrap().prepared_password_available);
    state.observe_snapshot("Waiting for user info...OK");
    assert!(state.login.as_ref().unwrap().verified);
}

#[cfg(windows)]
mod windows {
    use super::super::native::{LoginLaunch, run_launch};
    use super::*;
    use crate::{InstallCancellation, InstallDeadline, SteamCmdError, SteamCmdLifecycleGuard};
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::sync::mpsc;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = crate::tests::unique_test_root();
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn launch(&self, script: &str) -> LoginLaunch {
            let executable = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            LoginLaunch {
                executable,
                directory: self.0.clone(),
                arguments: [
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    script,
                ]
                .map(str::to_owned)
                .into(),
            }
        }
        async fn guard(&self) -> SteamCmdLifecycleGuard {
            SteamCmdLifecycleGuard(
                crate::ResourceLocks::acquire(
                    crate::install_operation::test_install_lock_context(),
                    &[],
                    std::slice::from_ref(&self.0),
                    deadline(),
                )
                .await
                .unwrap(),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn deadline() -> InstallDeadline {
        InstallDeadline::new("synthetic Steam account login", Duration::from_secs(25))
    }

    #[tokio::test]
    async fn native_half_line_prompts_accept_answers_without_exposing_echoed_secrets() {
        let fixture = Fixture::new();
        let script = "[Console]::Write('Pass'); Start-Sleep -Milliseconds 100; [Console]::Write('word: '); $p=[Console]::ReadLine(); if($p -cne 'synthetic-password'){exit 9}; [Console]::WriteLine($p); [Console]::Write('Steam Guard code: '); $g=[Console]::ReadLine(); if($g -cne 'ABCDE'){exit 8}; [Console]::WriteLine($g); exit 0";
        let (sender, mut input) = mpsc::channel(1);
        let cancellation = InstallCancellation::new();
        let mut phases = Vec::new();
        let guard = run_launch(
            fixture.launch(script),
            fixture.guard().await,
            &cancellation,
            &mut input,
            deadline(),
            &mut |progress| {
                let phase = progress.phase;
                phases.push(phase);
                let value = match phase {
                    Phase::Password => "synthetic-password",
                    Phase::SteamGuard => "ABCDE",
                    _ => return,
                };
                sender
                    .try_send(SteamWorkshopAccountInput {
                        challenge_revision: progress.challenge_revision,
                        value: value.into(),
                    })
                    .unwrap();
            },
        )
        .await
        .unwrap();
        drop(guard);
        assert_eq!(
            phases
                .iter()
                .filter(|phase| **phase == Phase::Password)
                .count(),
            1
        );
        assert_eq!(
            phases
                .iter()
                .filter(|phase| **phase == Phase::SteamGuard)
                .count(),
            1
        );
        assert!(!cancellation.is_cancelled());
        let serialized = serde_json::to_string(&phases).unwrap();
        assert!(!serialized.contains("synthetic-password"));
        assert!(!serialized.contains("ABCDE"));
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn native_retry_requires_a_new_real_prompt_before_accepting_another_code() {
        let fixture = Fixture::new();
        let script = "[Console]::Write('Steam Guard code: '); $first=[Console]::ReadLine(); if($first -cne 'ABCDE'){exit 9}; [Console]::WriteLine('Login Failure: Invalid Login Auth Code'); [Console]::Write('Steam Guard code: '); $second=[Console]::ReadLine(); if($second -cne 'FGHIJ'){exit 8}; exit 0";
        let (sender, mut input) = mpsc::channel(1);
        let mut prompts = 0;
        let guard = run_launch(
            fixture.launch(script),
            fixture.guard().await,
            &InstallCancellation::new(),
            &mut input,
            deadline(),
            &mut |progress| {
                if progress.phase == Phase::SteamGuard {
                    prompts += 1;
                    sender
                        .try_send(SteamWorkshopAccountInput {
                            challenge_revision: progress.challenge_revision,
                            value: String::from(if prompts == 1 { "ABCDE" } else { "FGHIJ" }),
                        })
                        .unwrap();
                }
            },
        )
        .await
        .unwrap();
        drop(guard);
        assert_eq!(prompts, 2);
    }

    #[tokio::test]
    async fn queued_old_challenge_input_is_not_rebound_to_a_new_native_prompt() {
        let fixture = Fixture::new();
        // The fixture advances without receiving a secret, so arrival after
        // the new prompt is deterministic and independent of polling timing.
        let script = r#"[Console]::Write('Password: ')
while(!(Test-Path -LiteralPath 'advance-prompt')){Start-Sleep -Milliseconds 10}
[Console]::WriteLine()
[Console]::WriteLine('Login Failure: Invalid Password')
[Console]::Write('Steam Guard code: ')
$value=[Console]::ReadLine()
if($value -cne '7Q9XW'){exit 9}
exit 0"#;
        let (sender, mut input) = mpsc::channel(3);
        let mut old_revision = None;
        let mut new_revision = None;
        let guard = run_launch(
            fixture.launch(script),
            fixture.guard().await,
            &InstallCancellation::new(),
            &mut input,
            deadline(),
            &mut |progress| match progress.phase {
                Phase::Password => {
                    old_revision = progress.challenge_revision;
                    std::fs::write(fixture.0.join("advance-prompt"), []).unwrap();
                }
                Phase::SteamGuard => {
                    new_revision = progress.challenge_revision;
                    assert!(new_revision.unwrap() > old_revision.unwrap());
                    for (challenge_revision, value) in [
                        (None, "unbound-response"),
                        (old_revision, "old-password-response"),
                        (new_revision, "7Q9XW"),
                    ] {
                        sender
                            .try_send(SteamWorkshopAccountInput {
                                challenge_revision,
                                value: value.into(),
                            })
                            .unwrap();
                    }
                }
                _ => {}
            },
        )
        .await
        .unwrap();
        drop(guard);
        assert!(old_revision.is_some());
        assert!(new_revision.is_some());
    }

    #[tokio::test]
    async fn cancelling_native_prompt_reaps_before_the_same_lifecycle_lock_is_reacquired() {
        let fixture = Fixture::new();
        let cancellation = InstallCancellation::new();
        let (_sender, mut input) = mpsc::channel(1);
        let result = run_launch(
            fixture.launch("[Console]::Write('Password: '); Start-Sleep -Seconds 120"),
            fixture.guard().await,
            &cancellation,
            &mut input,
            deadline(),
            &mut |progress| {
                if progress.phase == Phase::Password {
                    cancellation.cancel();
                }
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(SteamCmdError::InstallCancelled { .. })
        ));
        drop(fixture.guard().await);
    }

    #[tokio::test]
    async fn native_echo_never_appears_in_a_login_failure_error() {
        let fixture = Fixture::new();
        let (sender, mut input) = mpsc::channel(1);
        let result = run_launch(fixture.launch("[Console]::Write('Password: '); $p=[Console]::ReadLine(); [Console]::WriteLine($p); exit 9"),
            fixture.guard().await, &InstallCancellation::new(), &mut input, deadline(), &mut |progress| {
                if progress.phase == Phase::Password {
                    sender.try_send(SteamWorkshopAccountInput {
                        challenge_revision: progress.challenge_revision,
                        value: String::from("synthetic-sensitive-response"),
                    }).unwrap();
                }
            }).await;
        let SteamCmdError::SteamAccountAuthenticationFailed { output_excerpt } =
            result.err().unwrap()
        else {
            panic!("expected fixed safe authentication error");
        };
        assert!(!output_excerpt.contains("synthetic-sensitive-response"));
        assert!(output_excerpt.contains("exited before account authentication"));
    }
}
