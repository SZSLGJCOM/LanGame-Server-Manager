//! Boundary fixtures for the documented SDK flow; these do not run ASA.
use super::*;
use std::io::Write;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lg-asa-cf-evidence-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> PathBuf {
        self.0.join("ark-native.log")
    }

    fn write(&self, text: impl AsRef<[u8]>) {
        fs::write(self.path(), text).unwrap();
    }

    fn append(&self, text: &str) {
        OpenOptions::new()
            .append(true)
            .open(self.path())
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn line(milliseconds: &str, message: &str) -> String {
    format!("[2026.10.17-10.00.00:{milliseconds}][  0]{message}\n")
}

fn evidence() -> Evidence {
    let cutoff = app_storage::ark_native_log_message(&line("100", "cutoff"))
        .unwrap()
        .0;
    Evidence::new(vec!["123456".into(), "654321".into()], cutoff)
}

#[test]
fn asa_curseforge_query_diagnostic_keeps_the_confirmed_cause() {
    let fixture = Fixture::new();
    fixture.write(line(
        "110",
        "LogCFCore: Error: Error querying server mods: ApiError: Failed (serverUnreachable)",
    ));
    let mut observed = evidence();
    let error = observed.read(&fixture.path()).unwrap_err();
    let diagnostic: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(diagnostic["code"], "asa_curseforge_startup_failed");
    assert_eq!(diagnostic["stage"], "query");
    assert_eq!(diagnostic["reason"], "server_unreachable");
    assert!(diagnostic["http_status"].is_null());
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("does not distinguish DNS, TLS")
    );
    assert!(!error.contains("https://"));
    assert!(!observed.is_complete());
}

#[test]
fn asa_curseforge_query_unknown_response_stays_redacted_and_does_not_guess_a_network_cause() {
    let fixture = Fixture::new();
    fixture.write(line("110", "LogCFCore: Error: Error querying server mods: ApiError: Failed https://example.invalid/?api-key=synthetic-fixture (serverUnreachable)"));
    let mut observed = evidence();
    let error = observed.read(&fixture.path()).unwrap_err();
    let diagnostic: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(diagnostic["reason"], "unknown");
    assert!(!error.contains("synthetic-fixture"));
    assert!(!error.contains("https://"));
    assert!(!observed.is_complete());
}

#[test]
fn asa_curseforge_query_diagnostic_distinguishes_access_denied_from_network_failure() {
    let fixture = Fixture::new();
    fixture.write(line("110", "LogCFCore: Error: Error querying server mods: ApiError: Missing user token or api key missing privileges to access end point"));
    let mut observed = evidence();
    let error = observed.read(&fixture.path()).unwrap_err();
    let diagnostic: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(diagnostic["reason"], "access_denied");
    assert!(!error.contains("api key"));
    assert!(!error.contains("server_unreachable"));
}

#[test]
fn asa_curseforge_download_attempt_failures_allow_native_retry_and_full_package_fallback() {
    let fixture = Fixture::new();
    fixture.write([
        line("110", "LogCFCore: Warning: Failed downloading: https://example.invalid/mod (desc=Failed (serverUnreachable), chunk retries left=2, download retries left=8)"),
        line("120", "LogCFCore: Error: Download retry budget exhausted for: https://example.invalid/delta"),
        line("120", "LogCFCore: Error: Chunk retry budget exhausted for chunk at bytes 0-65535"),
        line("120", "LogCFCore: Error: Failed to perform request: https://example.invalid/delta with status code (0): private body"),
        line("130", "LogCFCore: Mod valid: First (123456)"),
        line("130", "LogCFCore: Mod valid: Second (654321)"),
    ].concat());
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert!(observed.is_complete());
}

#[test]
fn asa_native_missing_mod_failure_is_current_exact_and_sticky() {
    let fixture = Fixture::new();
    let failure = "ASAMods: Error: Not all mods were installed. Check the log for CFCore errors.";
    fixture.write(
        [
            line("099", failure),
            line(
                "110",
                "ASAMods: Warning: Not all mods were installed. Check the log for CFCore errors.",
            ),
            line(
                "110",
                "LogOther: Error: Not all mods were installed. Check the log for CFCore errors.",
            ),
            line("110", "ASAMods: Error: Not all mods were installed."),
            line("120", "LogCFCore: Mod valid: First (123456)"),
            line("120", "LogCFCore: Mod valid: Second (654321)"),
        ]
        .concat(),
    );
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert!(observed.is_complete());
    fixture.append(&line("130", failure));
    let error = observed.read(&fixture.path()).unwrap_err();
    assert!(error.contains("could not install every requested Mod"));
    assert!(!observed.is_complete());
    fixture.write(line("140", "LogCFCore: Mod valid: First (123456)"));
    assert!(observed.read(&fixture.path()).is_err());
}

