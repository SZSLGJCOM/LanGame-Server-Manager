use super::*;
use app_core::{InstanceProcessState, ProcessIdentity};

// CL 517554: the dedicated entry world emits this once; CreateNewGame then
// loads Persistent_Level and produces navigation/static-mesh output instead.
const READY: &str = "LogServer: Display: Server startup time elapsed and saving/level loading is done, auto-pause is allowed to proceed from now on (if enabled in server settings). WorldTimeSeconds = 30.12";
const NOISE: &str = "LogNavigation: Display: Rebuilding navigation";

struct Fixture {
    root: PathBuf,
    native: PathBuf,
    run: ActiveInstanceRun,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-satisfactory-health-{}", uuid::Uuid::new_v4()));
        let native = root.join("data/Saved/Logs/FactoryGame.log");
        let console = root.join("logs/managed-console/run-41-main.log");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::create_dir_all(console.parent().unwrap()).unwrap();
        fs::write(&console, "Current managed process\n").unwrap();
        let log_path = Some(console.to_string_lossy().into_owned());
        let run = ActiveInstanceRun {
            run_id: 41,
            session_id: Some("current-session".into()),
            pid: Some(100),
            log_path: log_path.clone(),
            process_count: 1,
            processes: vec![InstanceProcessState {
                run_id: 41,
                session_id: Some("current-session".into()),
                process_key: "main".into(),
                display_name: "Satisfactory".into(),
                pid: Some(100),
                process_identity: Some(ProcessIdentity {
                    // 2026-09-30 01:02:03 UTC, before DB registration.
                    creation_time: 134_352_037_230_000_000,
                    image_path: root
                        .join("runtime/Engine/Binaries/Win64/FactoryServer-Win64-Shipping-Cmd.exe")
                        .to_string_lossy()
                        .into_owned(),
                }),
                status: "running".into(),
                started_at: Some("2026-09-30 01:02:10".into()),
                stopped_at: None,
                exit_code: None,
                crash_flag: false,
                log_path,
                is_primary: true,
            }],
        };
        Self { root, native, run }
    }

    fn write(&self, contents: &str) {
        fs::write(&self.native, contents).unwrap();
    }

    fn append(&self, contents: &str) {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(&self.native)
            .unwrap()
            .write_all(contents.as_bytes())
            .unwrap();
    }

    fn tail(&self) -> LogTailSnapshot {
        read_log_snapshot(
            Some(self.native.to_string_lossy().into_owned()),
            RUNTIME_HEALTH_SCAN_LINE_LIMIT,
        )
    }

    fn health_for(&self, run: Option<&ActiveInstanceRun>) -> app_core::RuntimeHealth {
        analyze_runtime_health_with_startup_evidence(
            "satisfactory",
            &InstanceStatus::Running,
            &self.tail(),
            run,
            &[],
        )
    }

    fn health(&self) -> app_core::RuntimeHealth {
        self.health_for(Some(&self.run))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn native_line(message: &str) -> String {
    format!("[2026.09.30-01.02.03:010][ 1]{message}\n")
}

#[test]
fn satisfactory_native_startup_survives_new_world_output_outside_the_tail() {
    let fixture = Fixture::new();
    let sequence = [
        "LogGameState: Match State Changed from EnteringMap to WaitingToStart",
        "LogGlobalStatus: UEngine::LoadMap Load map complete /Game/FactoryGame/Map/DedicatedserverEntry",
        READY,
        "LogGameState: Match State Changed from EnteringMap to WaitingToStart",
        "LogLoad: Took 10.394681 seconds to LoadMap(/Game/FactoryGame/Map/GameLevel01/Persistent_Level)",
        "LogGlobalStatus: UEngine::LoadMap Load map complete /Game/FactoryGame/Map/GameLevel01/Persistent_Level",
    ]
    .map(native_line)
    .concat();
    fixture.write(&format!("{sequence}{}", native_line(NOISE).repeat(500)));
    assert!(fixture.tail().truncated);
    assert!(!fixture.tail().lines.iter().any(|line| line.contains(READY)));
    // Even the first health read can happen after the successful startup line
    // has aged out. It must use this process's complete retained evidence.
    let health = fixture.health();
    assert_eq!(health.status, "ready");
    assert_eq!(health.reason.code, "ready_signal");
    assert_eq!(
        health.matched_line.as_deref(),
        Some(native_line(READY).trim_end())
    );
    fixture.append(&native_line(NOISE).repeat(500));
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn satisfactory_native_evidence_rejects_old_undated_and_unowned_readiness() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "[2026.09.30-01.02.02:999][ 1]{READY}\n[2026.09.30-01.02.02:999][ 1]Fatal error: prior process\n{READY}\n"
    ));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&native_line(READY));
    assert_eq!(fixture.health().status, "ready");
    assert_eq!(fixture.health_for(None).status, "starting");
    for invalid in 0..7 {
        let mut run = fixture.run.clone();
        match invalid {
            0 => run.processes[0].process_identity = None,
            1 => run.processes[0].run_id += 1,
            2 => run.processes[0].session_id = Some("unrelated-session".into()),
            3 => run.processes[0].pid = Some(101),
            4 => run.processes[0].status = "stopped".into(),
            5 => run.processes[0].is_primary = false,
            _ => run.processes[0]
                .process_identity
                .as_mut()
                .unwrap()
                .image_path
                .clear(),
        }
        assert_eq!(
            fixture.health_for(Some(&run)).status,
            "starting",
            "invalid identity {invalid}"
        );
    }
    assert_eq!(fixture.health().status, "ready");
}

