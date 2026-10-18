use super::*;

fn target(id: &str) -> SteamAccountLoginTarget {
    SteamAccountLoginTarget::Instance(id.into())
}

fn progress(phase: SteamWorkshopAccountPhase) -> SteamWorkshopAccountProgress {
    SteamWorkshopAccountProgress {
        phase,
        challenge_revision: matches!(
            phase,
            SteamWorkshopAccountPhase::GuardCode
                | SteamWorkshopAccountPhase::EmailCode
                | SteamWorkshopAccountPhase::Password
                | SteamWorkshopAccountPhase::SteamGuard
        )
        .then_some(1),
    }
}

fn fixture() -> (
    SteamWorkshopAccountSessions,
    mpsc::Receiver<SteamWorkshopAccountInput>,
) {
    let sessions = SteamWorkshopAccountSessions::default();
    let (sender, input) = mpsc::channel(1);
    sessions
        .begin(
            &target("palworld-a"),
            "session-a",
            sender,
            std::path::Path::new("profile-a"),
        )
        .unwrap();
    (sessions, input)
}

#[test]
fn session_and_revision_bind_inputs_to_one_actual_prompt() {
    let (sessions, mut input) = fixture();
    assert!(
        sessions
            .submit(&target("palworld-b"), "session-a", 1, "login alice".into())
            .is_err()
    );
    assert!(
        sessions
            .submit(
                &target("palworld-a"),
                "wrong-session",
                1,
                "login alice".into()
            )
            .is_err()
    );
    assert!(
        sessions
            .submit(&target("palworld-a"), "session-a", 2, "login alice".into())
            .is_err()
    );
    let sent = sessions
        .submit(&target("palworld-a"), "session-a", 1, "login Alice".into())
        .unwrap();
    assert_eq!(input.try_recv().unwrap().value, "alice");
    assert_eq!(sent.accepted_input_revision, Some(1));
    assert_eq!(sent.phase, LoginPhase::InputSubmitted);
    assert_eq!(sent.input_mode, InputMode::Disabled);
    assert!(
        sessions
            .submit(
                &target("palworld-a"),
                "session-a",
                sent.revision,
                "login bob".into()
            )
            .is_err()
    );
}

#[test]
fn account_entry_accepts_a_name_or_login_command_without_password_arguments() {
    for command in ["Alice", "login Alice", "Login Alice", "  login   Alice  "] {
        let (sessions, mut input) = fixture();
        let accepted = sessions
            .submit(&target("palworld-a"), "session-a", 1, command.into())
            .unwrap();
        assert_eq!(input.try_recv().unwrap().value, "alice");
        assert_eq!(accepted.accepted_input_revision, Some(1));
    }
    for command in [
        "login",
        "login alice secret-password",
        "echo secret-password",
        "alice secret-password",
    ] {
        let (sessions, _) = fixture();
        let error = sessions
            .submit(&target("palworld-a"), "session-a", 1, command.into())
            .err()
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(payload["code"], "steam_account_command_invalid");
        assert!(!error.contains("secret-password"));
        assert_eq!(
            sessions
                .snapshot(&target("palworld-a"), "session-a")
                .unwrap()
                .accepted_input_revision,
            None
        );
    }
}

#[test]
fn progress_distinguishes_preparation_connection_authentication_and_cache_verification() {
    let (sessions, _) = fixture();
    for (native, expected) in [
        (SteamWorkshopAccountPhase::Queued, LoginPhase::Queued),
        (
            SteamWorkshopAccountPhase::PreparingSteamcmd,
            LoginPhase::PreparingSteamcmd,
        ),
        (
            SteamWorkshopAccountPhase::Connecting,
            LoginPhase::Connecting,
        ),
        (
            SteamWorkshopAccountPhase::Authenticating,
            LoginPhase::Authenticating,
        ),
        (
            SteamWorkshopAccountPhase::VerifyingCache,
            LoginPhase::VerifyingCache,
        ),
    ] {
        sessions.progress("session-a", progress(native));
        let snapshot = sessions
            .snapshot(&target("palworld-a"), "session-a")
            .unwrap();
        assert_eq!(snapshot.phase, expected);
        assert_eq!(snapshot.input_mode, InputMode::Disabled);
        assert_eq!(snapshot.events.last(), Some(&expected));
        assert!(
            sessions
                .submit(
                    &target("palworld-a"),
                    "session-a",
                    snapshot.revision,
                    "unrequested-input".into()
                )
                .is_err()
        );
    }
}

