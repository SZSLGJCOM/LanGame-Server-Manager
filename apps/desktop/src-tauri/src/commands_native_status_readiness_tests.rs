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
        self.write_at(text, UNIX_EPOCH + Duration::from_secs(timestamp));
    }

    fn write_at(&self, text: &[u8], modified: SystemTime) {
        write_file(&self.0.join(STATUS_PATH), text, modified);
    }

    fn replace_at(&self, text: &[u8], modified: SystemTime) {
        let path = self.0.join(STATUS_PATH);
        let next = path.with_file_name("Status.next.json");
        write_file(&next, text, modified);
        // Same-volume rename atomically replaces the directory entry while the
        // reader's original handle remains valid, including on Windows.
        fs::rename(next, path).unwrap();
    }
}

fn write_file(path: &Path, text: &[u8], modified: SystemTime) {
    fs::write(path, text).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
}

fn process(created_unix_ticks: u64) -> ProcessIdentity {
    ProcessIdentity {
        creation_time: WINDOWS_UNIX_EPOCH_TICKS + created_unix_ticks,
        image_path: "MoriaServer.exe".into(),
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
    assert!(ready_for_process(&fixture.0, &baseline, &process(0)).is_err());
}

#[test]
fn atomic_replacement_cannot_publish_the_old_handles_running_value() {
    for replacement in [br#"{"Status":"stopped"}"#, br#"{"Status":"running"}"#] {
        let fixture = Fixture::new();
        let modified = UNIX_EPOCH + Duration::from_secs(101);
        fixture.write_at(br#"{"Status":"running"}"#, modified);
        let old = File::open(fixture.0.join(STATUS_PATH)).unwrap();
        let old_identity = log_identity(&fixture.0.join(STATUS_PATH), &old).unwrap();
        let observed = read_snapshot_with(&fixture.0, || {
            fixture.replace_at(replacement, modified);
        })
        .unwrap();
        assert!(matches!(observed, Observation::Updating));
        let current = File::open(fixture.0.join(STATUS_PATH)).unwrap();
        assert_ne!(
            old_identity,
            log_identity(&fixture.0.join(STATUS_PATH), &current).unwrap()
        );
        assert_eq!(
            old.metadata().unwrap().len(),
            current.metadata().unwrap().len()
        );
        assert_eq!(
            old.metadata().unwrap().modified().unwrap(),
            current.metadata().unwrap().modified().unwrap()
        );
    }
}

#[test]
fn a_disappearing_current_path_is_pending_even_when_its_old_handle_is_readable() {
    let fixture = Fixture::new();
    fixture.write(br#"{"Status":"running"}"#, 100);
    let observed = read_snapshot_with(&fixture.0, || {
        fs::rename(fixture.0.join(STATUS_PATH), fixture.0.join("retired.json")).unwrap();
    })
    .unwrap();
    assert!(matches!(observed, Observation::Updating));
    assert!(matches!(
        read_snapshot(&fixture.0).unwrap(),
        Observation::Missing
    ));
}

#[test]
fn in_place_updates_after_reading_invalidate_the_observed_running_state() {
    let fixture = Fixture::new();
    fixture.write(br#"{"Status":"running"}"#, 100);
    let observed = read_snapshot_with(&fixture.0, || {
        fixture.write(br#"{"Status":"loading"}"#, 101);
    })
    .unwrap();
    assert!(matches!(observed, Observation::Updating));
    let Observation::Present(current) = read_snapshot(&fixture.0).unwrap() else {
        panic!("stable replacement should have a readable snapshot");
    };
    assert!(!current.running);
}

#[test]
fn partial_rewrites_stay_pending_until_complete_bom_json_is_available() {
    let fixture = Fixture::new();
    let baseline = capture(&fixture.0).unwrap();
    fixture.write(br#"{"Status":"running"}"#, 100);
    let observed = read_snapshot_with(&fixture.0, || fixture.write(b"{", 101)).unwrap();
    assert!(matches!(observed, Observation::Updating));
    let identity = process(101 * 10_000_000);
    assert!(!ready_for_process(&fixture.0, &baseline, &identity).unwrap());
    fixture.write(b"\xEF\xBB\xBF{\"Status\":\"running\"}", 102);
    assert!(ready_for_process(&fixture.0, &baseline, &identity).unwrap());
}

#[test]
fn process_readiness_rejects_a_newly_copied_old_running_file_without_rounding_time() {
    let fixture = Fixture::new();
    fixture.write(br#"{"Status":"loading"}"#, 99);
    let baseline = capture(&fixture.0).unwrap();
    let identity = process(100 * 10_000_000 + 1);
    let before_creation = UNIX_EPOCH + Duration::from_secs(100);
    fixture.replace_at(br#"{"Status":"running"}"#, before_creation);
    assert!(ready(&fixture.0, &baseline).unwrap());
    assert!(!ready_for_process(&fixture.0, &baseline, &identity).unwrap());
    fixture.write_at(
        br#"{"Status":"running"}"#,
        before_creation + Duration::from_nanos(100),
    );
    assert!(ready_for_process(&fixture.0, &baseline, &identity).unwrap());
}

#[test]
fn process_readiness_requires_both_baseline_change_and_nonfuture_writer_time() {
    let fixture = Fixture::new();
    let identity = process(100 * 10_000_000);
    fixture.write(br#"{"Status":"running"}"#, 101);
    let baseline = capture(&fixture.0).unwrap();
    assert!(!ready_for_process(&fixture.0, &baseline, &identity).unwrap());
    fixture.replace_at(
        br#"{"Status":"running"}"#,
        SystemTime::now() + Duration::from_secs(24 * 60 * 60),
    );
    assert!(!ready_for_process(&fixture.0, &baseline, &identity).unwrap());
    fixture.write(br#"{"Status":"running"}"#, 102);
    assert!(ready_for_process(&fixture.0, &baseline, &identity).unwrap());
}

#[test]
fn invalid_process_creation_and_real_io_failures_are_explicit_errors() {
    let fixture = Fixture::new();
    let baseline = capture(&fixture.0).unwrap();
    let invalid = ProcessIdentity {
        creation_time: 0,
        image_path: "MoriaServer.exe".into(),
    };
    assert!(
        ready_for_process(&fixture.0, &baseline, &invalid)
            .unwrap_err()
            .contains("process creation token")
    );
    fs::create_dir(fixture.0.join(STATUS_PATH)).unwrap();
    assert!(ready(&fixture.0, &baseline).is_err());
    assert!(ready_for_process(&fixture.0, &baseline, &process(0)).is_err());
}
