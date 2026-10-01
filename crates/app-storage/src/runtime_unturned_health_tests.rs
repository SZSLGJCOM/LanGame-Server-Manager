use super::*;

const STEAM_READY: &str = "Steam servers ready!";
const LEVEL_READY: &str = "Loading level: 100%";

struct Fixture {
    root: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-unturned-health-{}", uuid::Uuid::new_v4()));
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
            "unturned",
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
fn runtime_health_unturned_requires_both_native_startup_stages() {
    let fixture = Fixture::new();
    for content in [
        format!("{STEAM_READY}\nLoading level: 94%\n"),
        format!("{LEVEL_READY}\n"),
        format!("[LanGame startup] {STEAM_READY}\n{LEVEL_READY}\n"),
    ] {
        fixture.write(&content);
        assert_eq!(fixture.health().status, "starting");
    }
    fixture.write(&format!(
        "{STEAM_READY}\nLoading level: 5%\n{LEVEL_READY}\n"
    ));
    let health = fixture.health();
    assert_eq!(health.status, "ready");
    assert_eq!(health.matched_line.as_deref(), Some(LEVEL_READY));
}

#[test]
fn runtime_health_unturned_preserves_stages_outside_tail_and_revokes_reloading() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{STEAM_READY}\n{}{LEVEL_READY}\n",
        "Loading asset\n".repeat(200)
    ));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "Loading level: 5%\n{}",
        "Loading asset\n".repeat(200)
    ));
    assert_eq!(
        fixture.health().status,
        "starting",
        "Old startup head cannot override a later reload"
    );
    fixture.append(&format!("{LEVEL_READY}\n"));
    assert_eq!(fixture.health().status, "ready");
    fixture.write("Loading level: 5%\n");
    assert_eq!(
        fixture.health().status,
        "starting",
        "Rewritten log cannot inherit Steam readiness"
    );
}

#[test]
fn runtime_health_unturned_bounded_catchup_and_new_run_do_not_reuse_prior_ready() {
    let mut fixture = Fixture::new();
    fixture.write(&format!(
        "{STEAM_READY}\n{}{LEVEL_READY}\n",
        "Loading asset\n".repeat(90_000)
    ));
    assert_eq!(
        fixture.health().status,
        "starting",
        "Unread log remainder may contain a reset"
    );
    assert_eq!(fixture.health().status, "ready");
    fixture.run.run_id += 1;
    fixture.run.log_path = Some(
        fixture
            .root
            .join("run-second-main.log")
            .to_string_lossy()
            .into_owned(),
    );
    fixture.write(&format!("{LEVEL_READY}\n"));
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn runtime_health_unturned_current_steam_disconnect_revokes_completed_level() {
    for disconnected in [
        "Lost connection to Steam servers because NoConnection",
        "Failed to connect to Steam servers because NoConnection, still retrying",
        "Failed to connect to Steam servers because NoConnection, no longer retrying",
        "Waiting for Steam servers...",
    ] {
        let fixture = Fixture::new();
        fixture.write(&format!("{STEAM_READY}\n{LEVEL_READY}\n"));
        assert_eq!(fixture.health().status, "ready");
        fixture.append(&format!(
            "{disconnected}\n{}",
            "Steam diagnostics\n".repeat(200)
        ));
        let health = fixture.health();
        assert_eq!(health.status, "starting");
        assert_eq!(health.matched_line.as_deref(), Some(disconnected));
        fixture.append(&format!("{STEAM_READY}\n"));
        assert_eq!(
            fixture.health().status,
            "ready",
            "A completed world may reconnect to Steam"
        );
    }
}

#[test]
fn runtime_health_unturned_rejects_incomplete_lines_foreign_sources_and_runtime_failures() {
    let fixture = Fixture::new();
    fixture.write(&format!("{STEAM_READY}\n{LEVEL_READY}"));
    assert_eq!(fixture.health().status, "starting");
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    let mut tail = read_log_snapshot(fixture.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
    tail.source_path = Some(
        fixture
            .root
            .join("other-run.log")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(
        analyze_runtime_health_with_startup_evidence(
            "unturned",
            &InstanceStatus::Running,
            &tail,
            Some(&fixture.run),
            &[]
        )
        .status,
        "starting"
    );
    fixture.append("Fatal error: simulated failure\n");
    assert_eq!(fixture.health().status, "error");
}
