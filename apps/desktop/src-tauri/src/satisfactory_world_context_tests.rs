use super::*;

#[cfg(windows)]
#[test]
fn loopback_inventory_retains_foreign_listener_owners_before_authorized_requests() {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let owners = WindowsPlatform::loopback_tcp_listener_owners(port).unwrap();
    assert_eq!(owners, BTreeSet::from([std::process::id()]));
    let unrelated_target = std::process::id().checked_add(1).unwrap();
    assert_ne!(
        owners,
        BTreeSet::from([unrelated_target]),
        "An unrelated managed PID must not hide the actual listener's owner"
    );
}

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
