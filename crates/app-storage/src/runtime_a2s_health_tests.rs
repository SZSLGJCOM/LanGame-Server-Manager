use super::*;
use app_core::{InstanceProcessState, ProcessIdentity};
use std::cell::Cell;

fn spec() -> ModuleStartupReadinessSpec {
    ModuleStartupReadinessSpec {
        timeout_ms: 600_000,
        probe: StartupReadinessProbe::A2s {
            port_name: "query".into(),
            port_argument: Some("-QueryPort".into()),
            ready_log: None,
        },
    }
}

fn final_phase_spec() -> ModuleStartupReadinessSpec {
    let mut declaration = spec();
    if let StartupReadinessProbe::A2s { ready_log, .. } = &mut declaration.probe {
        *ready_log = Some(app_core::startup_readiness::StartupLogSource::Managed);
    }
    declaration
}

#[test]
fn declared_final_phase_and_current_protocol_are_both_required_for_health_and_save_readiness() {
    for status in [InstanceStatus::Starting, InstanceStatus::Running] {
        let health = apply_with(
            Some(&final_phase_spec()),
            &status,
            "127.0.0.1",
            Some(&run()),
            &ports(),
            pending(),
            |_, _| panic!("an early protocol reply must not replace the final native phase"),
        );
        assert_eq!(health.status, "starting");
        assert_ne!(
            health.status, "ready",
            "the save consumer must remain blocked"
        );
        for protocol_ready in [false, true] {
            let mut final_phase = result("ready", "ready_signal", "Native world startup completed");
            final_phase.matched_line = Some("Server startup complete".into());
            let health = apply_with(
                Some(&final_phase_spec()),
                &status,
                "127.0.0.1",
                Some(&run()),
                &ports(),
                final_phase,
                |_, _| Ok(protocol_ready),
            );
            assert_eq!(health.status == "ready", protocol_ready);
            if protocol_ready {
                assert_eq!(
                    health.matched_line.as_deref(),
                    Some("Server startup complete")
                );
            }
        }
    }
}

struct LogFixture(std::path::PathBuf);

impl LogFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-a2s-world-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for LogFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn conan_and_rust_keep_late_final_phases_after_the_health_tail_moves_on() {
    for (module, ready) in [
        (
            "conanexiles",
            "[2026.09.30-01.02.04:001][ 1]LogGameState: Display: Entered application state 'WorldPersistenceDone'",
        ),
        ("rust", "Server startup complete"),
    ] {
        let fixture = LogFixture::new();
        let path = fixture.0.join("current.log");
        let mut current = run();
        current.processes[0]
            .process_identity
            .as_mut()
            .unwrap()
            .creation_time = 134_352_037_230_000_000;
        current.log_path = Some(
            if module == "rust" {
                path.clone()
            } else {
                fixture.0.join("managed.log")
            }
            .to_string_lossy()
            .into(),
        );
        let noise =
            "[2026.09.30-01.02.05:000][ 1]Ordinary native status with no readiness evidence\n";
        std::fs::write(
            &path,
            format!("{}{ready}\n{}", noise.repeat(30_000), noise.repeat(300)),
        )
        .unwrap();
        let snapshot = app_core::LogTailSnapshot {
            source_path: Some(path.to_string_lossy().into()),
            lines: vec![noise.trim().into()],
            total_lines: 30_301,
            truncated: true,
            read_error: None,
        };
        let mut health = pending();
        for _ in 0..5 {
            health = log_health(
                module,
                &InstanceStatus::Running,
                &snapshot,
                Some(&current),
                &ports(),
                pending(),
            );
            if health.status == "ready" {
                break;
            }
        }
        assert_eq!(
            health.status, "ready",
            "{module}: native phase outside both fixed windows"
        );
        let mut log = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut log, noise.repeat(500).as_bytes()).unwrap();
        let health = log_health(
            module,
            &InstanceStatus::Running,
            &snapshot,
            Some(&current),
            &ports(),
            pending(),
        );
        assert_eq!(
            health.status, "ready",
            "{module}: subsequent health/save snapshots retain the phase"
        );
        assert_eq!(health.matched_line.as_deref(), Some(ready));
    }
}

