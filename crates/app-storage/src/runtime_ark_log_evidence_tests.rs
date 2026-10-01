use super::*;
use app_core::ProcessIdentity;
use std::fs;
use std::io::Write;

const READY: &str = "[2026.09.30-01.02.04:001][ 1]Server is advertising for join\n";

struct Fixture {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    process: InstanceProcessState,
    keys: Vec<String>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-ark-evidence-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("ark-ascended-server.log");
        fs::write(&path, b"").unwrap();
        let process = InstanceProcessState {
            run_id: 1,
            session_id: Some("owned".into()),
            process_key: "main".into(),
            display_name: "Main".into(),
            pid: Some(100),
            process_identity: Some(ProcessIdentity {
                creation_time: 134_352_037_230_000_000,
                image_path: "SyntheticNeverExecuted.exe".into(),
            }),
            status: "running".into(),
            started_at: None,
            stopped_at: None,
            exit_code: None,
            crash_flag: false,
            log_path: None,
            is_primary: true,
        };
        Self {
            root,
            path,
            process,
            keys: vec!["main".into(), "map-extra".into()],
        }
    }

    fn observe(&self) -> bool {
        observe(&self.path, &self.process, 1, &self.keys, false).unwrap()
    }
    fn entry(&self) -> Entry {
        load(&self.root.join(".ark-readiness.json"))
            .unwrap()
            .0
            .entries["main"]
            .clone()
    }
    fn append(&self, text: &str) {
        File::options()
            .append(true)
            .open(&self.path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn ready_beyond_a_megabyte_and_4096_lines_is_found_and_retained_after_later_logs() {
    let fixture = Fixture::new();
    let loading = "[2026.09.30-01.02.03:500][ 1]Loading synthetic Mod assets and native packages\n";
    let mut text = loading.repeat(30_000);
    text.push_str(READY);
    text.push_str(&loading.repeat(300));
    fs::write(&fixture.path, text).unwrap();
    let mut previous = 0;
    let mut ready = false;
    for _ in 0..8 {
        ready = fixture.observe();
        let offset = fixture.entry().offset;
        assert!(offset > previous && offset - previous <= SCAN_BYTES);
        previous = offset;
        if ready {
            break;
        }
    }
    assert!(
        ready,
        "a marker outside prefix and tail must be reached across bounded polls"
    );
    assert!(previous > SCAN_BYTES);
    fixture.append(&loading.repeat(1000));
    assert!(
        fixture.observe(),
        "ordinary later logs cannot remove same-process readiness"
    );
    assert_eq!(
        fixture.entry().offset,
        previous,
        "an already ready process must not keep scanning ordinary logs"
    );
    assert!(
        fs::metadata(fixture.root.join(".ark-readiness.json"))
            .unwrap()
            .len()
            < STATE_BYTES
    );
}

#[test]
fn a_new_creation_token_cannot_inherit_an_earlier_runs_evidence() {
    let mut fixture = Fixture::new();
    fs::write(&fixture.path, READY).unwrap();
    assert!(fixture.observe());
    fixture.process.run_id += 1;
    fixture
        .process
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time += 600_000_000;
    assert!(!fixture.observe());
    assert!(!fixture.entry().ready);
    fixture.append("[2026.09.30-01.03.04:001][ 1]Server is advertising for join\n");
    assert!(fixture.observe());
}

#[test]
fn recent_valid_readiness_skips_catchup_and_unchanged_ready_evidence_skips_publication() {
    let fixture = Fixture::new();
    fs::write(&fixture.path, "Loading native packages\n".repeat(100_000)).unwrap();
    fixture.append(READY);
    assert!(observe(&fixture.path, &fixture.process, 1, &fixture.keys, true).unwrap());
    assert_eq!(fixture.entry().offset, 0);
    let state_path = fixture.root.join(".ark-readiness.json");
    let before = fs::read(&state_path).unwrap();
    fixture.append("Later ordinary native activity\n");
    assert!(fixture.observe());
    assert_eq!(fs::read(&state_path).unwrap(), before);
    assert_eq!(fixture.entry().offset, 0);
}

#[test]
fn corrupt_or_oversized_checkpoint_is_reported_instead_of_adopted_as_ready() {
    let fixture = Fixture::new();
    fs::write(&fixture.path, READY).unwrap();
    let state_path = fixture.root.join(".ark-readiness.json");
    for bytes in [
        b"invalid derived checkpoint".to_vec(),
        vec![b'x'; STATE_BYTES as usize + 1],
    ] {
        fs::write(&state_path, bytes).unwrap();
        assert!(observe(&fixture.path, &fixture.process, 1, &fixture.keys, false).is_err());
    }
}

#[test]
fn truncation_and_replacement_reset_only_the_cursor_of_the_same_process() {
    let fixture = Fixture::new();
    fs::write(&fixture.path, b"Loading native package\n".repeat(100_000)).unwrap();
    assert!(!fixture.observe());
    assert_eq!(fixture.entry().offset, SCAN_BYTES);
    fs::write(&fixture.path, READY).unwrap();
    assert!(
        fixture.observe(),
        "truncate/regrow must reset the cursor to fresh output"
    );
    let previous_identity = fixture.entry().file_identity;
    let replacement = fixture.root.join("replacement.log");
    fs::write(
        &replacement,
        b"new log generation, still the same OS process\n",
    )
    .unwrap();
    fs::rename(&fixture.path, fixture.root.join("previous.log")).unwrap();
    fs::rename(replacement, &fixture.path).unwrap();
    assert!(
        fixture.observe(),
        "log rotation cannot erase already observed readiness of this OS process"
    );
    let current = fixture.entry();
    assert_ne!(current.file_identity, previous_identity);
    assert_eq!(current.offset, 0);
}

#[test]
fn complete_native_lines_can_cross_poll_boundaries_but_unbounded_lines_are_skipped() {
    let fixture = Fixture::new();
    let mut text = "\n".repeat(SCAN_BYTES as usize - 10);
    text.push_str(READY);
    fs::write(&fixture.path, text).unwrap();
    assert!(!fixture.observe());
    assert!(fixture.observe());

    let mut next = fixture.process.clone();
    next.run_id += 1;
    let oversized = format!(
        "[2026.09.30-01.02.04:001]{}advertising for join\n",
        "x".repeat(MAX_LINE_BYTES as usize)
    );
    fs::write(&fixture.path, oversized).unwrap();
    assert!(!observe(&fixture.path, &next, 1, &fixture.keys, false).unwrap());
    fixture.append(READY.trim_end_matches('\n'));
    assert!(
        !observe(&fixture.path, &next, 1, &fixture.keys, false).unwrap(),
        "an incomplete line cannot establish readiness"
    );
    fixture.append("\n");
    assert!(observe(&fixture.path, &next, 1, &fixture.keys, false).unwrap());
}

#[test]
fn a_stale_cas_publication_cannot_regress_a_more_advanced_readiness_cursor() {
    let fixture = Fixture::new();
    let mut text = "Loading native packages\n".repeat(60_000);
    text.push_str(READY);
    fs::write(&fixture.path, text).unwrap();
    let ready = observe_with(
        &fixture.path,
        &fixture.process,
        1,
        &fixture.keys,
        false,
        |attempt| {
            if attempt == 0 {
                // Publish two newer inspections after this request captured its
                // old expected bytes, forcing a real atomic CAS conflict.
                assert!(!fixture.observe());
                assert!(fixture.observe());
            }
        },
    )
    .unwrap();
    assert!(ready);
    assert!(fixture.entry().ready);
    assert!(fixture.entry().offset > SCAN_BYTES);
}

#[test]
fn concurrent_map_evidence_is_preserved_and_removed_maps_are_pruned() {
    let fixture = Fixture::new();
    fs::write(&fixture.path, READY).unwrap();
    let extra_path = fixture.root.join("ark-ascended-map-extra.log");
    fs::write(&extra_path, READY).unwrap();
    let mut extra = fixture.process.clone();
    extra.process_key = "map-extra".into();
    extra.pid = Some(101);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        for (path, process) in [(&fixture.path, &fixture.process), (&extra_path, &extra)] {
            let barrier = std::sync::Arc::clone(&barrier);
            let keys = &fixture.keys;
            scope.spawn(move || {
                assert!(
                    observe_with(path, process, 1, keys, false, |attempt| {
                        if attempt == 0 {
                            barrier.wait();
                        }
                    })
                    .unwrap()
                );
            });
        }
    });
    let state_path = fixture.root.join(".ark-readiness.json");
    let (checkpoint, _) = load(&state_path).unwrap();
    assert_eq!(checkpoint.entries.len(), 2);
    assert!(checkpoint.entries.values().all(|entry| entry.ready));
    assert!(observe(&fixture.path, &fixture.process, 1, &["main".into()], false).unwrap());
    assert_eq!(load(&state_path).unwrap().0.entries.len(), 1);
}

