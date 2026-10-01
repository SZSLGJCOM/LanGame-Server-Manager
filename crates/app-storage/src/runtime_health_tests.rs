use super::*;

fn log_snapshot(lines: &[&str]) -> LogTailSnapshot {
    LogTailSnapshot {
        source_path: None,
        lines: lines.iter().map(|line| String::from(*line)).collect(),
        total_lines: lines.len(),
        truncated: false,
        read_error: None,
    }
}

#[test]
fn runtime_health_enshrouded_requires_completed_online_session_transition() {
    let online =
        "[Session] finished transition from 'Lobby' to 'Host_Online' (current='Host_Online')!";
    let starting =
        "[Session] started transition from 'Lobby' to 'Host_Online' (current='<invalid>')!";
    let leaving =
        "[Session] started transition from 'Host_Online' to 'Lobby' (current='<invalid>')!";
    let offline = "[Session] finished transition from 'Host_Online' to 'Lobby' (current='Lobby')!";
    for (lines, expected) in [
        (vec![starting], "starting"),
        (vec![starting, online], "ready"),
        (vec![online, leaving], "starting"),
        (vec![online, offline], "starting"),
        (
            vec![online, "unrelated periodic session statistics"],
            "ready",
        ),
    ] {
        let health = analyze_runtime_health(
            "enshrouded",
            &InstanceStatus::Running,
            &log_snapshot(&lines),
            None,
            &[],
        );
        assert_eq!(health.status, expected);
        assert!(health.matched_line.is_some());
    }
    for line in [
        format!("[LanGame startup] {online}"),
        format!("{online} pending"),
        "[Session] unfinished transition from 'Lobby' to 'Host_Online' (current='Host_Online')!"
            .into(),
    ] {
        assert_eq!(
            analyze_runtime_health(
                "enshrouded",
                &InstanceStatus::Running,
                &log_snapshot(&[&line]),
                None,
                &[]
            )
            .status,
            "starting"
        );
    }
    assert_eq!(
        analyze_runtime_health(
            "test",
            &InstanceStatus::Running,
            &log_snapshot(&[online]),
            None,
            &[]
        )
        .status,
        "starting"
    );
    assert_eq!(
        analyze_runtime_health(
            "enshrouded",
            &InstanceStatus::Error,
            &log_snapshot(&[online]),
            None,
            &[]
        )
        .status,
        "error"
    );
}

#[test]
fn runtime_health_regression_manager_metadata_cannot_report_game_ready() {
    for (module, signal) in [
        ("test", "Server ready"),
        ("test", "Listening on port 7777"),
        ("abioticfactor", "Session short code: instance-name"),
        ("runescapedragonwilds", "ReadyToJoin Value[1]"),
        ("corekeeper", "timescale = 0"),
    ] {
        for prefix in ["[LanGame startup]", "[LanGame startup update]", "[LanGame]"] {
            let line = format!("{prefix} Preparing {signal} startup...");
            let health = analyze_runtime_health(
                module,
                &InstanceStatus::Running,
                &log_snapshot(&[&line]),
                None,
                &[],
            );
            assert_eq!(health.status, "starting", "{module}: {line}");
        }
    }
}

#[test]
fn runtime_health_regression_abiotic_loading_map_is_not_ready() {
    for status in [InstanceStatus::Starting, InstanceStatus::Running] {
        for (line, reason) in [
            (
                "Dedicated server is now loading the main map",
                "abiotic_loading_map",
            ),
            ("Listening on port 7777", "abiotic_listening"),
            ("Load map complete", "abiotic_listening"),
        ] {
            let health =
                analyze_runtime_health("abioticfactor", &status, &log_snapshot(&[line]), None, &[]);
            assert_eq!(health.status, "starting");
            assert_eq!(health.reason.code, reason);
        }
    }
    assert_eq!(
        analyze_runtime_health(
            "abioticfactor",
            &InstanceStatus::Running,
            &log_snapshot(&[
                "Dedicated server is now loading the main map",
                "Session short code: current-session",
            ]),
            None,
            &[],
        )
        .status,
        "ready"
    );
}

#[test]
fn runtime_health_regression_corekeeper_requires_world_initialization() {
    let listening = "Listening on ip:0.0.0.0:27039";
    for line in [listening, "Listening on port 27039", "timescale = 0.5"] {
        assert_eq!(
            analyze_runtime_health(
                "corekeeper",
                &InstanceStatus::Running,
                &log_snapshot(&[line]),
                None,
                &[],
            )
            .status,
            "starting"
        );
    }
    let ready = analyze_runtime_health(
        "corekeeper",
        &InstanceStatus::Running,
        &log_snapshot(&[listening, "timescale = 0"]),
        None,
        &[],
    );
    assert_eq!(ready.status, "ready");
    assert_eq!(ready.matched_line.as_deref(), Some("timescale = 0"));
}

