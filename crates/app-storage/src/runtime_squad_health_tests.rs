use super::*;

const WORLD_READY: &str = "[2026.10.01-07.14.01:312][  0]LogGameState: Match State Changed from EnteringMap to WaitingToStart";
const SESSION_READY: &str = "[2026.10.01-07.14.04:732][103]LogSquadOnlineServices: Session created: Started USQOnlineServicesUpdateSessionManager updates";
const MAP_LOADING: &str = "[2026.10.01-07.13.55:000][  0]LogLoad: LoadMap: /Al_Basrah/Maps/Gameplay_Layers/AlBasrah_AAS_v1?Name=Player";

struct Fixture {
    root: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-squad-health-{}", uuid::Uuid::new_v4()));
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
            "squad",
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
fn runtime_health_squad_requires_current_world_and_online_session() {
    let fixture = Fixture::new();
    for content in [
        format!("{WORLD_READY}\n"),
        format!("{SESSION_READY}\n"),
        format!("LogInit: Game Engine Initialized.\n{SESSION_READY}\n"),
        format!("[LanGame startup] {WORLD_READY}\n{SESSION_READY}\n"),
    ] {
        fixture.write(&content);
        assert_eq!(fixture.health().status, "starting");
    }
    fixture.write(&format!("{MAP_LOADING}\n{WORLD_READY}\n{SESSION_READY}\n"));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn runtime_health_squad_retains_both_stages_but_later_map_load_revokes_them() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{WORLD_READY}\n{}{SESSION_READY}\n{}",
        "LogNet: periodic diagnostic\n".repeat(200),
        "LogNet: periodic diagnostic\n".repeat(200)
    ));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "{MAP_LOADING}\n{}",
        "LogNet: periodic diagnostic\n".repeat(200)
    ));
    assert_eq!(
        fixture.health().status,
        "starting",
        "A later map load invalidates earlier startup evidence"
    );
    fixture.append(&format!("{WORLD_READY}\n"));
    assert_eq!(
        fixture.health().status,
        "starting",
        "A previous map's online session is not proof for a new map"
    );
    fixture.append(&format!("{SESSION_READY}\n"));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn runtime_health_squad_rejects_foreign_sources_new_runs_and_incomplete_output() {
    let mut fixture = Fixture::new();
    fixture.write(&format!("{WORLD_READY}\n{SESSION_READY}"));
    assert_eq!(fixture.health().status, "starting");
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    let mut tail = read_log_snapshot(fixture.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
    tail.source_path = Some(
        fixture
            .root
            .join("SquadGame.log")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(
        analyze_runtime_health_with_startup_evidence(
            "squad",
            &InstanceStatus::Running,
            &tail,
            Some(&fixture.run),
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
    fixture.write(&format!("{SESSION_READY}\n"));
    assert_eq!(fixture.health().status, "starting");
}