#[test]
fn satisfactory_current_fatal_remains_failed_after_aging_and_another_ready_line() {
    for fatal in [
        native_line("Fatal error: current process failed"),
        "Fatal error: failure without timestamp\n".into(),
    ] {
        let fixture = Fixture::new();
        fixture.write(&native_line(READY));
        assert_eq!(fixture.health().status, "ready");
        fixture.append(&format!("{fatal}{}", native_line(NOISE).repeat(500)));
        assert!(
            !fixture
                .tail()
                .lines
                .iter()
                .any(|line| line.contains("Fatal error:"))
        );
        assert_eq!(fixture.health().status, "error");
        fixture.append(&native_line(READY));
        assert_eq!(fixture.health().reason.code, "fatal_log_pattern");
    }
}

#[test]
fn satisfactory_restart_and_late_old_observations_cannot_reuse_readiness() {
    let fixture = Fixture::new();
    fixture.write(&format!(
        "{}{}",
        native_line(READY),
        native_line(NOISE).repeat(500)
    ));
    assert_eq!(fixture.health().status, "ready");
    let mut restarted = fixture.run.clone();
    restarted.run_id += 1;
    restarted.session_id = Some("next-session".into());
    restarted.processes[0].run_id = restarted.run_id;
    restarted.processes[0].session_id = restarted.session_id.clone();
    restarted.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time += 20_000_000;
    // The native filename and PID can both be reused. Process birth and run
    // identity, rather than the file's timestamp, decide which lines are current.
    assert_eq!(fixture.health_for(Some(&restarted)).status, "starting");
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&format!("[2026.09.30-01.02.06:010][ 1]{READY}\n"));
    assert_eq!(fixture.health_for(Some(&restarted)).status, "ready");
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn satisfactory_native_rewrite_truncation_and_replacement_reset_evidence() {
    let fixture = Fixture::new();
    let noise = native_line(NOISE).repeat(500);
    fixture.write(&format!("{}{noise}", native_line(READY)));
    assert_eq!(fixture.health().status, "ready");
    let changed = READY.replace("loading is done", "loading pending");
    assert_eq!(READY.len(), changed.len());
    // The beginning, final anchor and length can all remain unchanged: validate
    // the actual retained event bytes before using their checkpoint.
    fixture.write(&format!("{}{noise}", native_line(&changed)));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&native_line(READY));
    assert_eq!(fixture.health().status, "ready");
    fixture.write(&native_line(NOISE));
    assert_eq!(fixture.health().status, "starting");
    fixture.append(&native_line(READY));
    assert_eq!(fixture.health().status, "ready");
    let replacement = fixture.root.join("replacement.log");
    fs::write(
        &replacement,
        format!("[2026.09.30-01.02.02:999][ 1]{READY}\n"),
    )
    .unwrap();
    fs::remove_file(&fixture.native).unwrap();
    fs::rename(replacement, &fixture.native).unwrap();
    assert_eq!(fixture.health().status, "starting");
}

#[test]
fn satisfactory_native_requires_complete_lines_and_bounded_catchup() {
    let fixture = Fixture::new();
    fixture.write(native_line(READY).trim_end());
    assert_eq!(fixture.health().status, "starting");
    fixture.append("\n");
    assert_eq!(fixture.health().status, "ready");
    let backlog = native_line(NOISE).repeat(20_000);
    assert!((1024 * 1024..2 * 1024 * 1024).contains(&backlog.len()));
    fixture.append(&backlog);
    assert_eq!(
        fixture.health().status,
        "starting",
        "unread output may contain a later failure"
    );
    assert_eq!(
        fixture.health().status,
        "ready",
        "a later bounded read finishes the same cursor"
    );
}

#[test]
fn satisfactory_native_read_failures_and_stopped_runs_never_keep_cached_ready() {
    let fixture = Fixture::new();
    fixture.write(&native_line(READY));
    assert_eq!(fixture.health().status, "ready");
    let tail = fixture.tail();
    for (status, expected) in [
        (InstanceStatus::Stopped, "stopped"),
        (InstanceStatus::Stopping, "idle"),
        (InstanceStatus::Error, "error"),
    ] {
        assert_eq!(
            analyze_runtime_health_with_startup_evidence(
                "satisfactory",
                &status,
                &tail,
                Some(&fixture.run),
                &[]
            )
            .status,
            expected
        );
    }
    fs::write(
        fixture
            .native
            .parent()
            .unwrap()
            .join(".satisfactory-startup.json"),
        "invalid",
    )
    .unwrap();
    assert_eq!(fixture.health().reason.code, "log_read_failed");
    fs::remove_file(&fixture.native).unwrap();
    assert_ne!(fixture.health().status, "ready");
}
