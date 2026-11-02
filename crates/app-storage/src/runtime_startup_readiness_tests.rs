use super::*;

const NIGHTINGALE_READY: &str = "LogNWXGameMode: OnAllLevelsLoaded: levels finished loading, finished restoring world state and building navigation. Going ready.";
const SQUAD_WORLD: &str = "LogGameState: Match State Changed from EnteringMap to WaitingToStart";
const SQUAD_SESSION: &str = "LogSquadOnlineServices: Session created: Started USQOnlineServicesUpdateSessionManager updates";
const SOULMASK_GAME: &str = "logStoreGamemode: Display: [ GAME STARTED. ]";
const ENGINE_READY: &str = "LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()";
const WINDROSE_READY: &str = "R5LogCoopProxy: [42] UR5CoopProxy::SetIsReadyForHostOwnerConnect Host server is ready for owner to connect. Semaphore 1";

// These messages come from the existing health/native-signal regressions and
// module smoke contracts; they are parser fixtures, not a new native run.
const READY_LINES: [(&str, &[&str]); 29] = [
    (
        "abioticfactor",
        &["LogAbiotic: Warning: Session short code: XGSJP"],
    ),
    (
        "arksurvivalascended",
        &[
            "[2026.10.11-03.45.40:098][235]Server has completed startup and is now advertising for join. (10.31GB Mem)",
        ],
    ),
    ("barotrauma", &["Server started"]),
    (
        "arksurvivalevolved",
        &["[2026.10.24-05.49.42:826][  0]Full Startup: 101.85 seconds (BP compile: 0.00 seconds)"],
    ),
    (
        "conanexiles",
        &["LogGameState: Display: Entered application state 'WorldPersistenceDone'"],
    ),
    ("corekeeper", &["timescale = 0"]),
    (
        "enshrouded",
        &["[Session] finished transition from 'Lobby' to 'Host_Online' (current='Host_Online')!"],
    ),
    (
        "humanitz",
        &["[2026.10.11-03.45.40:098][235]LogHZSuccess: Display: Success => Session created!"],
    ),
    (
        "minecraft",
        &["[16:44:17] [Server thread/INFO]: Done (0.170s)! For help, type \"help\""],
    ),
    ("necesse", &["Started server using port 14159 with 8 slots"]),
    ("nightingale", &[NIGHTINGALE_READY]),
    ("palworld", &["Running Palworld dedicated server on :8211"]),
    (
        "projectzomboid",
        &["LOG  : Network      f:0 st:45,923,860> *** SERVER STARTED ****"],
    ),
    (
        "rimworld",
        &["[16:33:31] | Listening for users at 0.0.0.0:25555"],
    ),
    ("romestead", &["Server ready"]),
    ("rust", &["Server startup complete"]),
    (
        "runescapedragonwilds",
        &["LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]"],
    ),
    (
        "satisfactory",
        &[
            "LogServer: Display: Server startup time elapsed and saving/level loading is done, auto-pause is allowed to proceed from now on (if enabled in server settings).",
        ],
    ),
    (
        "scum",
        &["LogSCUM: Global Stats: 199.0ms (5.0FPS) | C: 0 (0), P: 0 (0)"],
    ),
    (
        "sevendaystodie",
        &["2026-09-08T18:58:57 24.041 INF StartGame done"],
    ),
    ("sonsoftheforest", &["#DSL Dedicated server loaded."]),
    ("soulmask", &[SOULMASK_GAME, ENGINE_READY]),
    ("squad", &[SQUAD_WORLD, SQUAD_SESSION]),
    ("terraria", &["Server started"]),
    ("theforest", &["[LanGame native control] world_ready=True"]),
    ("unturned", &["Steam servers ready!", "Loading level: 100%"]),
    ("valheim", &["09/08/2026 16:50:28: Opened Steam server"]),
    (
        "vrising",
        &["[Server] Startup Completed - Disabling Scene Loading Systems"],
    ),
    ("windrose", &[WINDROSE_READY]),
];