#[test]
fn bounded_or_closed_input_channels_return_specific_safe_rejections() {
    let (sessions, mut input) = fixture();
    sessions
        .submit(&target("palworld-a"), "session-a", 1, "alice".into())
        .unwrap();
    sessions.progress("session-a", progress(SteamWorkshopAccountPhase::Password));
    let pending = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    let error = sessions
        .submit(
            &target("palworld-a"),
            "session-a",
            pending.revision,
            "synthetic-secret".into(),
        )
        .err()
        .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(payload["code"], "steam_account_input_pending");
    assert!(!error.contains("synthetic-secret"));
    assert_eq!(
        sessions
            .snapshot(&target("palworld-a"), "session-a")
            .unwrap()
            .accepted_input_revision,
        Some(1)
    );
    assert_eq!(input.try_recv().unwrap().value, "alice");
    drop(input);
    let error = sessions
        .submit(
            &target("palworld-a"),
            "session-a",
            pending.revision,
            "synthetic-secret".into(),
        )
        .err()
        .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(payload["code"], "steam_account_input_closed");
    assert!(!error.contains("synthetic-secret"));
}

#[test]
fn rejected_inputs_explain_the_reason_without_echoing_the_submitted_text() {
    let (sessions, _input) = fixture();
    for (revision, value, code) in [
        (2, "private-input", "steam_account_prompt_changed"),
        (
            1,
            "private-input\nsecret-password",
            "steam_account_input_invalid",
        ),
        (1, "login invalid;account", "steam_account_name_invalid"),
    ] {
        let error = sessions
            .submit(&target("palworld-a"), "session-a", revision, value.into())
            .err()
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(payload["code"], code);
        assert!(!error.contains("private-input"));
        assert!(!error.contains("secret-password"));
        assert!(!error.contains("invalid;account"));
    }
}

#[test]
fn a_lost_input_response_can_be_reconciled_from_the_receipt_without_resending() {
    let (sessions, mut input) = fixture();
    sessions
        .submit(&target("palworld-a"), "session-a", 1, "alice".into())
        .unwrap();
    input.try_recv().unwrap();
    sessions.progress("session-a", progress(SteamWorkshopAccountPhase::Password));
    let revision = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap()
        .revision;
    sessions
        .submit(
            &target("palworld-a"),
            "session-a",
            revision,
            "synthetic-secret".into(),
        )
        .unwrap();
    let recovered = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    assert_eq!(recovered.accepted_input_revision, Some(revision));
    assert!(
        !serde_json::to_string(&recovered)
            .unwrap()
            .contains("synthetic-secret")
    );
    assert_eq!(input.try_recv().unwrap().value, "synthetic-secret");
    assert!(
        sessions
            .submit(
                &target("palworld-a"),
                "session-a",
                revision,
                "synthetic-secret".into()
            )
            .is_err()
    );
    assert!(input.try_recv().is_err());
}

#[test]
fn secrets_are_single_use_and_absent_from_snapshots_and_errors() {
    let (sessions, mut input) = fixture();
    for command in [
        "login alice secret-password",
        "echo secret-password",
        "login anonymous",
    ] {
        let error = sessions
            .submit(&target("palworld-a"), "session-a", 1, command.into())
            .err()
            .unwrap();
        assert!(!error.contains("secret-password"));
    }
    sessions.progress("session-a", progress(SteamWorkshopAccountPhase::Password));
    let revision = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap()
        .revision;
    let response = concat!("fixture-password", "-秘密");
    let snapshot = sessions
        .submit(
            &target("palworld-a"),
            "session-a",
            revision,
            response.into(),
        )
        .unwrap();
    assert_eq!(input.try_recv().unwrap().value, response);
    assert!(!serde_json::to_string(&snapshot).unwrap().contains(response));
    let error = sessions
        .submit(
            &target("palworld-a"),
            "session-a",
            revision,
            response.into(),
        )
        .err()
        .unwrap();
    assert!(!error.contains(response));
    assert!(input.try_recv().is_err());
    sessions.progress("session-a", progress(SteamWorkshopAccountPhase::SteamGuard));
    let revision = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap()
        .revision;
    let snapshot = sessions
        .submit(&target("palworld-a"), "session-a", revision, "ABCDE".into())
        .unwrap();
    assert_eq!(input.try_recv().unwrap().value, "ABCDE");
    assert!(!serde_json::to_string(&snapshot).unwrap().contains("ABCDE"));
}

