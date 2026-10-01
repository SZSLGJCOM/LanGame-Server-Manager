use super::*;
use app_storage::StoragePaths;

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    app: tauri::App<tauri::test::MockRuntime>,
    details: InstanceDetails,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-periodic-save-{}", uuid::Uuid::new_v4()));
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
        let descriptor = find_descriptor(&descriptors, "unturned").unwrap();
        sync_modules(&paths, std::slice::from_ref(descriptor))
            .await
            .unwrap();
        // Creation requires a private copy of an installed package. This
        // suite runs its own stdin capture process, not a native game server.
        let package = paths.games_root.join("unturned");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("fixture-package.txt"), b"managed save fixture").unwrap();
        let created = create_instance(
            &paths,
            descriptor,
            CreateInstanceInput {
                name: String::from("Periodic save fixture"),
                module_id: String::from("unturned"),
            },
        )
        .await
        .unwrap();
        let details = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let storage = StorageBootstrap {
            settings: paths.settings(),
            storage_status: paths.probe_status(),
            paths,
        };
        let state = DesktopState::default();
        state.app_state.write().unwrap().settings = storage.settings.clone();
        let app = tauri::test::mock_builder()
            .manage(state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        Self {
            root,
            storage,
            app,
            details,
        }
    }

    async fn start_capture(&self) -> PathBuf {
        crate::commands::tests::register_smoke_stdin_capture_process(
            self.app.state::<DesktopState>(),
            &self.storage,
            &self.details,
            &self.root,
            "main",
        )
        .await
        .unwrap()
    }

    async fn update_interval(
        &self,
        interval: Value,
    ) -> Result<InstanceDetails, app_storage::StorageError> {
        let details = read_instance_details(&self.storage.paths, &self.details.summary.id)
            .await
            .unwrap();
        let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
        settings["managed_save_interval_seconds"] = interval;
        let result = app_storage::update_instance(
            &self.storage.paths,
            UpdateInstanceInput {
                id: details.summary.id,
                bind_ip: details.summary.bind_ip,
                auto_backup_on_stop: details.auto_backup_on_stop,
                backup_retention_count: details.backup_retention_count,
                settings_json: settings.to_string(),
                ports: details.ports,
            },
        )
        .await;
        if result.is_ok() {
            invalidate_instance_policy(&self.app.state::<DesktopState>());
        }
        result
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let state = self.app.state::<DesktopState>();
        let mut runtime = state.runtime_supervisor.lock().unwrap();
        for instance in runtime.tracked_instances() {
            if let Some(mut running) = runtime.take_running_for_stop(&instance.summary.id) {
                stop_managed_instance(&mut running).unwrap();
            }
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn managed_save_interval_accepts_defaults_disable_and_bounded_seconds() {
    assert_eq!(save_interval("{}").unwrap(), Duration::from_secs(300));
    for seconds in [0, 1, 300, 86400] {
        assert_eq!(
            save_interval(&json!({ "managed_save_interval_seconds": seconds }).to_string())
                .unwrap(),
            Duration::from_secs(seconds)
        );
    }
    for invalid in [
        json!(-1),
        json!(1.5),
        json!(86401),
        json!("300"),
        Value::Null,
    ] {
        assert!(
            save_interval(&json!({ "managed_save_interval_seconds": invalid }).to_string())
                .is_err()
        );
    }
}

#[test]
fn managed_save_schedule_resets_only_when_interval_changes_and_bounds_failure_retry() {
    let now = Instant::now();
    let mut schedule = SaveSchedule::new(4);
    schedule.configure(Duration::from_secs(300), 1, now);
    assert_eq!(schedule.next_due, Some(now + Duration::from_secs(300)));
    schedule.configure(Duration::from_secs(300), 2, now + Duration::from_secs(20));
    assert_eq!(schedule.next_due, Some(now + Duration::from_secs(300)));
    schedule.configure(Duration::from_secs(1), 3, now);
    schedule.completed(false, now + Duration::from_secs(5));
    assert_eq!(schedule.next_due, Some(now + Duration::from_secs(6)));
    schedule.completed(true, now);
    assert_eq!(schedule.next_due, Some(now + Duration::from_secs(60)));
    schedule.configure(Duration::ZERO, 4, now);
    assert!(schedule.next_due.is_none());
}

#[test]
fn managed_save_worker_limits_repeated_storage_error_logs() {
    let mut worker = ManagedSaveWorker::default();
    let now = Instant::now();
    assert!(worker.should_log_error(now));
    assert!(!worker.should_log_error(now + Duration::from_secs(59)));
    assert!(worker.should_log_error(now + Duration::from_secs(60)));
}

#[tokio::test]
async fn managed_save_schema_rejects_invalid_values_without_changing_persisted_policy() {
    let fixture = Fixture::new().await;
    for invalid in [json!(-1), json!(1.5), json!(86401), json!("300")] {
        assert!(fixture.update_interval(invalid).await.is_err());
        let details = read_instance_details(&fixture.storage.paths, &fixture.details.summary.id)
            .await
            .unwrap();
        assert_eq!(
            save_interval(&details.settings_json).unwrap(),
            Duration::from_secs(300)
        );
    }
}

#[tokio::test]
async fn managed_save_writes_declared_save_only_after_interval_for_owned_running_process() {
    let fixture = Fixture::new().await;
    let capture = fixture.start_capture().await;
    let state = fixture.app.state::<DesktopState>();
    let mut worker = ManagedSaveWorker::default();
    let now = Instant::now();
    worker.tick(&state, &fixture.storage, now).await.unwrap();
    assert!(!capture.exists());
    worker
        .tick(&state, &fixture.storage, now + Duration::from_secs(299))
        .await
        .unwrap();
    assert!(!capture.exists());
    worker
        .tick(&state, &fixture.storage, now + Duration::from_secs(300))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !capture.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        fs::read_to_string(capture)
            .unwrap()
            .trim_start_matches('\u{feff}')
            .trim(),
        "Save"
    );
    let log = fs::read_to_string(desktop_app_log_path(&fixture.storage)).unwrap();
    assert!(log.contains("instance.periodic_save.command_written"));
    assert!(!log.contains("instance.periodic_save.failed"));
}

#[tokio::test]
async fn managed_save_disable_and_shutdown_prevent_due_writes() {
    let fixture = Fixture::new().await;
    let capture = fixture.start_capture().await;
    let state = fixture.app.state::<DesktopState>();
    let mut worker = ManagedSaveWorker::default();
    let now = Instant::now();
    worker.tick(&state, &fixture.storage, now).await.unwrap();
    fixture.update_interval(json!(0)).await.unwrap();
    worker
        .tick(&state, &fixture.storage, now + Duration::from_secs(300))
        .await
        .unwrap();
    assert!(!capture.exists());
    assert!(
        worker.schedules[&fixture.details.summary.id]
            .next_due
            .is_none()
    );
    fixture.update_interval(json!(1)).await.unwrap();
    worker
        .tick(&state, &fixture.storage, now + Duration::from_secs(301))
        .await
        .unwrap();
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    worker
        .tick(&state, &fixture.storage, now + Duration::from_secs(302))
        .await
        .unwrap();
    assert!(!capture.exists());
    assert!(worker.schedules.is_empty());
}

#[tokio::test]
async fn managed_save_prunes_stopped_runs_and_defers_pending_confirmation() {
    let fixture = Fixture::new().await;
    let capture = fixture.start_capture().await;
    let state = fixture.app.state::<DesktopState>();
    let mut worker = ManagedSaveWorker::default();
    let now = Instant::now();
    worker.tick(&state, &fixture.storage, now).await.unwrap();
    worker
        .schedules
        .get_mut(&fixture.details.summary.id)
        .unwrap()
        .completion = Some(RuntimeStdinWriteCompletion::default());
    worker
        .tick(&state, &fixture.storage, now + Duration::from_secs(600))
        .await
        .unwrap();
    assert!(!capture.exists());
    let mut running = state
        .runtime_supervisor
        .lock()
        .unwrap()
        .take_running_for_stop(&fixture.details.summary.id)
        .unwrap();
    let stopped = stop_managed_instance(&mut running).unwrap();
    worker
        .tick(&state, &fixture.storage, now + Duration::from_secs(601))
        .await
        .unwrap();
    assert!(worker.schedules.is_empty());
    assert!(
        read_active_instance_run(&fixture.storage.paths, &fixture.details.summary.id)
            .await
            .unwrap()
            .is_some(),
        "a database record alone must never authorize stdin writes"
    );
    for process in stopped {
        mark_instance_process_stopped(
            &fixture.storage.paths,
            &fixture.details.summary.id,
            process.run_id,
            process.exit_code,
            false,
        )
        .await
        .unwrap();
    }
    fixture.start_capture().await;
    worker
        .tick(&state, &fixture.storage, now + Duration::from_secs(900))
        .await
        .unwrap();
    assert!(
        !capture.exists(),
        "a replacement process must get a fresh timer"
    );
    assert_eq!(
        worker.schedules[&fixture.details.summary.id].next_due,
        Some(now + Duration::from_secs(1200))
    );
}