#[test]
fn terraria_console_prompt_preserves_the_exact_ready_message_boundary() {
    // Captured from Terraria 1.4.5.8 running in the managed ConPTY console.
    for line in ["Server started", ": Server started"] {
        let mut observer = StartupLogReadiness::new("terraria").unwrap();
        observer.observe(line);
        assert!(observer.is_ready(), "{line}");
        assert!(simple_ready_signal("terraria", line), "health: {line}");
    }
    for line in [
        ":",
        "Listening on port 7778",
        ": Server start",
        ": Server started pending",
        "Server name: Server started",
        "say Server started",
        ": say Server started",
        ":: Server started",
    ] {
        let mut observer = StartupLogReadiness::new("terraria").unwrap();
        observer.observe(line);
        assert!(!observer.is_ready(), "{line}");
        assert!(!simple_ready_signal("terraria", line), "health: {line}");
    }
    assert!(!simple_ready_signal("barotrauma", ": Server started"));
}

#[test]
fn every_log_module_requires_its_native_startup_completion() {
    for (module, lines) in READY_LINES {
        let mut observer = StartupLogReadiness::new(module).unwrap();
        assert!(!observer.is_ready(), "{module}");
        for line in lines {
            observer.observe(line);
        }
        assert!(observer.is_ready(), "{module}");
        for _ in 0..1000 {
            observer.observe("ordinary periodic native status");
        }
        assert!(observer.is_ready(), "{module}");
        observer.reset();
        assert!(!observer.is_ready(), "{module}");
    }
    for module in ["astroneer", "returntomoria", "dontstarve", "unknown"] {
        assert!(StartupLogReadiness::new(module).is_err(), "{module}");
    }
}

#[test]
fn listening_metadata_and_names_do_not_complete_startup() {
    for (module, lines) in READY_LINES {
        let mut observer = StartupLogReadiness::new(module).unwrap();
        for early in [
            "Listening on port 7777",
            "Listening on UDP 7777",
            "Heartbeat ok",
            "Load map complete",
            "Ready for connections",
            "Server startup complete",
        ] {
            if module == "rust" && early == "Server startup complete" {
                continue;
            }
            observer.observe(early);
            assert!(!observer.is_ready(), "{module}: {early}");
        }
        for line in lines {
            for prefix in [
                "[LanGame startup] ",
                "Server name: ",
                "Preparing instance ",
                "[00:00:00] Preparing instance ",
            ] {
                observer.observe(&format!("{prefix}{line}"));
                assert!(!observer.is_ready(), "{module}: {prefix}{line}");
            }
        }
    }
}

#[test]
fn a2s_world_logs_require_complete_final_messages_at_the_native_boundary() {
    for line in [
        "Full Startup: 51.36 seconds",
        "[2026.10.01-08.50.50:548][  0]Full Startup: 51.36 seconds (BP compile: 0.00 seconds)",
    ] {
        assert!(simple_ready_signal("arksurvivalevolved", line), "{line}");
    }
    for line in [
        "Entered application state 'WorldPersistenceDone'",
        "LogGameState: Entered application state 'WorldPersistenceDone'",
        "[2026.10.01-08.50.50:548][  0]LogGameState:Display: Entered application state 'WorldPersistenceDone'",
        "ApplicationState: Display: Entered application state 'WorldPersistenceDone'",
    ] {
        assert!(simple_ready_signal("conanexiles", line), "{line}");
    }
    for (module, line) in [
        (
            "arksurvivalevolved",
            "Server: \"Fixture\" has successfully started!",
        ),
        ("arksurvivalevolved", "Full Startup: NaN seconds"),
        ("arksurvivalevolved", "Full Startup: -1 seconds"),
        (
            "arksurvivalevolved",
            "Full Startup: 51.36 seconds (BP compile: NaN seconds)",
        ),
        ("arksurvivalevolved", "Full Startup: 51.36 seconds pending"),
        (
            "arksurvivalevolved",
            "Server name: Full Startup: 51.36 seconds",
        ),
        (
            "conanexiles",
            "WorldPersistence: Finished loading 0 actors in 0.0 seconds.",
        ),
        (
            "conanexiles",
            "Entered application state 'WorldPersistence'",
        ),
        (
            "conanexiles",
            "LogGameState: Display: Entered application state 'WorldPersistenceDone' pending",
        ),
        (
            "conanexiles",
            "ServerName: Entered application state 'WorldPersistenceDone'",
        ),
        (
            "conanexiles",
            "Server name: Entered application state 'WorldPersistenceDone'",
        ),
        (
            "conanexiles",
            "{\"name\":\"Entered application state 'WorldPersistenceDone'\"}",
        ),
        ("rust", "SteamServer Connected"),
        ("rust", "Server startup complete pending"),
        ("rust", "Server name: Server startup complete"),
        ("rust", "{\"message\":\"Server startup complete\"}"),
    ] {
        let mut observer = StartupLogReadiness::new(module).unwrap();
        observer.observe(line);
        assert!(!observer.is_ready(), "{module}: {line}");
        assert!(
            !simple_ready_signal(module, line),
            "health {module}: {line}"
        );
    }
}