#[test]
fn cancelled_login_never_selects_an_account_and_stale_progress_cannot_reopen_it() {
    let (sessions, _) = fixture();
    sessions.finish("session-a", Err("installation_cancelled".into()));
    sessions.progress("session-a", progress(SteamWorkshopAccountPhase::Password));
    let snapshot = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    assert_eq!(snapshot.phase, LoginPhase::Cancelled);
    assert_eq!(snapshot.input_mode, InputMode::Disabled);
    assert!(snapshot.account_name.is_none());
    let (sender, _) = mpsc::channel(1);
    sessions
        .begin(
            &target("palworld-b"),
            "session-b",
            sender,
            std::path::Path::new("profile-a"),
        )
        .unwrap();
    sessions.finish(
        "session-a",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "fixture".into(),
        }),
    );
    assert_eq!(
        sessions
            .snapshot(&target("palworld-b"), "session-b")
            .unwrap()
            .phase,
        LoginPhase::Ready
    );
}

#[test]
fn guard_code_is_a_secret_single_use_challenge_with_a_read_only_receipt() {
    let (sessions, mut input) = fixture();
    sessions.progress("session-a", progress(SteamWorkshopAccountPhase::GuardCode));
    let prompt = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    assert_eq!(prompt.phase, LoginPhase::GuardCode);
    assert_eq!(prompt.input_mode, InputMode::Secret);
    let accepted = sessions
        .submit(
            &target("palworld-a"),
            "session-a",
            prompt.revision,
            "7q9xw".into(),
        )
        .unwrap();
    assert_eq!(input.try_recv().unwrap().value, "7q9xw");
    assert_eq!(accepted.accepted_input_revision, Some(prompt.revision));
    assert!(!serde_json::to_string(&accepted).unwrap().contains("7q9xw"));
    assert_eq!(
        sessions
            .snapshot(&target("palworld-a"), "session-a")
            .unwrap()
            .accepted_input_revision,
        Some(prompt.revision)
    );
    assert!(
        sessions
            .submit(
                &target("palworld-a"),
                "session-a",
                prompt.revision,
                "7q9xw".into()
            )
            .is_err()
    );
    assert!(input.try_recv().is_err());
}

#[test]
fn guard_code_rejects_invalid_inputs_without_consuming_the_prompt_or_echoing_secrets() {
    let (sessions, mut input) = fixture();
    sessions.progress("session-a", progress(SteamWorkshopAccountPhase::GuardCode));
    let prompt = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    for invalid in [
        "12",
        "123456",
        "7Q 9W",
        "令牌验证码",
        "abc;quit",
        "7Q9XW\nquit",
        "a\"bcd",
    ] {
        let error = sessions
            .submit(
                &target("palworld-a"),
                "session-a",
                prompt.revision,
                invalid.into(),
            )
            .err()
            .unwrap();
        let error_data: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(error_data["code"], "steam_account_input_invalid");
        assert!(!error.contains(invalid));
        let unchanged = sessions
            .snapshot(&target("palworld-a"), "session-a")
            .unwrap();
        assert_eq!(unchanged.revision, prompt.revision);
        assert_eq!(unchanged.accepted_input_revision, None);
        assert!(input.try_recv().is_err());
    }
}

#[test]
fn empty_input_never_selects_or_bypasses_a_verification_method() {
    for phase in [
        SteamWorkshopAccountPhase::Password,
        SteamWorkshopAccountPhase::GuardCode,
        SteamWorkshopAccountPhase::EmailCode,
        SteamWorkshopAccountPhase::SteamGuard,
        SteamWorkshopAccountPhase::Connecting,
        SteamWorkshopAccountPhase::MobileConfirmation,
    ] {
        let (sessions, mut input) = fixture();
        sessions.progress("session-a", progress(phase));
        let prompt = sessions
            .snapshot(&target("palworld-a"), "session-a")
            .unwrap();
        assert!(
            sessions
                .submit(
                    &target("palworld-a"),
                    "session-a",
                    prompt.revision,
                    String::new()
                )
                .is_err()
        );
        assert!(input.try_recv().is_err());
    }
    let (sessions, mut input) = fixture();
    assert!(
        sessions
            .submit(&target("palworld-a"), "session-a", 1, String::new())
            .is_err()
    );
    assert!(input.try_recv().is_err());
    assert_eq!(
        sessions
            .snapshot(&target("palworld-a"), "session-a")
            .unwrap()
            .accepted_input_revision,
        None
    );
}

