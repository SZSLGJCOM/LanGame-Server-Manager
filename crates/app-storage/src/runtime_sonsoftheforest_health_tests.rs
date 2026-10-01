use super::*;

const READY: &str = "#DSL Dedicated server loaded.";
const STARTING: &str = "#DSL [Dedicated] Starting Sons of the Forest Dedicated Server...";

struct Fixture {
    root: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-sons-health-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let run = ActiveInstanceRun {
            run_id: 1,
            session_id: Some("fixture-run".into()),
            pid: None,
            log_path: Some(
                root.join("run-first-main.log")
                    .to_string_lossy()
                    .into_owned(),
            ),
            process_count: 1,
            processes: Vec::new(),
        };
        Self { root, run }
    }

    fn write(&self, content: &str) {
        fs::write(self.run.log_path.as_ref().unwrap(), content).unwrap();
    }

    fn append(&self, content: &str) {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(self.run.log_path.as_ref().unwrap())
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
    }

    fn health(&self) -> app_core::RuntimeHealth {
        let tail = read_log_snapshot(self.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
        analyze_runtime_health_with_startup_evidence(
            "sonsoftheforest",
            &InstanceStatus::Running,
            &tail,
            Some(&self.run),
            &[],
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn runtime_health_sons_requires_completed_world_not_self_tests_or_listener() {
    let fixture = Fixture::new();
    fixture.write(&format!("{STARTING}\n#DSL [Self-Tests] Self tests passed.\nlistening on port 8766\n#DSL Loading progress: 99\n"));
    assert_eq!(fixture.health().status, "starting");
    fixture.write(&format!(
        "{STARTING}\nERROR: Shader HDRP/Lit shader is not supported on this GPU\n{READY}\n"
    ));
    assert_eq!(fixture.health().status, "ready");
    assert_eq!(fixture.health().matched_line.as_deref(), Some(READY));
}

#[test]
fn runtime_health_sons_retains_ready_beyond_tail_and_revokes_on_new_start() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{READY}\n{}",
        "Unloading unused assets\n".repeat(600)
    ));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!("{STARTING}\n"));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!(
        "{READY}\nAssertion Failed: synthetic Steam failure\n"
    ));
    assert_eq!(fixture.health().status, "error");
}

#[test]
fn runtime_health_sons_requires_complete_current_run_native_line() {
    let fixture = Fixture::new();
    for content in [
        format!("[LanGame startup] {READY}\n"),
        format!("ServerName: {READY}\n"),
        format!("{READY} failure\n"),
        READY.into(),
    ] {
        fixture.write(&content);
        assert_eq!(fixture.health().status, "starting", "{content}");
    }
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    let mut tail = read_log_snapshot(fixture.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
    tail.source_path = Some(
        fixture
            .root
            .join("foreign.log")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(
        analyze_runtime_health_with_startup_evidence(
            "sonsoftheforest",
            &InstanceStatus::Running,
            &tail,
            Some(&fixture.run),
            &[]
        )
        .status,
        "starting"
    );
    assert_eq!(
        analyze_runtime_health_with_startup_evidence(
            "sonsoftheforest",
            &InstanceStatus::Running,
            &tail,
            None,
            &[]
        )
        .status,
        "starting"
    );
}

#[test]
fn runtime_health_sons_fatal_survives_tail_eviction_and_later_ready_in_same_run() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{READY}\nAssertion Failed: synthetic Steam failure\n{}",
        "Unloading unused assets\n".repeat(600)
    ));
    assert_eq!(fixture.health().status, "error");
    fixture.append(&format!(
        "{STARTING}\n{READY}\n{}",
        "Unloading unused assets\n".repeat(600)
    ));
    assert_eq!(fixture.health().status, "error");
    assert!(
        fixture
            .health()
            .matched_line
            .unwrap()
            .contains("Assertion Failed")
    );
}

#[test]
fn runtime_health_sons_new_run_and_replaced_log_clear_previous_fatal() {
    let mut fixture = Fixture::new();
    fixture.write(&format!(
        "{READY}\nAssertion Failed: synthetic Steam failure\n{}",
        "Unloading unused assets\n".repeat(600)
    ));
    assert_eq!(fixture.health().status, "error");
    // Keep the old inode/file identity alive so replacement cannot reuse it.
    let old = fixture.run.log_path.as_ref().unwrap().clone();
    fs::rename(&old, fixture.root.join("previous.log")).unwrap();
    fixture.write(&format!(
        "{READY}\n{}",
        "Unloading unused assets\n".repeat(1200)
    ));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "Fatal: another native failure\n{}",
        "Unloading unused assets\n".repeat(600)
    ));
    assert_eq!(fixture.health().status, "error");
    fixture.run.run_id += 1;
    fixture.run.session_id = Some("fixture-next-run".into());
    fixture.run.log_path = Some(
        fixture
            .root
            .join("run-next-main.log")
            .to_string_lossy()
            .into_owned(),
    );
    fixture.write(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
}
