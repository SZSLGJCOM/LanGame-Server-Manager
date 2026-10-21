use super::*;
use serde_json::json;

fn instance() -> InstanceDetails {
    serde_json::from_value(json!({
        "summary":{"id":"astro-startup","name":"Fixture","module_id":"astroneer",
            "status":"Running","bind_ip":"0.0.0.0","port_count":1,"autostart":false,"active_process_count":1},
        "config_file_path":"","saves_path":"","auto_backup_on_stop":false,"backup_retention_count":1,
        "settings_json":"{}","ports":[{"name":"console","protocol":"tcp","port":1234}],
        "active_run":{"run_id":1,"session_id":"session","pid":123,"log_path":null,"process_count":1,
            "processes":[{"run_id":1,"session_id":"session","process_key":"main","display_name":"Fixture",
                "pid":123,"process_identity":{"creation_time":456,"image_path":"fixture.exe"},"status":"running",
                "started_at":null,"stopped_at":null,"exit_code":null,"crash_flag":false,"log_path":null,"is_primary":true}]}
    })).unwrap()
}

#[test]
fn explicit_travel_and_load_choices_are_preserved_without_label_restrictions() {
    for settings in [
        json!({"load_auto_save":false,"disable_server_travel":false}),
        json!({"load_auto_save":true,"disable_server_travel":true}),
        json!({"load_auto_save":false,"disable_server_travel":true}),
    ] {
        assert!(!enabled(&settings).unwrap());
        let mut details = instance();
        details.settings_json = settings.to_string();
        assert!(prepare(&details).unwrap().is_none());
    }
    assert!(enabled(&json!({"load_auto_save":true,"disable_server_travel":false})).unwrap());
    assert!(enabled(&json!({})).is_err());
    let mut details = instance();
    details.summary.module_id = "another-game".into();
    assert!(prepare(&details).unwrap().is_none());
}

