use super::*;
use app_core::{InstanceProcessState, ProcessIdentity};

const SHARED_LOGS: [(&str, &str, &str); 3] = [
    (
        "conanexiles",
        "logs/conan-server.log",
        "LogNet: GameNetDriver IpNetDriver listening on port 7777",
    ),
    (
        "scum",
        "runtime/SCUM/Saved/Logs/SCUM.log",
        "LogSCUM: Global Stats: 199.0ms (5.0FPS) | C: 0 (0), P: 0 (0)",
    ),
    (
        "satisfactory",
        "data/Saved/Logs/FactoryGame.log",
        "LogServer: Display: Server startup time elapsed and saving/level loading is done",
    ),
];

fn current_run() -> ActiveInstanceRun {
    ActiveInstanceRun {
        run_id: 41,
        session_id: Some("current-session".into()),
        pid: Some(100),
        log_path: Some("logs/managed-console/run-current-main.log".into()),
        process_count: 1,
        processes: vec![InstanceProcessState {
            run_id: 41,
            session_id: Some("current-session".into()),
            process_key: "main".into(),
            display_name: "Server".into(),
            pid: Some(100),
            process_identity: Some(ProcessIdentity {
                // 2026-09-30 01:02:03 UTC, before DB registration below.
                creation_time: 134_352_037_230_000_000,
                image_path: "D:/fixture/runtime/Server.exe".into(),
            }),
            status: "running".into(),
            started_at: Some("2026-09-30 01:02:10".into()),
            stopped_at: None,
            exit_code: None,
            crash_flag: false,
            log_path: Some("logs/managed-console/run-current-main.log".into()),
            is_primary: true,
        }],
    }
}

fn health(
    module: &str,
    run: Option<&ActiveInstanceRun>,
    source: &str,
    lines: &[&str],
) -> app_core::RuntimeHealth {
    super::super::analyze_runtime_health(
        module,
        &InstanceStatus::Running,
        &LogTailSnapshot {
            source_path: Some(source.into()),
            lines: lines.iter().map(|line| (*line).into()).collect(),
            total_lines: lines.len(),
            truncated: false,
            read_error: None,
        },
        run,
        &[],
    )
}

#[test]
fn shared_unreal_logs_cannot_carry_readiness_into_a_new_process() {
    let run = current_run();
    for (module, source, ready) in SHARED_LOGS {
        let old = format!("[2026.09.30-01.02.02:999][ 1]{ready}");
        let fresh = format!("[2026.09.30-01.02.03:001][ 1]{ready}");
        assert_eq!(
            health(module, Some(&run), source, &[&old]).status,
            "starting",
            "{module}: a retained ready line belongs to the previous process"
        );
        assert_eq!(
            health(
                module,
                Some(&run),
                source,
                &[&old, "[2026.09.30-01.02.03:001][ 1]Loading world"],
            )
            .status,
            "starting",
            "{module}: appending current loading output must not revive old readiness"
        );
        assert_eq!(
            health(module, Some(&run), source, &[&fresh]).status,
            "ready",
            "{module}: the native ready line can precede DB registration"
        );
        assert_eq!(
            health(module, Some(&run), source, &[ready]).status,
            "starting",
            "{module}: an undated shared-file line has no process generation"
        );
        assert_eq!(
            health(module, None, source, &[&fresh]).status,
            "starting",
            "{module}: a native timestamp alone cannot identify an owned process"
        );
    }
}

#[test]
fn shared_unreal_logs_require_the_current_run_process_identity() {
    for (module, source, ready) in SHARED_LOGS {
        let fresh = format!("[2026.09.30-01.02.03:001][ 1]{ready}");
        for invalid in 0..7 {
            let mut run = current_run();
            match invalid {
                0 => run.processes[0].process_identity = None,
                1 => run.processes[0].run_id += 1,
                2 => run.processes[0].session_id = Some("previous-session".into()),
                3 => run.processes[0].pid = Some(101),
                4 => run.processes[0].status = "stopped".into(),
                5 => run.processes[0].is_primary = false,
                _ => {
                    // The PID and file path can be reused by a newer process.
                    run.processes[0]
                        .process_identity
                        .as_mut()
                        .unwrap()
                        .creation_time += 20_000;
                }
            }
            assert_eq!(
                health(module, Some(&run), source, &[&fresh]).status,
                "starting",
                "{module}: invalid process evidence {invalid}"
            );
        }
    }
}

