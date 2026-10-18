use super::*;
use serde_json::{Value, json};

fn value(error: &str) -> Value {
    serde_json::from_str(error).unwrap()
}

#[test]
fn verified_cache_diagnostic_does_not_claim_the_native_game_can_start_or_is_current() {
    let error = Failure::Query(Cause::from_native("ApiError: Failed (serverUnreachable)")).error();
    let error = with_cache_verification(error, true, true);
    let parsed = value(&error);
    assert_eq!(parsed["reason"], "server_unreachable");
    assert_eq!(parsed["cache_verified"], true);
    assert_eq!(parsed["native_process_exited"], true);
    assert!(readable(&error).contains("not confirmed latest"));
    assert!(readable(&error).contains("cannot bypass"));
    assert!(readable(&error).contains("has exited"));
    let error = with_cache_verification(error, false, false);
    assert_eq!(value(&error)["cache_verified"], false);
    assert!(readable(&error).contains("lack integrity metadata"));
    assert_eq!(
        with_cache_verification("unrelated".into(), true, true),
        "unrelated"
    );
}

#[test]
fn asa_curseforge_failure_classifies_verified_sdk_descriptions_and_fixed_http_statuses() {
    for (description, expected) in [
        (
            "ApiError: Failed (serverUnreachable)",
            Reason::ServerUnreachable,
        ),
        ("ApiError: Failed to parse JSON response", Reason::Response),
        ("ApiError: Request Cancelled", Reason::Cancelled),
        ("ApiError: User token has expired", Reason::Authentication),
        (
            "ApiError: Missing user token or api key missing privileges to access end point",
            Reason::AccessDenied,
        ),
        ("ApiError: The resource expired", Reason::Expired),
    ] {
        let cause = Cause::from_native(description);
        assert_eq!(cause.reason, expected, "{description}");
        assert!(cause.http_status.is_none());
    }
    for status in [400, 404, 429, 503] {
        let cause = Cause::from_native(&format!(
            "ApiError: Failed to send request to server with error: {status}"
        ));
        assert_eq!(cause.reason, Reason::Http);
        assert_eq!(cause.http_status, Some(status));
    }
}

#[test]
fn asa_curseforge_failure_unknown_bodies_do_not_become_network_or_key_advice() {
    for description in [
        "https://example.invalid/?api-key=secret",
        "ApiError: arbitrary backend message containing HTTP 403",
        "ApiError: Failed to send request to server with error: 0",
        "ApiError: Failed to send request to server with error: 600",
        "ApiError: Failed to send request to server with error: 403 signed-url",
        "ApiError: unknown (serverUnreachable) trailing text",
        "ApiError: Failed https://example.invalid/?api-key=secret (serverUnreachable)",
        "ApiError: service message (tokenExpired)",
        "ApiError: service message (missingPrivileges)",
        "ApiError: service message (badRequest)",
        "ApiError: service message (entityNotFound)",
        "ApiError: service message (resourceExpired)",
        "ApiError: service message (failedToParseServerResponse)",
        "ApiError: service message (cancelled)",
    ] {
        let error = Failure::Query(Cause::from_native(description)).error();
        let parsed = value(&error);
        assert_eq!(parsed["reason"], "unknown");
        assert!(parsed["http_status"].is_null());
        assert!(!error.contains("api-key"));
        assert!(!error.contains("secret"));
        assert!(!error.contains("https://"));
        assert!(!error.contains("HTTP 403"));
    }
}

#[test]
fn asa_curseforge_failure_context_and_cleanup_keep_one_valid_contract() {
    let error = Failure::Query(Cause::from_native("ApiError: Failed (serverUnreachable)")).error();
    let error = with_native_log(error, Path::new("D:/instance/logs/native.log"));
    let error = for_map(
        error,
        "main",
        Some("map-second"),
        Some("D:/instance/logs/console.log"),
    );
    let error = for_instance(error, "fixture");
    let error = with_cleanup_note(error, "; failed-start cleanup: map-second pid 123 remains");
    let parsed = value(&error);
    assert_eq!(parsed["code"], "asa_curseforge_startup_failed");
    assert_eq!(parsed["reason"], "server_unreachable");
    assert_eq!(parsed["instance_id"], "fixture");
    assert_eq!(parsed["map_key"], "main");
    assert_eq!(parsed["before_process_key"], "map-second");
    assert_eq!(parsed["native_log_path"], "D:/instance/logs/native.log");
    assert_eq!(
        parsed["cleanup_error"],
        "failed-start cleanup: map-second pid 123 remains"
    );
    assert_eq!(readable(&error), parsed["message"].as_str().unwrap());
    assert!(readable(&error).contains("Mod cache startup barrier"));
    assert!(readable(&error).contains("pid 123 remains"));
}

#[test]
fn asa_curseforge_failure_context_preserves_unrelated_errors_verbatim() {
    assert_eq!(
        with_native_log("file denied".into(), Path::new("a.log")),
        "file denied Native log: a.log"
    );
    assert_eq!(
        for_instance("bind denied".into(), "fixture"),
        "instance `fixture` failed strict bind-address verification: bind denied"
    );
    assert_eq!(
        for_map("other".into(), "main", None, None),
        "ASA map `main`: other"
    );
    assert_eq!(
        with_cleanup_note("other".into(), "; cleanup failed"),
        "other; cleanup failed"
    );
    assert_eq!(readable("other"), "other");
    let unknown = json!({"code":"different_error", "message":"original"}).to_string();
    assert_eq!(readable(&unknown), unknown);
}

#[test]
fn asa_curseforge_failure_terminal_install_markers_do_not_include_download_retries() {
    for marker in [
        "Failed downloading: https://example.invalid/private (desc=Failed (serverUnreachable), chunk retries left=2, download retries left=8)",
        "Download retry budget exhausted for: https://example.invalid/private",
        "Chunk retry budget exhausted for chunk at bytes 0-65535",
        "Failed to perform request: https://example.invalid/private with status code (0): private body",
    ] {
        assert!(Failure::from_native(marker).is_none());
    }
    for (marker, reason) in [
        ("Missing installed mods - (1/2)", "missing_mods"),
        (
            "Detected an unavailable mod: Required (123)! Ensuring mod is deleted...",
            "unavailable_mod",
        ),
        (
            "Mod: Required (123) doesn't contain it's main file (456)!",
            "missing_file",
        ),
    ] {
        assert_eq!(
            value(&Failure::from_native(marker).unwrap().error())["reason"],
            reason
        );
    }
}

#[test]
fn asa_curseforge_failure_validation_timeout_and_exit_have_log_context() {
    for (exited, reason) in [(false, "timeout"), (true, "process_exited")] {
        let error = validation_error(
            "Current run did not validate".into(),
            Path::new("native.log"),
            exited,
        );
        let parsed = value(&error);
        assert_eq!(parsed["stage"], "validation");
        assert_eq!(parsed["reason"], reason);
        assert_eq!(parsed["native_log_path"], "native.log");
    }
}
