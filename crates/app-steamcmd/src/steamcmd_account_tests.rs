use super::*;

#[test]
fn account_names_are_normalized_without_accepting_command_syntax() {
    assert_eq!(
        normalize_workshop_account_name("Steam_Account-1@example.org").unwrap(),
        "steam_account-1@example.org"
    );
    assert_eq!(
        normalize_workshop_account_name(&"a".repeat(64))
            .unwrap()
            .len(),
        64
    );
    for value in [
        "",
        "anonymous",
        "ANONYMOUS",
        " user",
        "user ",
        "user name",
        "user\nquit",
        "user\r",
        "user\t",
        "user+quit",
        "user;quit",
        "user&quit",
        "user|quit",
        "user%PATH%",
        "user!PATH!",
        "user^",
        "user>file",
        "user<file",
        "user\"",
        "user'",
        "用户",
        "user\0",
        "user/../file",
    ] {
        assert!(
            matches!(
                normalize_workshop_account_name(value),
                Err(SteamCmdError::InvalidSteamAccountName)
            ),
            "{value:?}"
        );
    }
    assert!(normalize_workshop_account_name(&"a".repeat(65)).is_err());
}

#[test]
fn anonymous_remains_the_explicit_default_and_account_scripts_contain_only_names() {
    assert_eq!(workshop_login_line(None), "login anonymous");
    assert_eq!(
        workshop_login_line(Some("test_account")),
        "login test_account"
    );
}

fn observe(lines: &[&str]) -> AccountLoginEvidence {
    let mut evidence = AccountLoginEvidence::default();
    for line in lines {
        evidence.observe_line(line);
    }
    evidence
}

#[test]
fn cached_login_requires_an_identified_account_and_completed_user_info() {
    for login in [
        "Logging in user 'Test_Account' [U:1:42] to Steam Public...OK",
        "[2026-10-17 13:50:44] Logging in user \"test_account\" [U:1:42] to Steam Public...",
        "Logging in user test_account to Steam Public...Logged in OK",
    ] {
        let evidence = observe(&[
            "Logging in using cached credentials.",
            login,
            "Waiting for client config...OK",
            "Waiting for user info...OK",
        ]);
        assert_eq!(evidence.authenticated_account_name(), Some("test_account"));
        assert!(
            verify_cached_account(
                "TEST_ACCOUNT",
                true,
                evidence.authenticated_account_name(),
                "fresh output"
            )
            .is_ok()
        );
    }
    for lines in [
        vec!["Steam Console Client", "Loading Steam API...OK"],
        vec!["Waiting for user info...OK"],
        vec![
            "Connecting anonymously to Steam Public...OK",
            "Waiting for user info...OK",
        ],
        vec![
            "Logging in user 'test_account' to Steam Public...",
            "Waiting for user info...",
        ],
        vec![
            "Logging in user 'test_account' to Steam Public...FAILED",
            "Login Failure: Invalid Password",
        ],
        vec![
            "Logging in user 'test_account' to Steam Public...OK",
            "Waiting for user info...FAILED. Timed out.",
            "OK",
        ],
        vec![
            "Logging in user 'test_account' to Steam Public...OK",
            "Waiting for user info...",
            "FAILED. Timed out.",
            "OK",
        ],
    ] {
        assert_eq!(
            observe(&lines).authenticated_account_name(),
            None,
            "{lines:?}"
        );
    }
}

#[test]
fn split_console_success_survives_later_output_but_account_switch_resets_it() {
    let mut evidence = observe(&[
        "[2026-10-17 13:50:44] Logging in user 'test_account' [U:1:42] to Steam Public...",
        "[2026-10-17 13:50:44] OK",
        "Waiting for user info...",
        "OK",
    ]);
    for _ in 0..100 {
        evidence.observe_line("downloading unrelated progress");
    }
    assert_eq!(evidence.authenticated_account_name(), Some("test_account"));
    evidence.observe_line("Logging in user 'different_account' to Steam Public...");
    assert_eq!(evidence.authenticated_account_name(), None);
    evidence.observe_line("Waiting for user info...OK");
    assert_eq!(
        evidence.authenticated_account_name(),
        Some("different_account")
    );
    evidence.observe_line("Connecting anonymously to Steam Public...OK");
    assert_eq!(evidence.authenticated_account_name(), None);
}

#[test]
fn verified_split_user_info_preserves_identity_after_workshop_download_failure() {
    let evidence = observe(&[
        "Logging in user 'test_account' [U:1:42] to Steam Public...",
        "OK",
        "Waiting for user info...",
        "OK",
        "workshop_download_item 1623730 3816899969 validate",
        "ERROR! Download item 3816899969 failed (Failure).",
        "Update canceled: Failed to initialize depot 1623730, manifest 42 (Missing decryption key) (Missing decryption key)",
    ]);
    assert_eq!(evidence.authenticated_account_name(), Some("test_account"));
    assert!(
        verify_authenticated_account(
            "test_account",
            evidence.authenticated_account_name(),
            "Workshop download failed",
        )
        .is_ok()
    );
}

#[test]
fn cached_verification_rejects_wrong_account_zero_exit_without_evidence_and_failed_process() {
    for (success, account) in [
        (true, None),
        (true, Some("other_account")),
        (false, Some("test_account")),
    ] {
        let error =
            verify_cached_account("test_account", success, account, "fresh failure details")
                .unwrap_err();
        let SteamCmdError::SteamAccountAuthenticationFailed { output_excerpt } = error else {
            panic!("expected account authentication error");
        };
        assert!(output_excerpt.contains("fresh failure details"));
        assert!(output_excerpt.contains("not switched to anonymous"));
    }
}
