//! SteamCMD can exit successfully after a Workshop command fails. Cached files
//! cannot establish that the current download or update succeeded.
use super::steamcmd_stream::StreamingCommandResult;
use super::*;

#[path = "workshop_download_paths.rs"]
mod paths;
pub(crate) use paths::DownloadPaths;

#[path = "workshop_manifest_access.rs"]
mod manifest_access;
pub(crate) use manifest_access::WorkshopEvidence;
#[path = "workshop_manifest_seed.rs"]
mod manifest_seed;
pub(crate) use manifest_seed::requested_item_manifests;

pub(super) fn is_workshop_download_failure_line(line: &str) -> bool {
    let Some((_, remainder)) = line.split_once("ERROR! Download item ") else {
        return false;
    };
    let Some((id, _)) = remainder.split_once(" failed (") else {
        return false;
    };
    !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn verify_command_result(
    consumer_app_id: u32,
    account_name: Option<&str>,
    output: &StreamingCommandResult,
) -> Result<(), SteamCmdError> {
    if let Some(account_name) = account_name {
        super::steamcmd_account::verify_authenticated_account(
            account_name,
            output.authenticated_account_name.as_deref(),
            &output.excerpt,
        )?;
    }
    if output.success
        && !output.workshop_download_failed
        && !has_missing_key(consumer_app_id, output)
        && !has_workshop_permission_denial(consumer_app_id, output)
    {
        return Ok(());
    }
    Err(command_error(
        consumer_app_id,
        output,
        failure_context(output),
    ))
}

pub(super) fn failure_context(output: &StreamingCommandResult) -> String {
    let mut context = steamcmd_failure_context(
        &output.excerpt,
        output.exit_code,
        output.content_log_excerpt.as_deref(),
    );
    if output.workshop_download_failed {
        context.push_str("\nSteamCMD reported a Workshop download failure during this invocation.");
    }
    if let Some(log) = output.workshop_log_excerpt.as_deref().map(str::trim)
        && !log.is_empty()
    {
        context.push_str("\n\n[steamcmd workshop log]\n");
        context.push_str(log);
    }
    context
}

pub(super) fn command_error(
    consumer_app_id: u32,
    output: &StreamingCommandResult,
    mut output_excerpt: String,
) -> SteamCmdError {
    // The content-log diagnostic tail may retain older records when new lines
    // are appended. Only this invocation's output and Workshop log establish
    // the reason, together with separately captured invocation-only manifest
    // evidence; historical context must never classify a network failure.
    if let Some(evidence) = output
        .workshop_command_evidence
        .diagnostic_excerpt(consumer_app_id, &output.denied_manifests)
        && !output_excerpt.contains(&evidence)
    {
        output_excerpt.push_str("\n\n[steamcmd retained Workshop evidence]\n");
        output_excerpt.push_str(&evidence);
    }
    if has_missing_key(consumer_app_id, output) {
        SteamCmdError::WorkshopDecryptionKeyMissing {
            consumer_app_id,
            output_excerpt,
        }
    } else if has_workshop_permission_denial(consumer_app_id, output) {
        SteamCmdError::WorkshopAccountRequired {
            consumer_app_id,
            output_excerpt,
        }
    } else {
        SteamCmdError::SteamCmdCommandFailed { output_excerpt }
    }
}

fn has_workshop_permission_denial(consumer_app_id: u32, output: &StreamingCommandResult) -> bool {
    output
        .workshop_command_evidence
        .permission_denial(consumer_app_id)
        || manifest_access::tail_evidence(output).permission_denial(consumer_app_id)
        || manifest_access::has_permission_denial(consumer_app_id, output)
}

fn has_missing_key(consumer_app_id: u32, output: &StreamingCommandResult) -> bool {
    output
        .workshop_command_evidence
        .missing_key(consumer_app_id)
        || manifest_access::tail_evidence(output).missing_key(consumer_app_id)
}

fn is_workshop_permission_denial(consumer_app_id: u32, line: &str) -> bool {
    let message = line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .filter(|(stamp, _)| {
            stamp.len() == 19 && stamp.as_bytes()[4] == b'-' && stamp.as_bytes()[10] == b' '
        })
        .map_or(line, |(_, message)| message)
        .trim();
    // Valve's console reports the Steam result for this exact item command.
    // An OS error mentioning access denied elsewhere is not an entitlement.
    if let Some(remainder) = message.strip_prefix("ERROR! Download item ") {
        let Some((id, result)) = remainder.split_once(" failed (") else {
            return false;
        };
        let Some((result, tail)) = result.split_once(')') else {
            return false;
        };
        return decimal_id(id)
            && matches!(tail.trim(), "" | "." | "quit" | ".quit")
            && steam_permission_result(result);
    }
    if let Some((app, reason)) = message
        .strip_prefix("AppID ")
        .and_then(|rest| rest.split_once(" update canceled : "))
    {
        return app.parse::<u32>().ok() == Some(consumer_app_id)
            && no_subscription_result(final_result(reason));
    }
    // Workshop log result rows identify the consumer application explicitly.
    let Some(remainder) = message.strip_prefix("[AppID ") else {
        return false;
    };
    let Some((app_id, remainder)) = remainder.split_once(']') else {
        return false;
    };
    if app_id.parse::<u32>().ok() != Some(consumer_app_id) {
        return false;
    }
    let remainder = remainder.trim_start();
    if let Some(remainder) = remainder.strip_prefix("Download item ") {
        let Some((id, result)) = remainder.split_once(" result : ") else {
            return false;
        };
        return decimal_id(id) && steam_permission_result(result.trim());
    }
    if let Some(reason) = remainder
        .strip_prefix("Update canceled: ")
        .or_else(|| remainder.strip_prefix("Update canceled : "))
    {
        // NoSubscription is unambiguously a Steam entitlement failure. An
        // Update-canceled AccessDenied can instead describe local file I/O;
        // that ambiguous form keeps the original generic failure diagnostic.
        return no_subscription_result(final_result(reason));
    }
    false
}

fn final_result(reason: &str) -> &str {
    reason
        .rsplit_once(" (")
        .and_then(|(_, result)| result.strip_suffix(')'))
        .unwrap_or(reason)
        .trim()
}

fn decimal_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
}

