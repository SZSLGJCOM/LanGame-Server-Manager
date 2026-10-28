use super::*;
use app_core::{InstanceDetails, InstanceStatus, InstanceSummary};
use serde_json::json;

fn rendered_arguments(settings: &Value) -> Vec<String> {
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: "moria-worker-threads".into(),
            name: "Moria worker threads".into(),
            module_id: "returntomoria".into(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        config_file_path: "C:/LanGame/configuration-test/config/settings.json".into(),
        saves_path: "C:/LanGame/configuration-test/saves".into(),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 1,
        settings_json: settings.to_string(),
        ports: Vec::new(),
        active_run: None,
    };
    let root = Path::new("C:/LanGame/configuration-test");
    let context = TemplateContext {
        instance: &instance,
        settings,
        install_root: root,
        config_dir: root,
        data_dir: root,
        logs_dir: root,
        saves_dir: root,
    };
    let process =
        app_modules::discover_modules(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"))
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == "returntomoria")
            .unwrap()
            .process
            .unwrap();
    process
        .args_template
        .iter()
        .flat_map(|segment| expand_resolved_argument_segments(segment, &context))
        .collect()
}

#[test]
fn unset_worker_override_keeps_native_default_without_injecting_an_argument() {
    let settings = json!({});
    assert!(rendered_arguments(&settings).is_empty());
    assert!(
        crate::collect_module_launch_setting_issues("returntomoria", &settings, &[]).is_empty()
    );
}

#[test]
fn unset_worker_override_keeps_the_existing_raw_value_once() {
    let settings = json!({"extra_launch_args":"-NumServerWorkerThreads=3 -log"});
    assert_eq!(
        rendered_arguments(&settings),
        ["-NumServerWorkerThreads=3", "-log"]
    );
    assert!(
        crate::collect_module_launch_setting_issues("returntomoria", &settings, &[]).is_empty()
    );
}

#[test]
fn explicit_worker_override_emits_one_effective_argument() {
    let settings = json!({"server_worker_threads":2,"extra_launch_args":"-log"});
    assert_eq!(
        rendered_arguments(&settings),
        ["-NumServerWorkerThreads=2", "-log"]
    );
    assert!(
        crate::collect_module_launch_setting_issues("returntomoria", &settings, &[]).is_empty()
    );
}

#[test]
fn explicit_worker_override_and_raw_copy_are_rejected_before_launch() {
    for raw in [
        "-NumServerWorkerThreads=3",
        "\"-numserverworkerthreads=3\" -log",
        "-NUMSERVERWORKERTHREADS 3",
    ] {
        let settings = json!({"server_worker_threads":2,"extra_launch_args":raw});
        let issues = crate::collect_module_launch_setting_issues("returntomoria", &settings, &[]);
        assert_eq!(issues.len(), 1, "{raw}");
        assert_eq!(issues[0].code, "managed_launch_option_conflict");
        assert_eq!(issues[0].severity, "error");
        assert_eq!(issues[0].context["field"], "extra_launch_args");
    }
}

#[test]
fn worker_option_text_inside_an_unrelated_raw_value_is_not_a_conflict() {
    let settings = json!({
        "server_worker_threads":2,
        "extra_launch_args":"-Note=\"literal -NumServerWorkerThreads=3\""
    });
    assert!(
        crate::collect_module_launch_setting_issues("returntomoria", &settings, &[]).is_empty()
    );
    assert_eq!(
        rendered_arguments(&settings),
        [
            "-NumServerWorkerThreads=2",
            "-Note=literal -NumServerWorkerThreads=3"
        ]
    );
}
