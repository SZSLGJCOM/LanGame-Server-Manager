use super::*;
use std::fs;
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("native-log-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> PathBuf {
        self.0.join("native.log")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn markers() -> Vec<String> {
    vec!["ReadyToJoin value[1]".into()]
}

#[test]
fn native_log_checkpoint_detects_rewrite_regrown_before_first_poll() {
    let fixture = Fixture::new();
    let path = fixture.path();
    fs::write(&path, vec![b'a'; 128]).unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    let original_identity = baseline.identity.clone();
    let mut replacement = vec![b'a'; 192];
    let marker = markers().remove(0);
    replacement[64..64 + marker.len()].copy_from_slice(marker.as_bytes());
    fs::write(&path, replacement).unwrap();
    assert_eq!(
        LogBaseline::capture(&path).unwrap().identity,
        original_identity
    );
    // The marker is before the old offset even though the rewritten file is
    // longer. Its unchanged first 64 bytes require the tail checkpoint too.
    assert!(new_log_contains(&path, &mut baseline, &markers()).unwrap());
}

#[test]
fn native_log_keeps_reset_start_after_observed_truncation_and_regrowth() {
    let fixture = Fixture::new();
    let path = fixture.path();
    fs::write(&path, vec![b'a'; 128]).unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    let mut replacement = markers().remove(0).into_bytes();
    fs::write(&path, &replacement).unwrap();
    assert!(new_log_contains(&path, &mut baseline, &markers()).unwrap());
    replacement.resize(192, b'a');
    fs::write(&path, replacement).unwrap();
    assert!(new_log_contains(&path, &mut baseline, &markers()).unwrap());
    assert!(new_log_contains(&path, &mut baseline, &markers()).unwrap());
}

#[test]
fn native_log_identity_detects_replacement_with_unchanged_checkpoints() {
    let fixture = Fixture::new();
    let path = fixture.path();
    fs::write(&path, vec![b'a'; 256]).unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    let original_identity = baseline.identity.clone();
    fs::rename(&path, fixture.0.join("previous.log")).unwrap();
    let mut replacement = vec![b'a'; 256];
    let marker = markers().remove(0);
    replacement[100..100 + marker.len()].copy_from_slice(marker.as_bytes());
    fs::write(&path, replacement).unwrap();
    let replaced = LogBaseline::capture(&path).unwrap();
    assert_ne!(replaced.identity, original_identity);
    assert_eq!(replaced.head, baseline.head);
    assert_eq!(replaced.tail, baseline.tail);
    assert!(new_log_contains(&path, &mut baseline, &markers()).unwrap());
}

#[test]
fn native_log_never_reuses_prestart_markers_when_unchanged_or_appending() {
    let fixture = Fixture::new();
    let path = fixture.path();
    let ready = vec!["ReadyToJoin".into()];
    let value = vec!["value[1]".into()];
    let mut text = "ReadyToJoin value[1] old run\n".to_owned();
    fs::write(&path, &text).unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    assert!(!new_log_contains(&path, &mut baseline, &ready).unwrap());
    text.push_str("current run still loading\n");
    fs::write(&path, &text).unwrap();
    assert!(!new_log_contains(&path, &mut baseline, &ready).unwrap());
    assert!(!new_log_contains(&path, &mut baseline, &value).unwrap());
    text.push_str("ReadyToJoin\n");
    fs::write(&path, &text).unwrap();
    assert!(new_log_contains(&path, &mut baseline, &ready).unwrap());
    assert!(!new_log_contains(&path, &mut baseline, &value).unwrap());
    text.push_str("value[1]\n");
    fs::write(&path, text).unwrap();
    assert!(new_log_contains(&path, &mut baseline, &ready).unwrap());
    assert!(new_log_contains(&path, &mut baseline, &value).unwrap());
}
