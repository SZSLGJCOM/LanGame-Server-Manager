use super::*;
use std::time::{Duration, Instant};

#[path = "runtime_reliability_probe.rs"]
mod probe;
#[path = "runtime_reliability_resources.rs"]
mod resources;

use probe::{Fixture, wait_until};

const SURFACES: [ProcessHostSurface; 2] = [
    ProcessHostSurface::ManagedTerminal,
    ProcessHostSurface::ManagedPseudoConsole,
];

#[test]
fn runtime_reliability_large_unbroken_output_and_crash_tail_reach_the_owned_sink() {
    for surface in SURFACES {
        let ordinary_pipe = matches!(surface, ProcessHostSurface::ManagedTerminal);
        let mut fixture = Fixture::new(surface, "flood");
        fixture.begin();
        fixture.wait_file("written");
        assert!(
            wait_until(
                || fixture.output_contains("LGSM_FLOOD_COMPLETE"),
                Duration::from_secs(10)
            ),
            "output stalled while the server remained alive"
        );
        assert!(fixture.handles[0].is_running().unwrap());
        if ordinary_pipe {
            assert_eq!(fixture.output.lock().unwrap().payload, probe::FLOOD_BYTES);
        } else {
            // ConPTY produces a screen transcript, not a byte-preserving pipe.
            assert!(fixture.output.lock().unwrap().payload > 0);
        }
        fixture.signal("release");
        assert_eq!(fixture.wait_exit(Duration::from_secs(10)), Some(23));
        fixture.close_owner();
        assert!(fixture.output_contains("LGSM_STDERR_服务器"));
        assert!(fixture.output_contains("LGSM_FINAL_WITHOUT_NEWLINE"));
        fixture.cleanup();
    }
}

#[test]
fn runtime_reliability_native_launch_stdin_deadline_preserves_shutdown_ownership() {
    let mut fixture = Fixture::new(ProcessHostSurface::ManagedTerminal, "unread");
    fixture.begin();
    let child = fixture.spawned.as_mut().unwrap().child.as_mut().unwrap();
    let mut stdin = child.take_stdin().unwrap();
    let started = Instant::now();
    let error = stdin
        .write_stdin_line(&"x".repeat(1024 * 1024))
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(
        stdin.write_stdin_line("Save").unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    child.restore_stdin(stdin);
    assert!(fixture.handles[0].is_running().unwrap());
    stop_spawned_process(fixture.spawned.as_mut().unwrap()).unwrap();
    fixture.close_owner();
    fixture.cleanup();
}

#[test]
fn runtime_reliability_unicode_commands_and_order_survive_both_native_hosts() {
    for surface in SURFACES {
        let mut fixture = Fixture::new(surface, "echo");
        fixture.begin();
        let child = fixture.spawned.as_mut().unwrap().child.as_mut().unwrap();
        let mut stdin = child.take_stdin().unwrap();
        stdin.write_stdin_line("状态😀").unwrap();
        child.restore_stdin(stdin);
        let mut stdin = child.take_stdin().unwrap();
        stdin.write_stdin_line("Save").unwrap();
        // Real dispatch returns the live input endpoint to its process owner.
        // Closing a ConPTY input pipe instead requests terminal shutdown.
        child.restore_stdin(stdin);
        assert_eq!(fixture.wait_exit(Duration::from_secs(10)), Some(0));
        fixture.close_owner();
        assert!(fixture.output_contains("LGSM_UNICODE_ORDER_VERIFIED"));
        fixture.cleanup();
    }
}

#[test]
fn runtime_reliability_launcher_detection_retains_job_and_stops_handed_off_workload() {
    for surface in SURFACES {
        let mut fixture = Fixture::new(surface, "launcher");
        fixture.begin();
        fixture.wait_file("leaf-ready");
        fixture.wait_file("descendant");
        let descendant = fs::read_to_string(fixture.root.join("descendant"))
            .unwrap()
            .parse()
            .unwrap();
        fixture.handles.push(
            WindowsProcessHandle::open(descendant, PROCESS_TERMINATE)
                .unwrap()
                .unwrap(),
        );
        let spawned = fixture.spawned.as_mut().unwrap();
        assert_eq!(
            stabilize_spawned_process("probe.exe", spawned, Duration::ZERO).unwrap(),
            None
        );
        assert_eq!(
            spawned.pid, descendant,
            "handoff must be discovered, not assigned by the test"
        );
        fixture.signal("handoff");
        assert_eq!(fixture.wait_exit(Duration::from_secs(10)), Some(0));
        let spawned = fixture.spawned.as_mut().unwrap();
        assert_eq!(
            stabilize_spawned_process("probe.exe", spawned, Duration::ZERO).unwrap(),
            None
        );
        assert!(spawned.child.as_ref().unwrap().has_owned_process_tree());
        assert!(fixture.handles[1].is_running().unwrap());
        stop_spawned_process(spawned).unwrap();
        fixture.close_owner();
        fixture.cleanup();
    }
}

#[test]
fn runtime_reliability_repeated_stop_restart_releases_resources_after_warmup() {
    let mut fixture = Fixture::at(
        test_support::unique_test_root(),
        ProcessHostSurface::ManagedTerminal,
        "resources",
        resources::RESOURCE_PROBE,
    );
    let exit = fixture.wait_exit(resources::probe_budget());
    fixture.close_owner();
    if let Ok(report) = fs::read_to_string(fixture.root.join("resource-samples.json")) {
        println!("RELIABILITY_METRICS {report}");
    }
    assert_eq!(exit, Some(0));
    let report = fs::read_to_string(fixture.root.join("resources-verified.json")).unwrap();
    let report: serde_json::Value = serde_json::from_str(&report).unwrap();
    assert_eq!(report["measured_cycles"], resources::measured_cycles());
    assert_eq!(report["launches_per_cycle"], 4);
    assert_eq!(report["schema_version"], 2);
    fixture.cleanup();
}
