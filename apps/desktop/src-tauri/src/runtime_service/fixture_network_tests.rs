use super::*;
use app_core::{
    InstanceStatus, InstanceSummary, LaunchPlan, PortBinding, ProcessHostSurface,
    ProcessWindowPolicy,
};
use std::fs;
use std::path::PathBuf;

struct Fixture {
    owner: PathBuf,
    config: Config,
    paths: StoragePaths,
    instance: InstanceDetails,
    plans: Vec<ProcessLaunchPlan>,
    reference: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let owner = std::env::temp_dir().join(format!(
            "langame-runtime-service-MiXeD-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&owner).unwrap();
        let config = Config {
            root: owner.join("fixture"),
            nonce: uuid::Uuid::new_v4().to_string(),
            scenario: super::super::config::Scenario::Normal,
        };
        fs::create_dir(&config.root).unwrap();
        write_new(&config.root.join(super::super::config::MARKER), &config).unwrap();
        let app_data_root = config.root.join("localappdata/LanGame/ServerManager");
        let paths = StoragePaths {
            settings_path: app_data_root.join("settings.json"),
            database_path: app_data_root.join("db/lgs.db"),
            logs_root: app_data_root.join("logs"),
            app_data_root,
            instances_root: config.root.join("runtime/instances"),
            archives_root: config.root.join("runtime/instances").join(".trash"),
            games_root: config.root.join("runtime/games"),
            steamcmd_root: config.root.join("runtime/steamcmd"),
            modules_root: config.root.join("modules"),
            migrations_root: config.root.join("migrations"),
        };
        for dir in [
            &paths.app_data_root,
            &paths.instances_root,
            &paths.games_root,
            &paths.steamcmd_root,
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        let root = paths.instances_root.join("owned-instance");
        for relative in ["config", "data/saves", "runtime/jre/bin"] {
            fs::create_dir_all(root.join(relative)).unwrap();
        }
        let config_file = root.join("config/instance.json");
        fs::write(&config_file, b"{}").unwrap();
        let executable = root.join("runtime/jre/bin/java.exe");
        // Only the private validator takes this small reference; the live
        // capability always supplies current_exe internally.
        let reference = owner.join("reference.exe");
        fs::write(&reference, vec![42; 130_000]).unwrap();
        fs::copy(&reference, &executable).unwrap();
        let instance = InstanceDetails {
            summary: InstanceSummary {
                id: "owned-instance".into(),
                name: "Fixture".into(),
                module_id: "necesse".into(),
                status: InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: "127.0.0.1".into(),
                port_count: 1,
                autostart: false,
            },
            config_file_path: config_file.to_string_lossy().into_owned(),
            saves_path: root.join("data/saves").to_string_lossy().into_owned(),
            backup_uses_declared_saves_path: true,
            auto_backup_on_stop: true,
            backup_retention_count: 1,
            settings_json: "{}".into(),
            ports: vec![PortBinding {
                name: "game".into(),
                protocol: "udp".into(),
                port: 41234,
            }],
            active_run: None,
        };
        let plan = LaunchPlan {
            instance_id: instance.summary.id.clone(),
            instance_name: "Fixture".into(),
            module_id: "necesse".into(),
            install_root: root.join("runtime").to_string_lossy().into_owned(),
            install_state: app_core::InstallState::Installed,
            uses_private_runtime: true,
            working_directory: root.join("runtime").to_string_lossy().into_owned(),
            executable_path: executable.to_string_lossy().into_owned(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: Vec::new(),
            args: vec![
                "-ip".into(),
                "127.0.0.1".into(),
                "-port".into(),
                "41234".into(),
                "-datadir".into(),
                root.join("data").to_string_lossy().into_owned(),
            ],
            environment: Default::default(),
            command_line: String::new(),
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: Default::default(),
            performance_preview: Default::default(),
        };
        Self {
            owner,
            config,
            paths,
            instance,
            plans: vec![ProcessLaunchPlan {
                process_key: "main".into(),
                display_name: "Fixture".into(),
                log_path: String::new(),
                launch_plan: plan,
            }],
            reference,
        }
    }

    fn verify(&self) -> Result<(), String> {
        validate_boundary(
            &self.config,
            &self.paths,
            &self.instance,
            &self.plans,
            &self.reference,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.owner).unwrap();
    }
}

#[test]
fn owned_loopback_synthetic_executable_is_accepted() {
    Fixture::new().verify().unwrap();
}

#[test]
fn normalized_windows_process_identity_still_resolves_to_the_owned_executable() {
    let fixture = Fixture::new();
    let actual = Path::new(&fixture.plans[0].launch_plan.executable_path);
    let normalized = actual.to_string_lossy().to_lowercase();
    // ProcessIdentity intentionally folds Windows paths to lowercase. The old
    // lexical starts_with check rejected this real owned executable.
    assert!(!Path::new(&normalized).starts_with(&fixture.config.root));
    same_path(Path::new(&normalized), actual).unwrap();
    assert!(same_path(&fixture.reference, actual).is_err());
}

#[test]
fn modified_executable_is_rejected() {
    use std::io::{Seek, SeekFrom, Write};
    let fixture = Fixture::new();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(&fixture.plans[0].launch_plan.executable_path)
        .unwrap();
    file.seek(SeekFrom::Start(80_000)).unwrap();
    file.write_all(&[43]).unwrap();
    drop(file);
    assert!(fixture.verify().unwrap_err().contains("identity mismatch"));
}

#[test]
fn broader_bind_and_extra_port_or_process_are_rejected() {
    let mut fixture = Fixture::new();
    fixture.instance.summary.bind_ip = "0.0.0.0".into();
    assert!(fixture.verify().is_err());
    fixture.instance.summary.bind_ip = "127.0.0.1".into();
    fixture
        .instance
        .ports
        .push(fixture.instance.ports[0].clone());
    assert!(fixture.verify().is_err());
    fixture.instance.ports.pop();
    fixture.plans.push(fixture.plans[0].clone());
    assert!(fixture.verify().is_err());
    fixture.plans.pop();
    fixture.plans[0].launch_plan.args[1] = "0.0.0.0".into();
    assert!(fixture.verify().is_err());
}

#[test]
fn wrong_marker_and_storage_or_instance_paths_are_rejected() {
    let mut fixture = Fixture::new();
    let original_nonce = fixture.config.nonce.clone();
    fixture.config.nonce = uuid::Uuid::new_v4().to_string();
    assert!(fixture.verify().is_err());
    fixture.config.nonce = original_nonce;
    let original = fixture.paths.games_root.clone();
    fixture.paths.games_root = fixture.paths.instances_root.clone();
    assert!(fixture.verify().is_err());
    fixture.paths.games_root = original;
    fixture.instance.saves_path = fixture.owner.to_string_lossy().into_owned();
    assert!(fixture.verify().is_err());
}

#[tokio::test]
async fn no_managed_fixture_capability_preserves_the_production_boundary() {
    let fixture = Fixture::new();
    assert!(
        !try_fixture_firewall_boundary(None, &fixture.paths, &fixture.instance, &fixture.plans)
            .await
            .unwrap()
    );
    assert!(
        !fixture
            .config
            .root
            .join("data/firewall-boundary.json")
            .exists()
    );
}