#[test]
fn an_older_group_cannot_prune_new_maps_after_a_concurrent_group_publication() {
    let fixture = Fixture::new();
    fs::write(&fixture.path, READY).unwrap();
    assert!(fixture.observe());
    let old_extra_path = fixture.root.join("old-extra.log");
    fs::write(&old_extra_path, READY).unwrap();
    let mut old_extra = fixture.process.clone();
    old_extra.run_id = 2;
    old_extra.process_key = "map-extra".into();
    old_extra.pid = Some(101);

    let mut new_main = fixture.process.clone();
    new_main.run_id = 3;
    new_main.process_identity.as_mut().unwrap().creation_time += 10_000_000;
    let mut new_map = new_main.clone();
    new_map.run_id = 4;
    new_map.process_key = "map-new".into();
    new_map.pid = Some(102);
    let new_path = fixture.root.join("new-map.log");
    fs::write(&new_path, READY).unwrap();
    let new_keys = vec!["main".into(), "map-new".into()];
    let state_path = fixture.root.join(".ark-readiness.json");
    let mut new_bytes = None;

    let stale_ready = observe_with(
        &old_extra_path,
        &old_extra,
        1,
        &fixture.keys,
        false,
        |attempt| {
            if attempt == 0 {
                // Both newer maps publish after the old extra-map request has
                // captured its expected bytes but before its real CAS write.
                assert!(observe(&fixture.path, &new_main, 3, &new_keys, false).unwrap());
                assert!(observe(&new_path, &new_map, 3, &new_keys, false).unwrap());
                new_bytes = Some(fs::read(&state_path).unwrap());
            }
        },
    )
    .unwrap();

    assert!(!stale_ready);
    assert_eq!(fs::read(&state_path).unwrap(), new_bytes.unwrap());
    let (checkpoint, _) = load(&state_path).unwrap();
    assert_eq!(checkpoint.group_run_id, 3);
    assert_eq!(checkpoint.entries.len(), 2);
    assert!(checkpoint.entries["main"].ready);
    assert!(checkpoint.entries["map-new"].ready);
    assert!(!checkpoint.entries.contains_key("map-extra"));
    assert!(!observe(&old_extra_path, &old_extra, 1, &fixture.keys, false).unwrap());
}