#[test]
fn email_code_is_a_bound_hidden_challenge_and_rejects_empty_or_injected_input() {
    let (sessions, mut input) = fixture();
    sessions.progress("session-a", progress(SteamWorkshopAccountPhase::EmailCode));
    let prompt = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    assert_eq!(prompt.phase, LoginPhase::EmailCode);
    assert_eq!(prompt.input_mode, InputMode::Secret);
    for value in ["", "has space", "abc\nquit", "abc;quit", "验证码"] {
        assert!(
            sessions
                .submit(
                    &target("palworld-a"),
                    "session-a",
                    prompt.revision,
                    value.into()
                )
                .is_err()
        );
        assert!(input.try_recv().is_err());
        assert_eq!(
            sessions
                .snapshot(&target("palworld-a"), "session-a")
                .unwrap()
                .revision,
            prompt.revision
        );
    }
    let accepted = sessions
        .submit(
            &target("palworld-a"),
            "session-a",
            prompt.revision,
            "ABCDE".into(),
        )
        .unwrap();
    let received = input.try_recv().unwrap();
    assert_eq!(received.value, "ABCDE");
    assert_eq!(received.challenge_revision, Some(1));
    assert_eq!(accepted.accepted_input_revision, Some(prompt.revision));
    assert!(!serde_json::to_string(&accepted).unwrap().contains("ABCDE"));
}

#[test]
fn queued_inputs_keep_the_native_challenge_accepted_by_the_desktop_receipt() {
    let (sessions, mut input) = fixture();
    sessions.progress(
        "session-a",
        SteamWorkshopAccountProgress {
            phase: SteamWorkshopAccountPhase::GuardCode,
            challenge_revision: Some(41),
        },
    );
    let prompt = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    sessions
        .submit(
            &target("palworld-a"),
            "session-a",
            prompt.revision,
            "7Q9XW".into(),
        )
        .unwrap();
    sessions.progress(
        "session-a",
        SteamWorkshopAccountProgress {
            phase: SteamWorkshopAccountPhase::Password,
            challenge_revision: Some(42),
        },
    );
    let queued = input.try_recv().unwrap();
    assert_eq!(queued.challenge_revision, Some(41));
    assert_eq!(queued.value, "7Q9XW");
    let current = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    assert_eq!(current.phase, LoginPhase::Password);
    assert!(
        sessions
            .submit(
                &target("palworld-a"),
                "session-a",
                prompt.revision,
                "7Q9XW".into()
            )
            .is_err()
    );
    assert!(input.try_recv().is_err());
}

#[test]
fn replacement_native_challenge_advances_the_ui_revision_even_with_the_same_phase() {
    let (sessions, mut input) = fixture();
    sessions.progress(
        "session-a",
        SteamWorkshopAccountProgress {
            phase: SteamWorkshopAccountPhase::GuardCode,
            challenge_revision: Some(51),
        },
    );
    let original = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    sessions.progress(
        "session-a",
        SteamWorkshopAccountProgress {
            phase: SteamWorkshopAccountPhase::GuardCode,
            challenge_revision: Some(52),
        },
    );
    let replacement = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    assert!(replacement.revision > original.revision);
    assert!(
        sessions
            .submit(
                &target("palworld-a"),
                "session-a",
                original.revision,
                "7Q9XW".into()
            )
            .is_err()
    );
    sessions
        .submit(
            &target("palworld-a"),
            "session-a",
            replacement.revision,
            "7Q9XW".into(),
        )
        .unwrap();
    let accepted = input.try_recv().unwrap();
    assert_eq!(accepted.challenge_revision, Some(52));
    assert_eq!(accepted.value, "7Q9XW");
}