#[test]
fn conan_native_evidence_cannot_inherit_an_older_process_or_an_undated_ready_line() {
    let fixture = LogFixture::new();
    let path = fixture.0.join("conan-server.log");
    let mut current = run();
    current.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time = 134_352_037_230_000_000;
    current.log_path = Some(fixture.0.join("managed.log").to_string_lossy().into());
    std::fs::write(&path, "[2026.09.30-01.01.59:999][ 1]Entered application state 'WorldPersistenceDone'\nEntered application state 'WorldPersistenceDone'\n").unwrap();
    let snapshot = app_core::LogTailSnapshot {
        source_path: Some(path.to_string_lossy().into()),
        lines: vec![],
        total_lines: 2,
        truncated: false,
        read_error: None,
    };
    let observe = |run: &ActiveInstanceRun| {
        log_health(
            "conanexiles",
            &InstanceStatus::Running,
            &snapshot,
            Some(run),
            &ports(),
            pending(),
        )
    };
    assert_eq!(observe(&current).status, "starting");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(
        &mut file,
        b"[2026.09.30-01.02.04:001][ 1]Entered application state 'WorldPersistenceDone'\n",
    )
    .unwrap();
    assert_eq!(observe(&current).status, "ready");
    current.run_id += 1;
    current.processes[0].run_id = current.run_id;
    current.processes[0]
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time += 30_000_000;
    assert_eq!(observe(&current).status, "starting");
}

fn run() -> ActiveInstanceRun {
    let process = InstanceProcessState {
        run_id: 7,
        session_id: Some("current-session".into()),
        process_key: "main".into(),
        display_name: "Fixture".into(),
        pid: Some(42),
        process_identity: Some(ProcessIdentity {
            creation_time: 456,
            image_path: "fixture.exe".into(),
        }),
        status: "running".into(),
        started_at: None,
        stopped_at: None,
        exit_code: None,
        crash_flag: false,
        log_path: None,
        is_primary: true,
    };
    ActiveInstanceRun {
        run_id: process.run_id,
        session_id: process.session_id.clone(),
        pid: process.pid,
        process_count: 1,
        log_path: None,
        processes: vec![process],
    }
}

fn ports() -> Vec<PortBinding> {
    vec![PortBinding {
        name: "query".into(),
        protocol: "udp".into(),
        port: 27015,
    }]
}

fn pending() -> RuntimeHealth {
    result("starting", "starting_waiting_logs", "Waiting for logs")
}

#[test]
fn declared_protocol_response_changes_health_without_a_log_ready_marker() {
    for ready in [false, true] {
        let observed = apply_with(
            Some(&spec()),
            &InstanceStatus::Running,
            "127.0.0.1",
            Some(&run()),
            &ports(),
            pending(),
            |target, port| {
                assert_eq!(target.pid, 42);
                assert_eq!(target.identity.creation_time, 456);
                assert_eq!(target.host, "127.0.0.1");
                assert_eq!(port, 27015);
                Ok(ready)
            },
        );
        assert_eq!(observed.status, if ready { "ready" } else { "starting" });
        assert!(observed.matched_line.is_none());
    }
}

#[test]
fn running_and_a_player_count_are_not_readiness_evidence() {
    let health = apply_with(
        Some(&spec()),
        &InstanceStatus::Running,
        "127.0.0.1",
        Some(&run()),
        &ports(),
        result("ready", "ready_signal", "An older log observation"),
        |_, _| Ok(false),
    );
    assert_eq!(health.status, "starting");
    let health = apply_with(
        Some(&spec()),
        &InstanceStatus::Running,
        "127.0.0.1",
        Some(&run()),
        &ports(),
        pending(),
        |_, _| Err("owner changed".into()),
    );
    assert_eq!(health.status, "warning");
    assert_eq!(health.reason.code, "native_readiness_unconfirmed");
}

#[test]
fn fatal_warning_unknown_and_stopped_health_cannot_be_overwritten_by_protocol_success() {
    for status in ["error", "warning", "unknown", "stopped", "idle"] {
        let mut original = result(status, "fixture-diagnostic", "Keep this diagnostic");
        original.matched_line = Some("Preserve original evidence".into());
        let health = apply_with(
            Some(&spec()),
            &InstanceStatus::Running,
            "127.0.0.1",
            Some(&run()),
            &ports(),
            original,
            |_, _| panic!("must not query"),
        );
        assert_eq!(health.status, status);
        assert_eq!(health.reason.code, "fixture-diagnostic");
        assert_eq!(
            health.matched_line.as_deref(),
            Some("Preserve original evidence")
        );
    }
    for status in [
        InstanceStatus::Stopped,
        InstanceStatus::Stopping,
        InstanceStatus::Error,
    ] {
        let health = apply_with(
            Some(&spec()),
            &status,
            "127.0.0.1",
            Some(&run()),
            &ports(),
            pending(),
            |_, _| panic!("must not query"),
        );
        assert_eq!(health.reason.code, "starting_waiting_logs");
    }
}

