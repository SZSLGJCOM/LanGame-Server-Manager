use super::*;
use std::io::Write;

const WORLD: &str = "Bootstrap world";
#[cfg(windows)]
use crate::commands::commands_runtime_lifecycle::native_console_fixture::{self, INTERRUPTED};

fn save_markers(world: &str) -> String {
    format!(
        "[2026.11.11][123]LogSpudSubsystem: Save to slot {world}: Success\n[2026.11.11][123]LogPersistence: [DedicatedServer] operator ()() : Save completed SUCCESSFULLY (slot: {world})\n"
    )
}

struct LogFixture(PathBuf);

impl LogFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lg-dragon-bootstrap-log-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> PathBuf {
        self.0.join("RSDragonwilds.log")
    }

    fn append(&self, text: &str) {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
}

impl Drop for LogFixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn bootstrap_requires_both_complete_save_markers_for_its_world_after_capture() {
    let fixture = LogFixture::new();
    fixture.append(&save_markers(WORLD));
    let mut readiness = WorldReadiness::before_spawn(fixture.path(), WORLD.into()).unwrap();
    assert!(
        !readiness.poll(|| false).unwrap(),
        "old save is not this boot"
    );
    fixture.append(&save_markers("Other world"));
    assert!(
        !readiness.poll(|| false).unwrap(),
        "wrong slot cannot authorize the world"
    );
    let markers = save_markers(WORLD);
    let split = markers.find('\n').unwrap() + 1;
    fixture.append(&markers[..split]);
    assert!(
        !readiness.poll(|| false).unwrap(),
        "SPUD alone is insufficient"
    );
    fixture.append(markers[split..].trim_end());
    assert!(
        !readiness.poll(|| false).unwrap(),
        "partial native record is insufficient"
    );
    fixture.append("\n");
    assert!(readiness.poll(|| false).unwrap());
}

#[test]
fn bootstrap_does_not_join_a_prelaunch_fragment_with_new_output() {
    let fixture = LogFixture::new();
    fixture.append(&format!("LogSpudSubsystem: Save to slot {WORLD}:"));
    let mut readiness = WorldReadiness::before_spawn(fixture.path(), WORLD.into()).unwrap();
    fixture.append(&format!(
        " Success\nLogPersistence: [DedicatedServer] Save completed SUCCESSFULLY (slot: {WORLD})\n"
    ));
    assert!(!readiness.poll(|| false).unwrap());
    fixture.append(&save_markers(WORLD));
    assert!(readiness.poll(|| false).unwrap());
}

#[test]
fn bootstrap_native_rotation_requires_success_in_the_new_log_generation() {
    let fixture = LogFixture::new();
    let mut readiness = WorldReadiness::before_spawn(fixture.path(), WORLD.into()).unwrap();
    fixture.append(&save_markers(WORLD));
    assert!(readiness.poll(|| false).unwrap());
    std::fs::rename(fixture.path(), fixture.0.join("previous.log")).unwrap();
    fixture.append("LogInit: starting another native log generation\n");
    assert!(!readiness.poll(|| false).unwrap());
    fixture.append(&save_markers(WORLD));
    assert!(readiness.poll(|| false).unwrap());
}

#[test]
fn bootstrap_cancelled_log_read_does_not_consume_save_evidence() {
    let fixture = LogFixture::new();
    let mut readiness = WorldReadiness::before_spawn(fixture.path(), WORLD.into()).unwrap();
    fixture.append(&save_markers(WORLD));
    assert!(readiness.poll(|| true).unwrap_err().contains("cancelled"));
    assert!(
        readiness.after_stopped().unwrap(),
        "normal stop must recover this launch's completed save even when cancellation prevented the polling read"
    );
}

#[test]
fn bootstrap_cancelled_recovery_never_claims_a_stale_or_incomplete_save() {
    let fixture = LogFixture::new();
    fixture.append(&save_markers(WORLD));
    let mut readiness = WorldReadiness::before_spawn(fixture.path(), WORLD.into()).unwrap();
    assert!(
        !readiness.after_stopped().unwrap(),
        "prelaunch success cannot claim an unobserved world"
    );
    fixture.append(&format!(
        "LogSpudSubsystem: Save to slot {WORLD}: Success\n"
    ));
    assert!(
        !readiness.after_stopped().unwrap(),
        "cancellation still requires native persistence completion"
    );
}

