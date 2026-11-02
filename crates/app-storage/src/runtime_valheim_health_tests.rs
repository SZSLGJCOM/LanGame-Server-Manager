use super::super::{analyze_runtime_health_with_startup_evidence, read_log_snapshot};
use super::*;
use app_core::{ActiveInstanceRun, InstanceStatus};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const OPENED: &str = "09/08/2026 16:50:28: Opened PlayFab server\n";
const ACTIVE: &str = "09/08/2026 16:50:29: Session \"Private Realm\" with join code 758304 and IP 192.0.2.8:2456 is active with 0 player(s)\n";
const NOISE: &str = "09/08/2026 16:50:30: Periodic world diagnostics\n";

struct Fixture {
    root: PathBuf,
    log: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-valheim-health-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let log = root.join("run-main.log");
        let run = ActiveInstanceRun {
            run_id: 41,
            session_id: Some("fixture".into()),
            pid: None,
            log_path: Some(log.to_string_lossy().into_owned()),
            process_count: 1,
            processes: Vec::new(),
        };
        Self { root, log, run }
    }
    fn write(&self, text: &str) {
        fs::write(&self.log, text).unwrap();
    }
    fn append(&self, text: &str) {
        fs::OpenOptions::new()
            .append(true)
            .open(&self.log)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    fn health(&self) -> app_core::RuntimeHealth {
        self.health_for(&self.run)
    }
    fn health_for(&self, run: &ActiveInstanceRun) -> app_core::RuntimeHealth {
        let tail = read_log_snapshot(Some(self.log.to_string_lossy().into_owned()), 160);
        analyze_runtime_health_with_startup_evidence(
            "valheim",
            &InstanceStatus::Running,
            &tail,
            Some(run),
            &[],
        )
    }
    fn checkpoint(&self) -> String {
        fs::read_to_string(self.root.join(".valheim-session.json")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn valheim_health_latest_revocation_cannot_be_overwritten_by_startup_head() {
    for revoke in [
        "Deactivated PlayFab lobby private-lobby",
        "Left PlayFab lobby private-lobby",
        "ZPlayFabMatchmaking::UnregisterServer - unregistering server now. State: Creating",
        "PlayFab reconnect server 'Private Realm'",
        "Unregister PlayFab server \"Private Realm\" and leaving network \"private-network\"",
    ] {
        let fixture = Fixture::new();
        fixture.write(&format!(
            "{OPENED}{ACTIVE}{}09/08/2026 16:50:31: {revoke}\n",
            NOISE.repeat(5_000)
        ));
        assert_eq!(fixture.health().status, "starting", "{revoke}");
        fixture.append(&NOISE.repeat(500));
        assert_eq!(
            fixture.health().status,
            "starting",
            "revocation outside tail"
        );
        fixture.append(ACTIVE);
        assert_eq!(
            fixture.health().status,
            "ready",
            "later native reactivation"
        );
    }
}

#[test]
fn valheim_health_late_ready_is_found_beyond_head_limits_and_retained() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{}{OPENED}{ACTIVE}{}",
        NOISE.repeat(30_000),
        NOISE.repeat(500)
    ));
    assert_eq!(
        fixture.health().status,
        "starting",
        "scan has not caught up"
    );
    assert_eq!(
        fixture.health().status,
        "ready",
        "late final markers consumed"
    );
    fixture.append(&NOISE.repeat(500));
    assert_eq!(
        fixture.health().status,
        "ready",
        "ready survives tail aging"
    );
}

#[test]
fn valheim_health_unread_suffix_prevents_publishing_cached_ready() {
    let fixture = Fixture::new();
    fixture.write(&format!("{OPENED}{ACTIVE}"));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "{}Left PlayFab lobby private-lobby\n{}",
        NOISE.repeat(30_000),
        NOISE.repeat(500)
    ));
    assert_eq!(
        fixture.health().status,
        "starting",
        "old cached ready is not returned while behind"
    );
    assert_eq!(
        fixture.health().status,
        "starting",
        "latest revocation is retained"
    );
}

#[test]
fn valheim_health_checkpoint_contains_no_join_code_or_log_fragments() {
    let fixture = Fixture::new();
    fixture.write(&format!("{OPENED}{}", ACTIVE.trim_end()));
    assert_eq!(
        fixture.health().status,
        "starting",
        "partial active record is not evidence"
    );
    for private in [
        "758304",
        "Private Realm",
        "192.0.2.8",
        "Session",
        "join code",
    ] {
        assert!(
            !fixture.checkpoint().contains(private),
            "partial record leaked {private}"
        );
    }
    fixture.append("\n");
    let health = fixture.health();
    assert_eq!(health.status, "ready");
    assert!(health.matched_line.is_none());
    let checkpoint = fixture.checkpoint();
    for private in [
        "758304",
        "Private Realm",
        "192.0.2.8",
        "Session",
        "join code",
    ] {
        assert!(
            !checkpoint.contains(private),
            "anchor or transition leaked {private}"
        );
    }
    let state: serde_json::Value = serde_json::from_str(&checkpoint).unwrap();
    assert_eq!(state["offset"], fs::metadata(&fixture.log).unwrap().len());
    assert!(state.get("partial").is_none());
    assert!(state.get("anchor").is_none());
}

#[test]
fn valheim_health_run_and_file_replacement_do_not_reuse_previous_state() {
    let fixture = Fixture::new();
    fixture.write(&format!("{OPENED}{ACTIVE}"));
    assert_eq!(fixture.health().status, "ready");
    let replacement = fixture.root.join("replacement.log");
    fs::write(&replacement, NOISE).unwrap();
    fs::remove_file(&fixture.log).unwrap();
    fs::rename(replacement, &fixture.log).unwrap();
    assert_eq!(fixture.health().status, "starting");
    let mut next = fixture.run.clone();
    next.run_id += 1;
    next.log_path = Some(
        fixture
            .root
            .join("next-run.log")
            .to_string_lossy()
            .into_owned(),
    );
    fs::write(next.log_path.as_ref().unwrap(), NOISE).unwrap();
    assert_eq!(
        observe(Path::new(next.log_path.as_deref().unwrap()), next.run_id)
            .unwrap()
            .status,
        "starting"
    );
    fixture.write(&format!("{OPENED}{ACTIVE}"));
    assert_eq!(
        fixture.health().status,
        "starting",
        "stale request cannot replace a newer run"
    );
}

#[test]
fn valheim_health_fatal_remains_after_tail_aging_and_later_ready() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{OPENED}{ACTIVE}FATAL: current process failed\n{}",
        NOISE.repeat(500)
    ));
    assert_eq!(fixture.health().status, "error");
    fixture.append(&format!("{OPENED}{ACTIVE}"));
    assert_eq!(fixture.health().status, "error");
}
