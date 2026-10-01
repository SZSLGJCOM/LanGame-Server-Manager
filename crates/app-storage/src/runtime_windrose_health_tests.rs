use super::*;

const READY: &str = "R5LogCoopProxy: [000566] ...opProxyServer::SetIsReadyForHostOwnerConnect Host server is ready for owner to connect. Semaphore null";
const LOADING: &str = "LogLoad: LoadMap: /Game/Maps/Lobby/R5ServerLobby";
const EXITING: &str = "LogCore: Engine exit requested (reason: ConsoleCtrl RequestExit)";

struct Fixture {
    root: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-windrose-health-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let run = ActiveInstanceRun {
            run_id: 1,
            session_id: Some("fixture".into()),
            pid: None,
            log_path: Some(root.join("run-1-main.log").to_string_lossy().into_owned()),
            process_count: 1,
            processes: Vec::new(),
        };
        Self { root, run }
    }
    fn write(&self, text: &str) {
        fs::write(self.run.log_path.as_ref().unwrap(), text).unwrap();
    }
    fn append(&self, text: &str) {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(self.run.log_path.as_ref().unwrap())
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    fn health(&self) -> app_core::RuntimeHealth {
        let tail = read_log_snapshot(self.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
        analyze_runtime_health_with_startup_evidence(
            "windrose",
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
fn runtime_health_windrose_listener_and_map_load_precede_realm_readiness() {
    // The native lobby map completed 18.7 seconds before HostReady.
    // Engine initialization and early listener output do not prove world readiness.
    for line in [
        "LogNet: IpNetDriver listening on port 7784",
        "LogGlobalStatus: UEngine::LoadMap Load map complete /Game/Maps/Lobby/R5ServerLobby",
        "R5LogDataKeeper: Server. Change state JustStarted => OpenedServerLobby",
        "R5LogDataKeeper: Server. Change state OpenedServerLobby => LoadedIslandData",
        "LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()",
    ] {
        let snapshot = LogTailSnapshot {
            source_path: None,
            lines: vec![line.into()],
            total_lines: 1,
            truncated: false,
            read_error: None,
        };
        let health =
            analyze_runtime_health("windrose", &InstanceStatus::Running, &snapshot, None, &[]);
        assert_eq!(health.status, "starting", "{line}");
    }
}

#[test]
fn runtime_health_windrose_final_native_world_phase_establishes_ready() {
    let fixture = Fixture::new();
    fixture.write(&format!("{LOADING}\n"));
    assert_eq!(fixture.health().status, "starting");
    let ready = format!("[2026.10.01-08.00.08:016][ 79]{READY}");
    fixture.append(&format!("{ready}\n"));
    let health = fixture.health();
    assert_eq!(health.status, "ready");
    assert_eq!(health.reason.code, "ready_signal");
    assert_eq!(health.matched_line.as_deref(), Some(ready.as_str()));
}

#[test]
fn runtime_health_windrose_keeps_ready_outside_tail_until_new_world_or_exit() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{READY}\n{}",
        "Periodic realm diagnostics\n".repeat(500)
    ));
    assert_eq!(fixture.health().status, "ready");
    fixture.append("LogLoad: LoadMap: /Game/Maps/GYM/Genlandia/GenlandiaMulty\n");
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{READY}\n{EXITING}\n"));
    assert_eq!(fixture.health().reason.code, "stopping");
}

#[test]
fn runtime_health_windrose_rejects_spoof_partial_foreign_and_previous_run() {
    let mut fixture = Fixture::new();
    for line in [format!("chat: {READY}"), format!("[LanGame startup] {READY}"),
        format!("[invalid][79]{READY}"), "R5LogCoopProxy: [000566] AnotherFunction Host server is ready for owner to connect. Semaphore null".into()] {
        fixture.write(&format!("{line}\n"));
        assert_eq!(fixture.health().status, "starting");
    }
    fixture.write(READY);
    assert_eq!(fixture.health().status, "starting");
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    let mut tail = read_log_snapshot(fixture.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
    tail.source_path = Some(
        fixture
            .root
            .join("shared-native.log")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(
        analyze_runtime_health_with_startup_evidence(
            "windrose",
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
            .join("run-2-main.log")
            .to_string_lossy()
            .into_owned(),
    );
    fixture.write(&format!("{LOADING}\n"));
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn runtime_health_windrose_unread_suffix_and_fatal_output_override_cached_ready() {
    let fixture = Fixture::new();
    fixture.write(&format!("{READY}\n"));
    assert_eq!(fixture.health().status, "ready");
    fixture.append(&format!(
        "{}{LOADING}\n",
        "Periodic realm diagnostics\n".repeat(60_000)
    ));
    assert_eq!(fixture.health().status, "starting");
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("{READY}\nFatal error: synthetic native failure\n"));
    assert_eq!(fixture.health().status, "error");
}