#[test]
fn current_run_transcripts_keep_undated_readiness_and_live_failures() {
    let run = current_run();
    for (module, source, ready) in SHARED_LOGS {
        assert_eq!(
            health(
                module,
                Some(&run),
                run.log_path.as_deref().unwrap(),
                &[ready]
            )
            .status,
            "ready",
            "{module}: the managed transcript already belongs to this run"
        );
        let fresh = format!("[2026.09.30-01.02.03:001][ 1]{ready}");
        assert_eq!(
            health(
                module,
                Some(&run),
                source,
                &[&fresh, "Fatal error: current failure"]
            )
            .status,
            "error",
            "{module}: uncertain-age fatal output must remain visible"
        );
        assert_eq!(
            health(
                module,
                Some(&run),
                source,
                &[
                    &fresh,
                    "[2026.09.30-01.02.02:999][ 1]Fatal error: previous failure"
                ],
            )
            .status,
            "ready",
            "{module}: an earlier process failure must not override current readiness"
        );
    }
}

#[tokio::test]
async fn corekeeper_native_readiness_follows_only_the_current_run_log() {
    use std::fs;
    use std::path::PathBuf;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "langame-corekeeper-log-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    let logs = fixture.0.join("logs");
    let console = logs.join("managed-console/run-42-main.log");
    let native = logs.join("corekeeper-run-42-main.log");
    let legacy = logs.join("CoreKeeperServer.log");
    fs::create_dir_all(console.parent().unwrap()).unwrap();
    fs::create_dir_all(fixture.0.join("runtime")).unwrap();
    fs::write(
        fixture
            .0
            .join("runtime")
            .join(crate::private_runtime::PRIVATE_RUNTIME_MARKER),
        b"managed\n",
    )
    .unwrap();
    fs::write(&console, "[LanGame startup] Starting current world\n").unwrap();
    fs::write(&legacy, "timescale = 0\n").unwrap();
    let mut run = current_run();
    run.log_path = Some(console.to_string_lossy().into_owned());
    run.processes[0].log_path = run.log_path.clone();
    let record = crate::StoredInstanceRecord {
        summary: app_core::InstanceSummary {
            id: "corekeeper-log-fixture".into(),
            name: "Core Keeper".into(),
            module_id: "corekeeper".into(),
            status: InstanceStatus::Running,
            active_process_count: 1,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        config_dir: fixture.0.join("config"),
        saves_dir: fixture.0.join("data/worlds"),
        runtime_mode: "independent".into(),
        program_install_root: Some(fixture.0.join("runtime")),
        auto_backup_on_stop: false,
        backup_retention_count: 1,
    };
    for (contents, expected_ready) in [
        (None, false),
        (Some(""), false),
        (Some("Loading world\n"), false),
        (Some("timescale = 0\n"), true),
    ] {
        if let Some(contents) = contents {
            fs::write(&native, contents).unwrap();
        }
        let selected = super::super::resolve_instance_log_source_path(&record, Some(&run), &[])
            .await
            .unwrap();
        assert_eq!(
            selected.as_deref().map(std::path::Path::new),
            Some(native.as_path()),
            "the current source must never fall back to the retained ready log"
        );
        let snapshot = super::super::read_log_snapshot(selected, 160);
        let health = super::super::analyze_runtime_health_with_startup_evidence(
            "corekeeper",
            &InstanceStatus::Running,
            &snapshot,
            Some(&run),
            &[],
        );
        assert_eq!(health.status == "ready", expected_ready);
    }
    fs::write(
        &native,
        format!("timescale = 0\n{}", "Ordinary game status\n".repeat(200)),
    )
    .unwrap();
    let snapshot =
        super::super::read_log_snapshot(Some(native.to_string_lossy().into_owned()), 160);
    assert!(snapshot.truncated);
    assert_eq!(
        super::super::analyze_runtime_health_with_startup_evidence(
            "corekeeper",
            &InstanceStatus::Running,
            &snapshot,
            Some(&run),
            &[]
        )
        .status,
        "ready",
        "the current run's initialized world survives the UI tail window"
    );
    run.log_path = Some(
        logs.join("managed-console/run-43-main.log")
            .to_string_lossy()
            .into_owned(),
    );
    run.processes[0].log_path = run.log_path.clone();
    fs::write(run.log_path.as_ref().unwrap(), "Loading restarted world\n").unwrap();
    let selected = super::super::resolve_instance_log_source_path(&record, Some(&run), &[])
        .await
        .unwrap();
    assert_eq!(
        selected.as_deref().map(std::path::Path::new),
        Some(logs.join("corekeeper-run-43-main.log").as_path())
    );
    assert_ne!(
        health(
            "corekeeper",
            Some(&run),
            native.to_str().unwrap(),
            &["timescale = 0"]
        )
        .status,
        "ready"
    );
    assert_eq!(fs::read_to_string(legacy).unwrap(), "timescale = 0\n");
    assert!(
        fs::read_to_string(native)
            .unwrap()
            .starts_with("timescale = 0\n")
    );
}