#[test]
fn only_verified_completion_publishes_the_selected_account_and_output_is_bounded() {
    let (sessions, _) = fixture();
    for _ in 0..80 {
        sessions.progress("session-a", progress(SteamWorkshopAccountPhase::Password));
        sessions.progress("session-a", progress(SteamWorkshopAccountPhase::Connecting));
    }
    sessions.progress(
        "session-a",
        progress(SteamWorkshopAccountPhase::Authenticated),
    );
    assert!(
        sessions
            .snapshot(&target("palworld-a"), "session-a")
            .unwrap()
            .account_name
            .is_none()
    );
    sessions.finish(
        "session-a",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "fixture".into(),
        }),
    );
    let snapshot = sessions
        .snapshot(&target("palworld-a"), "session-a")
        .unwrap();
    assert_eq!(snapshot.phase, LoginPhase::Authenticated);
    assert_eq!(snapshot.account_name.as_deref(), Some("alice"));
    assert!(snapshot.events.len() <= 32);
}

#[test]
fn steam_login_target_contract_rejects_missing_unknown_or_mixed_scope() {
    for (value, expected) in [
        (
            serde_json::json!({"kind":"instance", "id":"game-a"}),
            target("game-a"),
        ),
        (
            serde_json::json!({"kind":"module", "id":"game-a"}),
            SteamAccountLoginTarget::Module("game-a".into()),
        ),
    ] {
        let decoded: SteamAccountLoginTarget = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
    }
    for value in [
        serde_json::json!({"kind":"instance"}),
        serde_json::json!({"kind":"unknown", "id":"game-a"}),
        serde_json::json!({"kind":"module", "id":"game-a", "instanceId":"game-b"}),
    ] {
        assert!(serde_json::from_value::<SteamAccountLoginTarget>(value).is_err());
    }
}

#[test]
fn module_login_inputs_and_receipts_cannot_cross_into_an_instance_with_the_same_id() {
    let sessions = SteamWorkshopAccountSessions::default();
    let module = SteamAccountLoginTarget::Module("game-a".into());
    let instance = target("game-a");
    let (sender, mut input) = mpsc::channel(1);
    sessions
        .begin(
            &module,
            "module-session",
            sender,
            std::path::Path::new("profile-a"),
        )
        .unwrap();
    assert!(sessions.snapshot(&instance, "module-session").is_err());
    assert!(
        sessions
            .submit(&instance, "module-session", 1, "alice".into())
            .is_err()
    );
    let snapshot = sessions
        .submit(&module, "module-session", 1, "alice".into())
        .unwrap();
    assert_eq!(snapshot.target, module);
    assert_eq!(snapshot.accepted_input_revision, Some(1));
    assert_eq!(input.try_recv().unwrap().value, "alice");
    assert!(input.try_recv().is_err());
}

#[test]
fn automatic_workshop_account_requires_the_exact_authenticated_instance_and_context() {
    let (sessions, _) = fixture();
    let steamcmd_root = std::path::Path::new("steamcmd-a");
    let storage_root = std::path::Path::new("profile-a");
    assert_eq!(
        sessions
            .workshop_download_account(&target("palworld-a"), steamcmd_root, storage_root)
            .unwrap(),
        None
    );
    sessions.finish(
        "session-a",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "steamcmd-a".into(),
        }),
    );
    assert_eq!(
        sessions
            .workshop_download_account(&target("palworld-a"), steamcmd_root, storage_root)
            .unwrap()
            .as_deref(),
        Some("alice")
    );
    for (requested, native, profile) in [
        (target("palworld-b"), "steamcmd-a", "profile-a"),
        (
            SteamAccountLoginTarget::Module("palworld-a".into()),
            "steamcmd-a",
            "profile-a",
        ),
        (target("palworld-a"), "steamcmd-b", "profile-a"),
        (target("palworld-a"), "steamcmd-a", "profile-b"),
    ] {
        assert_eq!(
            sessions
                .workshop_download_account(
                    &requested,
                    std::path::Path::new(native),
                    std::path::Path::new(profile),
                )
                .unwrap(),
            None
        );
    }
}