#[test]
fn runtime_health_regression_dragonwilds_native_log_requires_current_process_output() {
    let mut run = dragonwilds_health_run();
    let old = "[2026.09.30-01.02.02:999][ 1]ReadyToJoin Value[1]";
    let current = "[2026.09.30-01.02.03:001][ 1]ReadyToJoin Value[1]";
    let evaluate = |run: Option<&ActiveInstanceRun>, lines: &[&str], source: &str| {
        let mut snapshot = log_snapshot(lines);
        snapshot.source_path = Some(source.into());
        analyze_runtime_health_with_startup_evidence(
            "runescapedragonwilds",
            &InstanceStatus::Running,
            &snapshot,
            run,
            &[],
        )
        .status
    };
    let native = "runtime/RSDragonwilds/Saved/Logs/RSDragonwilds.log";
    assert_eq!(evaluate(Some(&run), &[old], native), "starting");
    assert_eq!(
        evaluate(
            Some(&run),
            &[old, "[2026.09.30-01.02.03:001][ 1]Loading world"],
            native,
        ),
        "starting"
    );
    let before_ready = [
        "[2026.09.30-01.02.03:001][ 1]LogNet: Name:GameNetDriver IpNetDriver listening on port 17777",
        "[2026.09.30-01.02.03:002][ 1]LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[0]",
        "[2026.09.30-01.02.03:003][ 1]LogNetSessionSettings: Setting [\"OtherFlag\"] written with key[x0] value[1]",
    ];
    assert_eq!(evaluate(Some(&run), &before_ready, native), "starting");
    let mut ready_lines = before_ready.to_vec();
    ready_lines.push(
        "[2026.09.30-01.02.03:004][ 1]LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]",
    );
    assert_eq!(evaluate(Some(&run), &ready_lines, native), "ready");
    assert_eq!(evaluate(Some(&run), &[current], native), "ready");
    assert_eq!(
        evaluate(
            Some(&run),
            &[current, "FATAL: main process crashed"],
            native
        ),
        "error"
    );
    assert_eq!(
        evaluate(
            Some(&run),
            &[
                current,
                "[2026.09.30-01.02.02:999][ 1]FATAL: old process crashed",
            ],
            native,
        ),
        "ready"
    );
    assert_eq!(evaluate(None, &[current], native), "starting");
    assert_eq!(
        evaluate(Some(&run), &["ReadyToJoin Value[1]"], native),
        "starting"
    );
    // The immutable process transcript already belongs to this recorded run.
    assert_eq!(
        evaluate(
            Some(&run),
            &["ReadyToJoin Value[1]"],
            run.log_path.as_deref().unwrap(),
        ),
        "ready"
    );
    // Reusing a PID or appending to the same native file does not reuse readiness.
    run.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time += 20_000;
    assert_eq!(evaluate(Some(&run), &[current], native), "starting");
    run.processes[0].process_identity = None;
    assert_eq!(evaluate(Some(&run), &[current], native), "starting");
}

