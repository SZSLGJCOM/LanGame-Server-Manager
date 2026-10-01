use super::*;

fn snapshot(lines: &[&str]) -> LogTailSnapshot {
    LogTailSnapshot {
        source_path: None,
        lines: lines.iter().map(|line| (*line).into()).collect(),
        total_lines: lines.len(),
        truncated: false,
        read_error: None,
    }
}

#[test]
fn runtime_health_palworld_requires_its_native_running_declaration() {
    let signal = "Running Palworld dedicated server on :8211";
    for status in [InstanceStatus::Starting, InstanceStatus::Running] {
        let health = analyze_runtime_health("palworld", &status, &snapshot(&[signal]), None, &[]);
        assert_eq!(health.status, "ready");
        assert_eq!(health.matched_line.as_deref(), Some(signal));
    }
    for line in [
        "REST API started on port 8212",
        "LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()",
        "Running Palworld dedicated server on :0",
        "Running Palworld dedicated server on :65536",
        "Running Palworld dedicated server on :8211 pending",
        "Running Palworld dedicated server on :+8211",
        "[LanGame startup] Running Palworld dedicated server on :8211",
    ] {
        assert_eq!(
            analyze_runtime_health(
                "palworld",
                &InstanceStatus::Running,
                &snapshot(&[line]),
                None,
                &[]
            )
            .status,
            "starting",
            "{line}"
        );
    }
    assert_eq!(
        analyze_runtime_health(
            "test",
            &InstanceStatus::Running,
            &snapshot(&[signal]),
            None,
            &[]
        )
        .status,
        "starting"
    );
    assert_eq!(
        analyze_runtime_health(
            "palworld",
            &InstanceStatus::Error,
            &snapshot(&[signal]),
            None,
            &[]
        )
        .status,
        "error"
    );
    assert_eq!(
        analyze_runtime_health(
            "palworld",
            &InstanceStatus::Stopped,
            &snapshot(&[signal]),
            None,
            &[]
        )
        .status,
        "stopped"
    );
    assert_eq!(
        analyze_runtime_health(
            "palworld",
            &InstanceStatus::Running,
            &snapshot(&[signal, "Fatal error: simulated failure"]),
            None,
            &[]
        )
        .status,
        "error"
    );
}
