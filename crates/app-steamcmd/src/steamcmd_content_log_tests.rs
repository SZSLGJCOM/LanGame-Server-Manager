use super::*;
use std::io::Write;

const WORKSHOP_FAILURE: &str = "[2026-10-17 13:50:44] [AppID 1623730] Update canceled: Failed to initialize depot 1623730, manifest 123456789 (Missing decryption key) (Missing decryption key)\n";

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let root = crate::tests::unique_test_root();
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(root.join("logs/content_log.txt"), bytes).unwrap();
        Self(root)
    }

    fn workshop(bytes: Option<&[u8]>) -> Self {
        let fixture = Self::new(&[]);
        if let Some(bytes) = bytes {
            std::fs::write(fixture.workshop_path(), bytes).unwrap();
        }
        fixture
    }

    fn workshop_path(&self) -> std::path::PathBuf {
        self.0.join("logs/workshop_log.txt")
    }

    fn append_workshop(&self, bytes: &[u8]) {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.workshop_path())
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[tokio::test]
async fn historical_log_reads_only_its_bounded_tail_and_retains_recent_lines() {
    let mut bytes = vec![b'x'; EXCERPT_BYTES * 3];
    bytes.extend_from_slice(b"\n");
    for index in 0..50 {
        bytes.extend_from_slice(format!("recent-{index}\n").as_bytes());
    }
    let root = Fixture::new(&bytes);
    let excerpt = read_steamcmd_content_log_excerpt(&root.0).await.unwrap();
    assert!(excerpt.starts_with(TRUNCATION_NOTICE));
    assert!(!excerpt.contains("xxxxxxxx"));
    assert!(!excerpt.contains("recent-9\n"));
    assert!(excerpt.contains("recent-10\n"));
    assert!(excerpt.ends_with("recent-49"));
    assert!(excerpt.len() <= EXCERPT_BYTES);
}

#[tokio::test]
async fn a_single_overlong_unicode_line_retains_a_bounded_valid_tail() {
    for ending in ["", "\n"] {
        let bytes = "服务器😀".repeat(EXCERPT_BYTES) + ending;
        let root = Fixture::new(bytes.as_bytes());
        let excerpt = read_steamcmd_content_log_excerpt(&root.0).await.unwrap();
        assert!(excerpt.starts_with(TRUNCATION_NOTICE));
        assert!(excerpt.ends_with("服务器😀"));
        assert!(excerpt.len() <= EXCERPT_BYTES);
    }
}

#[tokio::test]
async fn incomplete_utf8_does_not_hide_the_surrounding_error_excerpt() {
    let root = Fixture::new(b"Failed writing depot\npartial:\xe7\x8e");
    let excerpt = read_steamcmd_content_log_excerpt(&root.0).await.unwrap();
    assert!(excerpt.starts_with("Failed writing depot\n"));
    assert!(excerpt.contains("partial:"));
}

#[tokio::test]
async fn workshop_diagnostics_include_only_this_invocations_appended_records() {
    let fixture = Fixture::workshop(Some(b"historical Workshop error\n"));
    let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
    fixture.append_workshop(WORKSHOP_FAILURE.as_bytes());
    assert_eq!(
        log.finish().await.as_deref(),
        Some(WORKSHOP_FAILURE.trim_end())
    );
}

#[tokio::test]
async fn workshop_missing_key_reason_survives_later_diagnostic_lines() {
    let fixture = Fixture::workshop(Some(b"old download: Missing decryption key\n"));
    let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
    fixture.append_workshop(WORKSHOP_FAILURE.as_bytes());
    for index in 0..80 {
        fixture.append_workshop(format!("later diagnostic {index}\n").as_bytes());
    }
    let excerpt = log.finish().await.unwrap();
    assert!(excerpt.contains(WORKSHOP_FAILURE.trim_end()));
    assert!(excerpt.ends_with("later diagnostic 79"));
    assert!(!excerpt.contains("old download"));
    assert!(excerpt.len() <= EXCERPT_BYTES);
}

#[tokio::test]
async fn unchanged_and_missing_workshop_logs_have_no_diagnostics() {
    for initial in [Some(WORKSHOP_FAILURE.as_bytes()), None] {
        let fixture = Fixture::workshop(initial);
        let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
        assert_eq!(log.finish().await, None);
    }
}

#[tokio::test]
async fn workshop_log_created_during_invocation_is_read() {
    let fixture = Fixture::workshop(None);
    let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
    fixture.append_workshop(WORKSHOP_FAILURE.as_bytes());
    assert_eq!(
        log.finish().await.as_deref(),
        Some(WORKSHOP_FAILURE.trim_end())
    );
}

