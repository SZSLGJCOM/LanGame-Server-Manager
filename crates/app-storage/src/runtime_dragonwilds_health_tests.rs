use super::*;
use std::io::Write;

const READY: &str = "[2026.09.30-01.02.03:010][ 1]LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]";
const NOT_READY: &str = "[2026.09.30-01.02.03:020][ 1]LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[0]";
const IDLE: &str = "[2026.09.30-01.02.03:030][ 1]Periodic statistics\n";

struct Fixture {
    directory: PathBuf,
    run: ActiveInstanceRun,
    native: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "langame-dragonwilds-health-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).unwrap();
        let mut run = health_tests::dragonwilds_health_run();
        let transcript = directory
            .join("run-main.log")
            .to_string_lossy()
            .into_owned();
        run.log_path = Some(transcript.clone());
        run.processes[0].log_path = Some(transcript);
        let native = directory.join("RSDragonwilds.log");
        Self {
            directory,
            run,
            native,
        }
    }

    fn write(&self, contents: &str) {
        fs::write(&self.native, contents).unwrap();
    }

    fn append(&self, contents: &str) {
        fs::OpenOptions::new()
            .append(true)
            .open(&self.native)
            .unwrap()
            .write_all(contents.as_bytes())
            .unwrap();
    }

    fn health(&self) -> app_core::RuntimeHealth {
        self.health_for(Some(&self.run), &InstanceStatus::Running)
    }

    fn health_for(
        &self,
        run: Option<&ActiveInstanceRun>,
        status: &InstanceStatus,
    ) -> app_core::RuntimeHealth {
        let recent = read_log_snapshot(
            Some(self.native.to_string_lossy().into_owned()),
            RUNTIME_HEALTH_SCAN_LINE_LIMIT,
        );
        analyze_runtime_health_with_startup_evidence(
            "runescapedragonwilds",
            status,
            &recent,
            run,
            &[],
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

#[test]
fn dragonwilds_native_readiness_survives_tail_aging_and_observer_reentry() {
    let fixture = Fixture::new();
    fixture.write(&format!("{READY}\n{}", IDLE.repeat(500)));
    let tail = read_log_snapshot(
        Some(fixture.native.to_string_lossy().into_owned()),
        RUNTIME_HEALTH_SCAN_LINE_LIMIT,
    );
    assert!(tail.truncated);
    assert!(!tail.lines.iter().any(|line| line == READY));
    for _ in 0..3 {
        let health = fixture.health();
        assert_eq!(health.status, "ready");
        assert_eq!(health.matched_line.as_deref(), Some(READY));
        fixture.append(&IDLE.repeat(200));
    }
}

#[test]
fn dragonwilds_latest_not_ready_and_fatal_survive_tail_aging() {
    let fixture = Fixture::new();
    fixture.write(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!("{NOT_READY}\n{}", IDLE.repeat(500)));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{READY}\n{}", IDLE.repeat(500)));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "[2026.09.30-01.02.03:040][ 1]FATAL: current process crashed\n{}",
        IDLE.repeat(500)
    ));
    assert_eq!(fixture.health().status, "error");
    fixture.append(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "error");
}

#[test]
fn dragonwilds_native_rewrite_with_same_length_and_tail_resets_readiness() {
    let fixture = Fixture::new();
    fixture.write(&format!("{READY}\n{}", IDLE.repeat(500)));
    assert_eq!(fixture.health().status, "ready");
    let not_ready = READY.replace("value[1]", "value[0]");
    assert_eq!(READY.len(), not_ready.len());
    fixture.write(&format!("{not_ready}\n{}", IDLE.repeat(500)));
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn dragonwilds_native_evidence_isolated_by_run_process_and_file_generation() {
    let mut fixture = Fixture::new();
    fixture.write(&format!("{READY}\n{}", IDLE.repeat(500)));
    assert_eq!(fixture.health().status, "ready");
    let mut next = fixture.run.clone();
    next.run_id += 1;
    next.processes[0].run_id = next.run_id;
    next.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time += 20_000_000;
    assert_eq!(
        fixture
            .health_for(Some(&next), &InstanceStatus::Running)
            .status,
        "starting"
    );
    fixture.run = next;
    fixture.write("[2026.09.30-01.02.06:010][ 1]ReadyToJoin Value[1]\n");
    assert_eq!(fixture.health().status, "ready");
    fixture.write(IDLE);
    assert_eq!(fixture.health().status, "starting");
    let replacement = fixture.directory.join("replacement.log");
    fs::write(&replacement, format!("{READY}\n{}", IDLE.repeat(500))).unwrap();
    fs::remove_file(&fixture.native).unwrap();
    fs::rename(replacement, &fixture.native).unwrap();
    assert_eq!(fixture.health().status, "starting");
    assert_eq!(
        fixture.health_for(None, &InstanceStatus::Running).status,
        "starting"
    );
    let mut unknown = fixture.run.clone();
    unknown.processes[0].process_identity = None;
    assert_ne!(
        fixture
            .health_for(Some(&unknown), &InstanceStatus::Running)
            .status,
        "ready"
    );
    assert_eq!(
        fixture
            .health_for(Some(&fixture.run), &InstanceStatus::Stopped)
            .status,
        "stopped"
    );
}

#[test]
fn dragonwilds_scan_must_catch_up_before_returning_ready() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{READY}\n{}{NOT_READY}\n{}",
        IDLE.repeat(30_000),
        IDLE.repeat(500)
    ));
    assert_eq!(fixture.health().status, "starting");
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn dragonwilds_known_fatal_does_not_wait_for_scan_catch_up() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "[2026.09.30-01.02.03:001][ 1]FATAL: current process crashed\n{}{READY}\n",
        IDLE.repeat(30_000)
    ));
    assert_eq!(fixture.health().status, "error");
}

