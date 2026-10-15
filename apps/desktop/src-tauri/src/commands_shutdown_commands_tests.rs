use super::*;

fn unreal_console_details(module_id: &str) -> InstanceDetails {
    serde_json::from_value(json!({
        "summary": {
            "id": "untracked-unreal-fixture", "name": "Untracked Unreal fixture", "module_id": module_id,
            "status": app_core::InstanceStatus::Running, "bind_ip": "127.0.0.1", "port_count": 0, "autostart": false
        },
        "config_file_path": "", "saves_path": "", "auto_backup_on_stop": false,
        "backup_retention_count": 1, "settings_json": "{}", "ports": [], "active_run": null
    }))
    .unwrap()
}

#[tokio::test]
async fn unreal_console_supported_modules_reach_runtime_ownership_validation() {
    for module_id in ["windrose", "runescapedragonwilds"] {
        let details = unreal_console_details(module_id);
        let error = dispatch_instance_runtime_transport(
            &DesktopState::default(),
            &details,
            &RuntimeTransportRequest {
                command: "quit",
                transport: "unreal_console",
                process_key: None,
                port_name: None,
                password_setting_key: None,
                enabled_setting_key: None,
            },
        )
        .await
        .expect_err("a declared transport must still require an owned running instance");
        assert_eq!(
            error, "tracked instance `untracked-unreal-fixture` is not running",
            "{module_id} must pass the module gate and fail runtime ownership"
        );
    }
}

#[tokio::test]
async fn unreal_console_rejects_every_other_catalog_module_before_runtime_dispatch() {
    let module_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let modules =
        app_modules::discover_modules(&module_root).expect("discover current module catalog");
    let mut rejected = 0;
    for module in modules {
        let module_id = module.summary.id;
        if matches!(module_id.as_str(), "windrose" | "runescapedragonwilds") {
            continue;
        }
        let details = unreal_console_details(&module_id);
        let error = dispatch_instance_runtime_transport(
            &DesktopState::default(),
            &details,
            &RuntimeTransportRequest {
                command: "quit",
                transport: "unreal_console",
                process_key: None,
                port_name: None,
                password_setting_key: None,
                enabled_setting_key: None,
            },
        )
        .await
        .expect_err("undeclared native console transports must fail at the module gate");
        assert_eq!(
            error, "Unreal GUI console is not declared for this module",
            "{module_id} must never reach the runtime console dispatcher"
        );
        rejected += 1;
    }
    assert_eq!(rejected, 30);
}

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