#[test]
fn early_world_markers_and_invalid_numeric_fields_stay_starting() {
    for (module, line) in [
        (
            "sevendaystodie",
            "2026-09-08T18:58:57 24.041 INF [Steamworks.NET] GameServer.Init successful",
        ),
        ("sevendaystodie", "INF StartAsServer"),
        ("valheim", "09/08/2026 16:50:28: Game server connected"),
        (
            "arksurvivalascended",
            "Server: \"Server has completed startup and is now advertising for join.\" has successfully started!",
        ),
        (
            "minecraft",
            "[16:44:17] [Server thread/INFO]: Done (NaNs)! For help, type \"help\"",
        ),
        (
            "minecraft",
            "[16:44:17] [Server thread/INFO]: Done (-1s)! For help, type \"help\"",
        ),
        ("necesse", "Started server using port 65536 with 8 slots"),
        ("palworld", "Running Palworld dedicated server on :0"),
        (
            "palworld",
            "Running Palworld dedicated server on :8211 (not ready)",
        ),
        ("rimworld", "[16:33:31] | Listening for users at 0.0.0.0:0"),
        ("corekeeper", "timescale = 0.5"),
        ("theforest", "Dedicated Server Running"),
        (
            "nightingale",
            "LogNWXGameMode: OnAllLevelsLoaded: levels finished loading, restoring world state and building navigation.",
        ),
        ("windrose", ENGINE_READY),
    ] {
        let mut observer = StartupLogReadiness::new(module).unwrap();
        observer.observe(line);
        assert!(!observer.is_ready(), "{module}: {line}");
    }
}

#[test]
fn palworld_native_host_and_port_format_accepts_configured_ip_bindings() {
    for endpoint in [
        ":8211",
        "0.0.0.0:8211",
        "127.0.0.1:8211",
        "[::1]:8211",
        ":::8211",
    ] {
        let mut observer = StartupLogReadiness::new("palworld").unwrap();
        observer.observe(&format!("Running Palworld dedicated server on {endpoint}"));
        assert!(observer.is_ready(), "{endpoint}");
    }
    for endpoint in [
        "server-name:8211",
        "0.0.0.0:0",
        "127.0.0.1:65536",
        ":8211 extra",
        "[not-ip]:8211",
    ] {
        let mut observer = StartupLogReadiness::new("palworld").unwrap();
        observer.observe(&format!("Running Palworld dedicated server on {endpoint}"));
        assert!(!observer.is_ready(), "{endpoint}");
    }
}

#[test]
fn health_no_longer_promotes_early_or_cross_game_log_messages() {
    for (module, signal) in [
        (
            "sevendaystodie",
            "INF [Steamworks.NET] GameServer.Init successful",
        ),
        ("valheim", "09/08/2026 16:50:28: Game server connected"),
        ("terraria", "Listening on port 7777"),
        ("satisfactory", "heartbeat ok"),
        ("unknown", "Server ready"),
        ("scum", "Server name: LogSCUM: Global Stats: 10FPS"),
        (
            "abioticfactor",
            "Server name: Session short code: user-controlled",
        ),
    ] {
        let health = super::super::analyze_runtime_health(
            module,
            &app_core::InstanceStatus::Starting,
            &app_core::LogTailSnapshot {
                source_path: None,
                lines: vec![signal.into()],
                total_lines: 1,
                truncated: false,
                read_error: None,
            },
            None,
            &[],
        );
        assert_ne!(health.status, "ready", "{module}: {signal}");
    }
}