#[test]
fn only_an_explicit_a2s_declaration_enables_protocol_health() {
    let other = ModuleStartupReadinessSpec {
        timeout_ms: 600_000,
        probe: StartupReadinessProbe::DstWorld,
    };
    for declaration in [None, Some(&other)] {
        let health = apply_with(
            declaration,
            &InstanceStatus::Running,
            "127.0.0.1",
            Some(&run()),
            &ports(),
            pending(),
            |_, _| panic!("must not query"),
        );
        assert_eq!(health.reason.code, "starting_waiting_logs");
    }
}

#[test]
fn incomplete_or_replaced_run_metadata_cannot_authorize_a_protocol_query() {
    for variant in 0..13 {
        let mut current = run();
        match variant {
            0 => current.run_id = 0,
            1 => current.session_id = None,
            2 => current.process_count = 2,
            3 => current.processes.clear(),
            4 => current.processes[0].run_id += 1,
            5 => current.processes[0].session_id = Some("replacement".into()),
            6 => current.processes[0].pid = Some(99),
            7 => current.processes[0].process_identity = None,
            8 => current.processes[0].exit_code = Some(0),
            9 => current.processes[0].crash_flag = true,
            10 => current.processes[0].is_primary = false,
            11 => current.processes[0].status = "stopped".into(),
            _ => current.processes[0].process_key.clear(),
        }
        let queried = Cell::new(false);
        let health = apply_with(
            Some(&spec()),
            &InstanceStatus::Running,
            "127.0.0.1",
            Some(&current),
            &ports(),
            pending(),
            |_, _| {
                queried.set(true);
                Ok(true)
            },
        );
        assert!(!queried.get(), "variant {variant}");
        assert_eq!(health.status, "warning");
    }
}

#[test]
fn declared_query_binding_must_be_unique_nonzero_udp() {
    for variant in 0..4 {
        let mut bindings = ports();
        match variant {
            0 => bindings.clear(),
            1 => bindings[0].port = 0,
            2 => bindings[0].protocol = "tcp".into(),
            _ => bindings.push(bindings[0].clone()),
        }
        let health = apply_with(
            Some(&spec()),
            &InstanceStatus::Running,
            "127.0.0.1",
            Some(&run()),
            &bindings,
            pending(),
            |_, _| panic!("must not query"),
        );
        assert_eq!(health.status, "warning");
    }
}

#[cfg(windows)]
#[test]
fn real_owned_a2s_response_makes_current_run_health_ready() {
    use std::net::{Ipv4Addr, UdpSocket};
    use std::time::Duration;
    let listener = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let responder = listener.try_clone().unwrap();
    let server = std::thread::spawn(move || {
        let mut request = [0; 128];
        let (length, peer) = responder.recv_from(&mut request).unwrap();
        assert_eq!(
            &request[..length],
            b"\xff\xff\xff\xffTSource Engine Query\0"
        );
        let reply = b"\xff\xff\xff\xffI\x11Fixture\0Map\0Folder\0Game\0\x01\0\0\x14\0dw\0\x01v1\0";
        responder.send_to(reply, peer).unwrap();
    });
    let mut current = run();
    current.pid = Some(std::process::id());
    current.processes[0].pid = current.pid;
    current.processes[0].process_identity =
        app_runtime::inspect_process_identity(std::process::id()).unwrap();
    let mut bindings = ports();
    bindings[0].port = listener.local_addr().unwrap().port();
    let health = apply(
        Some(&spec()),
        &InstanceStatus::Running,
        "127.0.0.1",
        Some(&current),
        &bindings,
        pending(),
    );
    assert_eq!(health.status, "ready");
    assert_eq!(health.reason.code, "ready_signal");
    server.join().unwrap();
    drop(listener);
}
