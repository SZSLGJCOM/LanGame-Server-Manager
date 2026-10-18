use super::*;

fn output(success: bool, excerpt: &str) -> StreamingCommandResult {
    StreamingCommandResult {
        success,
        exit_code: Some(if success { 0 } else { 1 }),
        excerpt: excerpt.into(),
        content_log_excerpt: None,
        workshop_log_excerpt: None,
        workshop_download_failed: false,
        workshop_command_evidence: Default::default(),
        app_command_evidence: Default::default(),
        authenticated_account_name: None,
        denied_manifests: Vec::new(),
    }
}

#[test]
fn explicit_current_app_entitlement_results_require_an_account_even_after_exit_zero() {
    for message in [
        "ERROR! Failed to install app '223350' (No subscription)",
        "ERROR! Failed to install app '223350' (NoSubscription).quit",
        "ERROR! Failed to install app '223350' (No license)",
        "ERROR! Failed to install app '223350' (NoLicense).",
        "[2026-10-17 17:00:00] ERROR! Failed to install app '223350' (Access Denied)",
        "ERROR! Failed to install app '223350' (AccessDenied)",
        "[2026-10-17 17:00:00] AppID 223350 update canceled : No subscription (No subscription)",
        "AppID 223350 update canceled : No license",
    ] {
        for success in [true, false] {
            let result = output(success, message);
            let error = verify_command_result(223_350, None, &result).unwrap_err();
            let SteamCmdError::SteamAppAccountRequired {
                app_id,
                output_excerpt,
            } = error
            else {
                panic!("wrong error for {message}");
            };
            assert_eq!(app_id, 223_350);
            assert!(output_excerpt.contains(message));
        }
    }
}

#[test]
fn generic_native_failure_cannot_certify_cached_server_files_after_exit_zero() {
    for message in [
        "ERROR! Failed to install app '223350' (Failure)",
        "ERROR! Failed to install app '223350' (Timeout)",
        "Error! App '223350' state is 0x6 after update job.",
        "AppID 223350 update canceled : Failed writing local cache (Access Denied)",
    ] {
        assert!(
            matches!(
                verify_command_result(223_350, None, &output(true, message)),
                Err(SteamCmdError::SteamCmdCommandFailed { .. })
            ),
            "{message}"
        );
    }
}

#[test]
fn stream_evidence_survives_tail_rollover_and_is_bound_to_the_app() {
    for (message, permission) in [
        (
            "ERROR! Failed to install app '223350' (No subscription)",
            true,
        ),
        ("Error! App '223350' state is 0x6 after update job.", false),
    ] {
        let mut result = output(true, "quit\nUnloading Steam API...OK");
        result.app_command_evidence.observe_line(message);
        let error = verify_command_result(223_350, None, &result).unwrap_err();
        assert_eq!(
            matches!(error, SteamCmdError::SteamAppAccountRequired { .. }),
            permission
        );
        assert!(error.to_string().contains("during this invocation"));
        assert!(verify_command_result(1_623_730, None, &result).is_ok());
    }
}

#[test]
fn historical_or_unrelated_app_diagnostics_never_request_login() {
    for message in [
        "ERROR! Failed to install app '42' (No subscription)",
        "Failed to write D:/steamcmd/cache: Access Denied",
        "ERROR! Failed to install app '223350' (Access Denied (os error 5))",
        "ERROR! Failed to install app '223350' (Access Denied). writing cache",
        "ERROR! Failed to install app 'abc' (No subscription)",
        "ERROR! Failed to install app '0' (No subscription)",
        "CDepotDownloadMgr::BYldRequestDepotManifest(App: 223350, Depot: 223351, Manifest: 1, branch: ): Failed to get manifest request code, 'Access Denied'",
        "Connection failed: SSL handshake error",
    ] {
        let mut result = output(false, message);
        result.content_log_excerpt =
            Some("ERROR! Failed to install app '223350' (No subscription)".into());
        assert!(
            matches!(
                verify_command_result(223_350, None, &result),
                Err(SteamCmdError::SteamCmdCommandFailed { .. })
            ),
            "{message}"
        );
    }
}

#[test]
fn cached_login_must_verify_the_selected_identity_before_command_results() {
    for message in [
        "Success! App '223350' fully installed.",
        "ERROR! Failed to install app '223350' (No subscription)",
    ] {
        for account in [None, Some("other_account")] {
            let mut result = output(true, message);
            result.authenticated_account_name = account.map(str::to_owned);
            assert!(matches!(
                verify_command_result(223_350, Some("selected_account"), &result),
                Err(SteamCmdError::SteamAccountAuthenticationFailed { .. })
            ));
        }
    }
}

#[test]
fn verified_account_preserves_the_real_native_download_result() {
    for (success, message) in [
        (true, "Success! App '223350' fully installed."),
        (false, "Steam download connection timed out"),
    ] {
        let mut result = output(success, message);
        result.authenticated_account_name = Some("SELECTED_ACCOUNT".into());
        let verified = verify_command_result(223_350, Some("selected_account"), &result);
        assert_eq!(verified.is_ok(), success);
        if !success {
            assert!(matches!(
                verified,
                Err(SteamCmdError::SteamCmdCommandFailed { .. })
            ));
        }
    }
    let mut result = output(
        false,
        "ERROR! Failed to install app '223350' (No subscription)",
    );
    result.authenticated_account_name = Some("selected_account".into());
    assert!(matches!(
        verify_command_result(223_350, Some("selected_account"), &result),
        Err(SteamCmdError::SteamAppAccountRequired { .. })
    ));
}