#[test]
fn composite_readiness_uses_the_shared_revocable_state() {
    for (module, first, second, revoke, restore, restored) in [
        (
            "squad",
            SQUAD_WORLD,
            SQUAD_SESSION,
            "LogLoad: LoadMap: /Game/Next",
            SQUAD_WORLD,
            false,
        ),
        (
            "soulmask",
            SOULMASK_GAME,
            ENGINE_READY,
            "LogLoad: LoadMap: /Game/Next",
            SOULMASK_GAME,
            true,
        ),
        (
            "unturned",
            "Steam servers ready!",
            "Loading level: 100%",
            "Loading level: 10%",
            "Loading level: 100%",
            true,
        ),
        (
            "unturned",
            "Loading level: 100%",
            "Steam servers ready!",
            "Waiting for Steam servers...",
            "Steam servers ready!",
            true,
        ),
    ] {
        let mut observer = StartupLogReadiness::new(module).unwrap();
        observer.observe(first);
        assert!(!observer.is_ready(), "{module}");
        observer.observe(second);
        assert!(observer.is_ready(), "{module}");
        observer.observe(revoke);
        assert!(!observer.is_ready(), "{module}");
        observer.observe(restore);
        assert_eq!(observer.is_ready(), restored, "{module}");
    }
}

#[test]
fn later_native_transitions_revoke_ready() {
    for (module, ready, revoked) in [
        (
            "enshrouded",
            "[Session] finished transition from 'Lobby' to 'Host_Online' (current='Host_Online')!",
            "[Session] started transition from 'Host_Online' to 'Lobby' (current='<invalid>')!",
        ),
        (
            "runescapedragonwilds",
            "ReadyToJoin Value[1]",
            "ReadyToJoin Value[0]",
        ),
        (
            "sonsoftheforest",
            "#DSL Dedicated server loaded.",
            "#DSL [Dedicated] Starting Sons of the Forest Dedicated Server...",
        ),
        (
            "theforest",
            "[LanGame native control] world_ready=True",
            "[LanGame native control] world_ready=False",
        ),
        (
            "nightingale",
            NIGHTINGALE_READY,
            "LogLoad: LoadMap: /Game/Next",
        ),
        (
            "windrose",
            WINDROSE_READY,
            "LogCore: Engine exit requested (reason: shutdown)",
        ),
    ] {
        let mut observer = StartupLogReadiness::new(module).unwrap();
        observer.observe(ready);
        assert!(observer.is_ready(), "{module}");
        observer.observe(revoked);
        assert!(!observer.is_ready(), "{module}");
    }
}

#[test]
fn json_properties_and_invalid_framing_are_not_native_messages() {
    for (module, signal) in [
        ("palworld", "Running Palworld dedicated server on :8211"),
        ("nightingale", NIGHTINGALE_READY),
    ] {
        for json in [
            serde_json::json!({"server_name": signal}).to_string(),
            serde_json::json!({"message": {"name": signal}}).to_string(),
            serde_json::json!({"message": signal}).to_string(),
            format!("{{\"message\":\"{signal}"),
        ] {
            let mut observer = StartupLogReadiness::new(module).unwrap();
            observer.observe(&json);
            assert!(!observer.is_ready(), "unverified envelope: {json}");
            // Palworld's final marker bypasses its JSON logger and is printed
            // directly to stdout (native build 25247047, RVA 0x02dfbccd).
            observer.observe(signal);
            assert!(
                observer.is_ready(),
                "plain final marker after JSON: {module}"
            );
        }
    }
    let mut observer = StartupLogReadiness::new("terraria").unwrap();
    observer.observe("Server started\nname: fake");
    observer.observe(&format!("{}Server started", " ".repeat(MAX_LINE_BYTES)));
    assert!(!observer.is_ready());
}

#[test]
fn fatal_failure_cannot_be_erased_by_later_ready_output() {
    let mut observer = StartupLogReadiness::new("terraria").unwrap();
    observer.observe("Server started");
    assert!(observer.is_ready());
    assert!(!observer.has_failed());
    observer.observe("FATAL: main process crashed");
    observer.observe("Server started");
    assert!(!observer.is_ready());
    assert!(observer.has_failed());
    observer.reset();
    assert!(!observer.has_failed());
    observer.observe("Server started");
    assert!(observer.is_ready());
}

