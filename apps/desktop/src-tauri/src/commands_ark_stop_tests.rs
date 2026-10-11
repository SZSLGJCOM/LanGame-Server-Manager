use super::*;
use crate::runtime_transport::{source_rcon_read_packet, source_rcon_write_packet};

// Substitute only the game RCON boundary. All maps run real owned native
// processes; their exit and the production shutdown dispatcher remain real.
fn map_peer(
    listener: std::net::TcpListener,
    runtime: Arc<StdMutex<app_runtime::RuntimeSupervisor>>,
    instance_id: String,
    key: String,
    exits: bool,
    commands: usize,
    received: Arc<StdMutex<Vec<String>>>,
) -> std::thread::JoinHandle<Result<(), String>> {
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        for index in 0..commands {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => return Err(error.to_string()),
                }
            };
            stream.set_nonblocking(false).map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .map_err(|e| e.to_string())?;
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .map_err(|e| e.to_string())?;
            let auth = source_rcon_read_packet(&mut stream)?;
            source_rcon_write_packet(&mut stream, auth.id, 2, "")?;
            let command = source_rcon_read_packet(&mut stream)?;
            let expected = if index == 0 { "saveworld" } else { "DoExit" };
            if command.body != expected.as_bytes() {
                return Err(format!("unexpected map command for {key}"));
            }
            received.lock().unwrap().push(format!("{key}:{expected}"));
            if index == 0 || exits {
                runtime
                    .lock()
                    .unwrap()
                    .dispatch_command(
                        &instance_id,
                        Some(&key),
                        if index == 0 { "saveworld" } else { "release" },
                    )
                    .map_err(|e| e.to_string())?;
            }
            if index == 0 {
                source_rcon_write_packet(&mut stream, command.id, 0, "saved")?;
                let marker = source_rcon_read_packet(&mut stream)?;
                if marker.body != b"ListPlayers" {
                    return Err("missing read-only ARK completion marker".into());
                }
                source_rcon_write_packet(&mut stream, marker.id, 0, "")?;
            }
            // DoExit closes RCON without replying, as the native game does.
        }
        Ok(())
    })
}

#[tokio::test]
async fn ark_stop_lost_exit_response_still_stops_every_map_without_replaying() {
    assert_map_stop(true, false, 2).await;
}

#[tokio::test]
async fn ark_stop_lost_exit_response_retains_a_refusing_map_without_replaying() {
    assert_map_stop(false, false, 2).await;
}

#[tokio::test]
async fn ark_stop_retry_after_primary_reaped_stops_the_same_sessions_surviving_map() {
    assert_map_stop(true, true, 2).await;
}

#[tokio::test]
async fn ark_stop_maximum_maps_saves_all_before_stopping_each_without_replaying() {
    assert_map_stop(true, false, app_core::ark_maps::MAX_ADDITIONAL_MAPS + 1).await;
}

