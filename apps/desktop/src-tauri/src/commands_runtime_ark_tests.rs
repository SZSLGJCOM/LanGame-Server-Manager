use super::*;
use serde_json::json;

fn instance(edition: &str) -> InstanceDetails {
    let main = if edition == "arksurvivalascended" {
        "TheIsland_WP"
    } else {
        "TheIsland"
    };
    let extra = if edition == "arksurvivalascended" {
        "ScorchedEarth_WP"
    } else {
        "ScorchedEarth_P"
    };
    serde_json::from_value(json!({
        "summary": { "id": "cluster-a", "name": "Cluster A", "module_id": edition,
            "status": "Running", "bind_ip": "127.0.0.1", "port_count": 8, "autostart": false },
        "config_file_path": "C:/fixture/cluster-a/config/instance.json",
        "saves_path": "C:/fixture/cluster-a/runtime/ShooterGame/Saved",
        "auto_backup_on_stop": false, "backup_retention_count": 0,
        "settings_json": json!({ "map_name": main, "server_name": "Cluster A",
            "max_players": 20, "rcon_enabled": true, "admin_password": "fixture-admin-password",
            "additional_maps": [
                { "id": "scorched", "map_name": extra, "name": "Scorched Earth", "enabled": true },
                { "id": "paused", "map_name": extra, "name": "Paused world", "enabled": false }
            ]
        }).to_string(),
        "ports": [
            { "name": "game", "protocol": "udp", "port": 7777 },
            { "name": "peer", "protocol": "udp", "port": 7778 },
            { "name": "query", "protocol": "udp", "port": 27015 },
            { "name": "rcon", "protocol": "tcp", "port": 27020 },
            { "name": "map-scorched-game", "protocol": "udp", "port": 7787 },
            { "name": "map-scorched-peer", "protocol": "udp", "port": 7788 },
            { "name": "map-scorched-query", "protocol": "udp", "port": 27025 },
            { "name": "map-scorched-rcon", "protocol": "tcp", "port": 27030 }
        ],
        "active_run": { "run_id": 1, "session_id": "session-a", "pid": 101,
            "log_path": null, "process_count": 2, "processes": [
                { "run_id": 1, "session_id": "session-a", "process_key": "main", "display_name": "Island",
                    "pid": 101, "status": "running", "started_at": null, "stopped_at": null,
                    "exit_code": null, "crash_flag": false, "log_path": null, "is_primary": true },
                { "run_id": 2, "session_id": "session-a", "process_key": "map-scorched", "display_name": "Scorched Earth",
                    "pid": 102, "status": "running", "started_at": null, "stopped_at": null,
                    "exit_code": null, "crash_flag": false, "log_path": null, "is_primary": false }
            ] }
    })).unwrap()
}

fn descriptor(edition: &str) -> ModuleDescriptor {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("modules");
    discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == edition)
        .unwrap()
}

#[test]
fn ark_map_launches_share_program_and_keep_owned_identity_and_distinct_native_paths() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let instance = instance(edition);
        let settings = AppSettings::default();
        let module = map_module_details_with_install_state(
            &settings,
            &descriptor(edition),
            Some("C:/fixture/program"),
        );
        let plans =
            build_launch_plans(&settings, &module, &instance, Some("C:/fixture/program")).unwrap();
        assert_eq!(plans.len(), 2, "Disabled maps must not launch");
        assert_eq!(plans[0].process_key, "main");
        assert_eq!(plans[1].process_key, "map-scorched");
        assert_eq!(plans[1].display_name, "Scorched Earth");
        assert_eq!(
            plans[0].launch_plan.executable_path,
            plans[1].launch_plan.executable_path
        );
        assert_eq!(
            plans[0].launch_plan.performance_policy.resource_limits,
            plans[1].launch_plan.performance_policy.resource_limits
        );
        for plan in &plans {
            assert_eq!(plan.launch_plan.instance_id, "cluster-a");
            assert_eq!(plan.launch_plan.instance_name, "Cluster A");
            assert!(
                plan.launch_plan
                    .args
                    .contains(&String::from("-clusterid=lgsm-cluster-a"))
            );
            assert!(
                !plan
                    .launch_plan
                    .command_line
                    .contains("fixture-admin-password")
            );
        }
        assert!(plans[0].launch_plan.args[0].contains("?AltSaveDirectoryName=cluster-a?"));
        assert!(
            plans[1].launch_plan.args[0].contains("?AltSaveDirectoryName=cluster-a-map-scorched?")
        );
        assert!(plans[1].launch_plan.args[0].contains("?RCONPort=27030"));
        assert!(plans[1].launch_plan.args[0].contains("?QueryPort=27025"));
        let main_log = plans[0]
            .launch_plan
            .args
            .iter()
            .find(|arg| arg.starts_with("-abslog="))
            .unwrap();
        let extra_log = plans[1]
            .launch_plan
            .args
            .iter()
            .find(|arg| arg.starts_with("-abslog="))
            .unwrap();
        assert_ne!(main_log, extra_log);
        assert!(extra_log.ends_with("map-scorched.log"));
    }
}

