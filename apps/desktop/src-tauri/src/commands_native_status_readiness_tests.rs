use super::*;
use std::fs::{self, FileTimes};
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lg-moria-status-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(root.join("Moria/Saved/Config")).unwrap();
        Self(root)
    }

    fn write(&self, text: &[u8], timestamp: u64) {
        let path = self.0.join(STATUS_PATH);
        fs::write(&path, text).unwrap();
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(timestamp)))
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn returntomoria_readiness_rejects_copied_running_status_until_a_rewrite() {
    let fixture = Fixture::new();
    let running = b"\xEF\xBB\xBF{\"Status\":\"running\",\"WorldName\":\"fixture-a\"}";
    fixture.write(running, 100);
    let baseline = capture(&fixture.0).unwrap();
    assert!(!ready(&fixture.0, &baseline).unwrap());
    fixture.write(running, 101);
    assert!(ready(&fixture.0, &baseline).unwrap());

    let baseline = capture(&fixture.0).unwrap();
    fixture.write(
        b"\xEF\xBB\xBF{\"Status\":\"running\",\"WorldName\":\"fixture-b\"}",
        101,
    );
    assert!(
        ready(&fixture.0, &baseline).unwrap(),
        "content fingerprint detects a rewrite even if metadata matches"
    );
}

#[test]
fn returntomoria_readiness_requires_new_valid_running_json() {
    let fixture = Fixture::new();
    let baseline = capture(&fixture.0).unwrap();
    assert!(!ready(&fixture.0, &baseline).unwrap());
    for text in [
        b"{".as_slice(),
        br#"{"Status":"loading"}"#,
        br#"{"Status":"stopped"}"#,
        br#"{"Status":"RUNNING"}"#,
        br#"{"WorldName":"fixture"}"#,
        b"[]",
    ] {
        fixture.write(text, 100);
        assert!(!ready(&fixture.0, &baseline).unwrap());
    }
    fixture.write(br#"{"Status":"running"}"#, 101);
    assert!(ready(&fixture.0, &baseline).unwrap());
}

#[test]
fn returntomoria_readiness_bounds_the_status_file_before_parsing() {
    let fixture = Fixture::new();
    let baseline = capture(&fixture.0).unwrap();
    fixture.write(&vec![b' '; MAX_STATUS_BYTES as usize + 1], 100);
    assert!(capture(&fixture.0).is_err());
    assert!(ready(&fixture.0, &baseline).is_err());
}
