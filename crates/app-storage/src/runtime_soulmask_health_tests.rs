use super::*;

const GAME_STARTED: &str = "logStoreGamemode: Display: [ GAME STARTED. ]";
const ENGINE_INITIALIZED: &str =
    "LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()";
const MAP_LOADING: &str = "LogLoad: LoadMap: /Game/Maps/Level01/Level01_Main";
const EXIT_REQUESTED: &str = "LogCore: Engine exit requested (reason: ConsoleCtrl RequestExit)";

struct Fixture {
    root: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-soulmask-health-{}", uuid::Uuid::new_v4()));
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
            "soulmask",
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
fn runtime_health_soulmask_requires_game_start_and_completed_engine_initialization() {
    let fixture = Fixture::new();
    for content in [
        format!("{GAME_STARTED}\n"),
        format!("{ENGINE_INITIALIZED}\n"),
        format!("LogInit: Display: Game Engine Initialized.\n{GAME_STARTED}\n"),
        format!("[LanGame startup] {GAME_STARTED}\n{ENGINE_INITIALIZED}\n"),
    ] {
        fixture.write(&content);
        assert_eq!(fixture.health().status, "starting");
    }
    fixture.write(&format!("{GAME_STARTED}\n{ENGINE_INITIALIZED}\n"));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn runtime_health_soulmask_background_streaming_is_not_a_new_map_start() {
    let fixture = Fixture::new();
    fixture.write(&format!("{GAME_STARTED}\n{ENGINE_INITIALIZED}\n{}",
        "LogStreaming: Display: ULevelStreaming::RequestLevel(/Game/Maps/Dungeon) is flushing async loading\n".repeat(200)));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "{MAP_LOADING}\n{}",
        "LogStreaming: Loading level\n".repeat(200)
    ));
    let loading = fixture.health();
    assert_eq!(loading.status, "starting");
    assert_eq!(loading.matched_line.as_deref(), Some(MAP_LOADING));
    fixture.append(&format!("{GAME_STARTED}\n"));
    assert_eq!(
        fixture.health().status,
        "ready",
        "A map change does not rerun engine initialization"
    );
    fixture.append(&format!(
        "{EXIT_REQUESTED}\n{}",
        "LogExit: Releasing resources\n".repeat(200)
    ));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{GAME_STARTED}\n"));
    assert_eq!(
        fixture.health().status,
        "starting",
        "Exit invalidates the engine as well as its current world"
    );
}

#[test]
fn runtime_health_soulmask_rewritten_logs_foreign_sources_and_new_runs_reset_stages() {
    let mut fixture = Fixture::new();
    fixture.write(&format!("{GAME_STARTED}\n{ENGINE_INITIALIZED}\n"));
    assert_eq!(fixture.health().status, "ready");
    fixture.write(&format!("{GAME_STARTED}\n"));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{ENGINE_INITIALIZED}\n"));
    assert_eq!(fixture.health().status, "ready");
    let mut tail = read_log_snapshot(fixture.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
    tail.source_path = Some(fixture.root.join("WS.log").to_string_lossy().into_owned());
    assert_eq!(
        analyze_runtime_health_with_startup_evidence(
            "soulmask",
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
    fixture.write(&format!("{GAME_STARTED}\n"));
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn runtime_health_soulmask_waits_for_complete_evidence_and_preserves_fatal_errors() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{GAME_STARTED}\n{}{ENGINE_INITIALIZED}",
        "LogStreaming: Loading level\n".repeat(50_000)
    ));
    assert_eq!(
        fixture.health().status,
        "starting",
        "Unread bytes may contain a later reset"
    );
    assert_eq!(
        fixture.health().status,
        "starting",
        "The final evidence line is incomplete"
    );
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    fixture.append("Fatal error: simulated failure\n");
    assert_eq!(fixture.health().status, "error");
}
