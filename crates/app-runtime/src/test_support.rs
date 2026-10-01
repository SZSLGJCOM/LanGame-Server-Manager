use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn unique_test_root() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let sequence = TEST_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "langame-runtime-test-{}-{stamp}-{sequence}",
        std::process::id()
    ))
}
