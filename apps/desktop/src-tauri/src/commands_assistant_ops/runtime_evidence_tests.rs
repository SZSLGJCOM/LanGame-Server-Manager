use super::*;

struct RuntimeEvidenceFiles(std::path::PathBuf);

impl RuntimeEvidenceFiles {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lgsm-assistant-runtime-evidence-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn write(&self, name: &str, content: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }
}

impl Drop for RuntimeEvidenceFiles {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn runtime_evidence_instance(logs: Vec<(&str, Option<String>)>) -> InstanceDetails {
    let (_, mut instance) = super::task_tests::task_fixture(false);
    let processes = logs
        .into_iter()
        .enumerate()
        .map(|(index, (key, path))| app_core::InstanceProcessState {
            run_id: 100 + index as i64,
            session_id: Some(String::from("runtime-evidence-session")),
            process_key: key.to_owned(),
            display_name: key.to_owned(),
            pid: Some(200 + index as u32),
            process_identity: Some(app_core::ProcessIdentity {
                creation_time: 300 + index as u64,
                image_path: String::from("C:/fixture/server.exe"),
            }),
            status: String::from("running"),
            started_at: Some(String::from("2026-01-01T00:00:00Z")),
            stopped_at: None,
            exit_code: None,
            crash_flag: false,
            log_path: path,
            is_primary: index == 0,
        })
        .collect::<Vec<_>>();
    instance.summary.status = InstanceStatus::Running;
    instance.summary.active_process_count = processes.len();
    instance.active_run = Some(app_core::ActiveInstanceRun {
        run_id: 100,
        session_id: Some(String::from("runtime-evidence-session")),
        pid: Some(200),
        log_path: processes
            .first()
            .and_then(|process| process.log_path.clone()),
        process_count: processes.len(),
        processes,
    });
    instance
}

fn runtime_evidence_stopped_session(instance: &mut InstanceDetails) -> app_core::InstanceRunRecord {
    let run = instance.active_run.take().unwrap();
    instance.summary.status = InstanceStatus::Error;
    instance.summary.active_process_count = 0;
    let processes = run
        .processes
        .into_iter()
        .map(|mut process| {
            process.status = String::from("crashed");
            process.stopped_at = Some(String::from("2026-01-01T00:01:00Z"));
            process.exit_code = Some(1);
            process.crash_flag = true;
            process
        })
        .collect();
    app_core::InstanceRunRecord {
        run_id: run.run_id,
        session_id: run.session_id,
        status: String::from("crashed"),
        pid: run.pid,
        started_at: Some(String::from("2026-01-01T00:00:00Z")),
        stopped_at: Some(String::from("2026-01-01T00:01:00Z")),
        exit_code: Some(1),
        crash_flag: true,
        log_path: run.log_path,
        process_count: run.process_count,
        processes,
    }
}

fn runtime_evidence_process<'a>(evidence: &'a Value, key: &str) -> &'a Value {
    evidence["processLogs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|process| process["processKey"] == key)
        .unwrap()
}

#[test]
fn runtime_evidence_ready_master_does_not_hide_failed_caves() {
    let files = RuntimeEvidenceFiles::new();
    let master = files.write("master.log", "[00:00:01]: Server listening on port 10999\n");
    let caves = files.write("caves.log", "[00:00:02]: Failed to load modoverrides.lua\nMOD ERROR: local_consumer: failed initialization\n");
    let mut instance =
        runtime_evidence_instance(vec![("master", Some(master)), ("caves", Some(caves))]);
    let caves = &mut instance.active_run.as_mut().unwrap().processes[1];
    caves.status = String::from("crashed");
    caves.exit_code = Some(1);
    caves.crash_flag = true;
    let evidence = collect_assistant_registered_runtime_logs(&instance, None, 80);
    let evidence =
        finalize_assistant_runtime_evidence(&instance, None, &instance, None, evidence).unwrap();

    assert_eq!(evidence["observationStatus"], "current");
    assert_eq!(evidence["livenessVerified"], false);
    assert_eq!(evidence["modErrorNames"], json!(["local_consumer"]));
    let master = runtime_evidence_process(&evidence, "master");
    assert_eq!(master["log"]["failureLines"], json!([]));
    let caves = runtime_evidence_process(&evidence, "caves");
    assert!(
        caves["log"]["failureLines"]
            .to_string()
            .contains("modoverrides.lua")
    );
    assert_eq!(caves["crashFlag"], true);
    assert_eq!(caves["recordedStatus"], "crashed");
    assert_eq!(caves["runId"], 101);
    assert_eq!(caves["processCreationTime"], 301);
    assert!(!evidence.to_string().contains("C:/fixture"));
}

