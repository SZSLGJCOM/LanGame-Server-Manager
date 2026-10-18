#![cfg(test)]
use super::*;
use crate::steamcmd_content_evidence::ContentLogEvidence;
use crate::steamcmd_manifest_recovery::ManifestDenial;
use crate::{SteamCmdError, workshop_download};
use std::io::Write;

const APP: u32 = 1_623_730;
const DENIED: &str = "[2026-10-17 20:42:46] CDepotDownloadMgr::BYldRequestDepotManifest(App: 1623730, Depot: 1623730, Manifest: 8195362111486240380, branch: public): Failed to get manifest request code, 'Access Denied'\n";
const WORKSHOP: &str = "[2026-10-17 20:42:45] [AppID 1623730] Download item 3817722715 requested by app\n[2026-10-17 20:42:46] [AppID 1623730] Detected workshop change (latest from server) : changed cached item 3817722715, new manifest 8195362111486240380\n[2026-10-17 20:42:46] [AppID 1623730] Update canceled: Failed downloading 1 manifests (No connection)\n[2026-10-17 20:42:46] [AppID 1623730] Download item 3817722715 result : No Connection";

fn output() -> StreamingCommandResult {
    StreamingCommandResult {
        success: true,
        exit_code: Some(0),
        excerpt: "Connecting anonymously to Steam Public...OK\nworkshop_download_item 1623730 3817722715 validate\nERROR! Download item 3817722715 failed (No Connection).quit".into(),
        content_log_excerpt: Some(DENIED.into()),
        workshop_log_excerpt: Some(WORKSHOP.into()),
        workshop_download_failed: true,
        workshop_command_evidence: Default::default(),
        app_command_evidence: Default::default(),
        authenticated_account_name: None,
        denied_manifests: vec![ManifestDenial {
            app: APP,
            depot: u64::from(APP),
            manifest: 8_195_362_111_486_240_380,
        }],
    }
}

fn assert_generic(result: &StreamingCommandResult) {
    assert!(matches!(
        workshop_download::verify_command_result(APP, None, result),
        Err(SteamCmdError::SteamCmdCommandFailed { .. })
    ));
}

#[test]
fn current_denied_manifest_explains_no_connection_even_with_zero_exit() {
    let mut result = output();
    for account in [None, Some("test_account")] {
        result.authenticated_account_name = account.map(String::from);
        let error = workshop_download::verify_command_result(APP, account, &result).unwrap_err();
        let SteamCmdError::WorkshopAccountRequired {
            consumer_app_id,
            output_excerpt,
        } = error
        else {
            panic!("expected a Workshop access failure");
        };
        assert_eq!(consumer_app_id, APP);
        assert!(output_excerpt.contains("code 0"));
        assert!(output_excerpt.contains("No Connection"));
        assert!(output_excerpt.contains("Access Denied"));
        assert!(output_excerpt.contains(DENIED.trim()));
        assert!(output_excerpt.contains(WORKSHOP));
    }
}

#[test]
fn current_workshop_failures_do_not_depend_on_a_console_error_row() {
    let mut result = output();
    result.workshop_download_failed = false;
    result.excerpt = "quit".into();
    assert!(matches!(
        workshop_download::verify_command_result(APP, None, &result),
        Err(SteamCmdError::WorkshopAccountRequired { .. })
    ));
    result.workshop_log_excerpt =
        Some("[AppID 1623730] Update canceled: Missing decryption key".into());
    assert!(matches!(
        workshop_download::verify_command_result(APP, None, &result),
        Err(SteamCmdError::WorkshopDecryptionKeyMissing { .. })
    ));
    for app in ["42", "abc", "0"] {
        result.workshop_log_excerpt = Some(format!(
            "[AppID {app}] Update canceled: Missing decryption key"
        ));
        assert!(workshop_download::verify_command_result(APP, None, &result).is_ok());
    }
}

#[test]
fn retained_permission_and_key_evidence_survives_tail_eviction_with_diagnostics() {
    for line in [
        "ERROR! Download item 3817722715 failed (Access Denied)",
        "[AppID 1623730] Download item 3817722715 result : No subscription",
        "[AppID 1623730] Update canceled: Missing decryption key",
    ] {
        let mut result = output();
        result.excerpt = "later unrelated output".into();
        result.workshop_log_excerpt = None;
        result.workshop_command_evidence.observe_line(line);
        for _ in 0..100 {
            result
                .workshop_command_evidence
                .observe_line("later diagnostic");
        }
        result.workshop_command_evidence.finish_capture();
        let error = workshop_download::verify_command_result(APP, None, &result).unwrap_err();
        assert!(matches!(
            error,
            SteamCmdError::WorkshopAccountRequired { .. }
                | SteamCmdError::WorkshopDecryptionKeyMissing { .. }
        ));
        assert!(error.to_string().contains(line));
    }
}