#[test]
fn asa_curseforge_evidence_requires_each_requested_final_valid_item() {
    let fixture = Fixture::new();
    fixture.write(line("110", "LogCFCore: Mod valid: First (123456)"));
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 1);
    assert!(!observed.is_complete());
    fixture.append(&line("120", "LogCFCore: Log: Mod valid: First (123456)"));
    fixture.append(&line(
        "120",
        "LogCFCore: Display: Mod valid: Not requested (777777)",
    ));
    fixture.append(&line(
        "120",
        "LogCFCore: Display: Mod valid: Name (with parentheses) (654321)",
    ));
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 2);
    assert!(observed.is_complete());
}

#[test]
fn asa_curseforge_evidence_ignores_old_logs_progress_wrong_levels_and_channels() {
    let fixture = Fixture::new();
    fixture.write(
        [
            line("099", "LogCFCore: Display: Mod valid: Old (123456)"),
            line(
                "099",
                "LogCFCore: Error: Error querying server mods: old failure",
            ),
            line(
                "110",
                "LogCFCore: Display: No need to update existing mod: Cached (123456)",
            ),
            line("110", "LogOther: Display: Mod valid: Spoofed (123456)"),
            line("110", "LogCFCore: Warning: Mod valid: Wrong level (123456)"),
            line("110", "LogCFCore: Error: Mod valid: Wrong level (123456)"),
            line(
                "110",
                "LogCFCore: Display: Mod valid: Partial (123456) extra",
            ),
            line("110", "LogCFCore: Warning: transient retry warning"),
            line(
                "110",
                "LogCFCore: Error: unrelated optional feature is unavailable",
            ),
            "LogCFCore: Display: Mod valid: No native timestamp (123456)\n".into(),
        ]
        .concat(),
    );
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 0);
    assert!(!observed.is_complete());
}

#[test]
fn asa_curseforge_evidence_preserves_partial_lines_until_the_append_finishes() {
    let fixture = Fixture::new();
    let record = line("110", "LogCFCore: Display: Mod valid: First (123456)");
    fixture.write(&record.as_bytes()[..record.len() - 5]);
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 0);
    fixture.append(&record[record.len() - 5..]);
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 1);
    assert!(observed.pending.is_empty());
}

#[test]
fn asa_curseforge_evidence_accepts_native_default_and_explicit_log_display_success() {
    for prefix in ["LogCFCore: ", "LogCFCore: Log: ", "LogCFCore: Display: "] {
        let fixture = Fixture::new();
        fixture.write(line("110", &format!("{prefix}Mod valid: First (123456)")));
        let cutoff = evidence().created_ms;
        let mut observed = Evidence::new(vec!["123456".into()], cutoff);
        observed.read(&fixture.path()).unwrap();
        assert!(observed.is_complete(), "{prefix}");
    }
}

#[test]
fn asa_curseforge_evidence_waits_until_trailing_failure_beyond_poll_budget_is_consumed() {
    let fixture = Fixture::new();
    let mut bytes = [
        line("110", "LogCFCore: Mod valid: First (123456)"),
        line("110", "LogCFCore: Mod valid: Second (654321)"),
    ]
    .concat()
    .into_bytes();
    bytes.resize(MAX_POLL_BYTES, b'\n');
    bytes.extend_from_slice(
        line(
            "120",
            "LogCFCore: Error: Error querying server mods: failed",
        )
        .as_bytes(),
    );
    fixture.write(bytes);
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 2);
    assert!(
        !observed.is_complete(),
        "unread trailing records cannot be ignored"
    );
    assert!(observed.read(&fixture.path()).is_err());
    assert!(!observed.is_complete());
}

#[test]
fn asa_curseforge_evidence_incomplete_tail_cannot_publish_complete_validation() {
    let fixture = Fixture::new();
    let mut text = [
        line("110", "LogCFCore: Mod valid: First (123456)"),
        line("110", "LogCFCore: Mod valid: Second (654321)"),
        line(
            "120",
            "LogCFCore: Error: Error querying server mods: failed",
        ),
    ]
    .concat();
    text.pop();
    fixture.write(text);
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 2);
    assert!(!observed.is_complete());
    fixture.append("\n");
    assert!(observed.read(&fixture.path()).is_err());
    assert!(!observed.is_complete());
}

#[test]
fn asa_curseforge_evidence_bounds_each_poll_and_discards_oversized_records() {
    let fixture = Fixture::new();
    let mut bytes = vec![b'x'; MAX_POLL_BYTES];
    bytes.extend_from_slice(b"\n");
    bytes
        .extend_from_slice(line("110", "LogCFCore: Display: Mod valid: First (123456)").as_bytes());
    fixture.write(bytes);
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.offset, MAX_POLL_DATA_BYTES as u64);
    assert!(observed.pending.len() <= MAX_PENDING_BYTES);
    assert_eq!(observed.valid_count(), 0);
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 1);
    assert!(observed.pending.is_empty());
}