#[test]
fn dragonwilds_incomplete_and_overlong_lines_do_not_prove_ready() {
    let fixture = Fixture::new();
    fixture.write(READY);
    assert_eq!(fixture.health().status, "starting");
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    fixture.write(&format!("{} {READY}\n", "x".repeat(20_000)));
    assert_eq!(fixture.health().status, "starting");
    fixture.write("[2026.09.30-01.02.03:010][ 1]OtherFlag Value[1] ReadyToJoin Value[0]\n");
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn dragonwilds_stale_observation_cannot_replace_newer_process_evidence() {
    let fixture = Fixture::new();
    fixture.write(&format!("{READY}\n{}", IDLE.repeat(500)));
    assert_eq!(fixture.health().status, "ready");
    let mut next = fixture.run.clone();
    next.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time += 20_000_000;
    assert_eq!(
        fixture
            .health_for(Some(&next), &InstanceStatus::Running)
            .status,
        "starting"
    );
    assert_eq!(fixture.health().status, "starting");
    fixture.append("[2026.09.30-01.02.06:010][ 1]ReadyToJoin Value[1]\n");
    assert_eq!(
        fixture
            .health_for(Some(&next), &InstanceStatus::Running)
            .status,
        "ready"
    );
    let mut new_session = next.clone();
    new_session.session_id = Some("replacement-session".into());
    // A mismatched primary process does not belong to the active session.
    assert_ne!(
        fixture
            .health_for(Some(&new_session), &InstanceStatus::Running)
            .status,
        "ready"
    );
}

#[test]
fn dragonwilds_shared_native_evidence_rejects_old_or_manager_messages() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "[2026.09.30-01.02.02:999][ 1]FATAL: old run crashed\n[2026.09.30-01.02.02:999][ 1]ReadyToJoin Value[1]\n[LanGame startup] Preparing ReadyToJoin Value[1]\n{}",
        IDLE.repeat(500)
    ));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "[2026.09.30-01.02.03:001][ 1]ReadyToJoin Value[0]\n{}",
        IDLE.repeat(500)
    ));
    assert_eq!(fixture.health().status, "starting");
}

#[test]
#[ignore = "requires an explicit current native log/run fixture and permission to write its derived readiness checkpoint"]
fn dragonwilds_installed_native_readiness_probe() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        native_log: PathBuf,
        active_run: ActiveInstanceRun,
        expected_status: String,
    }
    let input = PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_HEALTH_PROBE")
            .expect("explicit probe input is required"),
    );
    assert!(input.is_absolute());
    let input: Input = serde_json::from_slice(&fs::read(input).unwrap()).unwrap();
    assert!(input.native_log.is_absolute());
    assert!(matches!(
        input.expected_status.as_str(),
        "ready" | "starting" | "error"
    ));
    let recent = read_log_snapshot(
        Some(input.native_log.to_string_lossy().into_owned()),
        RUNTIME_HEALTH_SCAN_LINE_LIMIT,
    );
    assert!(recent.read_error.is_none());
    let health = analyze_runtime_health_with_startup_evidence(
        "runescapedragonwilds",
        &InstanceStatus::Running,
        &recent,
        Some(&input.active_run),
        &[],
    );
    assert_eq!(health.status, input.expected_status);
    println!(
        "DRAGONWILDS_NATIVE_HEALTH status={} reason={} tail_lines={} truncated={}",
        health.status,
        health.reason.code,
        recent.lines.len(),
        recent.truncated
    );
}
