use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::BootstrapLog;

struct LogFixture(PathBuf);

impl LogFixture {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "langame-bootstrap-log-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        fs::create_dir(fixture.0.join("logs")).unwrap();
        fixture
    }

    fn path(&self) -> PathBuf {
        self.0.join("logs/bootstrap_log.txt")
    }

    fn write(&self, bytes: impl AsRef<[u8]>) {
        fs::write(self.path(), bytes).unwrap();
    }

    fn append(&self, bytes: &[u8]) {
        let mut file = OpenOptions::new().append(true).open(self.path()).unwrap();
        file.write_all(bytes).unwrap();
    }
}

impl Drop for LogFixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("failed to remove bootstrap log fixture: {error}");
        }
    }
}

#[tokio::test]
async fn before_spawn_skips_old_content_and_reads_appended_chinese() {
    let fixture = LogFixture::new();
    fixture.write("旧日志：更新完成\n");
    let mut log = BootstrapLog::before_spawn(&fixture.0).await.unwrap();
    let old = log.poll().await.unwrap();
    assert!(old.bytes.is_empty());
    assert!(!old.activity);
    assert!(!old.reset);

    fixture.append("正在下载更新：中文日志\n".as_bytes());
    let update = log.poll().await.unwrap();
    assert_eq!(
        std::str::from_utf8(&update.bytes).unwrap(),
        "正在下载更新：中文日志\n"
    );
    assert!(update.activity);
    assert!(!update.reset);
    assert!(!log.poll().await.unwrap().activity);
}

#[tokio::test]
async fn utf8_across_read_boundaries_stays_complete_and_each_poll_is_bounded() {
    let fixture = LogFixture::new();
    let mut log = BootstrapLog::before_spawn(&fixture.0).await.unwrap();
    let expected = format!("{}{}", "a".repeat(4095), "😀中".repeat(3000));
    fixture.write(&expected);

    let first = log.poll().await.unwrap();
    assert_eq!(first.bytes, vec![b'a'; 4095]);
    assert!(first.activity);
    let mut received = first.bytes;
    for _ in 0..expected.len() / 4096 + 2 {
        let update = log.poll().await.unwrap();
        assert!(update.bytes.len() <= 4099);
        assert!(!update.reset);
        let text = std::str::from_utf8(&update.bytes).unwrap();
        assert!(!text.contains('\u{fffd}'));
        received.extend_from_slice(&update.bytes);
        if !update.activity {
            break;
        }
    }
    assert_eq!(received, expected.as_bytes());
    assert!(log.poll().await.unwrap().bytes.is_empty());
}

#[tokio::test]
async fn reads_a_log_created_after_spawn() {
    let fixture = LogFixture::new();
    let mut log = BootstrapLog::before_spawn(&fixture.0).await.unwrap();
    let missing = log.poll().await.unwrap();
    assert!(missing.bytes.is_empty());
    assert!(!missing.activity);

    fixture.write("新建日志\n");
    let update = log.poll().await.unwrap();
    assert_eq!(update.bytes, "新建日志\n".as_bytes());
    assert!(update.activity);
}

#[tokio::test]
async fn truncation_restarts_reading_at_the_beginning() {
    let fixture = LogFixture::new();
    fixture.write(vec![b'a'; 128]);
    let mut log = BootstrapLog::before_spawn(&fixture.0).await.unwrap();
    fixture.write("短日志\n");

    let update = log.poll().await.unwrap();
    assert!(update.reset);
    assert!(update.activity);
    assert_eq!(update.bytes, "短日志\n".as_bytes());
}

#[tokio::test]
async fn checkpoint_detects_truncate_and_regrow_past_the_old_offset() {
    let fixture = LogFixture::new();
    fixture.write(vec![b'a'; 128]);
    let created = fs::metadata(fixture.path()).unwrap().created().ok();
    let mut log = BootstrapLog::before_spawn(&fixture.0).await.unwrap();
    let mut replacement = vec![b'a'; 192];
    // Change only the first byte of the old 64-byte checkpoint. The final
    // checkpoint bytes and the file's creation time cannot reveal this rewrite.
    replacement[64] = b'b';
    fixture.write(&replacement);
    assert_eq!(
        fs::metadata(fixture.path()).unwrap().created().ok(),
        created
    );

    let update = log.poll().await.unwrap();
    assert!(update.reset);
    assert!(update.activity);
    assert_eq!(update.bytes, replacement);
}

#[tokio::test]
async fn deletion_and_replacement_discard_incomplete_utf8_from_the_old_file() {
    let fixture = LogFixture::new();
    let mut log = BootstrapLog::before_spawn(&fixture.0).await.unwrap();
    fixture.write(&"中".as_bytes()[..2]);
    let partial = log.poll().await.unwrap();
    assert!(partial.activity);
    assert!(partial.bytes.is_empty());
    fs::remove_file(fixture.path()).unwrap();
    let missing = log.poll().await.unwrap();
    assert!(missing.reset);
    assert!(!missing.activity);
    assert!(missing.bytes.is_empty());

    fixture.write("替换日志\n");
    let update = log.poll().await.unwrap();
    assert_eq!(update.bytes, "替换日志\n".as_bytes());
    assert!(update.activity);
}