#[test]
fn runtime_evidence_reads_both_logs_after_the_entire_session_exits() {
    let files = RuntimeEvidenceFiles::new();
    let master = files.write("master.log", "FATAL: primary terminated\n");
    let caves = files.write(
        "caves.log",
        "MOD ERROR: cave_consumer: failed initialization\nFailed to load modoverrides.lua\n",
    );
    let mut instance =
        runtime_evidence_instance(vec![("master", Some(master)), ("caves", Some(caves))]);
    let latest = runtime_evidence_stopped_session(&mut instance);
    let evidence = collect_assistant_registered_runtime_logs(&instance, Some(&latest), 80);
    let evidence = finalize_assistant_runtime_evidence(
        &instance,
        Some(&latest),
        &instance,
        Some(&latest),
        evidence,
    )
    .unwrap();

    assert_eq!(evidence["observationStatus"], "historical");
    assert_eq!(evidence["evidenceScope"], "latest_registered_session");
    assert_eq!(evidence["modErrorNames"], json!(["cave_consumer"]));
    for key in ["master", "caves"] {
        let process = runtime_evidence_process(&evidence, key);
        assert_eq!(process["recordedStatus"], "crashed");
        assert_eq!(
            process["log"]["scope"],
            "historical_registered_process_tail"
        );
        assert!(
            !process["log"]["failureLines"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn runtime_evidence_discards_observation_when_run_or_process_ownership_changes() {
    let before = runtime_evidence_instance(vec![("master", None), ("caves", None)]);
    for change in 0..7 {
        let mut after = before.clone();
        let run = after.active_run.as_mut().unwrap();
        match change {
            0 => run.run_id += 1,
            1 => run.session_id = Some(String::from("replacement-session")),
            2 => {
                run.processes[1]
                    .process_identity
                    .as_mut()
                    .unwrap()
                    .creation_time += 1
            }
            3 => run.processes[1].log_path = Some(String::from("replacement.log")),
            4 => run.processes[1].status = String::from("crashed"),
            5 => run.processes[1].pid = Some(999),
            6 => after.active_run = None,
            _ => unreachable!(),
        }
        let mut evidence = collect_assistant_registered_runtime_logs(&before, None, 80);
        evidence["health"] = json!({"summary": "stale ready status"});
        evidence["modErrorNames"] = json!(["stale_mod"]);
        let evidence =
            finalize_assistant_runtime_evidence(&before, None, &after, None, evidence).unwrap();
        assert_eq!(evidence["observationStatus"], "unknown", "change {change}");
        assert_eq!(evidence["reason"], "runtime_changed_during_read");
        assert_eq!(evidence["processLogs"], json!([]));
        assert_eq!(evidence["modErrorNames"], json!([]));
        assert!(evidence.get("health").is_none());
    }
}

#[test]
fn runtime_evidence_new_session_replaces_historical_evidence_even_if_it_has_already_exited() {
    let mut before = runtime_evidence_instance(vec![("master", None), ("caves", None)]);
    let latest = runtime_evidence_stopped_session(&mut before);
    for still_running in [true, false] {
        let mut after = runtime_evidence_instance(vec![("master", None), ("caves", None)]);
        let run = after.active_run.as_mut().unwrap();
        run.run_id += 10;
        run.session_id = Some(String::from("next-session"));
        for process in &mut run.processes {
            process.run_id += 10;
            process.session_id = run.session_id.clone();
        }
        let mut stopped = after.clone();
        let new_latest = runtime_evidence_stopped_session(&mut stopped);
        if !still_running {
            after = stopped;
        }
        let evidence = collect_assistant_registered_runtime_logs(&before, Some(&latest), 80);
        let evidence = finalize_assistant_runtime_evidence(
            &before,
            Some(&latest),
            &after,
            Some(&new_latest),
            evidence,
        )
        .unwrap();
        assert_eq!(evidence["observationStatus"], "unknown");
        assert_eq!(evidence["observedRunId"], 100);
        assert_eq!(evidence["currentRunId"], 110);
        assert_eq!(evidence["processLogs"], json!([]));
    }
}

#[test]
fn runtime_evidence_missing_log_does_not_fall_back_to_a_healthy_primary() {
    let files = RuntimeEvidenceFiles::new();
    let master = files.write("master.log", "Server listening\n");
    let missing = files
        .0
        .join("missing-caves.log")
        .to_string_lossy()
        .into_owned();
    for path in [None, Some(missing)] {
        let instance =
            runtime_evidence_instance(vec![("master", Some(master.clone())), ("caves", path)]);
        let evidence = collect_assistant_registered_runtime_logs(&instance, None, 80);
        let evidence =
            finalize_assistant_runtime_evidence(&instance, None, &instance, None, evidence)
                .unwrap();
        let caves = runtime_evidence_process(&evidence, "caves");
        assert!(
            caves["log"]["readError"]
                .as_str()
                .is_some_and(|error| !error.is_empty())
        );
        assert_eq!(caves["log"]["lines"], json!([]));
        assert_eq!(evidence["modErrorNames"], json!([]));
        assert!(runtime_evidence_process(&evidence, "master")["log"]["readError"].is_null());
    }
}

#[test]
fn runtime_evidence_budget_keeps_each_process_failure_and_reports_omissions() {
    let files = RuntimeEvidenceFiles::new();
    let noise = "Unicode 日志 and JSON escaping \\\"\u{0001}".repeat(24);
    let mut content =
        String::from("FATAL: retained failure excerpt\nFailed to load modoverrides.lua\n");
    for _ in 0..48 {
        content.push_str(&noise);
        content.push('\n');
    }
    let path = files.write("bounded.log", &content);
    let keys = (0..9)
        .map(|index| format!("process-{index}"))
        .collect::<Vec<_>>();
    let instance = runtime_evidence_instance(
        keys.iter()
            .map(|key| (key.as_str(), Some(path.clone())))
            .collect(),
    );
    let evidence = collect_assistant_registered_runtime_logs(&instance, None, usize::MAX);
    let evidence =
        finalize_assistant_runtime_evidence(&instance, None, &instance, None, evidence).unwrap();

    assert!(evidence.to_string().len() <= ASSISTANT_RUNTIME_EVIDENCE_BYTES);
    assert!(
        serde_json::from_str::<Value>(&assistant_tool_result_text(&evidence))
            .unwrap()
            .get("error")
            .is_none()
    );
    assert_eq!(evidence["omittedProcessCount"], 1);
    assert_eq!(evidence["evidenceTruncated"], true);
    let processes = evidence["processLogs"].as_array().unwrap();
    assert_eq!(processes.len(), ASSISTANT_RUNTIME_EVIDENCE_PROCESSES);
    for process in processes {
        assert!(
            process["log"]["failureLines"]
                .to_string()
                .contains("retained failure excerpt")
        );
        assert!(process["log"]["omittedTailLineCount"].as_u64().unwrap() > 0);
        assert_eq!(process["log"]["truncated"], true);
        assert_eq!(process["identityRecorded"], true);
    }
}

#[test]
fn runtime_evidence_redacts_multiline_secrets_before_splitting_and_bounds_encoded_text() {
    let snapshot = LogTailSnapshot {
        source_path: Some(String::from("C:/private/master.log")),
        lines: [
            "password: |",
            "  synthetic-hidden-value",
            "safe: visible",
            "server_password=synthetic-test-only",
        ]
        .map(String::from)
        .to_vec(),
        total_lines: 4,
        truncated: false,
        read_error: None,
    };
    let (log, _) = assistant_runtime_log_evidence("dontstarve", &snapshot);
    let text = log.to_string();
    assert!(!text.contains("synthetic-hidden-value"));
    assert!(!text.contains("synthetic-test-only"));
    assert!(!text.contains("C:/private"));
    assert!(text.contains("visible"));
    let shortened = assistant_runtime_evidence_text(&"日志\\\"\u{0001}".repeat(100), 64);
    assert!(shortened.ends_with(" [truncated]"));
    assert!(serde_json::to_string(&shortened).unwrap().len() <= 66);
}

#[test]
fn runtime_evidence_retains_failure_before_a_single_process_noisy_tail() {
    let files = RuntimeEvidenceFiles::new();
    let mut content = String::from("FATAL: failure before routine updates\n");
    for _ in 0..99 {
        content.push_str(&"Routine progress update ".repeat(20));
        content.push('\n');
    }
    let path = files.write("master.log", &content);
    let instance = runtime_evidence_instance(vec![("master", Some(path))]);
    let evidence = collect_assistant_registered_runtime_logs(&instance, None, 100);
    let evidence =
        finalize_assistant_runtime_evidence(&instance, None, &instance, None, evidence).unwrap();
    let process = runtime_evidence_process(&evidence, "master");
    assert!(
        process["log"]["failureLines"]
            .to_string()
            .contains("failure before routine updates")
    );
    assert!(process["log"]["omittedTailLineCount"].as_u64().unwrap() > 0);
    assert!(evidence.to_string().len() <= ASSISTANT_RUNTIME_EVIDENCE_BYTES);
}

#[test]
fn runtime_evidence_unregistered_tail_remains_explicitly_unknown() {
    let (_, instance) = super::task_tests::task_fixture(false);
    let mut evidence = collect_assistant_registered_runtime_logs(&instance, None, 80);
    evidence["unboundLog"] =
        json!({"lines": ["A prior attempt failed"], "scope": "registered_process_tail"});
    let evidence =
        finalize_assistant_runtime_evidence(&instance, None, &instance, None, evidence).unwrap();
    assert_eq!(evidence["observationStatus"], "unknown");
    assert!(evidence["runId"].is_null());
    assert_eq!(evidence["evidenceScope"], "no_registered_session");
    assert_eq!(
        evidence["unboundLog"]["scope"],
        "unbound_tail_not_current_run_evidence"
    );
}