#[tokio::test]
async fn workshop_diagnostics_do_not_finish_a_historical_partial_record() {
    let fixture = Fixture::workshop(Some(b"old partial Workshop error"));
    let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
    fixture.append_workshop(b" continued\n");
    fixture.append_workshop(WORKSHOP_FAILURE.as_bytes());
    assert_eq!(
        log.finish().await.as_deref(),
        Some(WORKSHOP_FAILURE.trim_end())
    );

    let fixture = Fixture::workshop(Some(b"old partial Workshop error"));
    let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
    fixture.append_workshop(b" continued");
    assert_eq!(log.finish().await, None);
}

#[tokio::test]
async fn rotated_workshop_log_does_not_import_replacement_history() {
    let initial = "historical Workshop record\n".repeat(20);
    let fixture = Fixture::workshop(Some(initial.as_bytes()));
    let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
    // The new path deliberately has the same checkpoint and a longer length.
    // Keeping the original shared-delete handle prevents false new diagnostics.
    std::fs::rename(
        fixture.workshop_path(),
        fixture.workshop_path().with_extension("previous"),
    )
    .unwrap();
    fixture.append_workshop(initial.as_bytes());
    fixture.append_workshop(WORKSHOP_FAILURE.as_bytes());
    assert_eq!(log.finish().await, None);
}

#[tokio::test]
async fn workshop_truncation_rewrite_and_regrowth_do_not_import_replacement_bytes() {
    for mode in ["truncation", "same-length", "regrow"] {
        let initial = "h".repeat(WORKSHOP_FAILURE.len() + 100) + "\n";
        let fixture = Fixture::workshop(Some(initial.as_bytes()));
        let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
        let replacement = match mode {
            "truncation" => WORKSHOP_FAILURE.to_string(),
            "same-length" => format!(
                "{}{}",
                WORKSHOP_FAILURE,
                "z".repeat(initial.len() - WORKSHOP_FAILURE.len())
            ),
            _ => WORKSHOP_FAILURE.repeat(5),
        };
        std::fs::write(fixture.workshop_path(), replacement).unwrap();
        assert_eq!(log.finish().await, None, "{mode}");
    }
}

#[tokio::test]
async fn workshop_diagnostic_growth_keeps_only_a_bounded_recent_tail() {
    let fixture = Fixture::workshop(Some(b"historical Workshop error\n"));
    let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
    fixture.append_workshop(&vec![b'x'; EXCERPT_BYTES * 3]);
    fixture.append_workshop(b"\n");
    for index in 0..50 {
        fixture.append_workshop(format!("recent-{index}\n").as_bytes());
    }
    fixture.append_workshop(WORKSHOP_FAILURE.as_bytes());
    let excerpt = log.finish().await.unwrap();
    assert!(excerpt.starts_with(WORKSHOP_TRUNCATION_NOTICE));
    assert!(!excerpt.contains("historical"));
    assert!(!excerpt.contains("xxxxxxxx"));
    assert!(!excerpt.contains("recent-10\n"));
    assert!(excerpt.contains("recent-11\n"));
    assert!(excerpt.ends_with(WORKSHOP_FAILURE.trim_end()));
    assert!(excerpt.len() <= EXCERPT_BYTES);
}

#[tokio::test]
async fn workshop_unicode_record_without_a_newline_has_a_bounded_valid_tail() {
    for ending in ["", "\n"] {
        let fixture = Fixture::workshop(None);
        let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
        fixture.append_workshop(("服务器😀".repeat(EXCERPT_BYTES) + ending).as_bytes());
        let excerpt = log.finish().await.unwrap();
        assert!(excerpt.starts_with(WORKSHOP_TRUNCATION_NOTICE));
        assert!(excerpt.ends_with("服务器😀"));
        assert!(excerpt.len() <= EXCERPT_BYTES);
    }
}

#[tokio::test]
async fn unavailable_workshop_snapshot_does_not_adopt_a_later_log() {
    let fixture = Fixture::workshop(None);
    std::fs::create_dir(fixture.workshop_path()).unwrap();
    let log = WorkshopLogExcerpt::before_spawn(&fixture.0).await;
    std::fs::remove_dir(fixture.workshop_path()).unwrap();
    fixture.append_workshop(WORKSHOP_FAILURE.as_bytes());
    assert_eq!(log.finish().await, None);
}
