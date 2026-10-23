use super::manual_runtime_tests::player_snapshot_fixture_instance;
use super::*;
use crate::commands::commands_runtime_supervision::assign_process_log_paths_with_stamp;

struct Fixture {
    root: PathBuf,
    instance: InstanceDetails,
    plans: Vec<ProcessLaunchPlan>,
}

impl Fixture {
    fn new() -> Self {
        let root = temp_test_dir("core-run-log");
        let runtime = root.join("runtime");
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("CoreKeeperServer.exe"), b"fixture").unwrap();
        let mut instance = player_snapshot_fixture_instance("corekeeper", InstanceStatus::Stopped);
        instance.config_file_path = root
            .join("config/instance.json")
            .to_string_lossy()
            .into_owned();
        instance.saves_path = root.join("data/worlds").to_string_lossy().into_owned();
        let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
        let descriptor = discover_modules(&modules)
            .unwrap()
            .into_iter()
            .find(|descriptor| descriptor.summary.id == "corekeeper")
            .unwrap();
        let settings = AppSettings::default();
        let module = map_module_details_with_install_state(
            &settings,
            &descriptor,
            Some(runtime.to_str().unwrap()),
        );
        let launch_plan = app_runtime::build_launch_plan_with_override(
            &settings,
            &module,
            &instance,
            Some(runtime.to_str().unwrap()),
        )
        .unwrap();
        Self {
            root,
            instance,
            plans: vec![ProcessLaunchPlan {
                process_key: "main".into(),
                display_name: "Server".into(),
                log_path: String::new(),
                launch_plan,
            }],
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn logfile(plan: &LaunchPlan) -> &Path {
    let index = plan.args.iter().position(|arg| arg == "-logfile").unwrap();
    Path::new(&plan.args[index + 1])
}

pub(super) fn running_instance(console: &Path) -> InstanceDetails {
    let mut instance = player_snapshot_fixture_instance("corekeeper", InstanceStatus::Running);
    let path = console.to_string_lossy().into_owned();
    instance.active_run = Some(app_core::ActiveInstanceRun {
        run_id: 42,
        session_id: Some("core-current".into()),
        pid: Some(100),
        log_path: Some(path.clone()),
        process_count: 1,
        processes: vec![app_core::InstanceProcessState {
            run_id: 42,
            session_id: Some("core-current".into()),
            process_key: "main".into(),
            display_name: "Server".into(),
            pid: Some(100),
            process_identity: None,
            status: "running".into(),
            started_at: None,
            stopped_at: None,
            exit_code: None,
            crash_flag: false,
            log_path: Some(path),
            is_primary: true,
        }],
    });
    instance
}

#[test]
fn corekeeper_actual_launch_binds_one_run_and_preserves_colliding_logs() {
    let mut fixture = Fixture::new();
    assert_eq!(
        logfile(&fixture.plans[0].launch_plan),
        fixture.root.join("logs/CoreKeeperServer.log")
    );
    let original_args = fixture.plans[0].launch_plan.args.clone();
    let native = fixture.root.join("logs/corekeeper-run-42-main.log");
    assign_process_log_paths_with_stamp(&fixture.instance, &mut fixture.plans, 42).unwrap();
    assert_eq!(logfile(&fixture.plans[0].launch_plan), native);
    let running = running_instance(Path::new(&fixture.plans[0].log_path));
    assert_eq!(
        app_core::runtime_logs::corekeeper_native_log_path(Path::new(
            running
                .active_run
                .as_ref()
                .unwrap()
                .log_path
                .as_deref()
                .unwrap()
        )),
        Some(native.clone())
    );
    assert_eq!(
        app_core::runtime_logs::corekeeper_native_log_path(Path::new(&fixture.plans[0].log_path)),
        Some(native.clone())
    );
    assert!(
        fixture.plans[0]
            .launch_plan
            .command_line
            .contains("corekeeper-run-42-main.log")
    );
    assert!(
        !fixture.plans[0]
            .launch_plan
            .command_line
            .contains("CoreKeeperServer.log")
    );
    let option = original_args
        .iter()
        .position(|arg| arg == "-logfile")
        .unwrap()
        + 1;
    for (index, original) in original_args.iter().enumerate() {
        if index != option {
            assert_eq!(&fixture.plans[0].launch_plan.args[index], original);
        }
    }
    // Preparation and formal dispatch use the same reserved run path.
    assign_process_log_paths_with_stamp(&fixture.instance, &mut fixture.plans, 42).unwrap();
    fs::write(&native, "timescale = 0\nprevious run evidence\n").unwrap();
    assert!(
        assign_process_log_paths_with_stamp(&fixture.instance, &mut fixture.plans, 42)
            .unwrap_err()
            .contains("already exists")
    );
    assign_process_log_paths_with_stamp(&fixture.instance, &mut fixture.plans, 43).unwrap();
    assert_eq!(
        logfile(&fixture.plans[0].launch_plan),
        fixture.root.join("logs/corekeeper-run-43-main.log")
    );
    assert_eq!(
        fs::read_to_string(native).unwrap(),
        "timescale = 0\nprevious run evidence\n"
    );
}

#[test]
fn corekeeper_actual_launch_rejects_missing_or_ambiguous_log_options() {
    let mut fixture = Fixture::new();
    for args in [
        vec!["-batchmode"],
        vec!["-logfile"],
        vec!["-logfile", "-datapath"],
        vec!["-logfile", "old.log", "-logfile", "other.log"],
    ] {
        fixture.plans[0].launch_plan.args = args.iter().map(|arg| (*arg).to_owned()).collect();
        let before = fixture.plans[0].launch_plan.args.clone();
        assert!(
            assign_process_log_paths_with_stamp(&fixture.instance, &mut fixture.plans, 42).is_err()
        );
        assert_eq!(fixture.plans[0].launch_plan.args, before);
    }
}
