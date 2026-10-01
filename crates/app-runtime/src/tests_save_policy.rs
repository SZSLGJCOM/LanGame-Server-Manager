use super::*;

use app_core::{InstanceStatus, InstanceSummary};
use serde_json::json;

struct SavePolicyRuntimeRoot(PathBuf);

impl Drop for SavePolicyRuntimeRoot {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).expect("remove save-policy runtime fixture");
        }
    }
}

fn save_policy_launch_plan(
    root: &Path,
    module: &app_modules::ModuleDescriptor,
    overrides: Value,
) -> LaunchPlan {
    let instance_id = format!("save-policy-{}", module.summary.id);
    let instance_name = "Save policy fixture";
    let instance_root = root.join("instances").join(&instance_id);
    let config_dir = instance_root.join("config");
    let install_root = root
        .join("games")
        .join(&module.install.as_ref().unwrap().shared_game_dir);
    let normalized = app_storage::normalize_complete_instance_settings(
        Some(module),
        overrides.as_object().unwrap().clone(),
        &instance_id,
        instance_name,
        "0.0.0.0",
    )
    .unwrap();
    let saves_dir = app_storage::plan_instance_saves_dir(
        Some(module),
        &app_storage::InstanceSavePathContext {
            install_root: &install_root,
            instance_root: &instance_root,
            config_dir: &config_dir,
            instance_id: &instance_id,
            instance_name,
            module_id: &module.summary.id,
            settings: Some(&normalized),
        },
    )
    .unwrap();
    for directory in [&config_dir, &install_root, &saves_dir] {
        fs::create_dir_all(directory).unwrap();
    }
    let executable = install_root.join(&module.process.as_ref().unwrap().executable);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(executable, []).unwrap();

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let details = ModuleDetails {
        summary: module.summary.clone(),
        schema_json: module.schema_json.clone(),
        default_ports: module.default_ports.clone(),
        install: module.install.clone(),
        process: module.process.clone(),
        workshop: module.workshop.clone(),
        mods: None,
        runtime: module.runtime.clone(),
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: instance_id,
            name: instance_name.to_owned(),
            module_id: module.summary.id.clone(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: "0.0.0.0".to_owned(),
            port_count: module.default_ports.len(),
            autostart: true,
        },
        config_file_path: config_dir
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: saves_dir.to_string_lossy().into_owned(),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: true,
        backup_retention_count: 7,
        settings_json: Value::Object(normalized).to_string(),
        ports: module.default_ports.clone(),
        active_run: None,
    };
    let plan = build_launch_plan(&settings, &details, &instance).unwrap();
    assert!(
        !plan
            .validation_issues
            .iter()
            .any(|issue| issue.code == "unresolved_launch_args"),
        "{} save policy must resolve into real startup arguments",
        module.summary.id
    );
    assert!(plan.args.iter().all(|argument| !argument.contains("{{")));
    plan
}

#[test]
fn save_policy_changes_reach_native_launch_arguments() {
    let root = SavePolicyRuntimeRoot(crate::test_support::unique_test_root());
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let modules = app_modules::discover_modules(repository.join("modules")).unwrap();
    let cases = [
        (
            "arksurvivalevolved",
            json!({ "max_num_of_save_backups": 13 }),
            vec!["-MaxNumOfSaveBackups=13"],
            json!({ "max_num_of_save_backups": 27 }),
            vec!["-MaxNumOfSaveBackups=27"],
        ),
        (
            "soulmask",
            json!({ "save_interval_seconds": 421, "backup_interval_seconds": 1261 }),
            vec!["-saving=421", "-backup=1261"],
            json!({ "save_interval_seconds": 601, "backup_interval_seconds": 1801 }),
            vec!["-saving=601", "-backup=1801"],
        ),
        (
            "valheim",
            json!({
                "save_interval_seconds": 1261, "backup_count": 7,
                "backup_short_seconds": 5431, "backup_long_seconds": 23401
            }),
            vec![
                "-saveinterval",
                "1261",
                "-backups",
                "7",
                "-backupshort",
                "5431",
                "-backuplong",
                "23401",
            ],
            json!({
                "save_interval_seconds": 901, "backup_count": 3,
                "backup_short_seconds": 3601, "backup_long_seconds": 14401
            }),
            vec![
                "-saveinterval",
                "901",
                "-backups",
                "3",
                "-backupshort",
                "3601",
                "-backuplong",
                "14401",
            ],
        ),
    ];
    for (module_id, first, first_arguments, second, second_arguments) in cases {
        let module = modules
            .iter()
            .find(|module| module.summary.id == module_id)
            .unwrap();
        let first_plan = save_policy_launch_plan(&root.0, module, first);
        let second_plan = save_policy_launch_plan(&root.0, module, second);
        for (plan, expected) in [
            (&first_plan, &first_arguments),
            (&second_plan, &second_arguments),
        ] {
            assert!(
                plan.args
                    .windows(expected.len())
                    .any(|arguments| arguments == expected.as_slice()),
                "{module_id} must emit the complete native save-policy arguments {expected:?}"
            );
        }
        assert!(
            !second_plan
                .args
                .windows(first_arguments.len())
                .any(|arguments| arguments == first_arguments.as_slice()),
            "{module_id} must not retain the previous save policy in the next launch"
        );
    }
}