fn dragonwilds_health_run() -> ActiveInstanceRun {
    ActiveInstanceRun {
        run_id: 1,
        session_id: Some("current-session".into()),
        pid: Some(100),
        log_path: Some("logs/managed-console/run-current-main.log".into()),
        process_count: 1,
        processes: vec![InstanceProcessState {
            run_id: 1,
            session_id: Some("current-session".into()),
            process_key: "main".into(),
            display_name: "Server".into(),
            pid: Some(100),
            process_identity: Some(ProcessIdentity {
                creation_time: 134_352_037_230_000_000,
                image_path: "RSDragonwildsServer.exe".into(),
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

#[test]
fn runtime_health_distinguishes_lifecycle_reasons_without_log_text() {
    for (status, lines, reason) in [
        (InstanceStatus::Stopped, vec![], "stopped"),
        (InstanceStatus::Stopping, vec![], "stopping"),
        (InstanceStatus::Error, vec![], "latest_run_failed"),
        (InstanceStatus::Running, vec![], "starting_waiting_logs"),
        (
            InstanceStatus::Running,
            vec!["Loading assets"],
            "starting_tasks",
        ),
    ] {
        let health = analyze_runtime_health("test", &status, &log_snapshot(&lines), None, &[]);
        assert_eq!(health.reason.code, reason);
        assert!(health.reason.params.is_empty());
    }
}

#[test]
fn runtime_health_read_failure_preserves_diagnostic_parameter() {
    let error = "Access denied: D:/instances/中文/server.log";
    let mut snapshot = log_snapshot(&[]);
    snapshot.read_error = Some(String::from(error));
    for status in [InstanceStatus::Running, InstanceStatus::Error] {
        let health = analyze_runtime_health("test", &status, &snapshot, None, &[]);
        assert_eq!(health.reason.code, "log_read_failed");
        assert_eq!(
            health.reason.params.get("error").map(String::as_str),
            Some(error)
        );
        assert!(health.summary.contains(error));
        assert!(health.matched_line.is_none());
    }
}

#[test]
fn native_server_ready_signals_are_scoped_to_their_modules() {
    for (module, signal) in [
        (
            "humanitz",
            "LogHZSuccess: Display: Success => Session created!",
        ),
        (
            "sevendaystodie",
            "2026-09-08T18:58:57 24.041 INF [Steamworks.NET] GameServer.Init successful",
        ),
        (
            "satisfactory",
            "LogServer: Display: Server startup time elapsed and saving/level loading is done, auto-pause is allowed to proceed from now on (if enabled in server settings).",
        ),
        ("corekeeper", "timescale = 0"),
        (
            "scum",
            "LogSCUM: Global Stats: 199.0ms (5.0FPS) | C: 0 (0), P: 0 (0)",
        ),
        (
            "projectzomboid",
            "LOG  : Network      f:0 st:45,923,860> *** SERVER STARTED ****",
        ),
        (
            "vrising",
            "[Server] Startup Completed - Disabling Scene Loading Systems",
        ),
    ] {
        let snapshot = log_snapshot(&[signal]);
        let health = analyze_runtime_health(module, &InstanceStatus::Running, &snapshot, None, &[]);
        assert_eq!(health.status, "ready", "{module}");
        let unrelated =
            analyze_runtime_health("test", &InstanceStatus::Running, &snapshot, None, &[]);
        assert_eq!(unrelated.status, "starting", "{module}");
    }
}

#[test]
fn native_log_selection_uses_the_instance_path_and_rejects_older_output() {
    let fixture = StartupLogFixture::new();
    for (module, relative_path) in [
        ("corekeeper", "logs/CoreKeeperServer.log"),
        ("satisfactory", "data/Saved/Logs/FactoryGame.log"),
        ("scum", "runtime/SCUM/Saved/Logs/SCUM.log"),
    ] {
        let record = StoredInstanceRecord {
            summary: app_core::InstanceSummary {
                id: String::from("native-log"),
                name: String::from("Native log"),
                module_id: module.into(),
                status: InstanceStatus::Running,
                active_process_count: 1,
                bind_ip: String::from("127.0.0.1"),
                port_count: 0,
                autostart: false,
            },
            config_dir: fixture.root.join("config"),
            saves_dir: fixture.root.join("data"),
            runtime_mode: String::from("independent"),
            program_install_root: Some(fixture.root.join("runtime")),
            auto_backup_on_stop: false,
            backup_retention_count: 1,
        };
        let native =
            known_module_runtime_log_path(module, &fixture.root.join("runtime"), &record).unwrap();
        assert_eq!(native, fixture.root.join(relative_path));
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(&native, "Previous server ready\n").unwrap();
        let process = fixture.run.log_path.clone().unwrap();
        fs::write(&process, "Starting current run\n").unwrap();
        let timestamp = UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        fs::File::options()
            .write(true)
            .open(&process)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(timestamp))
            .unwrap();
        fs::File::options()
            .write(true)
            .open(&native)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(timestamp - std::time::Duration::from_secs(1)),
            )
            .unwrap();
        assert_eq!(
            prefer_runtime_log_path(
                module,
                Some(process.clone()),
                Some(native.to_string_lossy().into_owned())
            ),
            Some(process.clone())
        );
        fs::File::options()
            .write(true)
            .open(&native)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(timestamp + std::time::Duration::from_secs(1)),
            )
            .unwrap();
        assert_eq!(
            prefer_runtime_log_path(
                module,
                Some(process),
                Some(native.to_string_lossy().into_owned())
            ),
            Some(native.to_string_lossy().into_owned())
        );
    }
}

#[tokio::test]
async fn native_runtime_logs_do_not_read_the_package_or_another_instance() {
    let fixture = StartupLogFixture::new();
    let shared_log = fixture.root.join("games/scum/SCUM/Saved/Logs/SCUM.log");
    fs::create_dir_all(shared_log.parent().unwrap()).unwrap();
    fs::write(&shared_log, b"shared package log").unwrap();
    let mut records = Vec::new();
    for id in ["alpha", "beta"] {
        let root = fixture.root.join("instances").join(id);
        let runtime = root.join("runtime");
        let log = runtime.join("SCUM/Saved/Logs/SCUM.log");
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(
            runtime.join(crate::private_runtime::PRIVATE_RUNTIME_MARKER),
            b"managed\n",
        )
        .unwrap();
        fs::write(&log, id.as_bytes()).unwrap();
        records.push(StoredInstanceRecord {
            summary: InstanceSummary {
                id: id.into(),
                name: id.into(),
                module_id: "scum".into(),
                status: InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: "127.0.0.1".into(),
                port_count: 0,
                autostart: false,
            },
            config_dir: root.join("config"),
            saves_dir: root.join("saves"),
            runtime_mode: String::from("independent"),
            program_install_root: Some(root.join("runtime")),
            auto_backup_on_stop: false,
            backup_retention_count: 1,
        });
    }
    for record in &records {
        let source = resolve_instance_log_source_path(record, None, &[])
            .await
            .unwrap();
        let snapshot = read_log_snapshot(source, 10);
        assert_eq!(snapshot.lines, vec![record.summary.id.clone()]);
    }
    let alpha_runtime = records[0].config_dir.parent().unwrap().join("runtime");
    fs::remove_file(alpha_runtime.join("SCUM/Saved/Logs/SCUM.log")).unwrap();
    assert!(
        resolve_instance_log_source_path(&records[0], None, &[])
            .await
            .unwrap()
            .is_none()
    );
    fs::remove_file(alpha_runtime.join(crate::private_runtime::PRIVATE_RUNTIME_MARKER)).unwrap();
    assert!(matches!(
        resolve_instance_log_source_path(&records[0], None, &[]).await,
        Err(StorageError::PrivateRuntimeRefresh { .. })
    ));
    let beta_source = resolve_instance_log_source_path(&records[1], None, &[])
        .await
        .unwrap();
    assert_eq!(
        read_log_snapshot(beta_source, 10).lines,
        vec![String::from("beta")]
    );
    assert_eq!(fs::read(shared_log).unwrap(), b"shared package log");
}

#[test]
fn runtime_health_log_reasons_keep_the_exact_matched_line() {
    for (module, line, reason) in [
        ("test", "[12:00] FATAL: raw diagnostic", "fatal_log_pattern"),
        ("test", "[12:01] Server ready", "ready_signal"),
        (
            "dontstarve",
            "[12:02] Failed to load modoverrides.lua",
            "dst_lua_config_failed",
        ),
        (
            "abioticfactor",
            "World save integrity state: corrupt",
            "abiotic_world_corrupt",
        ),
        (
            "abioticfactor",
            "Session short code: raw-code",
            "abiotic_session_published",
        ),
        (
            "abioticfactor",
            "Checking world save for corruption",
            "abiotic_validating_world",
        ),
        (
            "runescapedragonwilds",
            "DedicatedServer.ini configuration: OwnerId missing",
            "dragonwilds_owner_invalid",
        ),
    ] {
        let health = analyze_runtime_health(
            module,
            &InstanceStatus::Running,
            &log_snapshot(&[line]),
            None,
            &[],
        );
        assert_eq!(health.reason.code, reason);
        assert_eq!(health.matched_line.as_deref(), Some(line));
    }
}

#[test]
fn project_zomboid_dictionary_failure_overrides_startup_progress() {
    let failure = "WorldDictionary: Cannot load world due to WorldDictionary error";
    let snapshot = log_snapshot(&["Loading world", failure]);
    let health = analyze_runtime_health(
        "projectzomboid",
        &InstanceStatus::Running,
        &snapshot,
        None,
        &[],
    );
    assert_eq!(health.status, "error");
    assert_eq!(health.reason.code, "fatal_log_pattern");
    assert_eq!(health.matched_line.as_deref(), Some(failure));

    let unrelated = analyze_runtime_health("test", &InstanceStatus::Running, &snapshot, None, &[]);
    assert_eq!(unrelated.status, "starting");
}

#[test]
fn module_startup_signals_end_starting_without_hiding_fatal_errors() {
    for (module_id, line) in [
        ("necesse", "Started server using port 14159 with 8 slots"),
        ("arksurvivalascended", "Server has successfully started!"),
        ("arksurvivalascended", "Server is advertising for join"),
    ]
    .into_iter()
    .chain(NATIVE_READY_SIGNALS)
    {
        let health = analyze_runtime_health(
            module_id,
            &InstanceStatus::Running,
            &log_snapshot(&[line]),
            None,
            &[],
        );
        assert_eq!(health.status, "ready");
        assert_eq!(health.reason.code, "ready_signal");
        assert_eq!(health.matched_line.as_deref(), Some(line));

        let failed = analyze_runtime_health(
            module_id,
            &InstanceStatus::Running,
            &log_snapshot(&[line, "FATAL: server stopped"]),
            None,
            &[],
        );
        assert_eq!(failed.reason.code, "fatal_log_pattern");
        let unrelated = analyze_runtime_health(
            "test",
            &InstanceStatus::Running,
            &log_snapshot(&[line]),
            None,
            &[],
        );
        assert_eq!(unrelated.status, "starting");
    }
}

const NATIVE_READY_SIGNALS: [(&str, &str); 4] = [
    (
        "minecraft",
        "[16:44:17] [Server thread/INFO]: Done (0.170s)! For help, type \"help\"",
    ),
    ("valheim", "09/08/2026 16:50:28: Game server connected"),
    ("barotrauma", "  Server started"),
    (
        "rimworld",
        "[16:33:31] | Listening for users at 0.0.0.0:25555",
    ),
];

#[test]
fn runtime_health_native_ready_signals_reject_other_modules_and_startup_metadata() {
    for (module, signal) in NATIVE_READY_SIGNALS {
        for line in [
            format!("Preparing instance {signal}"),
            format!("[00:00:00] Preparing instance {signal}"),
            format!("[LanGame] Preparing instance {signal}"),
            format!("{signal} (not connected yet)"),
        ] {
            let health = analyze_runtime_health(
                module,
                &InstanceStatus::Running,
                &log_snapshot(&[&line]),
                None,
                &[],
            );
            assert_eq!(health.status, "starting", "{module}: {line}");
        }
        for (other_module, _) in NATIVE_READY_SIGNALS {
            if module != other_module {
                let health = analyze_runtime_health(
                    other_module,
                    &InstanceStatus::Running,
                    &log_snapshot(&[signal]),
                    None,
                    &[],
                );
                assert_eq!(health.status, "starting", "{other_module}: {signal}");
            }
        }
    }
    for (module, line) in [
        (
            "minecraft",
            "[16:44:17] [Server thread/INFO]: Done (NaNs)! For help, type \"help\"",
        ),
        (
            "minecraft",
            "[16:44:17] [Server thread/INFO]: Done (-1s)! For help, type \"help\"",
        ),
        ("rimworld", "[16:33:31] | Listening for users at pending"),
        ("rimworld", "[16:33:31] | Listening for users at 0.0.0.0:0"),
    ] {
        assert_eq!(
            analyze_runtime_health(
                module,
                &InstanceStatus::Running,
                &log_snapshot(&[line]),
                None,
                &[]
            )
            .status,
            "starting"
        );
    }
}

#[test]
fn abiotic_map_loading_reason_depends_on_evidence_not_process_status() {
    for status in [InstanceStatus::Starting, InstanceStatus::Running] {
        for (line, reason) in [
            (
                "Dedicated server is now loading the main map",
                "abiotic_loading_map",
            ),
            ("Listening on port 7777", "abiotic_listening"),
        ] {
            let health =
                analyze_runtime_health("abioticfactor", &status, &log_snapshot(&[line]), None, &[]);
            assert_eq!(health.reason.code, reason);
            assert_eq!(health.matched_line.as_deref(), Some(line));
        }
    }
}

#[test]
fn dragonwilds_ready_reason_carries_map_without_rewriting_the_signal() {
    let signal = "ReadyToJoin Value[1]";
    let snapshot = log_snapshot(&["Create GameSession MapName[ Test World ]", signal]);
    let health = analyze_runtime_health(
        "runescapedragonwilds",
        &InstanceStatus::Running,
        &snapshot,
        None,
        &[],
    );
    assert_eq!(health.reason.code, "dragonwilds_ready_map");
    assert_eq!(
        health.reason.params.get("map").map(String::as_str),
        Some("Test World")
    );
    assert_eq!(health.matched_line.as_deref(), Some(signal));

    let without_map = analyze_runtime_health(
        "runescapedragonwilds",
        &InstanceStatus::Running,
        &log_snapshot(&[signal]),
        None,
        &[],
    );
    assert_eq!(without_map.reason.code, "dragonwilds_ready");
    assert!(without_map.reason.params.is_empty());
}

struct StartupLogFixture {
    root: PathBuf,
    run: ActiveInstanceRun,
}

impl StartupLogFixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "langame-runtime-health-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        fs::create_dir_all(&root).unwrap();
        let run = ActiveInstanceRun {
            run_id: 1,
            session_id: Some(String::from("first-run")),
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

    fn write(&self, content: &str) -> LogTailSnapshot {
        fs::write(self.run.log_path.as_ref().unwrap(), content).unwrap();
        read_log_snapshot(self.run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT)
    }
}

impl Drop for StartupLogFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn runtime_health_recovers_current_run_ready_signal_beyond_recent_tail() {
    let fixture = StartupLogFixture::new();
    let line = "[00:00:02] LAN server started on port 10999";
    let tail = fixture.write(&format!(
        "{line}\n{}",
        "Player query complete\n".repeat(200)
    ));
    assert!(tail.truncated);
    assert!(!tail.lines.iter().any(|value| value == line));
    let health = analyze_runtime_health_with_startup_evidence(
        "dontstarve",
        &InstanceStatus::Running,
        &tail,
        Some(&fixture.run),
        &[],
    );
    assert_eq!(health.status, "ready");
    assert_eq!(health.matched_line.as_deref(), Some(line));
}

#[test]
fn runtime_health_enshrouded_recent_offline_transition_overrides_startup_online() {
    let fixture = StartupLogFixture::new();
    let online =
        "[Session] finished transition from 'Lobby' to 'Host_Online' (current='Host_Online')!";
    for latest in [
        "[Session] started transition from 'Host_Online' to 'Lobby' (current='<invalid>')!",
        "[Session] finished transition from 'Host_Online' to 'Lobby' (current='Lobby')!",
    ] {
        let tail = fixture.write(&format!(
            "{online}\n{}{latest}\n",
            "Session statistics\n".repeat(RUNTIME_HEALTH_SCAN_LINE_LIMIT + 1)
        ));
        assert!(tail.truncated);
        assert!(!tail.lines.iter().any(|line| line == online));
        let health = analyze_runtime_health_with_startup_evidence(
            "enshrouded",
            &InstanceStatus::Running,
            &tail,
            Some(&fixture.run),
            &[],
        );
        assert_eq!(health.status, "starting");
        assert_eq!(health.matched_line.as_deref(), Some(latest));
    }
}

#[test]
fn runtime_health_enshrouded_incomplete_startup_evidence_cannot_restore_online() {
    let fixture = StartupLogFixture::new();
    let online =
        "[Session] finished transition from 'Lobby' to 'Host_Online' (current='Host_Online')!";
    let offline = "[Session] finished transition from 'Host_Online' to 'Lobby' (current='Lobby')!";
    let tail = fixture.write(&format!(
        "{online}\n{}{offline}\n{}",
        "Session statistics\n".repeat(RUNTIME_STARTUP_SCAN_LINE_LIMIT),
        "Session statistics\n".repeat(RUNTIME_HEALTH_SCAN_LINE_LIMIT + 1)
    ));
    assert!(tail.truncated);
    assert!(tail.lines.iter().all(|line| line == "Session statistics"));
    let startup = read_startup_log_snapshot(fixture.run.log_path.as_deref().unwrap());
    assert!(startup.truncated);
    assert!(startup.lines.iter().any(|line| line == online));
    assert!(!startup.lines.iter().any(|line| line == offline));
    // A bounded head scan cannot prove that its online state is still current.
    let health = analyze_runtime_health_with_startup_evidence(
        "enshrouded",
        &InstanceStatus::Running,
        &tail,
        Some(&fixture.run),
        &[],
    );
    assert_eq!(health.status, "starting");
    assert_ne!(health.reason.code, "ready_signal");
}

#[test]
fn runtime_health_enshrouded_tracks_long_online_runs_and_resets_rewritten_logs() {
    use std::io::Write;
    let fixture = StartupLogFixture::new();
    let online =
        "[Session] finished transition from 'Lobby' to 'Host_Online' (current='Host_Online')!";
    let offline = "[Session] finished transition from 'Host_Online' to 'Lobby' (current='Lobby')!";
    let tail = fixture.write(&format!(
        "{online}\n{}",
        "Session statistics\n".repeat(70_000)
    ));
    let health = || {
        analyze_runtime_health_with_startup_evidence(
            "enshrouded",
            &InstanceStatus::Running,
            &tail,
            Some(&fixture.run),
            &[],
        )
    };
    assert_eq!(
        health().status,
        "starting",
        "Unread later bytes cannot prove online"
    );
    assert_eq!(
        health().status,
        "ready",
        "Bounded catch-up eventually preserves stable online"
    );
    assert_eq!(
        health().status,
        "ready",
        "A fresh observation reads persisted session evidence"
    );
    let path = fixture.run.log_path.as_ref().unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(format!("{offline}\n{}", "Session statistics\n".repeat(200)).as_bytes())
        .unwrap();
    assert_eq!(
        health().status,
        "starting",
        "Later offline revokes previously cached online"
    );
    // The same file object may be truncated, or replaced by another generation.
    fs::write(path, format!("{online}\n")).unwrap();
    assert_eq!(health().status, "ready");
    fs::remove_file(path).unwrap();
    fs::write(path, "Session statistics\n").unwrap();
    assert_eq!(health().status, "starting");
    let mut next_run = fixture.run.clone();
    next_run.run_id += 1;
    let next = analyze_runtime_health_with_startup_evidence(
        "enshrouded",
        &InstanceStatus::Running,
        &tail,
        Some(&next_run),
        &[],
    );
    assert_eq!(
        next.status, "starting",
        "New run cannot inherit the previous session state"
    );
}

#[test]
fn runtime_health_enshrouded_preserves_observed_state_across_managed_retention() {
    let mut fixture = StartupLogFixture::new();
    fixture.run.log_path = Some(
        fixture
            .root
            .join("managed-console")
            .join("run-1-main.log")
            .to_string_lossy()
            .into_owned(),
    );
    let path = fixture.run.log_path.as_ref().unwrap();
    let writer = crate::managed_console_log::ManagedConsoleLog::open(path).unwrap();
    let online =
        "[Session] finished transition from 'Lobby' to 'Host_Online' (current='Host_Online')!";
    writer.write_all(format!("{online}\n").as_bytes()).unwrap();
    let observe = || {
        let tail = read_log_snapshot(Some(path.clone()), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
        analyze_runtime_health_with_startup_evidence(
            "enshrouded",
            &InstanceStatus::Running,
            &tail,
            Some(&fixture.run),
            &[],
        )
    };
    assert_eq!(observe().status, "ready");
    let ordinary = "Session stats  \n".repeat(65_536); // Exactly one scan budget.
    for _ in 0..33 {
        writer.write_all(ordinary.as_bytes()).unwrap();
        assert_eq!(observe().status, "ready");
    }
    let parts = crate::managed_console_log::open_log_segments(Path::new(path))
        .unwrap()
        .unwrap();
    assert!(
        parts[0].start_offset > 0,
        "Original online marker has expired"
    );
    drop(parts);
    writer
        .write_all(
            b"[Session] finished transition from 'Host_Online' to 'Lobby' (current='Lobby')!\n",
        )
        .unwrap();
    assert_eq!(observe().status, "starting");
    drop(writer);
}

#[test]
fn runtime_health_startup_evidence_is_scoped_to_current_process_log() {
    let fixture = StartupLogFixture::new();
    let tail = fixture.write(&format!(
        "Server ready\n{}",
        "Player query complete\n".repeat(200)
    ));
    let mut next_run = fixture.run.clone();
    next_run.run_id = 2;
    next_run.session_id = Some(String::from("second-run"));
    next_run.log_path = Some(
        fixture
            .root
            .join("run-second-main.log")
            .to_string_lossy()
            .into_owned(),
    );
    fs::write(
        next_run.log_path.as_ref().unwrap(),
        "Starting a new server\n",
    )
    .unwrap();
    for run in [None, Some(&next_run)] {
        let health = analyze_runtime_health_with_startup_evidence(
            "dontstarve",
            &InstanceStatus::Running,
            &tail,
            run,
            &[],
        );
        assert_eq!(health.status, "starting");
    }
    let next_tail = read_log_snapshot(next_run.log_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
    let health = analyze_runtime_health_with_startup_evidence(
        "dontstarve",
        &InstanceStatus::Running,
        &next_tail,
        Some(&next_run),
        &[],
    );
    assert_eq!(health.status, "starting");
}

#[test]
fn startup_readiness_never_overrides_recent_failure_or_lifecycle_state() {
    let fixture = StartupLogFixture::new();
    let mut tail = fixture.write(&format!(
        "Server ready\n{}",
        "Player query complete\n".repeat(200)
    ));
    for (status, newest_line, expected) in [
        (
            InstanceStatus::Running,
            "FATAL: shard crashed",
            "fatal_log_pattern",
        ),
        (
            InstanceStatus::Running,
            "Failed to load modoverrides.lua",
            "dst_lua_config_failed",
        ),
        (
            InstanceStatus::Error,
            "Player query complete",
            "latest_run_failed",
        ),
        (InstanceStatus::Stopped, "Player query complete", "stopped"),
        (
            InstanceStatus::Stopping,
            "Player query complete",
            "stopping",
        ),
    ] {
        tail.lines = vec![String::from(newest_line)];
        let health = analyze_runtime_health_with_startup_evidence(
            "dontstarve",
            &status,
            &tail,
            Some(&fixture.run),
            &[],
        );
        assert_eq!(health.reason.code, expected);
    }
    tail.read_error = Some(String::from("Access denied"));
    let health = analyze_runtime_health_with_startup_evidence(
        "dontstarve",
        &InstanceStatus::Running,
        &tail,
        Some(&fixture.run),
        &[],
    );
    assert_eq!(health.reason.code, "log_read_failed");
    let stopped = analyze_runtime_health_with_startup_evidence(
        "dontstarve",
        &InstanceStatus::Stopped,
        &tail,
        Some(&fixture.run),
        &[],
    );
    assert_eq!(stopped.status, "stopped");
}

#[test]
fn startup_evidence_scan_has_byte_and_line_limits() {
    let fixture = StartupLogFixture::new();
    for prefix in [
        format!("{}\n", "x".repeat(RUNTIME_STARTUP_SCAN_BYTE_LIMIT as usize)),
        "Loading\n".repeat(RUNTIME_STARTUP_SCAN_LINE_LIMIT),
        format!(
            "{}Server ready\n",
            "x".repeat(RUNTIME_STARTUP_SCAN_BYTE_LIMIT as usize - 6)
        ),
    ] {
        let tail = fixture.write(&format!(
            "{prefix}Server ready\n{}",
            "Player query complete\n".repeat(200)
        ));
        let startup = read_startup_log_snapshot(fixture.run.log_path.as_deref().unwrap());
        assert!(startup.truncated);
        assert!(startup.lines.len() <= RUNTIME_STARTUP_SCAN_LINE_LIMIT);
        let health = analyze_runtime_health_with_startup_evidence(
            "dontstarve",
            &InstanceStatus::Running,
            &tail,
            Some(&fixture.run),
            &[],
        );
        assert_eq!(health.status, "starting");
    }
}

#[test]
fn ready_signals_cannot_hide_latest_run_failure_or_fatal_diagnostic() {
    for (module, signal) in [
        ("dontstarve", "Server ready"),
        ("abioticfactor", "Session short code: ABCD"),
        ("runescapedragonwilds", "ReadyToJoin Value[1]"),
    ] {
        let failed = analyze_runtime_health(
            module,
            &InstanceStatus::Error,
            &log_snapshot(&[signal]),
            None,
            &[],
        );
        assert_eq!(failed.status, "error");
        let fatal = analyze_runtime_health(
            module,
            &InstanceStatus::Running,
            &log_snapshot(&[signal, "FATAL: main process crashed"]),
            None,
            &[],
        );
        assert_eq!(fatal.reason.code, "fatal_log_pattern");
    }
    let owner_failure = analyze_runtime_health(
        "runescapedragonwilds",
        &InstanceStatus::Error,
        &log_snapshot(&["DedicatedServer.ini configuration: OwnerId missing"]),
        None,
        &[],
    );
    assert_eq!(owner_failure.reason.code, "dragonwilds_owner_invalid");
}

#[test]
fn runtime_health_dst_completed_world_generation_retry_clears_only_recovered_diagnostic() {
    let recovered = [
        "[00:00:10]: PANIC: missing required prefab [sculpture_bishop]! Expected 1, got 0",
        "[00:00:10]: An error occured during world gen we will retry! [was 1 of 5]",
        "[00:00:12]: Generation complete, injecting world entities.",
    ];
    let health = analyze_runtime_health(
        "dontstarve",
        &InstanceStatus::Running,
        &log_snapshot(&recovered),
        None,
        &[],
    );
    assert_eq!(health.reason.code, "starting_tasks");
    assert!(health.matched_line.is_none());

    let mut ready = recovered.to_vec();
    ready.push("[00:00:23]: Server ready");
    let health = analyze_runtime_health(
        "dontstarve",
        &InstanceStatus::Running,
        &log_snapshot(&ready),
        None,
        &[],
    );
    assert_eq!(health.reason.code, "ready_signal");
    let failed = analyze_runtime_health(
        "dontstarve",
        &InstanceStatus::Error,
        &log_snapshot(&ready),
        None,
        &[],
    );
    assert_eq!(failed.reason.code, "latest_run_failed");
}

#[test]
fn runtime_health_dst_generation_retry_preserves_unrelated_fatal_errors() {
    let fatal = "[00:00:24]: FATAL: main process crashed";
    for lines in [
        vec![fatal, "Server ready"],
        vec![
            fatal,
            "[00:00:10]: PANIC: missing required prefab [sculpture_bishop]! Expected 1, got 0",
            "[00:00:10]: An error occured during world gen we will retry! [was 1 of 5]",
            "[00:00:12]: Generation complete, injecting world entities.",
            "Server ready",
        ],
        vec![
            "[00:00:10]: PANIC: missing required prefab [sculpture_bishop]! Expected 1, got 0",
            "[00:00:10]: An error occured during world gen we will retry! [was 1 of 5]",
            "[00:00:12]: Generation complete, injecting world entities.",
            "Server ready",
            fatal,
        ],
    ] {
        let health = analyze_runtime_health(
            "dontstarve",
            &InstanceStatus::Running,
            &log_snapshot(&lines),
            None,
            &[],
        );
        assert_eq!(health.reason.code, "fatal_log_pattern");
        assert_eq!(health.matched_line.as_deref(), Some(fatal));
    }
}
