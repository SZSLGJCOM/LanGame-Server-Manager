use super::*;
use crate::state::AutostartQueue;
use app_storage::StoragePaths;

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-autostart-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("db")).expect("test database directory");
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
        Self {
            root,
            storage: StorageBootstrap {
                settings: paths.settings(),
                storage_status: paths.probe_status(),
                paths,
            },
        }
    }

    fn app(&self) -> tauri::App<tauri::test::MockRuntime> {
        let state = DesktopState::default();
        state.app_state.write().expect("state").settings = self.storage.settings.clone();
        tauri::test::mock_builder()
            .manage(state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock desktop")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove isolated autostart fixture");
    }
}

fn instance(id: &str, autostart: bool, status: InstanceStatus) -> InstanceSummary {
    InstanceSummary {
        id: String::from(id),
        name: String::from(id),
        module_id: String::from("minecraft"),
        active_process_count: usize::from(matches!(&status, InstanceStatus::Running)),
        status,
        bind_ip: String::from("127.0.0.1"),
        port_count: 0,
        autostart,
    }
}

fn capture(
    queue: &AutostartQueue,
    storage: &StorageBootstrap,
    instances: &[InstanceSummary],
) -> AutostartBatch {
    queue.mark_modules_ready().expect("modules ready");
    queue.capture(storage, instances).expect("capture");
    queue.take().expect("take queue").expect("batch")
}

#[test]
fn autostart_captures_enabled_idle_instances_once_after_modules_are_ready() {
    let fixture = Fixture::new();
    let queue = AutostartQueue::default();
    let instances = vec![
        instance("off", false, InstanceStatus::Stopped),
        instance("stopped", true, InstanceStatus::Stopped),
        instance("running", true, InstanceStatus::Running),
        instance("starting", true, InstanceStatus::Starting),
        instance("stopping", true, InstanceStatus::Stopping),
        instance("error", true, InstanceStatus::Error),
    ];
    queue.capture(&fixture.storage, &instances).unwrap();
    assert!(queue.take().unwrap().is_none());
    let batch = capture(&queue, &fixture.storage, &instances);
    assert_eq!(
        batch
            .instances
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["stopped", "error"]
    );
    queue.capture(&fixture.storage, &instances).unwrap();
    assert!(
        queue.take().unwrap().is_none(),
        "polling must not launch a second batch"
    );
}

#[test]
fn autostart_empty_initial_snapshot_is_not_rearmed_by_later_settings_changes() {
    let fixture = Fixture::new();
    let queue = AutostartQueue::default();
    queue.mark_modules_ready().unwrap();
    queue.capture(&fixture.storage, &[]).unwrap();
    queue
        .capture(
            &fixture.storage,
            &[instance("later", true, InstanceStatus::Stopped)],
        )
        .unwrap();
    assert!(queue.take().unwrap().is_none());
}

#[test]
fn autostart_concurrent_dispatchers_take_only_one_batch() {
    let fixture = Fixture::new();
    let queue = Arc::new(AutostartQueue::default());
    queue.mark_modules_ready().unwrap();
    queue
        .capture(
            &fixture.storage,
            &[instance("one", true, InstanceStatus::Stopped)],
        )
        .unwrap();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let queue = Arc::clone(&queue);
            std::thread::spawn(move || queue.take().unwrap().is_some())
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .filter_map(|thread| thread.join().unwrap().then_some(()))
            .count(),
        1
    );
}