async fn assert_map_stop(main_exits: bool, retry_after_primary_exit: bool, map_count: usize) {
    assert!((2..=app_core::ark_maps::MAX_ADDITIONAL_MAPS + 1).contains(&map_count));
    let mut first = Fixture::new().await;
    first.persist_running().await;
    let id = first.details.summary.id.clone();
    let mut instance = first
        .state
        .runtime_supervisor
        .lock()
        .unwrap()
        .take_running_for_stop(&id)
        .unwrap();
    let mut extra_fixtures = Vec::new();
    let mut additional_maps = Vec::new();
    for index in 1..map_count {
        let fixture = Fixture::new().await;
        let map_id = format!("extra-{index}");
        let key = app_core::ark_maps::process_key(&map_id);
        let name = format!("Extra map {index}");
        let recorded = mark_instance_process_started_with_identity(
            &first.storage.paths,
            &StartedInstanceProcess {
                instance_id: &id,
                session_id: first.active.session_id.as_deref(),
                process_key: &key,
                display_name: &name,
                pid: fixture.pid,
                log_path: fixture.active.log_path.as_deref().unwrap(),
                is_primary: false,
            },
            Some(&fixture.identity),
        )
        .await
        .unwrap();
        assert_eq!(recorded.run_id, index as i64 + 1);
        let mut other = fixture
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .take_running_for_stop(&id)
            .unwrap();
        other.processes[0].run_id = recorded.run_id;
        other.processes[0].process_key = key.clone();
        other.processes[0].is_primary = false;
        instance.processes.append(&mut other.processes);
        let mut extra = fixture.active.processes[0].clone();
        extra.run_id = recorded.run_id;
        extra.process_key = key.clone();
        extra.is_primary = false;
        first.active.processes.push(extra);
        additional_maps.push(json!({
            "id": map_id, "name": name, "enabled": true,
            "map_name": if index % 2 == 1 { "ScorchedEarth_WP" } else { "Extinction_WP" }
        }));
        extra_fixtures.push((key, fixture));
    }
    instance.summary.active_process_count = map_count;
    assert!(
        first
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .restore_running_after_failed_stop(instance)
    );
    first.active.process_count = map_count;
    first.details.active_run = Some(first.active.clone());
    first.details.summary.module_id = "arksurvivalascended".into();
    first.details.summary.active_process_count = map_count;
    first.details.config_file_path = first
        .root
        .join("config/instance.json")
        .to_string_lossy()
        .into_owned();
    first.details.saves_path = first
        .root
        .join("runtime/ShooterGame/Saved")
        .to_string_lossy()
        .into_owned();
    first.details.settings_json = json!({
        "map_name": "TheIsland_WP", "rcon_enabled": true, "admin_password": "fixture-password",
        "additional_maps": additional_maps
    })
    .to_string();
    let keys = std::iter::once("main".to_owned())
        .chain(extra_fixtures.iter().map(|(key, _)| key.clone()))
        .collect::<Vec<_>>();
    let mut listeners = Vec::new();
    let mut ports = Vec::new();
    for (index, key) in keys.iter().enumerate() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let prefix = if index == 0 {
            String::new()
        } else {
            format!("{key}-")
        };
        ports.extend([
            json!({"name":format!("{prefix}game"), "protocol":"udp", "port":7777 + index * 10}),
            json!({"name":format!("{prefix}peer"), "protocol":"udp", "port":7778 + index * 10}),
            json!({"name":format!("{prefix}query"), "protocol":"udp", "port":27015 + index * 10}),
            json!({"name":format!("{prefix}rcon"), "protocol":"tcp", "port":listener.local_addr().unwrap().port()}),
        ]);
        listeners.push(listener);
    }
    first.details.ports = serde_json::from_value(json!(ports)).unwrap();
    if retry_after_primary_exit {
        first
            .state
            .runtime_supervisor
            .lock()
            .unwrap()
            .dispatch_command(&id, Some("main"), "release")
            .unwrap();
        first.wait_for_log_line("received:release").await;
        let reaped = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                // Observe the retained child handle while it exits. Reopening
                // its PID can fail during Windows process teardown.
                let reaped = first
                    .state
                    .runtime_supervisor
                    .lock()
                    .unwrap()
                    .reap_exited()
                    .unwrap();
                if !reaped.is_empty() {
                    break reaped;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(reaped.len(), 1);
        assert_eq!(reaped[0].run_id, 1);
        assert_eq!(reaped[0].pid, first.pid);
        assert_eq!(reaped[0].session_id, first.active.session_id);
        assert!(reaped[0].is_primary);
        assert_eq!(reaped[0].exit_code, Some(0));
        mark_instance_process_stopped(&first.storage.paths, &id, 1, Some(0), false)
            .await
            .unwrap();
        first.active = read_active_instance_run(&first.storage.paths, &id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            first.active.run_id, 1,
            "the storage session retains the original primary"
        );
        assert_eq!(
            first
                .active
                .processes
                .iter()
                .filter(|process| process.status == "running")
                .count(),
            map_count - 1
        );
        first.details.active_run = Some(first.active.clone());
    }
    // Validate the complete map fixture before any peer begins accepting;
    // configuration failures must not be mistaken for shutdown failures.
    let maps = app_core::ark_maps::processes(&first.details).unwrap();
    assert_eq!(maps.len(), map_count);
    for map in maps {
        if retry_after_primary_exit && map.process_key == "main" {
            continue;
        }
        super::super::super::commands_runtime_ark::project_running_command(
            &first.details,
            Some(&map.process_key),
        )
        .unwrap();
    }
    let received = Arc::new(StdMutex::new(Vec::new()));
    let peers = keys
        .iter()
        .zip(listeners)
        .enumerate()
        .map(|(index, (key, listener))| {
            let commands = if index == 0 && retry_after_primary_exit {
                0
            } else if index == 0 || main_exits {
                2
            } else {
                1
            };
            map_peer(
                listener,
                Arc::clone(&first.state.runtime_supervisor),
                id.clone(),
                key.clone(),
                index != 0 || main_exits,
                commands,
                Arc::clone(&received),
            )
        })
        .collect::<Vec<_>>();
    let shutdown: ModuleShutdownSpec = serde_json::from_value(json!({
        "grace_period_ms": if main_exits { 2000 } else { 50 }, "commands": [
            {"transport":"source_rcon", "fallback_transport":"stdin", "command":"saveworld",
                "password_setting_key":"admin_password", "enabled_setting_key":"rcon_enabled"},
            {"transport":"source_rcon", "fallback_transport":"stdin", "command":"DoExit",
                "password_setting_key":"admin_password", "enabled_setting_key":"rcon_enabled"}
        ]
    }))
    .unwrap();
    let lease = first
        .state
        .begin_storage_context_operation("ARK map stop regression")
        .unwrap();
    let result = super::super::super::commands_instance_stop::stop_run(
        &first.state,
        &first.storage,
        &lease,
        &first.details,
        &first.active,
        Some(&shutdown),
        InstanceShutdownSource::Manual,
    )
    .await;
    for (key, peer) in keys.iter().zip(peers) {
        let peer_result = peer.join().unwrap();
        assert!(
            peer_result.is_ok(),
            "{key} RCON peer: {peer_result:?}; stop result: {result:?}; received: {:?}",
            received.lock().unwrap()
        );
    }
    let running_keys = keys.iter().skip(usize::from(retry_after_primary_exit));
    let mut expected = running_keys
        .clone()
        .map(|key| format!("{key}:saveworld"))
        .collect::<Vec<_>>();
    if main_exits {
        expected.extend(running_keys.map(|key| format!("{key}:DoExit")));
        let stopped =
            result.expect("the first map's RCON disconnect must not prevent stopping the next map");
        assert_eq!(
            stopped.len(),
            map_count - usize::from(retry_after_primary_exit)
        );
        assert!(stopped.iter().all(|process| process.exit_code == Some(0)));
        assert!(!process_matches_identity(first.pid, &first.identity).unwrap());
        for (_, fixture) in &extra_fixtures {
            assert!(!process_matches_identity(fixture.pid, &fixture.identity).unwrap());
        }
        assert!(
            read_active_instance_run(&first.storage.paths, &id)
                .await
                .unwrap()
                .is_none()
        );
    } else {
        let error = result.expect_err("a refusing map cannot be treated as exited");
        assert!(
            error.contains("no fallback or forced stop was attempted"),
            "{error}"
        );
        expected.push("main:DoExit".into());
        assert!(process_matches_identity(first.pid, &first.identity).unwrap());
        for (_, fixture) in &extra_fixtures {
            assert!(process_matches_identity(fixture.pid, &fixture.identity).unwrap());
        }
    }
    assert_eq!(*received.lock().unwrap(), expected);
    for fixture in std::iter::once(&first).chain(extra_fixtures.iter().map(|(_, map)| map)) {
        let log = fs::read_to_string(fixture.root.join("server.log")).unwrap();
        assert!(
            !log.contains("received:DoExit"),
            "lost responses must not replay over stdin"
        );
        if main_exits {
            assert_eq!(log.matches("received:release").count(), 1);
            assert_eq!(
                log.matches("received:saveworld").count(),
                usize::from(!retry_after_primary_exit || fixture.pid != first.pid)
            );
        }
    }
    // Cooperative cleanup of the disposable maps, including negative cases.
    let mut runtime = first.state.runtime_supervisor.lock().unwrap();
    for key in &keys {
        let _ = runtime.dispatch_command(&id, Some(key), "release");
    }
}
