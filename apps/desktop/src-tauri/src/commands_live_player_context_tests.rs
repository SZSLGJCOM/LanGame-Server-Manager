use super::*;

fn context() -> (InstanceDetails, ModuleDescriptor) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let descriptor = discover_modules(root)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "humanitz")
        .unwrap();
    let details = serde_json::from_value(json!({
        "summary": {
            "id": "count-context", "name": "Count context", "module_id": "humanitz",
            "status": "Running", "active_process_count": 1, "bind_ip": "127.0.0.1",
            "port_count": 1, "autostart": false
        },
        "config_file_path": "unused/config", "saves_path": "unused/saves",
        "auto_backup_on_stop": false, "backup_retention_count": 1,
        "settings_json": "{\"rcon_enabled\":false,\"rcon_password\":\"synthetic-test-only\"}",
        "ports": [{"name": "rcon", "protocol": "tcp", "port": 8888}],
        "active_run": {
            "run_id": 1, "session_id": "session", "pid": 4294967295_u32,
            "log_path": null, "process_count": 1,
            "processes": [{
                "run_id": 1, "session_id": "session", "process_key": "main",
                "display_name": "Fixture", "pid": 4294967295_u32,
                "process_identity": {"creation_time": 100, "image_path": "fixture.exe"},
                "status": "running", "started_at": null, "stopped_at": null,
                "exit_code": null, "crash_flag": false, "log_path": null, "is_primary": true
            }]
        }
    }))
    .unwrap();
    (details, descriptor)
}

#[test]
fn player_count_cached_context_rejects_disabled_rcon_and_missing_credentials() {
    let state = DesktopState::default();
    let (mut details, descriptor) = context();
    let key = build_cache_key(&details, &descriptor).unwrap();
    for (settings, expected_key) in [
        (
            r#"{"rcon_enabled":false,"rcon_password":"synthetic-test-only"}"#,
            "rcon_enabled",
        ),
        (
            r#"{"rcon_enabled":true,"rcon_password":""}"#,
            "rcon_password",
        ),
        ("invalid json", "settings_json"),
    ] {
        details.settings_json = settings.to_owned();
        let error =
            validate_cached_collection_context(&state, &details, &descriptor, &key).unwrap_err();
        assert_eq!(
            error.status,
            app_core::RuntimeLivePlayerStatus::Misconfigured
        );
        assert!(
            error
                .issue
                .unwrap()
                .setting_keys
                .iter()
                .any(|key| key == expected_key)
        );
        assert!(error.current_players.is_none());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn player_count_registry_polls_humanitz_collector_with_disabled_rcon() {
    use tauri::Manager;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let state = app.state::<DesktopState>();
    let (details, descriptor) = context();
    let key = build_cache_key(&details, &descriptor).unwrap();
    let requested_at = state.live_player_registry.now_unix_ms();

    // Poll the production collector through the registry gate. Disabled RCON
    // returns before any transport, storage lookup, or process inspection.
    let refresh = state
        .live_player_registry
        .refresh_or_join(key, requested_at, || async {
            super::sources::collect_direct_players(&state, &details, &descriptor, requested_at)
                .await
        });
    let snapshot = refresh.await;

    assert_eq!(
        snapshot.status,
        app_core::RuntimeLivePlayerStatus::Misconfigured
    );
    assert_eq!(snapshot.instance_id, details.summary.id);
    assert!(snapshot.current_players.is_none());
    assert!(!snapshot.complete);
    assert!(snapshot.entries.is_empty());
    let issue = snapshot
        .issue
        .expect("disabled RCON must identify its setting");
    assert_eq!(
        issue.code,
        RuntimeLivePlayerIssueCode::RuntimeActionUnavailable
    );
    assert_eq!(issue.setting_keys, vec![String::from("rcon_enabled")]);
}

#[test]
fn player_count_cache_key_binds_selected_process_generation_and_run() {
    let (details, descriptor) = context();
    let key = build_cache_key(&details, &descriptor).unwrap();
    for variant in 0..3 {
        let mut changed = details.clone();
        let run = changed.active_run.as_mut().unwrap();
        match variant {
            0 => run.run_id += 1,
            1 => run.processes[0].pid = Some(100),
            _ => {
                run.processes[0]
                    .process_identity
                    .as_mut()
                    .unwrap()
                    .creation_time += 1
            }
        }
        assert_ne!(build_cache_key(&changed, &descriptor).unwrap(), key);
        let error = validate_cached_collection_context(
            &DesktopState::default(),
            &changed,
            &descriptor,
            &key,
        )
        .unwrap_err();
        assert_eq!(
            error.issue.unwrap().code,
            RuntimeLivePlayerIssueCode::ProcessUnavailable
        );
        assert!(error.current_players.is_none());
    }
}

#[test]
fn player_count_cached_context_requires_a_live_supervised_process() {
    let (mut details, descriptor) = context();
    details.settings_json =
        String::from(r#"{"rcon_enabled":true,"rcon_password":"synthetic-test-only"}"#);
    let key = build_cache_key(&details, &descriptor).unwrap();
    let error =
        validate_cached_collection_context(&DesktopState::default(), &details, &descriptor, &key)
            .unwrap_err();
    assert_eq!(
        error.issue.unwrap().code,
        RuntimeLivePlayerIssueCode::ProcessUnavailable
    );
    assert!(error.current_players.is_none());
    details.active_run = None;
    let stopped =
        validate_cached_collection_context(&DesktopState::default(), &details, &descriptor, &key)
            .unwrap_err();
    assert_eq!(stopped.status, app_core::RuntimeLivePlayerStatus::Stopped);
}