#[tokio::test]
async fn autostart_reports_failure_and_continues_remaining_instances_in_order() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let batch = capture(
        &state.autostart,
        &fixture.storage,
        &[
            instance("broken", true, InstanceStatus::Stopped),
            instance("healthy", true, InstanceStatus::Stopped),
        ],
    );
    let mut started = Vec::new();
    run_autostart_batch_with(&state, &batch, |instance| {
        started.push(instance.id.clone());
        async move {
            if instance.id == "broken" {
                Err(String::from("fixture executable missing"))
            } else {
                Ok(())
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(started, ["broken", "healthy"]);
    let app_state = state.app_state.read().unwrap();
    assert_eq!(app_state.jobs.len(), 2);
    assert!(matches!(app_state.jobs[0].status, JobStatus::Completed));
    assert!(matches!(app_state.jobs[1].status, JobStatus::Failed));
    assert_eq!(
        app_state.jobs[1].output_excerpt.as_deref(),
        Some("fixture executable missing")
    );
    assert!(!state.autostart.is_eligible("broken").unwrap());
    assert!(!state.autostart.is_eligible("healthy").unwrap());
}

#[tokio::test]
async fn autostart_manual_intervention_cancels_queued_instance() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let batch = capture(
        &state.autostart,
        &fixture.storage,
        &[
            instance("cancelled", true, InstanceStatus::Stopped),
            instance("already-running", true, InstanceStatus::Stopped),
        ],
    );
    state.autostart.cancel("cancelled").unwrap();
    let mut started = Vec::new();
    run_autostart_batch_with(&state, &batch, |instance| {
        started.push(instance.id);
        async {
            Err(
                commands_runtime_lifecycle::InstanceRunConflict::ActiveRunRecord
                    .into_error("already-running"),
            )
        }
    })
    .await
    .unwrap();
    assert_eq!(started, ["already-running"]);
    let app_state = state.app_state.read().unwrap();
    assert!(
        app_state
            .jobs
            .iter()
            .all(|job| matches!(job.status, JobStatus::Cancelled))
    );
    assert_eq!(
        app_state.jobs[0].detail.as_deref(),
        Some(AUTOSTART_ALREADY_ACTIVE)
    );
}

#[tokio::test]
async fn autostart_shutdown_cancels_unstarted_instances() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let batch = capture(
        &state.autostart,
        &fixture.storage,
        &[
            instance("first", true, InstanceStatus::Stopped),
            instance("second", true, InstanceStatus::Stopped),
        ],
    );
    let mut started = Vec::new();
    run_autostart_batch_with(&state, &batch, |instance| {
        started.push(instance.id);
        state.shutdown_in_progress.store(true, Ordering::SeqCst);
        async { Ok(()) }
    })
    .await
    .unwrap();
    assert_eq!(started, ["first"]);
    assert!(!state.autostart.is_eligible("second").unwrap());
}

#[tokio::test]
async fn autostart_rejects_a_replaced_storage_context_before_launch() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let batch = capture(
        &state.autostart,
        &fixture.storage,
        &[instance("one", true, InstanceStatus::Stopped)],
    );
    state
        .app_state
        .write()
        .unwrap()
        .settings
        .servers_root
        .push_str("-changed");
    let result = run_autostart_batch_with(&state, &batch, |_| async {
        panic!("must not launch into changed paths")
    })
    .await;
    assert!(
        result
            .unwrap_err()
            .contains("paths that have since changed")
    );
}

#[tokio::test]
async fn autostart_stays_cancelled_when_busy_shutdown_resets_its_in_progress_flag() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let batch = capture(
        &state.autostart,
        &fixture.storage,
        &[instance("queued", true, InstanceStatus::Stopped)],
    );
    let _operation = state
        .begin_storage_context_operation("autostart fixture worker")
        .unwrap();
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    assert!(state.begin_storage_shutdown_exclusive().is_err());
    state.shutdown_in_progress.store(false, Ordering::SeqCst);
    run_autostart_batch_with(&state, &batch, |_| async {
        panic!("a failed close attempt must not resume automatic launches")
    })
    .await
    .unwrap();
    assert!(
        state
            .app_state
            .read()
            .unwrap()
            .jobs
            .iter()
            .all(|job| matches!(job.status, JobStatus::Cancelled))
    );
}