#[test]
fn asa_curseforge_evidence_resets_after_truncation_without_accepting_a_prior_run() {
    let fixture = Fixture::new();
    fixture.write(line(
        "110",
        &format!(
            "LogCFCore: Display: Mod valid: {} (123456)",
            "Long name ".repeat(20)
        ),
    ));
    fixture.append(&line("110", "LogCFCore: Mod valid: Second (654321)"));
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 2);
    assert!(observed.is_complete());
    fixture.write(line("099", "LogCFCore: Display: Mod valid: Old (654321)"));
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 0);
    assert!(!observed.is_complete());
}

fn completed_log() -> String {
    [
        line("110", "LogCFCore: Mod valid: First (123456)"),
        line("110", "LogCFCore: Mod valid: Second (654321)"),
    ]
    .concat()
}

#[test]
fn asa_curseforge_evidence_replacement_replays_error_before_previous_cursor() {
    let fixture = Fixture::new();
    fixture.write(completed_log());
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert!(observed.is_complete());
    let prior_identity = observed.identity.clone();
    let prior_offset = observed.offset;
    let mut replacement = line(
        "120",
        "LogCFCore: Error: Error querying server mods: replacement",
    )
    .into_bytes();
    replacement.resize(prior_offset as usize + 4096, b'\n');
    let replacement_path = fixture.0.join("replacement.log");
    fs::write(&replacement_path, replacement).unwrap();
    fs::rename(replacement_path, fixture.path()).unwrap();
    assert!(fs::metadata(fixture.path()).unwrap().len() > prior_offset);
    let error = observed.read(&fixture.path()).unwrap_err();
    assert!(error.contains("native Mod metadata query failed"));
    assert_ne!(observed.identity, prior_identity);
    assert_eq!(observed.valid_count(), 0);
    assert!(!observed.is_complete());
}

#[test]
fn asa_curseforge_evidence_same_file_regrowth_checks_tail_even_when_prefix_matches() {
    let fixture = Fixture::new();
    let prefix = "unrelated native startup info\n".repeat(40);
    fixture.write(format!("{prefix}{}", completed_log()));
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert!(observed.is_complete());
    let prior_identity = observed.identity.clone();
    let prior_offset = observed.offset;
    let prior_prefix = observed.prefix.clone();
    let mut rewritten = format!(
        "{prefix}{}",
        line(
            "120",
            "LogCFCore: Error: Error querying server mods: rebuilt"
        )
    )
    .into_bytes();
    rewritten.resize(prior_offset as usize + 4096, b'\n');
    // fs::write truncates the existing file object, then regrows it completely
    // before the next poll; a length-only cursor cannot observe the truncation.
    fixture.write(&rewritten);
    assert_eq!(&rewritten[..ANCHOR_BYTES], prior_prefix.as_slice());
    let current = log_identity(&fixture.path(), &File::open(fixture.path()).unwrap()).unwrap();
    assert_eq!(Some(current), prior_identity);
    assert!(fs::metadata(fixture.path()).unwrap().len() > prior_offset);
    let error = observed.read(&fixture.path()).unwrap_err();
    assert!(error.contains("native Mod metadata query failed"));
    assert_eq!(observed.valid_count(), 0);
    assert!(!observed.is_complete());
}

#[test]
fn asa_curseforge_evidence_missing_previously_read_log_invalidates_old_completion() {
    let fixture = Fixture::new();
    fixture.write(completed_log());
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert!(observed.is_complete());
    fs::remove_file(fixture.path()).unwrap();
    observed.read(&fixture.path()).unwrap();
    assert_eq!(observed.valid_count(), 0);
    assert!(!observed.is_complete());
    assert!(observed.identity.is_none());
}

#[test]
fn asa_curseforge_evidence_terminal_failure_is_sticky_and_redacts_details() {
    for marker in [
        "Error querying server mods: https://example.invalid/?api-key=synthetic-fixture",
        "Missing installed mods - (1/2)",
        "Detected an unavailable mod: Required (123456)! Ensuring mod is deleted...",
        "Mod: Required (123456) doesn't contain it's main file (7654321)!",
    ] {
        let fixture = Fixture::new();
        fixture.write(line("110", &format!("LogCFCore: Error: {marker}")));
        let mut observed = evidence();
        let error = observed.read(&fixture.path()).unwrap_err();
        assert!(!error.contains("api-key"));
        assert!(!error.contains("https://"));
        fixture.write(line("120", "LogCFCore: Display: Mod valid: First (123456)"));
        assert!(observed.read(&fixture.path()).is_err());
        assert!(!observed.is_complete());
    }
}

#[test]
fn asa_curseforge_evidence_missing_file_is_pending_and_directory_is_rejected() {
    let fixture = Fixture::new();
    let mut observed = evidence();
    observed.read(&fixture.path()).unwrap();
    assert!(!observed.is_complete());
    assert!(observed.read(&fixture.0).is_err());
    assert!(!Evidence::new(vec![], 0).is_complete());
}