#[cfg(windows)]
mod native_boundary {
    use super::*;

    const CHILD_TEST: &str = "commands::commands_runtime_lifecycle::dragonwilds_lifecycle::tests::native_boundary::dragonwilds_initialization_child";

    #[test]
    #[ignore = "synthetic owned Unreal console boundary, invoked only by the bootstrap cancellation test"]
    fn dragonwilds_initialization_child() {
        let root = PathBuf::from(std::env::var_os("LANGAME_WINDROSE_BOOTSTRAP_FIXTURE").unwrap());
        assert!(
            root.canonicalize()
                .unwrap()
                .starts_with(std::env::temp_dir().canonicalize().unwrap())
        );
        native_console_fixture::create();
        std::fs::write(root.join("bootstrap-pid"), std::process::id().to_string()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !INTERRUPTED.load(Ordering::SeqCst) {
            native_console_fixture::pump();
            assert!(
                Instant::now() < deadline,
                "owned bootstrap must receive native quit"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::write(root.join("bootstrap-flushed"), b"normal synthetic exit").unwrap();
    }

    struct Fixture {
        root: PathBuf,
        paths: app_storage::StoragePaths,
        instance: InstanceDetails,
        plan: ProcessLaunchPlan,
    }

    impl Fixture {
        async fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "lg-dragon-bootstrap-{}",
                uuid::Uuid::new_v4().simple()
            ));
            let repository = std::env::var_os("LANGAME_SMOKE_SOURCE_ROOT")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .canonicalize()
                .expect("native test source root");
            let paths = app_storage::StoragePaths {
                app_data_root: root.clone(),
                settings_path: root.join("settings.json"),
                database_path: root.join("database/lgsm.db"),
                logs_root: root.join("logs"),
                modules_root: repository.join("modules"),
                migrations_root: repository.join("migrations"),
                steamcmd_root: root.join("steamcmd"),
                games_root: root.join("games"),
                instances_root: root.join("instances"),
                archives_root: root.join("instances/.trash"),
            };
            let descriptor = discover_modules(&paths.modules_root)
                .unwrap()
                .into_iter()
                .find(|module| module.summary.id == "runescapedragonwilds")
                .unwrap();
            let install = paths.games_root.join("runescapedragonwilds");
            std::fs::create_dir_all(&install).unwrap();
            std::fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
            std::fs::write(
                install.join("RSDragonwildsServer.exe"),
                b"synthetic native program boundary",
            )
            .unwrap();
            app_storage::record_library_program_baseline(&install, &descriptor, true, None)
                .unwrap();
            initialize_database(&paths).await.unwrap();
            sync_modules(&paths, std::slice::from_ref(&descriptor))
                .await
                .unwrap();
            sync_game_installs(
                &paths,
                &[GameInstallSyncRecord {
                    module_id: "runescapedragonwilds".into(),
                    install_root: install.to_string_lossy().into_owned(),
                    install_state: InstallState::Installed,
                    current_version: Some("fixture".into()),
                    mark_verified: true,
                }],
            )
            .await
            .unwrap();
            let created = create_instance(
                &paths,
                &descriptor,
                CreateInstanceInput {
                    module_id: "runescapedragonwilds".into(),
                    name: "Dragonwilds bootstrap fixture".into(),
                },
            )
            .await
            .unwrap();
            let instance = read_instance_details(&paths, &created.summary.id)
                .await
                .unwrap();
            let draft = app_storage::read_dragonwilds_world_settings(&paths, &instance.summary.id)
                .await
                .unwrap();
            assert_eq!(draft.status, "new");
            app_storage::write_dragonwilds_world_settings(
                &paths,
                app_core::dragonwilds_world::WriteDragonwildsWorldSettingsInput {
                    instance_id: instance.summary.id.clone(),
                    world_file: None,
                    expected_revision: draft.revision.unwrap(),
                    world_mode: app_core::dragonwilds_world::DragonwildsWorldMode::Custom,
                    values: [(
                        "Difficulty.Progression.BuildingMaterialCostScale".into(),
                        1.5,
                    )]
                    .into(),
                },
            )
            .await
            .unwrap();
            let instance = app_storage::materialize_instance_configuration_for_start(
                &paths,
                &instance.summary.id,
            )
            .await
            .unwrap();
            let launch_plan =
                build_instance_launch_preview(&paths.settings(), &descriptor, &instance).unwrap();
            let plan = ProcessLaunchPlan {
                process_key: "main".into(),
                display_name: "Dragonwilds fixture".into(),
                log_path: root.join("bootstrap.log").to_string_lossy().into_owned(),
                launch_plan,
            };
            Self {
                root,
                paths,
                instance,
                plan,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bootstrap_cancel_stops_owned_tree_and_restores_password_without_consuming_draft() {
        let fixture = Fixture::new().await;
        let formal_args = fixture.plan.launch_plan.args.clone();
        let bootstrap = initialization_plan(&fixture.plan).unwrap();
        assert!(
            bootstrap
                .launch_plan
                .args
                .iter()
                .any(|arg| arg == "-MULTIHOME=127.0.0.1")
        );
        assert_eq!(fixture.plan.launch_plan.args, formal_args);
        let state = DesktopState::default();
        let id = &fixture.instance.summary.id;
        let reservation = match state.try_reserve_runtime_start(id, "manual").unwrap() {
            RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
            other => panic!("unexpected reservation: {other:?}"),
        };
        let runtime = PathBuf::from(&fixture.plan.launch_plan.install_root);
        let ini = runtime.join("RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini");
        let original = std::fs::read(&ini).unwrap();
        let prepared = app_storage::prepare_dragonwilds_bootstrap(&fixture.paths, id)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(std::fs::read(&ini).unwrap(), original);
        let group = state
            .runtime_resource_admission
            .reserve(
                id,
                &fixture.plan.launch_plan.performance_policy.resource_limits,
            )
            .unwrap();
        let mut plan = bootstrap;
        plan.launch_plan.executable_path = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        plan.launch_plan.executable_exists = true;
        plan.launch_plan.ready_to_launch = true;
        plan.launch_plan.validation_issues.clear();
        plan.launch_plan.args = ["--exact", CHILD_TEST, "--ignored", "--nocapture"]
            .map(String::from)
            .to_vec();
        // Reuse the synthetic Unreal GUI console contract, not a game/world mock.
        plan.launch_plan.environment.insert(
            "LANGAME_WINDROSE_BOOTSTRAP_FIXTURE".into(),
            runtime.to_string_lossy().into_owned(),
        );
        let worker = tokio::spawn(async move {
            let worker_lease = reservation.clone();
            spawn_storage_context_task(&worker_lease, async move {
                run_bootstrap(prepared, plan, reservation, group).await
            })
            .await
            .unwrap()
        });
        let pid = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if let Ok(pid) = std::fs::read_to_string(runtime.join("bootstrap-pid")) {
                    break pid.parse::<u32>().unwrap();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(id));
        let details = read_instance_details(&fixture.paths, id).await.unwrap();
        assert!(
            details.active_run.is_none(),
            "initialization is not a formal run"
        );
        let cancellation = state
            .request_runtime_start_cancellation(id)
            .unwrap()
            .unwrap();
        let result = worker.await.unwrap();
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("cancel must not finish initialization"),
        };
        assert!(error.contains("cancelled"), "{error}");
        cancellation
            .wait_until_settled(Duration::from_secs(5))
            .await
            .unwrap();
        assert!(!app_runtime::process_is_running(pid).unwrap());
        assert_eq!(
            std::fs::read(runtime.join("bootstrap-command")).unwrap(),
            b"quit"
        );
        assert!(runtime.join("bootstrap-flushed").is_file());
        assert_eq!(std::fs::read(&ini).unwrap(), original);
        let draft = app_storage::read_dragonwilds_world_settings(&fixture.paths, id)
            .await
            .unwrap();
        assert_eq!(draft.status, "new");
        assert_eq!(
            draft
                .values
                .get("Difficulty.Progression.BuildingMaterialCostScale"),
            Some(&1.5)
        );
        let retry = app_storage::prepare_dragonwilds_bootstrap(&fixture.paths, id)
            .await
            .unwrap()
            .unwrap();
        retry.abort_after_stopped().await.unwrap();
    }
}
