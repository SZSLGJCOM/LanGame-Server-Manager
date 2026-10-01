use super::*;

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let root = crate::tests::unique_test_root();
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(root.join("logs/content_log.txt"), bytes).unwrap();
        Self(root)
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
