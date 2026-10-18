use super::*;
use app_core::{InstallState, RuntimePerformancePolicy, RuntimePerformancePolicyPreview};
use serde_json::{Value, json};

fn instance(module_id: &str, settings: Value) -> InstanceDetails {
    let root =
        std::env::temp_dir().join(format!("lgsm-asa-plan-{}", uuid::Uuid::new_v4().simple()));
    serde_json::from_value(json!({
        "summary": {"id":"asa-plan", "name":"ASA plan fixture", "module_id":module_id,
            "status":"Stopped", "bind_ip":"127.0.0.1", "port_count":0, "autostart":false},
        "config_file_path":root.join("config/settings.json").to_string_lossy(),
        "saves_path":root.join("saves").to_string_lossy(),
        "auto_backup_on_stop":false, "backup_retention_count":0,
        "settings_json":settings.to_string(), "ports":[], "active_run":null
    }))
    .unwrap()
}

fn plan(instance: &InstanceDetails, process_key: &str, args: &[&str]) -> ProcessLaunchPlan {
    let root = std::path::Path::new(&instance.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let native =
        app_core::ark_maps::native_log_path("arksurvivalascended", &root.join("logs"), process_key)
            .unwrap();
    let mut args = args
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    args.push(format!("-abslog={}", native.display()));
    serde_json::from_value(json!({
        "process_key":process_key, "display_name":"Map fixture",
        "log_path":root.join("logs/managed.log").to_string_lossy(),
        "launch_plan":{
            "instance_id":instance.summary.id, "instance_name":instance.summary.name,
            "module_id":instance.summary.module_id, "install_root":root.join("runtime").to_string_lossy(),
            "install_state":InstallState::Installed, "uses_private_runtime":true,
            "working_directory":root.join("runtime").to_string_lossy(),
            "executable_path":root.join("runtime/ArkAscendedServer.exe").to_string_lossy(),
            "executable_exists":true, "ready_to_launch":true, "validation_issues":[],
            "args":args, "environment":{}, "command_line":"inert plan; never spawned",
            "window_policy":"background", "uses_script_entrypoint":false, "requires_admin":false,
            "host_surface":"managed_terminal", "host_notes":null,
            "performance_policy":RuntimePerformancePolicy::default(),
            "performance_preview":RuntimePerformancePolicyPreview::default()
        }
    })).unwrap()
}

fn requirements(instance: &InstanceDetails, plan: &ProcessLaunchPlan) -> Requirements {
    collect(instance, plan)
        .expect("valid native argv")
        .expect("Mod validation is required")
}

#[test]
fn active_and_passive_actual_arguments_are_deduplicated_for_a_single_map() {
    let instance = instance("arksurvivalascended", json!({}));
    assert_eq!(app_core::ark_maps::processes(&instance).unwrap().len(), 1);
    let plan = plan(&instance, "main", &["-mods=42,7,42", "-passivemods=7,99"]);
    assert!(requires_validation(
        "arksurvivalascended",
        &plan.launch_plan.args
    ));
    assert_eq!(
        requirements(&instance, &plan).project_ids,
        ["42", "7", "99"]
    );
    // The collector sees actual argv even when the settings do not declare Mods.
    assert_eq!(instance.settings_json, "{}");
}

#[test]
fn only_asa_mod_arguments_require_native_curseforge_validation() {
    for module in ["arksurvivalevolved", "palworld", "dontstarve"] {
        let instance = instance(module, json!({}));
        let plan = plan(&instance, "main", &["-mods=42", "-passivemods=7"]);
        assert!(!requires_validation(module, &plan.launch_plan.args));
        assert!(collect(&instance, &plan).unwrap().is_none());
    }
    let instance = instance("arksurvivalascended", json!({"mod_ids_csv":"42"}));
    let plan = plan(
        &instance,
        "main",
        &["TheIsland_WP?SessionName=-mods=42", "-othermods=42"],
    );
    assert!(!requires_validation(
        "arksurvivalascended",
        &plan.launch_plan.args
    ));
    assert!(collect(&instance, &plan).unwrap().is_none());
}

#[test]
fn custom_native_flags_accept_both_forms_case_and_preserve_author_dev_revisions() {
    let instance = instance("arksurvivalascended", json!({}));
    for args in [
        vec!["-MODS=42-dev,007", "-PASSIVEMODS=99"],
        vec!["-MoDs", "42-dev,007", "-PassiveMods", "99"],
    ] {
        let plan = plan(&instance, "main", &args);
        let before = plan.launch_plan.args.clone();
        assert_eq!(
            requirements(&instance, &plan).project_ids,
            ["42", "7", "99"]
        );
        assert_eq!(
            plan.launch_plan.args, before,
            "native -dev intent must not be rewritten"
        );
    }
}

#[test]
fn existing_native_cache_does_not_skip_the_validation_requirement() {
    let instance = instance("arksurvivalascended", json!({}));
    let plan = plan(&instance, "main", &["-mods=42"]);
    let root = PathBuf::from(&plan.launch_plan.install_root);
    let package = root.join("ShooterGame/Binaries/Win64/ShooterGame/Mods/83374/42_123/Plugin/Content/Paks/WindowsServer");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("Plugin.pak"), "preserved cached payload").unwrap();
    let result = requirements(&instance, &plan);
    assert_eq!(result.project_ids, ["42"]);
    assert_eq!(
        std::fs::read_to_string(package.join("Plugin.pak")).unwrap(),
        "preserved cached payload"
    );
    let fixture_root = root.parent().unwrap();
    assert_eq!(fixture_root.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::remove_dir_all(fixture_root).unwrap();
}

#[test]
fn native_log_requires_exactly_one_current_map_owned_argument() {
    let instance = instance(
        "arksurvivalascended",
        json!({"additional_maps":[{
            "id":"scorched", "map_name":"ScorchedEarth_WP", "name":"Scorched", "enabled":true
        }]}),
    );
    let main = plan(&instance, "main", &["-mods=42"]);
    let other = plan(&instance, "map-scorched", &["-passivemods=7"]);
    let owned = requirements(&instance, &main).native_log;
    assert_eq!(
        owned,
        PathBuf::from(
            app_core::ark_maps::processes(&instance).unwrap()[0]
                .native_log_path
                .clone()
        )
    );
    let mut separate = main.clone();
    separate.launch_plan.args.pop();
    separate
        .launch_plan
        .args
        .extend(["-ABSLOG".into(), owned.to_string_lossy().into_owned()]);
    assert_eq!(requirements(&instance, &separate).native_log, owned);
    let mut missing = main.clone();
    missing.launch_plan.args.pop();
    assert!(collect(&instance, &missing).is_err());
    let mut duplicate = main.clone();
    duplicate
        .launch_plan
        .args
        .push(main.launch_plan.args.last().unwrap().clone());
    assert!(collect(&instance, &duplicate).is_err());
    let mut crossed = main.clone();
    *crossed.launch_plan.args.last_mut().unwrap() = other.launch_plan.args.last().unwrap().clone();
    assert!(collect(&instance, &crossed).is_err());
    let mut external = main.clone();
    *external.launch_plan.args.last_mut().unwrap() = format!(
        "-abslog={}",
        std::env::temp_dir().join("external-asa.log").display()
    );
    assert!(collect(&instance, &external).is_err());
    let mut unknown_map = main;
    unknown_map.process_key = "map-unconfigured".into();
    assert!(collect(&instance, &unknown_map).is_err());
}

#[cfg(windows)]
#[test]
fn windows_native_log_slash_and_case_variants_retain_the_owned_canonical_path() {
    let instance = instance("arksurvivalascended", json!({}));
    let mut plan = plan(&instance, "main", &["-mods=42"]);
    let expected = requirements(&instance, &plan).native_log;
    *plan.launch_plan.args.last_mut().unwrap() = format!(
        "-ABSLOG={}",
        expected
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_uppercase()
    );
    assert_eq!(requirements(&instance, &plan).native_log, expected);
}

#[test]
fn project_ids_reject_nonpositive_overflow_empty_or_unverifiable_native_flags() {
    let instance = instance("arksurvivalascended", json!({}));
    for value in [
        "0",
        "000",
        "-1",
        "9223372036854775808",
        "18446744073709551615",
        "42-invalid",
        "invalid",
        ", ,",
    ] {
        let plan = plan(&instance, "main", &[&format!("-mods={value}")]);
        assert!(
            collect(&instance, &plan).is_err(),
            "invalid native Mod value {value:?}"
        );
    }
    let maximum = plan(&instance, "main", &["-mods=9223372036854775807"]);
    assert_eq!(
        requirements(&instance, &maximum).project_ids,
        ["9223372036854775807"]
    );
}

#[test]
fn empty_native_mod_flags_preserve_vanilla_start_without_a_mod_phase() {
    let instance = instance("arksurvivalascended", json!({}));
    for args in [
        vec!["-mods=", "-passivemods="],
        vec!["-mods", " ", "-passivemods", ""],
        vec!["-mods", "-log"],
    ] {
        let plan = plan(&instance, "main", &args);
        assert!(!requires_validation(
            "arksurvivalascended",
            &plan.launch_plan.args
        ));
        assert!(collect(&instance, &plan).unwrap().is_none());
    }
}

#[test]
fn duplicate_mod_flags_reject_ambiguous_native_override_semantics() {
    let instance = instance("arksurvivalascended", json!({}));
    for args in [
        vec!["-mods=42", "-mods=7"],
        vec!["-MODS", "42", "-mods=7"],
        vec!["-passivemods=42", "-PassiveMods", "7"],
    ] {
        let plan = plan(&instance, "main", &args);
        assert!(collect(&instance, &plan).is_err());
    }
}

#[test]
fn project_count_and_native_list_byte_boundaries_are_enforced() {
    let instance = instance("arksurvivalascended", json!({}));
    let maximum_ids = (1..=8192)
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let maximum_plan = plan(&instance, "main", &[&format!("-mods={maximum_ids}")]);
    assert_eq!(
        requirements(&instance, &maximum_plan).project_ids.len(),
        8192
    );
    let overflow = plan(&instance, "main", &[&format!("-mods={maximum_ids},8193")]);
    assert!(collect(&instance, &overflow).is_err());
    let maximum_bytes = "1,".repeat(64 * 1024);
    assert_eq!(maximum_bytes.len(), 128 * 1024);
    let maximum_plan = plan(&instance, "main", &[&format!("-mods={maximum_bytes}")]);
    assert_eq!(requirements(&instance, &maximum_plan).project_ids, ["1"]);
    let overflow = plan(&instance, "main", &[&format!("-mods={maximum_bytes},")]);
    assert!(collect(&instance, &overflow).is_err());
}