#[tokio::test]
async fn autostart_in_progress_is_not_cancelled_by_a_conflicting_manual_start() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let _batch = capture(
        &state.autostart,
        &fixture.storage,
        &[instance("reserved", true, InstanceStatus::Stopped)],
    );
    let lease = commands_runtime_lifecycle::reserve_runtime_instance_start(
        &state,
        &fixture.storage,
        "reserved",
        AUTOSTART_SOURCE,
    )
    .unwrap();
    let result = start_instance_process_after_reconcile(
        None,
        &state,
        &fixture.storage,
        String::from("reserved"),
        "manual",
        None,
    )
    .await;
    assert!(result.unwrap_err().contains("already starting"));
    assert!(state.autostart.is_eligible("reserved").unwrap());
    drop(lease);
    assert!(
        state
            .pending_runtime_start_instance_ids()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn autostart_rechecks_disabled_preference_under_the_runtime_mutation_lock() {
    let fixture = Fixture::new();
    initialize_database(&fixture.storage.paths).await.unwrap();
    let descriptors = discover_modules(&fixture.storage.paths.modules_root).unwrap();
    let descriptor = descriptors
        .iter()
        .find(|module| module.summary.id == "minecraft")
        .unwrap();
    sync_modules(&fixture.storage.paths, std::slice::from_ref(descriptor))
        .await
        .unwrap();
    crate::commands::tests::prepare_fake_registered_program(&fixture.storage.paths, descriptor)
        .await
        .unwrap();
    let created = create_instance(
        &fixture.storage.paths,
        descriptor,
        CreateInstanceInput {
            name: String::from("Autostart disabled fixture"),
            module_id: String::from("minecraft"),
        },
    )
    .await
    .unwrap();
    let enabled =
        app_storage::update_instance_autostart(&fixture.storage.paths, &created.summary.id, true)
            .await
            .unwrap();
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let _batch = capture(&state.autostart, &fixture.storage, &[enabled.summary]);
    app_storage::update_instance_autostart(&fixture.storage.paths, &created.summary.id, false)
        .await
        .unwrap();
    let result = start_instance_process_after_reconcile(
        None,
        &state,
        &fixture.storage,
        created.summary.id.clone(),
        AUTOSTART_SOURCE,
        None,
    )
    .await;
    assert_eq!(result.unwrap_err(), AUTOSTART_CANCELLED);
    assert!(
        read_active_instance_run(&fixture.storage.paths, &created.summary.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        state
            .runtime_start_reservations
            .lock()
            .unwrap()
            .pending_instance_ids()
            .is_empty()
    );
}

#[tokio::test]
async fn autostart_disable_cancels_before_waiting_for_mutation_and_preserves_save_failure() {
    let fixture = Fixture::new();
    initialize_database(&fixture.storage.paths).await.unwrap();
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let _batch = capture(
        &state.autostart,
        &fixture.storage,
        &[instance("missing-instance", true, InstanceStatus::Stopped)],
    );
    let instance_lock = state.acquire_instance_mutation("missing-instance").await;
    let operation = state
        .begin_storage_context_operation("autostart disable fixture")
        .unwrap();
    let mut save = std::pin::pin!(update_instance_autostart_with(
        &state,
        &fixture.storage,
        &operation,
        String::from("missing-instance"),
        false,
    ));
    std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(save.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(!state.autostart.is_eligible("missing-instance").unwrap());
    drop(instance_lock);
    let error = save.await.unwrap_err();
    assert!(error.contains("missing-instance"), "{error}");
    assert!(!state.autostart.is_eligible("missing-instance").unwrap());
}

#[tokio::test]
async fn autostart_persists_and_dispatches_every_repository_game() {
    let fixture = Fixture::new();
    initialize_database(&fixture.storage.paths).await.unwrap();
    let descriptors = discover_modules(&fixture.storage.paths.modules_root).unwrap();
    assert_eq!(
        descriptors.len(),
        32,
        "verify the complete supported catalog"
    );
    sync_modules(&fixture.storage.paths, &descriptors)
        .await
        .unwrap();
    for descriptor in &descriptors {
        crate::commands::tests::prepare_fake_registered_program(&fixture.storage.paths, descriptor)
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", descriptor.summary.id));
        let created = create_instance(
            &fixture.storage.paths,
            descriptor,
            CreateInstanceInput {
                name: format!("Autostart {}", descriptor.summary.id),
                module_id: descriptor.summary.id.clone(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{}: {error}", descriptor.summary.id));
        app_storage::update_instance_autostart(&fixture.storage.paths, &created.summary.id, true)
            .await
            .unwrap();
        let reloaded = read_instance_details(&fixture.storage.paths, &created.summary.id)
            .await
            .unwrap();
        assert!(reloaded.summary.autostart, "{}", descriptor.summary.id);
        let config: Value =
            serde_json::from_slice(&fs::read(&reloaded.config_file_path).unwrap()).unwrap();
        assert_eq!(config["autostart"], true, "{}", descriptor.summary.id);
    }
    let instances = list_instances(&fixture.storage.paths).await.unwrap();
    let app = fixture.app();
    let state = app.state::<DesktopState>();
    let batch = capture(&state.autostart, &fixture.storage, &instances);
    let mut dispatched = HashSet::new();
    run_autostart_batch_with(&state, &batch, |instance| {
        assert!(dispatched.insert(instance.module_id));
        async { Ok(()) }
    })
    .await
    .unwrap();
    assert_eq!(dispatched.len(), descriptors.len());
    assert!(
        descriptors
            .iter()
            .all(|item| dispatched.contains(&item.summary.id))
    );
    // The UI keeps bounded recent history; every result still has a durable log.
    let log = fs::read_to_string(desktop_app_log_path(&fixture.storage)).unwrap();
    let completed_ids: HashSet<_> = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|entry| entry["action"] == "instance.autostart.finished")
        .map(|entry| {
            assert_eq!(entry["context"]["status"], "Completed");
            entry["context"]["instance_id"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(
        completed_ids,
        instances
            .iter()
            .map(|instance| instance.id.clone())
            .collect()
    );
    assert!(state.autostart.take().unwrap().is_none());
}
