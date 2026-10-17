use super::*;
use serde_json::json;

fn soulmask_permission_arguments(settings: &Value) -> Vec<String> {
    let instance = InstanceDetails {
        summary: app_core::InstanceSummary {
            id: "soulmask-fixture".into(),
            name: "Soulmask fixture".into(),
            module_id: "soulmask".into(),
            status: app_core::InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        config_file_path: String::new(),
        saves_path: String::new(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 1,
        settings_json: settings.to_string(),
        ports: Vec::new(),
        active_run: None,
    };
    let root = Path::new("fixture");
    let context = TemplateContext {
        instance: &instance,
        settings,
        install_root: root,
        config_dir: root,
        data_dir: root,
        logs_dir: root,
        saves_dir: root,
    };
    [
        "{{soulmask.default_server_permissions_arg}}",
        "{{launch.extra_args}}",
    ]
    .into_iter()
    .flat_map(|template| expand_resolved_argument_segments(template, &context))
    .collect()
}

#[test]
fn soulmask_launch_enables_the_account_blacklist_when_permission_mode_is_absent() {
    assert_eq!(soulmask_permission_arguments(&Value::Null), ["-serverpm=2"]);
    for extra in [
        "-log",
        "-serverpm-extra=7",
        "-config=serverpm",
        r#"-config="C:/fixture/-serverpm=7.ini""#,
        r#"-message="show -serverpm=7""#,
    ] {
        let arguments = soulmask_permission_arguments(&json!({ "extra_launch_args": extra }));
        assert_eq!(arguments.first().map(String::as_str), Some("-serverpm=2"));
        assert_eq!(arguments.len(), 2);
    }
}

#[test]
fn soulmask_launch_preserves_explicit_permission_modes_without_adding_a_duplicate() {
    for (extra, expected) in [
        ("-serverpm=0", vec!["-serverpm=0"]),
        ("-serverpm=3", vec!["-serverpm=3"]),
        ("-SERVERPM=6", vec!["-SERVERPM=6"]),
        ("-log\n-serverpm=7", vec!["-log", "-serverpm=7"]),
        ("-serverpm 3", vec!["-serverpm", "3"]),
        (r#""-serverpm=3""#, vec!["-serverpm=3"]),
    ] {
        assert_eq!(
            soulmask_permission_arguments(&json!({ "extra_launch_args": extra })),
            expected,
            "operator permission mode changed: {extra}"
        );
    }
}
