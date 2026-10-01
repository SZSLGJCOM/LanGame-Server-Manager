use super::*;

const READY: &str = "[LanGame native control] world_ready=True";
const NOT_READY: &str = "[LanGame native control] world_ready=False";
const STARTING: &str = "Starting dedicated server";
const STOPPING: &str = "Shutdown.";

struct Fixture {
    root: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-theforest-health-{}", uuid::Uuid::new_v4()));
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
            "theforest",
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
fn runtime_health_theforest_bootstrap_running_does_not_prove_world_operable() {
    let fixture = Fixture::new();
    fixture.write("Starting dedicated server\nDedicated Server Running\n");
    assert_eq!(
        fixture.health().status,
        "starting",
        "Run 128 reported Dedicated Server Running while SceneTracker.FinishGameLoad was false"
    );
}

#[test]
fn runtime_health_theforest_requires_exact_world_ready_signal() {
    let fixture = Fixture::new();
    for content in [
        "",
        STARTING,
        "Connected to Steam successfully",
        "listening on port 27015",
        "Game Activation Sequence step 0 (GameStartType=New)",
        "[LanGame startup] Dedicated Server Running",
        "Server name: Dedicated Server Running",
        "Dedicated Server Running failed",
        "Dedicated Server Running",
        "[LanGame native control] ready; commands: help, status, save, shutdown",
        "[LanGame native control] world_ready=True serialization_suspended=False",
        "[LanGame native control] world_ready=true",
        "[LanGame native control] world_ready=True failed",
        "[LanGame startup] [LanGame native control] world_ready=True",
        "Server name: [LanGame native control] world_ready=True",
    ] {
        fixture.write(&format!("{content}\n"));
        assert_eq!(fixture.health().status, "starting", "{content}");
    }
    fixture.write(&format!(
        "{STARTING}\nConnected to Steam successfully\n{READY}\n"
    ));
    let health = fixture.health();
    assert_eq!(health.status, "ready");
    assert_eq!(health.reason.code, "ready_signal");
    assert_eq!(health.matched_line.as_deref(), Some(READY));
}

#[test]
fn runtime_health_theforest_retains_ready_outside_tail_and_observes_shutdown() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{STARTING}\n{READY}\n{}",
        "RenderTexture.Create failed: format unsupported - 2.\n".repeat(500)
    ));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "{STOPPING}\n{}",
        "Unloading unused assets\n".repeat(200)
    ));
    let health = fixture.health();
    assert_eq!(health.status, "idle");
    assert_eq!(health.reason.code, "stopping");
    assert_eq!(health.matched_line.as_deref(), Some(STOPPING));
    fixture.append(&format!("{STARTING}\n"));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn runtime_health_theforest_revokes_ready_when_native_world_becomes_inoperable() {
    let fixture = Fixture::new();
    fixture.write(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "{NOT_READY}\n{}",
        "Periodic diagnostic\n".repeat(500)
    ));
    let health = fixture.health();
    assert_eq!(health.status, "starting");
    assert_eq!(health.matched_line.as_deref(), Some(NOT_READY));
    assert_eq!(
        fixture.health().status,
        "starting",
        "The persisted false observation must revoke prior readiness"
    );
    fixture.append("Dedicated Server Running\n");
    assert_eq!(
        fixture.health().status,
        "starting",
        "The old bootstrap banner cannot restore readiness"
    );
    fixture.append(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn runtime_health_theforest_bounded_catchup_must_reach_the_latest_event() {
    let fixture = Fixture::new();
    fixture.write(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "{}{STARTING}\n",
        "Periodic diagnostic\n".repeat(60_000)
    ));
    assert_eq!(
        fixture.health().status,
        "starting",
        "An unread suffix may revoke previously observed readiness"
    );
    let health = fixture.health();
    assert_eq!(health.status, "starting");
    assert_eq!(health.matched_line.as_deref(), Some(STARTING));
    fixture.append(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn runtime_health_theforest_rejects_incomplete_output_and_replaced_logs() {
    let fixture = Fixture::new();
    fixture.write(READY);
    assert_eq!(fixture.health().status, "starting");
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    fixture.write(&format!("{STARTING}\n"));
    assert_eq!(
        fixture.health().status,
        "starting",
        "A truncated/replaced file cannot inherit readiness"
    );
    fixture.append(&format!("{READY}\nFatal error: simulated native failure\n"));
    assert_eq!(
        fixture.health().status,
        "error",
        "Fatal output retains priority over the ready marker"
    );
}

#[test]
fn runtime_health_theforest_rejects_foreign_sources_and_previous_runs() {
    let mut fixture = Fixture::new();
    fixture.write(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
    let mut tail = read_log_snapshot(fixture.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
    tail.source_path = Some(
        fixture
            .root
            .join("native-shared.log")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(
        analyze_runtime_health_with_startup_evidence(
            "theforest",
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
            "theforest",
            &InstanceStatus::Running,
            &tail,
            None,
            &[]
        )
        .status,
        "starting"
    );
    fixture.run.run_id += 1;
    fixture.run.log_path = Some(
        fixture
            .root
            .join("run-second-main.log")
            .to_string_lossy()
            .into_owned(),
    );
    fixture.write("Connected to Steam successfully\n");
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn runtime_health_theforest_retains_observed_readiness_across_managed_rotation() {
    let mut fixture = Fixture::new();
    fixture.run.log_path = Some(
        fixture
            .root
            .join("managed-console/run-1-main.log")
            .to_string_lossy()
            .into_owned(),
    );
    let path = fixture.run.log_path.as_ref().unwrap();
    let writer = crate::managed_console_log::ManagedConsoleLog::open(path).unwrap();
    writer.write_all(format!("{READY}\n").as_bytes()).unwrap();
    assert_eq!(fixture.health().status, "ready");
    let ordinary = "Periodic stats \n".repeat(65_536); // Exactly one 1 MiB scan.
    for _ in 0..33 {
        writer.write_all(ordinary.as_bytes()).unwrap();
        assert_eq!(fixture.health().status, "ready");
    }
    let segments = crate::managed_console_log::open_log_segments(Path::new(path))
        .unwrap()
        .unwrap();
    assert!(
        segments[0].start_offset > 0,
        "The original ready marker expired under retention"
    );
    drop(segments);
    writer
        .write_all(format!("{STOPPING}\n").as_bytes())
        .unwrap();
    assert_eq!(fixture.health().status, "idle");
    drop(writer);
}