#[test]
fn native_log_timestamps_reject_prior_process_generations() {
    let identity = app_core::ProcessIdentity {
        creation_time: 116_444_736_000_015_000,
        image_path: "server.exe".into(),
    };
    assert!(!startup_log_line_is_current(
        &identity,
        "[1970.01.01-00.00.00:000][ 0]old"
    ));
    assert!(startup_log_line_is_current(
        &identity,
        "[1970.01.01-00.00.00:001][ 0]same millisecond"
    ));
    assert!(startup_log_line_is_current(
        &identity,
        "[1970.01.01-00.00.00:002][ 0]new"
    ));
    assert!(startup_log_line_is_current(
        &identity,
        "undated output from the fresh byte cursor"
    ));
    let unknown_identity = app_core::ProcessIdentity {
        creation_time: 0,
        ..identity
    };
    assert!(!startup_log_line_is_current(
        &unknown_identity,
        "[1970.01.01-00.00.00:002][ 0]unknown generation"
    ));
}

#[test]
fn asa_startup_requires_a_complete_native_timestamp_prefix() {
    let final_message = "Server has completed startup and is now advertising for join.";
    let mut observer = StartupLogReadiness::new("arksurvivalascended").unwrap();
    for prefix in [
        "",
        "[today]",
        "[2026.02.30-03.45.40:098][235]",
        "[2026.10.11-03.45.40:098][server-name]",
    ] {
        observer.observe(&format!("{prefix}{final_message}"));
        assert!(!observer.is_ready(), "{prefix}");
    }
    observer.observe(&format!("[2026.10.11-03.45.40:098][235]{final_message}"));
    assert!(observer.is_ready());
}

#[test]
fn valheim_crossplay_requires_world_and_an_active_network_and_revokes_disconnects() {
    const ACTIVE: &str = "Session \"Fixture\" with join code 123456 and IP 192.0.2.1:2456 is active with 0 player(s)";
    for revoke in [
        "PlayFab reconnect server 'Fixture'",
        "ZPlayFabMatchmaking::UnregisterServer - unregistering server now. State: Creating",
        "Unregister PlayFab server \"Fixture\" and leaving network \"test-network\"",
        "Deactivated PlayFab lobby test-lobby",
        "Left PlayFab lobby test-lobby",
    ] {
        let mut observer = StartupLogReadiness::new("valheim").unwrap();
        observer.observe("09/08/2026 16:50:28: Opened PlayFab server");
        observer.observe("Session \"Fixture\" registered with join code 123456");
        assert!(!observer.is_ready());
        observer.observe(ACTIVE);
        assert!(observer.is_ready());
        observer.observe(revoke);
        assert!(!observer.is_ready());
        observer.observe(ACTIVE);
        assert!(observer.is_ready());
    }
    let mut observer = StartupLogReadiness::new("valheim").unwrap();
    observer.observe(ACTIVE);
    assert!(!observer.is_ready());
    observer.observe("Opened PlayFab server");
    assert!(observer.is_ready());
    for endpoint in ["<RESOLVING>", "[2001:db8::1]:2456"] {
        observer.reset();
        observer.observe("Opened PlayFab server");
        observer.observe(&ACTIVE.replace("192.0.2.1:2456", endpoint));
        assert!(observer.is_ready(), "native Active address: {endpoint}");
    }
    for invalid in [
        ACTIVE.replace("123456", "12345"),
        ACTIVE.replace("123456", "12345x"),
        ACTIVE.replace("192.0.2.1:2456", "pending"),
        ACTIVE.replace("192.0.2.1:2456", "<resolving>"),
        ACTIVE.replace("192.0.2.1:2456", "192.0.2.1:0"),
        ACTIVE.replace("0 player(s)", "-1 player(s)"),
        format!("Server name: {ACTIVE}"),
        format!("{ACTIVE} pending"),
    ] {
        observer.reset();
        observer.observe("Opened PlayFab server");
        observer.observe(&invalid);
        assert!(!observer.is_ready());
    }
    let health = super::super::analyze_runtime_health(
        "valheim",
        &app_core::InstanceStatus::Starting,
        &app_core::LogTailSnapshot {
            source_path: None,
            lines: vec!["Opened PlayFab server".into(), ACTIVE.into()],
            total_lines: 2,
            truncated: false,
            read_error: None,
        },
        None,
        &[],
    );
    assert_eq!(health.status, "ready");
    assert_eq!(
        health.matched_line, None,
        "health must not retain the join code"
    );
}