#[test]
fn ark_console_targets_require_an_enabled_live_owned_map() {
    let mut details = instance("arksurvivalascended");
    let selected = project_running_command(&details, Some("map-scorched"))
        .unwrap()
        .unwrap();
    assert_eq!(selected.summary.id, "cluster-a");
    assert_eq!(
        selected
            .ports
            .iter()
            .find(|port| port.name == "rcon")
            .unwrap()
            .port,
        27030
    );
    for key in ["map-paused", "map-unknown", "", "source_rcon"] {
        assert!(
            project_running_command(&details, Some(key)).is_err(),
            "{key}"
        );
    }
    details.active_run.as_mut().unwrap().processes[1].status = "error".into();
    assert!(project_running_command(&details, Some("map-scorched")).is_err());
    assert!(project_running_command(&details, None).is_ok());
    details.active_run = None;
    assert!(project_running_command(&details, None).is_err());
}

#[test]
fn ark_shutdown_saves_every_live_map_before_exiting_and_waits_once_per_phase() {
    let mut details = instance("arksurvivalascended");
    let native = descriptor("arksurvivalascended").runtime.shutdown.unwrap();
    let shutdown = shutdown_for_running_maps(&details, &native).unwrap();
    let commands = shutdown
        .commands
        .iter()
        .map(|command| {
            (
                command.command.as_str(),
                command.process_key.as_deref(),
                command.wait_after_ms,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        commands,
        [
            ("saveworld", Some("main"), 0),
            ("saveworld", Some("map-scorched"), 5000),
            ("DoExit", Some("main"), 0),
            ("DoExit", Some("map-scorched"), 6000)
        ]
    );
    details.active_run.as_mut().unwrap().processes[0].status = "error".into();
    let surviving = shutdown_for_running_maps(&details, &native).unwrap();
    assert_eq!(surviving.commands.len(), 2);
    assert!(
        surviving
            .commands
            .iter()
            .all(|command| command.process_key.as_deref() == Some("map-scorched"))
    );
}

fn endpoint(key: &str, address: &str, port: u16) -> ProcessNetworkEndpoint {
    ProcessNetworkEndpoint {
        protocol: "udp".into(),
        local_address: address.into(),
        local_port: port,
        owning_pid: if key == "main" { 101 } else { 102 },
        process_key: key.into(),
        relation: "root".into(),
    }
}

#[test]
fn ark_bind_readiness_requires_each_maps_own_listener() {
    let details = instance("arksurvivalascended");
    let policy = descriptor("arksurvivalascended").runtime.bind_address;
    let endpoints = [
        endpoint("main", "127.0.0.1", 7777),
        endpoint("map-scorched", "127.0.0.1", 7787),
    ];
    assert_eq!(
        evaluate_bind_endpoints(&details, &policy, "127.0.0.1", &endpoints).unwrap(),
        StrictBindEvaluation::Ready
    );
    let main_only = [
        endpoint("main", "127.0.0.1", 7777),
        endpoint("main", "127.0.0.1", 7787),
    ];
    assert_eq!(
        evaluate_bind_endpoints(&details, &policy, "127.0.0.1", &main_only).unwrap(),
        StrictBindEvaluation::Pending {
            missing_port_names: vec!["map-scorched:game".into()]
        }
    );
    let wrong = [
        endpoint("main", "127.0.0.1", 7777),
        endpoint("map-scorched", "0.0.0.0", 7787),
    ];
    assert!(matches!(
        evaluate_bind_endpoints(&details, &policy, "127.0.0.1", &wrong).unwrap(),
        StrictBindEvaluation::Failed { .. }
    ));
    assert_eq!(
        evaluate_bind_endpoints(&details, &policy, "0.0.0.0", &wrong).unwrap(),
        StrictBindEvaluation::Ready
    );
    assert!(matches!(
        evaluate_bind_endpoints(&details, &policy, "0.0.0.0", &main_only).unwrap(),
        StrictBindEvaluation::Pending { .. }
    ));
}

#[test]
fn ark_declared_console_actions_keep_map_selection_and_native_transport_metadata() {
    use super::super::commands_runtime_actions::{
        RuntimeCommandResolutionInput, resolve_runtime_command,
    };
    let descriptor = descriptor("arksurvivalascended");
    let mut input = RuntimeCommandResolutionInput {
        command: "forged",
        process_key: Some("map-scorched"),
        transport: Some("stdin"),
        port_name: Some("untrusted-port"),
        password_setting_key: Some("untrusted-password"),
        enabled_setting_key: None,
        runtime_action_id: Some("broadcast"),
        runtime_action_target: Some("Cluster message"),
        runtime_action_role: None,
    };
    let error = resolve_runtime_command(Some(&descriptor), input).unwrap_err();
    assert!(error.contains("single-token target"), "{error}");
    input.runtime_action_target = Some("Cluster-message");
    let resolved = resolve_runtime_command(Some(&descriptor), input).unwrap();
    assert_eq!(resolved.process_key.as_deref(), Some("map-scorched"));
    assert_eq!(resolved.command, "Broadcast Cluster-message");
    assert_eq!(resolved.transport, "source_rcon");
    assert_eq!(resolved.port_name.as_deref(), Some("rcon"));
    assert_eq!(
        resolved.password_setting_key.as_deref(),
        Some("admin_password")
    );
    assert_eq!(
        resolved.enabled_setting_key.as_deref(),
        Some("rcon_enabled")
    );
}

#[test]
fn ark_broadcast_text_uses_the_validated_message_channel() {
    use super::super::commands_broadcast::{
        find_broadcast_action, render_broadcast_command, validate_instance_broadcast_message,
    };
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let descriptor = descriptor(edition);
        let action = find_broadcast_action(&descriptor).unwrap();
        let message = validate_instance_broadcast_message("Cluster message").unwrap();
        assert_eq!(
            render_broadcast_command(action, &message).unwrap(),
            "Broadcast Cluster message"
        );
    }
    for message in [
        "Cluster\nDoExit",
        "Cluster;DoExit",
        "Cluster && DoExit",
        "Cluster || DoExit",
        "Cluster`DoExit",
        "Cluster$(DoExit)",
        "{{target}}",
    ] {
        assert!(
            validate_instance_broadcast_message(message).is_err(),
            "{message}"
        );
    }
}

#[tokio::test]
async fn ark_selected_map_rcon_command_reaches_only_its_registered_endpoint() {
    use crate::runtime_transport::{source_rcon_read_packet, source_rcon_write_packet};
    let main_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    main_listener.set_nonblocking(true).unwrap();
    let extra_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    extra_listener.set_nonblocking(true).unwrap();
    let mut details = instance("arksurvivalascended");
    details
        .ports
        .iter_mut()
        .find(|port| port.name == "rcon")
        .unwrap()
        .port = main_listener.local_addr().unwrap().port();
    details
        .ports
        .iter_mut()
        .find(|port| port.name == "map-scorched-rcon")
        .unwrap()
        .port = extra_listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || -> Result<(String, String), String> {
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut stream = loop {
            match extra_listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => return Err(format!("RCON test accept failed: {error}")),
            }
        };
        stream
            .set_nonblocking(false)
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|error| error.to_string())?;
        let auth = source_rcon_read_packet(&mut stream)?;
        assert_eq!(auth.packet_type, 3);
        source_rcon_write_packet(&mut stream, auth.id, 2, "")?;
        let command = source_rcon_read_packet(&mut stream)?;
        assert_eq!(command.packet_type, 2);
        assert_ne!(command.id, auth.id);
        source_rcon_write_packet(&mut stream, command.id, 0, "map command received")?;
        let marker = source_rcon_read_packet(&mut stream)?;
        assert_eq!(marker.packet_type, 2);
        assert_ne!(marker.id, command.id);
        assert_eq!(marker.body, b"ListPlayers");
        source_rcon_write_packet(&mut stream, marker.id, 0, "")?;
        Ok((
            String::from_utf8(auth.body).unwrap(),
            String::from_utf8(command.body).unwrap(),
        ))
    });
    let result = super::super::commands_runtime_lifecycle::dispatch_instance_runtime_transport(
        &DesktopState::default(),
        &details,
        &super::super::commands_runtime_lifecycle::RuntimeTransportRequest {
            command: "saveworld",
            transport: "source_rcon",
            process_key: Some("map-scorched"),
            port_name: Some("rcon"),
            password_setting_key: Some("admin_password"),
            enabled_setting_key: Some("rcon_enabled"),
        },
    )
    .await;
    let captured = server.join().unwrap().unwrap();
    result.unwrap();
    assert_eq!(
        captured,
        ("fixture-admin-password".into(), "saveworld".into())
    );
    assert_eq!(
        main_listener.accept().unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
}
