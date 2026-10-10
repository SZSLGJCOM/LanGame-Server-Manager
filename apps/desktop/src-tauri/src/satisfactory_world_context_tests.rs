use super::*;

#[test]
fn credential_namespace_survives_storage_moves_but_does_not_cross_instance_or_port() {
    let mut instance = InstanceDetails {
        summary: app_core::InstanceSummary {
            id: "server-a".into(),
            name: "Fixture".into(),
            module_id: "satisfactory".into(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: "0.0.0.0".into(),
            port_count: 1,
            autostart: false,
        },
        config_file_path: "D:/managed/instances/a/settings/instance.json".into(),
        saves_path: String::new(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 1,
        settings_json: "{}".into(),
        ports: vec![app_core::PortBinding {
            name: "game_tcp".into(),
            protocol: "tcp".into(),
            port: 7777,
        }],
        active_run: None,
    };
    let namespace = credential_identity(&instance).unwrap();
    instance.config_file_path = "E:/relocated/instances/a/settings/instance.json".into();
    assert_eq!(credential_identity(&instance).unwrap(), namespace);
    instance.summary.id = "server-b".into();
    assert_ne!(credential_identity(&instance).unwrap(), namespace);
    instance.summary.id = "server-a".into();
    instance.ports[0].port = 7778;
    assert_ne!(credential_identity(&instance).unwrap(), namespace);
}
