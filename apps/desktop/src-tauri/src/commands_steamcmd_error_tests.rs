use super::*;
use std::path::PathBuf;

#[test]
fn only_confirmed_cancellation_uses_the_stopped_result() {
    assert_eq!(
        steamcmd_error_message(&SteamCmdError::InstallCancelled {
            operation: String::from("game install"),
        }),
        "installation_cancelled"
    );
    assert_ne!(
        steamcmd_error_message(&SteamCmdError::InstallProcessCleanupFailed {
            operation: String::from("game install"),
            detail: String::from("A child process is still active"),
        }),
        "installation_cancelled"
    );
}

#[test]
fn steamcmd_business_errors_keep_variant_parameters_and_original_diagnostics() {
    let module_id = "game-\"双引号\"\\path\nline";
    let path = "D:/server\\game/服务端.exe";
    let cases = [
        (
            SteamCmdError::SteamCmdNotReady { path: path.into() },
            json!({"code": "steamcmd_not_ready", "path": path}),
        ),
        (
            SteamCmdError::MissingSteamCmdExecutable { path: path.into() },
            json!({"code": "steamcmd_executable_missing", "path": path}),
        ),
        (
            SteamCmdError::MissingInstalledExecutable {
                module_id: module_id.into(),
                operation: "install".into(),
                path: path.into(),
            },
            json!({"code": "installed_executable_missing", "module_id": module_id, "operation": "install", "path": path}),
        ),
        (
            SteamCmdError::InstallationVerificationFailed {
                module_id: module_id.into(),
                operation: "validate".into(),
                detail: "Missing file: \"server.exe\"\nRetry.".into(),
            },
            json!({"code": "installation_verification_failed", "module_id": module_id, "operation": "validate", "detail": "Missing file: \"server.exe\"\nRetry.", "output_excerpt": "Missing file: \"server.exe\"\nRetry."}),
        ),
        (
            SteamCmdError::OperationTimedOut {
                operation: "game server install lifecycle change",
                timeout_seconds: 900,
            },
            json!({"code": "install_operation_timed_out", "operation": "game server install lifecycle change", "timeout_seconds": 900}),
        ),
        (
            SteamCmdError::SteamCmdPreparationStalled {
                timeout_seconds: 90,
                output_excerpt: "Update complete, launching...".into(),
            },
            json!({"code": "steamcmd_preparation_stalled", "timeout_seconds": 90, "output_excerpt": "Update complete, launching..."}),
        ),
        (
            SteamCmdError::SteamCmdPreparationTimedOut {
                timeout_seconds: 900,
                output_excerpt: "Still downloading...".into(),
            },
            json!({"code": "install_operation_timed_out", "operation": "SteamCMD preparation", "timeout_seconds": 900, "output_excerpt": "Still downloading..."}),
        ),
        (
            SteamCmdError::UnmanagedSteamCmdRoot {
                path: PathBuf::from(path),
            },
            json!({"code": "steamcmd_root_unmanaged", "path": path}),
        ),
        (
            SteamCmdError::InvalidSteamCmdOwnership {
                path: PathBuf::from(path),
            },
            json!({"code": "steamcmd_ownership_invalid", "path": path}),
        ),
        (
            SteamCmdError::MissingInstallSource {
                module_id: module_id.into(),
            },
            json!({"code": "module_install_source_missing", "module_id": module_id}),
        ),
        (
            SteamCmdError::MissingInstallSpec {
                module_id: module_id.into(),
            },
            json!({"code": "module_install_spec_missing", "module_id": module_id}),
        ),
        (
            SteamCmdError::MissingProcessSpec {
                module_id: module_id.into(),
            },
            json!({"code": "module_process_spec_missing", "module_id": module_id}),
        ),
    ];

    for (error, expected) in cases {
        let payload: serde_json::Value = serde_json::from_str(&steamcmd_error_message(&error))
            .expect("business error must use the JSON contract");
        assert_eq!(payload["message"], error.to_string());
        assert_eq!(
            payload["output_excerpt"],
            json!(steamcmd_error_excerpt(&error))
        );
        for (field, expected_value) in expected.as_object().unwrap() {
            assert_eq!(
                &payload[field], expected_value,
                "parameter {field} must survive serialization"
            );
        }
    }
}

#[test]
fn steamcmd_command_failures_preserve_output_without_rewriting_it() {
    let output = "ERROR (0x12): \"network unavailable\"\nD:\\steamcmd\\日志\r\n";
    for (error, code) in [
        (
            SteamCmdError::SteamCmdCommandFailed {
                output_excerpt: output.into(),
            },
            "steamcmd_command_failed",
        ),
        (
            SteamCmdError::PrepareSteamCmd {
                output_excerpt: output.into(),
            },
            "steamcmd_prepare_failed",
        ),
        (
            SteamCmdError::DirectDownloadFailed {
                output_excerpt: output.into(),
            },
            "module_download_failed",
        ),
    ] {
        let payload: serde_json::Value = serde_json::from_str(&steamcmd_error_message(&error))
            .expect("command failure must use the JSON contract");
        assert_eq!(payload["code"], code);
        assert_eq!(payload["output_excerpt"], output);
        assert_eq!(payload["message"], error.to_string());
        assert_eq!(steamcmd_error_excerpt(&error).as_deref(), Some(output));
    }
}

#[test]
fn steamcmd_bootstrap_log_failure_keeps_the_io_reason_and_last_output() {
    let error = SteamCmdError::SteamCmdPreparationLogRead {
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Access is denied"),
        output_excerpt: "Downloading update".into(),
    };
    let payload: serde_json::Value = serde_json::from_str(&steamcmd_error_message(&error)).unwrap();
    assert_eq!(payload["code"], "steamcmd_prepare_failed");
    let excerpt = payload["output_excerpt"].as_str().unwrap();
    assert!(excerpt.contains("Access is denied"));
    assert!(excerpt.contains("Downloading update"));
}

#[test]
fn steamcmd_unmapped_os_errors_retain_their_original_context() {
    let error = SteamCmdError::CreatePath {
        path: PathBuf::from("D:/server/blocked"),
        source: std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Access is denied (os error 5)",
        ),
    };
    assert_eq!(steamcmd_error_message(&error), error.to_string());
    assert!(steamcmd_error_message(&error).contains("D:/server/blocked"));
    assert!(steamcmd_error_excerpt(&error).is_none());
}