#[test]
fn retained_manifest_correlation_and_cache_mapping_survive_other_batch_items() {
    for seeded in [false, true] {
        let mut result = output();
        for line in WORKSHOP.lines() {
            if !seeded || !line.contains("changed cached item") {
                result.workshop_command_evidence.observe_line(line);
            }
        }
        if seeded {
            result.workshop_command_evidence.seed_manifest(
                APP,
                3_817_722_715,
                8_195_362_111_486_240_380,
            );
        }
        for item in 1..500 {
            result.workshop_command_evidence.observe_line(&format!(
                "[AppID {APP}] Download item {item} requested by app"
            ));
            result
                .workshop_command_evidence
                .observe_line(&format!("[AppID {APP}] Download item {item} result : OK"));
        }
        result.workshop_command_evidence.finish_capture();
        result.excerpt = "quit".into();
        result.workshop_log_excerpt = None;
        result.content_log_excerpt = None;
        result.workshop_download_failed = false;
        let error = workshop_download::verify_command_result(APP, None, &result).unwrap_err();
        assert!(matches!(
            error,
            SteamCmdError::WorkshopAccountRequired { .. }
        ));
        assert!(error.to_string().contains("Access Denied"));
        assert!(error.to_string().contains("3817722715"));
        assert!(error.to_string().contains("8195362111486240380"));
    }
}

#[test]
fn authoritative_capture_does_not_reimport_rotated_diagnostic_tails() {
    let mut result = output();
    result.workshop_command_evidence.finish_capture();
    result.workshop_log_excerpt =
        Some("[AppID 1623730] Update canceled: Missing decryption key".into());
    result.excerpt = "ERROR! Download item 3817722715 failed (Access Denied).".into();
    assert_generic(&result);
}

#[test]
fn unrelated_app_depot_or_manifest_cannot_explain_the_network_failure() {
    for denial in [
        ManifestDenial {
            app: 42,
            ..output().denied_manifests[0]
        },
        ManifestDenial {
            depot: 42,
            ..output().denied_manifests[0]
        },
        ManifestDenial {
            manifest: 42,
            ..output().denied_manifests[0]
        },
    ] {
        let mut result = output();
        result.denied_manifests = vec![denial];
        assert_generic(&result);
    }
    let mut result = output();
    result.denied_manifests.clear();
    assert_generic(&result);
}

#[test]
fn requested_item_manifest_and_failed_item_must_correlate_in_current_workshop_log() {
    for log in [
        None,
        Some(WORKSHOP.replace("AppID 1623730", "AppID 42")),
        Some(WORKSHOP.replace(
            "Download item 3817722715 requested",
            "Download item 42 requested",
        )),
        Some(WORKSHOP.replace("cached item 3817722715", "cached item 42")),
        Some(WORKSHOP.replace("Download item 3817722715 result", "Download item 42 result")),
        Some(WORKSHOP.replace("new manifest 8195362111486240380", "new manifest 42")),
        Some(WORKSHOP.replace("result : No Connection", "result : OK")),
        Some(WORKSHOP.replace(
            "new manifest 8195362111486240380",
            "new manifest 8195362111486240380: Access Denied",
        )),
    ] {
        let mut result = output();
        result.workshop_log_excerpt = log;
        assert_generic(&result);
    }
}

#[test]
fn successful_download_and_specific_account_or_key_errors_keep_precedence() {
    let mut result = output();
    result.workshop_download_failed = false;
    result.workshop_log_excerpt = Some(WORKSHOP.replace("result : No Connection", "result : OK"));
    assert!(workshop_download::verify_command_result(APP, None, &result).is_ok());

    let mut result = output();
    assert!(matches!(
        workshop_download::verify_command_result(APP, Some("test_account"), &result),
        Err(SteamCmdError::SteamAccountAuthenticationFailed { .. })
    ));
    result
        .workshop_log_excerpt
        .as_mut()
        .unwrap()
        .push_str("\nMissing decryption key");
    assert!(matches!(
        workshop_download::verify_command_result(APP, None, &result),
        Err(SteamCmdError::WorkshopDecryptionKeyMissing { .. })
    ));
}

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = crate::tests::unique_test_root();
        std::fs::create_dir_all(root.join("logs")).unwrap();
        Self(root)
    }

    fn append(&self, text: &str) {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.0.join("logs/content_log.txt"))
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn only_denials_appended_by_this_invocation_can_require_a_workshop_account() {
    let fixture = Fixture::new();
    fixture.append(DENIED);
    let evidence = ContentLogEvidence::before_spawn(&fixture.0).await;
    fixture.append("scheduler finished: No connection\n");
    let mut result = output();
    result.denied_manifests = evidence.finish().await;
    // The diagnostic tail and current item logs alone cannot establish when
    // the content-log denial occurred.
    assert_generic(&result);

    let evidence = ContentLogEvidence::before_spawn(&fixture.0).await;
    fixture.append(DENIED);
    result.denied_manifests = evidence.finish().await;
    assert!(matches!(
        workshop_download::verify_command_result(APP, None, &result),
        Err(SteamCmdError::WorkshopAccountRequired { .. })
    ));
}