#[test]
fn every_disk_object_blocks_empty_world_admission() {
    let root = std::env::temp_dir().join(format!(
        "langame-astro-empty-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    assert!(empty(&root).unwrap());
    for name in [
        "unknown",
        "world.savegame",
        "world.backup.savegame",
        ".incomplete",
    ] {
        let path = root.join(name);
        fs::write(&path, b"preserve").unwrap();
        assert!(!empty(&root).unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"preserve");
        fs::remove_file(path).unwrap();
    }
    let folder = root.join("unknown-folder");
    fs::create_dir(&folder).unwrap();
    assert!(!empty(&root).unwrap());
    fs::remove_dir(folder).unwrap();
    fs::remove_dir(&root).unwrap();
    assert!(empty(&root).is_err());
}

#[test]
fn native_empty_requires_both_explicit_empty_names_and_an_empty_catalog() {
    let games = json!({"gameList":[],"activeSaveName":""});
    let statistics = json!({"saveGameName":""});
    assert!(native_empty(&games, &statistics));
    for games in [
        json!({}),
        json!({"gameList":[],"activeSaveName":" "}),
        json!({"gameList":[{}],"activeSaveName":""}),
        json!({"gameList":{},"activeSaveName":""}),
        json!({"gameList":[],"activeSaveName":null}),
    ] {
        assert!(!native_empty(&games, &statistics));
    }
    assert!(!native_empty(&games, &json!({"saveGameName":"SAVE_1"})));
    assert!(!native_empty(&games, &json!({})));
}

#[test]
fn selected_world_is_a_result_of_two_queries_not_fifo_or_players() {
    let games = json!({"gameList":[],"activeSaveName":"SAVE_1"});
    assert_eq!(
        selected(&games, &json!({"saveGameName":"SAVE_1"})),
        Some("SAVE_1")
    );
    assert!(selected(&games, &json!({"saveGameName":"OTHER"})).is_none());
    assert!(
        selected(
            &json!({"gameList":[],"activeSaveName":""}),
            &json!({"saveGameName":""})
        )
        .is_none()
    );
    assert!(
        selected(
            &json!({"gameList":[],"activeSaveName":" "}),
            &json!({"saveGameName":" "})
        )
        .is_none()
    );
    assert!(selected(&json!({"playerInfo":[]}), &json!({"saveGameName":"SAVE_1"})).is_none());
    // Existing Unicode labels are observed, never renamed by startup policy.
    let existing = json!({"gameList":[{}],"activeSaveName":"已有 世界"});
    assert_eq!(
        selected(&existing, &json!({"saveGameName":"已有 世界"})),
        Some("已有 世界")
    );
}

#[test]
fn registered_run_must_have_one_current_primary_generation() {
    assert_eq!(run(&instance()).unwrap().pid, 123);
    let mut variants = Vec::new();
    let mut value = instance();
    value.active_run = None;
    variants.push(value);
    let mut value = instance();
    value.summary.status = InstanceStatus::Stopped;
    variants.push(value);
    let mut value = instance();
    value.active_run.as_mut().unwrap().processes[0].session_id = Some("old".into());
    variants.push(value);
    let mut value = instance();
    value.active_run.as_mut().unwrap().processes[0].pid = Some(124);
    variants.push(value);
    let mut value = instance();
    value.active_run.as_mut().unwrap().processes[0].process_identity = None;
    variants.push(value);
    let mut value = instance();
    value.active_run.as_mut().unwrap().processes[0].exit_code = Some(0);
    variants.push(value);
    let mut value = instance();
    value.active_run.as_mut().unwrap().process_count = 2;
    variants.push(value);
    for value in variants {
        assert!(run(&value).is_err());
    }
}

#[test]
fn console_port_is_one_fixed_nonzero_tcp_binding() {
    assert_eq!(port(&instance()).unwrap(), 1234);
    let mut value = instance();
    value.ports[0].protocol = "udp".into();
    assert!(port(&value).is_err());
    value.ports[0].protocol = "tcp".into();
    value.ports[0].port = 0;
    assert!(port(&value).is_err());
    value.ports[0].port = 1234;
    value.ports.push(value.ports[0].clone());
    assert!(port(&value).is_err());
}

#[test]
fn stop_request_interrupts_initialization_without_requiring_a_restart_flight() {
    let scheduler = Mutex::new(crate::state::RuntimeRestartScheduler::default());
    assert!(check_stop_request(&scheduler, "world").is_ok());
    assert!(!scheduler.lock().unwrap().restart_allowed("world"));
    scheduler.lock().unwrap().request_stop("another-world");
    assert!(check_stop_request(&scheduler, "world").is_ok());
    scheduler.lock().unwrap().request_stop("world");
    assert!(
        check_stop_request(&scheduler, "world")
            .unwrap_err()
            .contains("停止请求")
    );
    // An explicitly new manual start clears the old stop flag in the existing
    // scheduler; the runtime generation and reservation are checked separately.
    scheduler.lock().unwrap().reset_for_manual_start("world");
    assert!(check_stop_request(&scheduler, "world").is_ok());
}

#[cfg(windows)]
#[tokio::test]
async fn stop_after_readiness_before_send_reaches_real_socket_without_any_command_or_retry() {
    use tokio::io::AsyncReadExt;
    let scheduler = Mutex::new(crate::state::RuntimeRestartScheduler::default());
    assert!(check_stop_request(&scheduler, "world").is_ok());
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 128];
        assert_eq!(socket.read(&mut bytes).await.unwrap(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    });
    let identity = app_runtime::inspect_process_identity(std::process::id())
        .unwrap()
        .unwrap();
    let result = console::create_once(
        port,
        "must-not-be-sent",
        "SAVE_1",
        std::process::id(),
        &identity,
        || {
            scheduler.lock().unwrap().request_stop("world");
            check_stop_request(&scheduler, "world")
        },
    )
    .await;
    assert!(result.unwrap_err().contains("停止请求"));
    server.await.unwrap();
}
