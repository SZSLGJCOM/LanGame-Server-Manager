use super::*;
use app_storage::StoragePaths;

#[path = "commands_runtime_restart_reliability_tests.rs"]
mod reliability;

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    instance_id: String,
}

impl Fixture {
    async fn new(max_restarts: usize, only_nonzero_exit: bool) -> Self {
        let root = std::env::temp_dir().join(format!("lg-restart-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("db")).unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let paths = StoragePaths {
            app_data_root: root.clone(),
            settings_path: root.join("settings.json"),
            database_path: root.join("db/lgs.db"),
            logs_root: root.join("logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        initialize_database(&paths).await.unwrap();
        let descriptors = discover_modules(&paths.modules_root).unwrap();
        let descriptor = find_descriptor(&descriptors, "dontstarve").unwrap();
        sync_modules(&paths, std::slice::from_ref(descriptor))
            .await
            .unwrap();
        crate::commands::tests::prepare_fake_registered_program(&paths, descriptor)
            .await
            .unwrap();
        let created = create_instance(
            &paths,
            descriptor,
            CreateInstanceInput {
                name: String::from("Restart fixture"),
                module_id: String::from("dontstarve"),
            },
        )
        .await
        .unwrap();
        update_instance(&paths, UpdateInstanceInput {
            id: created.summary.id.clone(), bind_ip: String::from("127.0.0.1"),
            auto_backup_on_stop: false, backup_retention_count: 3, ports: created.ports,
            settings_json: json!({"cluster_name":"Restart fixture", "runtime_restart": {
                "enabled":true, "max_restarts":max_restarts, "only_nonzero_exit":only_nonzero_exit,
                "backoff_ms":0
            }}).to_string(),
        }).await.unwrap();
        Self {
            root,
            instance_id: created.summary.id,
            storage: StorageBootstrap {
                settings: paths.settings(),
                storage_status: paths.probe_status(),
                paths,
            },
        }
    }

    async fn process(&self, session: &str, shard: &str) -> i64 {
        mark_instance_process_started_with_identity(
            &self.storage.paths,
            &StartedInstanceProcess {
                instance_id: &self.instance_id,
                session_id: Some(session),
                process_key: shard,
                display_name: shard,
                pid: 1234,
                log_path: "isolated-fixture.log",
                is_primary: shard == "master",
            },
            None,
        )
        .await
        .unwrap()
        .run_id
    }

    async fn exit(&self, run_id: i64, code: i32, crash: bool) {
        mark_instance_process_stopped(
            &self.storage.paths,
            &self.instance_id,
            run_id,
            Some(code),
            crash,
        )
        .await
        .unwrap();
    }

    async fn candidate(&self, expected_exit: bool, code: i32) -> Option<RuntimeRestartCandidate> {
        build_runtime_restart_candidate(
            &self.storage,
            &self.instance_id,
            "Fixture",
            expected_exit,
            Some(code),
        )
        .await
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove isolated restart fixture");
    }
}

#[tokio::test]
async fn runtime_restart_includes_unexpected_zero_exit_when_policy_requests_all_exits() {
    let fixture = Fixture::new(3, false).await;
    let run = fixture.process("first", "master").await;
    fixture.exit(run, 0, false).await;
    assert!(
        fixture.candidate(false, 0).await.is_some(),
        "only_nonzero_exit=false must include unexpected clean process exits"
    );
}

#[tokio::test]
async fn runtime_restart_limit_ten_is_not_truncated_to_eight_visible_sessions() {
    let fixture = Fixture::new(10, true).await;
    for index in 0..11 {
        let run = fixture.process(&format!("failed-{index}"), "master").await;
        fixture.exit(run, 7, true).await;
    }
    let overview = read_instance_runtime_overview(&fixture.storage.paths, &fixture.instance_id)
        .await
        .unwrap();
    assert_eq!(
        overview.recent_runs.len(),
        8,
        "the display history remains bounded independently"
    );
    assert!(
        fixture.candidate(false, 7).await.is_none(),
        "eleven failed sessions exhaust the configured ten restarts"
    );
}

fn due(state: &DesktopState, instance_id: &str) -> RuntimeRestartScheduleEntry {
    let mut scheduler = state.runtime_restart_scheduler.lock().unwrap();
    scheduler
        .schedule(RuntimeRestartScheduleRequest {
            instance_id: instance_id.to_owned(),
            instance_name: String::from("Fixture"),
            backoff: Duration::ZERO,
            recent_crash_count: 1,
            exit_code: Some(7),
        })
        .unwrap();
    scheduler.take_due().pop().unwrap()
}

#[test]
fn runtime_restart_stop_invalidates_taken_ticket_and_manual_start_does_not_revive_it() {
    let state = DesktopState::default();
    let entry = due(&state, "world");
    let mut scheduler = state.runtime_restart_scheduler.lock().unwrap();
    assert!(scheduler.entry_is_current(&entry));
    scheduler.request_stop("world");
    assert!(!scheduler.accepts_exit("world"));
    assert!(!scheduler.restart_allowed("world"));
    scheduler.reset_for_manual_start("world");
    assert!(!scheduler.entry_is_current(&entry));
    assert!(!scheduler.restart_allowed("world"));
    scheduler
        .schedule(RuntimeRestartScheduleRequest {
            instance_id: String::from("world"),
            instance_name: String::from("New run"),
            backoff: Duration::ZERO,
            recent_crash_count: 1,
            exit_code: Some(8),
        })
        .unwrap();
    assert!(
        scheduler.take_due().is_empty(),
        "new ticket must not authorize old task during its lock handoff"
    );
    scheduler.finish_restart(&entry);
    let replacement = scheduler.take_due().pop().unwrap();
    assert!(scheduler.entry_is_current(&replacement));
    scheduler.finish_restart(&entry);
    assert!(
        scheduler.entry_is_current(&replacement),
        "old cleanup cannot erase new ticket"
    );
}

#[tokio::test]
async fn runtime_restart_clean_exit_and_explicit_stop_follow_policy_without_new_failure() {
    let fixture = Fixture::new(3, true).await;
    assert!(fixture.candidate(false, 0).await.is_none());
    let all_exits = Fixture::new(3, false).await;
    assert!(all_exits.candidate(true, 7).await.is_none());
    assert!(all_exits.candidate(true, 0).await.is_none());
    let policy = runtime_restart_policy_from_settings(
        r#"{"runtime_restart":{"enabled":true,"only_nonzero_exit":false}}"#,
    );
    assert!(exit_marks_failed_session(false, Some(0), &policy));
    assert!(!exit_marks_failed_session(true, Some(0), &policy));
    assert!(
        exit_marks_failed_session(true, Some(7), &policy),
        "actual exit errors remain recorded"
    );
}

#[tokio::test]
async fn runtime_restart_limits_one_through_ten_bound_complete_sessions() {
    let fixture = Fixture::new(10, true).await;
    for index in 1..=11 {
        let session = format!("pair-{index}");
        let master = fixture.process(&session, "master").await;
        let caves = fixture.process(&session, "caves").await;
        fixture.exit(caves, 9, true).await;
        fixture.exit(master, 0, false).await;
        assert_eq!(
            app_storage::read_instance_restart_failure_count(
                &fixture.storage.paths,
                &fixture.instance_id,
                11
            )
            .await
            .unwrap(),
            index
        );
        for max_restarts in 1..=10 {
            let count = app_storage::read_instance_restart_failure_count(
                &fixture.storage.paths,
                &fixture.instance_id,
                max_restarts + 1,
            )
            .await
            .unwrap();
            assert_eq!(
                count > max_restarts,
                index > max_restarts,
                "limit {max_restarts}, session {index}"
            );
        }
    }
    let clean = fixture.process("clean-manual-session", "master").await;
    fixture.exit(clean, 0, false).await;
    assert_eq!(
        app_storage::read_instance_restart_failure_count(
            &fixture.storage.paths,
            &fixture.instance_id,
            11
        )
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn runtime_restart_saves_surviving_dst_shard_before_permitting_new_launch() {
    let fixture = Fixture::new(3, true).await;
    let state = DesktopState::default();
    let master = fixture.process("world-session", "master").await;
    let caves = fixture.process("world-session", "caves").await;
    fixture.exit(caves, 7, true).await;
    let entry = due(&state, &fixture.instance_id);
    let _lock = state.acquire_instance_mutation(&fixture.instance_id).await;
    prepare_restart_survivors(&state, &fixture.storage, &entry, |active| {
        assert_eq!(active.session_id.as_deref(), Some("world-session"));
        assert_eq!(
            active
                .processes
                .iter()
                .filter(|process| process.status == "running")
                .count(),
            1
        );
        async {
            fixture.exit(master, 0, false).await;
            Ok(())
        }
    })
    .await
    .unwrap();
    assert!(
        read_active_instance_run(&fixture.storage.paths, &fixture.instance_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        app_storage::read_instance_restart_failure_count(
            &fixture.storage.paths,
            &fixture.instance_id,
            4
        )
        .await
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn runtime_restart_save_failure_keeps_survivor_and_blocks_new_launch() {
    let fixture = Fixture::new(3, true).await;
    let state = DesktopState::default();
    fixture.process("world-session", "master").await;
    let caves = fixture.process("world-session", "caves").await;
    fixture.exit(caves, 7, true).await;
    let entry = due(&state, &fixture.instance_id);
    let error = prepare_restart_survivors(&state, &fixture.storage, &entry, |_| async {
        Err(String::from("save acknowledgement missing"))
    })
    .await
    .unwrap_err();
    assert_eq!(error, "save acknowledgement missing");
    assert!(
        read_active_instance_run(&fixture.storage.paths, &fixture.instance_id)
            .await
            .unwrap()
            .is_some()
    );
    state
        .runtime_restart_scheduler
        .lock()
        .unwrap()
        .finish_restart(&entry);
    assert!(
        !state
            .runtime_restart_scheduler
            .lock()
            .unwrap()
            .accepts_exit(&fixture.instance_id),
        "a late exit after unconfirmed save must not re-arm recovery"
    );
}

#[tokio::test]
async fn runtime_restart_stop_during_survivor_save_prevents_relaunch() {
    let fixture = Fixture::new(3, true).await;
    let state = DesktopState::default();
    let master = fixture.process("world-session", "master").await;
    let caves = fixture.process("world-session", "caves").await;
    fixture.exit(caves, 7, true).await;
    let entry = due(&state, &fixture.instance_id);
    let result = prepare_restart_survivors(&state, &fixture.storage, &entry, |_| async {
        state
            .runtime_restart_scheduler
            .lock()
            .unwrap()
            .request_stop(&fixture.instance_id);
        fixture.exit(master, 0, false).await;
        Ok(())
    })
    .await;
    assert!(result.unwrap_err().contains("cancelled"));
}

#[test]
fn runtime_restart_dropped_work_releases_all_taken_tickets() {
    let state = DesktopState::default();
    let first = due(&state, "first");
    let second = due(&state, "second");
    let flights = vec![
        RestartFlight {
            state: &state,
            entry: first,
        },
        RestartFlight {
            state: &state,
            entry: second,
        },
    ];
    drop(flights);
    let scheduler = state.runtime_restart_scheduler.lock().unwrap();
    assert!(!scheduler.restart_allowed("first"));
    assert!(!scheduler.restart_allowed("second"));
}

#[tokio::test]
async fn runtime_restart_never_stops_a_healthy_replacement_run() {
    let fixture = Fixture::new(3, true).await;
    let state = DesktopState::default();
    fixture.process("healthy-session", "master").await;
    let entry = due(&state, &fixture.instance_id);
    let result = prepare_restart_survivors(&state, &fixture.storage, &entry, |_| async {
        panic!("an unrelated healthy run must not receive a shutdown command")
    })
    .await;
    assert!(result.unwrap_err().contains("unrelated active run"));
    assert!(
        read_active_instance_run(&fixture.storage.paths, &fixture.instance_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn runtime_restart_requires_confirmed_survivor_exit_after_save_operation() {
    let fixture = Fixture::new(3, true).await;
    let state = DesktopState::default();
    fixture.process("world-session", "master").await;
    let caves = fixture.process("world-session", "caves").await;
    fixture.exit(caves, 7, true).await;
    let entry = due(&state, &fixture.instance_id);
    let error = prepare_restart_survivors(&state, &fixture.storage, &entry, |_| async { Ok(()) })
        .await
        .unwrap_err();
    assert!(error.contains("every surviving shard"));
    assert!(
        read_active_instance_run(&fixture.storage.paths, &fixture.instance_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[test]
fn runtime_restart_new_run_failure_can_queue_before_previous_restart_finishes() {
    let state = DesktopState::default();
    let first = due(&state, "world");
    let mut scheduler = state.runtime_restart_scheduler.lock().unwrap();
    assert!(scheduler.expect_survivor_shutdown(&first, [12]));
    assert!(scheduler.exit_is_expected("world", 12));
    assert!(
        !scheduler.exit_is_expected("world", 15),
        "new process failure is not a survivor shutdown"
    );
    assert!(
        scheduler
            .schedule(RuntimeRestartScheduleRequest {
                instance_id: String::from("world"),
                instance_name: String::from("New failed run"),
                backoff: Duration::ZERO,
                recent_crash_count: 2,
                exit_code: Some(8),
            })
            .is_some(),
        "a new run can fail between registration and completion of its launch future"
    );
    assert!(scheduler.take_due().is_empty());
    scheduler.finish_restart(&first);
    assert_eq!(scheduler.take_due().len(), 1);
}