fn no_subscription_result(result: &str) -> bool {
    result.eq_ignore_ascii_case("No subscription")
        || result.eq_ignore_ascii_case("NoSubscription")
        || result.eq_ignore_ascii_case("No license")
        || result.eq_ignore_ascii_case("NoLicense")
}

fn steam_permission_result(result: &str) -> bool {
    no_subscription_result(result)
        || result.eq_ignore_ascii_case("Access Denied")
        || result.eq_ignore_ascii_case("AccessDenied")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(success: bool, workshop_download_failed: bool) -> StreamingCommandResult {
        StreamingCommandResult {
            success,
            exit_code: Some(if success { 0 } else { 1 }),
            excerpt: String::from("quit\nUnloading Steam API...OK"),
            content_log_excerpt: None,
            workshop_log_excerpt: None,
            workshop_download_failed,
            workshop_command_evidence: Default::default(),
            app_command_evidence: Default::default(),
            authenticated_account_name: None,
            denied_manifests: Vec::new(),
        }
    }

    #[test]
    fn native_failure_is_recognized_with_console_timestamp_and_trailing_quit() {
        for line in [
            "ERROR! Download item 3816899969 failed (Failure).quit",
            "[2026-10-17 13:50:44] ERROR! Download item 3816899969 failed (Failure).",
        ] {
            assert!(is_workshop_download_failure_line(line));
        }
        for line in [
            "Success. Downloaded item 3816899969 to cache",
            "workshop_download_item 1623730 3816899969 validate",
            "ERROR! Download item abc failed (Failure).",
            "ERROR! Download item  failed (Failure).",
        ] {
            assert!(!is_workshop_download_failure_line(line));
        }
    }

    #[test]
    fn native_failure_survives_zero_exit_and_excerpt_eviction() {
        let result = output(true, true);
        let error = verify_command_result(1_623_730, None, &result).unwrap_err();
        let SteamCmdError::SteamCmdCommandFailed { output_excerpt } = error else {
            panic!("unexpected error");
        };
        assert!(output_excerpt.contains("code 0"));
        assert!(output_excerpt.contains("Workshop download failure"));
    }

    #[test]
    fn missing_key_has_specific_reason_and_preserves_both_logs() {
        let mut result = output(true, true);
        result.content_log_excerpt = Some(String::from("scheduler: Missing decryption key"));
        result.workshop_log_excerpt = Some(String::from(
            "[AppID 1623730] Update canceled: Failed to initialize depot 1623730, manifest 3622555758966187100 (Missing decryption key) (Missing decryption key)",
        ));
        let error = verify_command_result(1_623_730, None, &result).unwrap_err();
        let SteamCmdError::WorkshopDecryptionKeyMissing {
            consumer_app_id,
            output_excerpt,
        } = error
        else {
            panic!("expected missing decryption key");
        };
        assert_eq!(consumer_app_id, 1_623_730);
        assert!(output_excerpt.contains("[steamcmd content log]"));
        assert!(output_excerpt.contains("[steamcmd workshop log]"));
        assert!(output_excerpt.contains("3622555758966187100"));
    }

    #[test]
    fn generic_failure_does_not_claim_login_is_required() {
        let mut result = output(false, false);
        result.workshop_log_excerpt = Some(String::from("Download item result : Timeout"));
        assert!(matches!(
            verify_command_result(1_623_730, None, &result),
            Err(SteamCmdError::SteamCmdCommandFailed { .. })
        ));
        assert!(verify_command_result(1_623_730, None, &output(true, false)).is_ok());
    }

    #[test]
    fn historical_content_log_cannot_misclassify_the_current_failure() {
        let mut result = output(true, true);
        result.content_log_excerpt = Some(String::from(
            "old download: Missing decryption key\nnew invocation: scheduler finished",
        ));
        result.workshop_log_excerpt = Some(String::from("Download item result : Timeout"));
        assert!(matches!(
            verify_command_result(1_623_730, None, &result),
            Err(SteamCmdError::SteamCmdCommandFailed { .. })
        ));
    }

    #[test]
    fn explicit_workshop_permission_results_require_an_authorized_account() {
        for line in [
            "ERROR! Download item 3816899969 failed (No subscription).",
            "[2026-10-17 13:50:44] ERROR! Download item 3816899969 failed (NoSubscription).quit",
            "ERROR! Download item 3816899969 failed (Access Denied).",
            "[AppID 1623730] Download item 3816899969 result : No subscription",
            "[2026-10-17 13:50:44] [AppID 1623730] Download item 3816899969 result : AccessDenied",
            "[AppID 1623730] Update canceled: No subscription (No subscription)",
            "[AppID 1623730] Update canceled: Failed to initialize depot 1623730, manifest 42 (NoSubscription) (NoSubscription)",
        ] {
            for source in ["stdout", "workshop"] {
                let mut result = output(true, true);
                if source == "stdout" {
                    result.excerpt = line.into();
                } else {
                    result.workshop_log_excerpt = Some(line.into());
                }
                let error = verify_command_result(1_623_730, None, &result).unwrap_err();
                let SteamCmdError::WorkshopAccountRequired {
                    consumer_app_id,
                    output_excerpt,
                } = error
                else {
                    panic!("expected account permission error for {source}: {line}");
                };
                assert_eq!(consumer_app_id, 1_623_730);
                assert!(output_excerpt.contains(line));
            }
        }
    }

    #[test]
    fn permission_result_rejects_zero_exit_even_without_a_console_failure_line() {
        let mut result = output(true, false);
        result.workshop_log_excerpt = Some(String::from(
            "[AppID 1623730] Download item 3816899969 result : Access Denied",
        ));
        assert!(matches!(
            verify_command_result(1_623_730, None, &result),
            Err(SteamCmdError::WorkshopAccountRequired { .. })
        ));
        result.authenticated_account_name = Some(String::from("test_account"));
        assert!(matches!(
            verify_command_result(1_623_730, Some("test_account"), &result),
            Err(SteamCmdError::WorkshopAccountRequired { .. })
        ));
    }

    #[test]
    fn network_disk_and_ambiguous_access_denied_do_not_claim_an_account_is_required() {
        for line in [
            "ERROR! Download item 3816899969 failed (Failure).",
            "ERROR! Download item 3816899969 failed (Timeout).",
            "ERROR! Download item 3816899969 failed (Access Denied (os error 5)).",
            "ERROR! Download item 3816899969 failed (Access Denied). while writing local cache",
            "[AppID 1623730] Update canceled: Failed to write local cache file (Access Denied)",
            "[AppID 1623730] Update canceled: Access Denied (Access Denied)",
            "[AppID 1623730] Download item 3816899969 result : Timeout",
            "[AppID 1623730] Download item 3816899969 result : Access Denied writing file",
            "[AppID 221100] Download item 3816899969 result : No subscription",
            "ERROR! Download item abc failed (No subscription).",
            "Failed to create D:/steamcmd/cache: Access Denied",
            "TLS handshake: access denied",
            "NoSubscription",
        ] {
            let mut result = output(true, true);
            result.excerpt = line.into();
            result.workshop_log_excerpt = Some(line.into());
            assert!(
                matches!(
                    verify_command_result(1_623_730, None, &result),
                    Err(SteamCmdError::SteamCmdCommandFailed { .. })
                ),
                "{line}"
            );
        }
    }

    #[test]
    fn historical_permission_result_is_diagnostic_context_only() {
        let mut result = output(true, true);
        result.content_log_excerpt = Some(String::from(
            "[AppID 1623730] Download item 3816899969 result : No subscription\nERROR! Download item 3816899969 failed (Access Denied).",
        ));
        result.workshop_log_excerpt = Some(String::from(
            "[AppID 1623730] Download item 3816899969 result : Timeout",
        ));
        let error = verify_command_result(1_623_730, None, &result).unwrap_err();
        let SteamCmdError::SteamCmdCommandFailed { output_excerpt } = error else {
            panic!("historical permissions cannot classify the current timeout");
        };
        assert!(output_excerpt.contains("No subscription"));
        assert!(output_excerpt.contains("Timeout"));
    }

    #[test]
    fn missing_decryption_key_keeps_its_existing_specific_variant() {
        let mut result = output(true, true);
        result.excerpt = String::from("ERROR! Download item 3816899969 failed (Access Denied).");
        result.workshop_log_excerpt = Some(String::from(
            "[AppID 1623730] Update canceled: Failed to initialize depot 1623730, manifest 42 (Missing decryption key) (Missing decryption key)",
        ));
        assert!(matches!(
            verify_command_result(1_623_730, None, &result),
            Err(SteamCmdError::WorkshopDecryptionKeyMissing { .. })
        ));
    }

    #[test]
    fn authenticated_download_failure_preserves_the_workshop_reason_and_exit_status() {
        for missing_key in [false, true] {
            let mut result = output(false, true);
            result.authenticated_account_name = Some(String::from("test_account"));
            if missing_key {
                result.workshop_log_excerpt = Some(String::from("Missing decryption key"));
            }
            let error =
                verify_command_result(1_623_730, Some("test_account"), &result).unwrap_err();
            if missing_key {
                assert!(matches!(
                    error,
                    SteamCmdError::WorkshopDecryptionKeyMissing { .. }
                ));
            } else {
                assert!(matches!(error, SteamCmdError::SteamCmdCommandFailed { .. }));
            }
            for account in [None, Some(String::from("other_account"))] {
                result.authenticated_account_name = account;
                assert!(matches!(
                    verify_command_result(1_623_730, Some("test_account"), &result),
                    Err(SteamCmdError::SteamAccountAuthenticationFailed { .. })
                ));
            }
        }
    }
}