#[test]
fn module_authentication_never_grants_automatic_workshop_access() {
    let sessions = SteamWorkshopAccountSessions::default();
    let module = SteamAccountLoginTarget::Module("palworld-a".into());
    let (sender, _) = mpsc::channel(1);
    let storage_root = std::path::Path::new("profile-a");
    let steamcmd_root = std::path::Path::new("steamcmd-a");
    sessions
        .begin(&module, "module-session", sender, storage_root)
        .unwrap();
    sessions.finish(
        "module-session",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "steamcmd-a".into(),
        }),
    );
    for requested in [module, target("palworld-a")] {
        assert_eq!(
            sessions
                .workshop_download_account(&requested, steamcmd_root, storage_root)
                .unwrap(),
            None
        );
    }
}

#[test]
fn failed_cancelled_or_replaced_sessions_revoke_the_automatic_workshop_account() {
    let steamcmd_root = std::path::Path::new("steamcmd-a");
    let storage_root = std::path::Path::new("profile-a");
    for failure in ["authentication failed", "installation_cancelled"] {
        let (sessions, _) = fixture();
        sessions.finish(
            "session-a",
            Ok(SteamWorkshopAccountAuthenticationResult {
                account_name: "alice".into(),
                steamcmd_root: "steamcmd-a".into(),
            }),
        );
        sessions.finish("session-a", Err(failure.into()));
        assert_eq!(
            sessions
                .workshop_download_account(&target("palworld-a"), steamcmd_root, storage_root)
                .unwrap(),
            None
        );
    }
    let (sessions, _) = fixture();
    sessions.finish(
        "session-a",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "steamcmd-a".into(),
        }),
    );
    let (sender, _) = mpsc::channel(1);
    sessions
        .begin(
            &target("palworld-a"),
            "session-b",
            sender,
            std::path::Path::new("profile-b"),
        )
        .unwrap();
    sessions.finish(
        "session-a",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "steamcmd-a".into(),
        }),
    );
    for profile in [storage_root, std::path::Path::new("profile-b")] {
        assert_eq!(
            sessions
                .workshop_download_account(&target("palworld-a"), steamcmd_root, profile)
                .unwrap(),
            None
        );
    }
}

#[test]
fn anonymous_selection_revokes_workshop_reuse_until_a_fresh_explicit_request() {
    let (sessions, _) = fixture();
    let target = target("palworld-a");
    let steamcmd_root = std::path::Path::new("steamcmd-a");
    let storage_root = std::path::Path::new("profile-a");
    sessions.finish(
        "session-a",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "steamcmd-a".into(),
        }),
    );
    let queued = sessions
        .authorize_account(&target, "alice", steamcmd_root, storage_root)
        .unwrap();
    sessions.clear_server_download(&target).unwrap();
    assert_eq!(
        sessions
            .workshop_download_account(&target, steamcmd_root, storage_root)
            .unwrap(),
        None
    );
    assert!(
        sessions
            .authorize_workshop_download_account(
                &target,
                "alice",
                &queued,
                steamcmd_root,
                storage_root,
            )
            .is_err()
    );
    let explicit = sessions
        .authorize_account(&target, "alice", steamcmd_root, storage_root)
        .unwrap();
    for (requested, account, native, profile) in [
        (
            SteamAccountLoginTarget::Instance("palworld-b".into()),
            "alice",
            "steamcmd-a",
            "profile-a",
        ),
        (
            SteamAccountLoginTarget::Module("palworld-a".into()),
            "alice",
            "steamcmd-a",
            "profile-a",
        ),
        (target.clone(), "bob", "steamcmd-a", "profile-a"),
        (target.clone(), "alice", "steamcmd-b", "profile-a"),
        (target.clone(), "alice", "steamcmd-a", "profile-b"),
    ] {
        assert!(
            sessions
                .authorize_workshop_download_account(
                    &requested,
                    account,
                    &explicit,
                    std::path::Path::new(native),
                    std::path::Path::new(profile),
                )
                .is_err()
        );
    }
    assert_eq!(
        sessions
            .workshop_download_account(&target, steamcmd_root, storage_root)
            .unwrap(),
        None
    );
    sessions
        .authorize_workshop_download_account(
            &target,
            "alice",
            &explicit,
            steamcmd_root,
            storage_root,
        )
        .unwrap();
    assert_eq!(
        sessions
            .workshop_download_account(&target, steamcmd_root, storage_root)
            .unwrap()
            .as_deref(),
        Some("alice")
    );
}

#[path = "steam_server_account_authorization_tests.rs"]
mod server_authorization;
