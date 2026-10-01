use super::*;

#[tokio::test]
async fn shutdown_failed_save_prevents_quit_and_reports_both_transport_failures() {
    for source in [
        InstanceShutdownSource::Manual,
        InstanceShutdownSource::AppExit,
    ] {
        assert_failed_save_prevents_quit(source).await;
    }
}

async fn assert_failed_save_prevents_quit(source: InstanceShutdownSource) {
    let root = std::env::temp_dir().join(format!("langame-shutdown-{}", uuid::Uuid::new_v4()));
    let paths = app_storage::StoragePaths {
        app_data_root: root.clone(),
        settings_path: root.join("settings.json"),
        database_path: root.join("db/lgs.db"),
        logs_root: root.join("logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("archives"),
    };
    let storage = StorageBootstrap {
        settings: paths.settings(),
        storage_status: paths.probe_status(),
        paths,
    };
    let details: InstanceDetails = serde_json::from_value(json!({
        "summary": {
            "id": "shutdown-fixture", "name": "Shutdown fixture", "module_id": "fixture",
            "status": app_core::InstanceStatus::Running, "bind_ip": "127.0.0.1", "port_count": 0, "autostart": false
        },
        "config_file_path": "", "saves_path": "", "auto_backup_on_stop": false,
        "backup_retention_count": 1, "settings_json": "{}", "ports": [], "active_run": null
    }))
    .unwrap();
    let shutdown: ModuleShutdownSpec = serde_json::from_value(json!({
        "grace_period_ms": 0,
        "commands": [
            {"transport": "unavailable-save", "fallback_transport": "unavailable-fallback", "command": "save"},
            {"transport": "stdin", "command": "quit"}
        ]
    })).unwrap();
    let result = dispatch_instance_shutdown_commands(
        &DesktopState::default(),
        &storage,
        &details,
        &shutdown,
        source,
    )
    .await;
    let audit = fs::read_to_string(storage.paths.app_log_path()).unwrap();
    fs::remove_dir_all(&root).unwrap();

    let error = result.expect_err("failed save must not be reported as successful shutdown");
    assert!(error.contains("unavailable-fallback"), "{error}");
    assert!(error.contains("not forcibly stopped"), "{error}");
    assert!(audit.contains("unavailable-save"));
    assert!(audit.contains("unavailable-fallback"));
    assert!(
        !audit.contains("\"quit\""),
        "quit must not follow a failed save"
    );
}
