use super::*;

fn instance(edition: &str) -> InstanceDetails {
    serde_json::from_value(json!({
        "summary": { "id": "cluster-a", "name": "Cluster A", "module_id": edition,
            "status": "Running", "bind_ip": "127.0.0.1", "port_count": 12, "autostart": false },
        "config_file_path": "C:/fixture/cluster-a/config/instance.json",
        "saves_path": "C:/fixture/cluster-a/runtime/ShooterGame/Saved",
        "auto_backup_on_stop": false, "backup_retention_count": 0,
        "settings_json": json!({ "map_name": "TheIsland", "server_name": "Cluster A",
            "rcon_enabled": true, "admin_password": "fixture-password",
            "additional_maps": [
                { "id": "scorched", "map_name": "ScorchedEarth_P", "name": "Scorched", "enabled": true },
                { "id": "third", "map_name": "Aberration_P", "name": "Third map", "enabled": true },
                { "id": "paused", "map_name": "TheIsland", "name": "Paused", "enabled": false }
            ]
        }).to_string(),
        "ports": [
            { "name": "game", "protocol": "udp", "port": 7777 },
            { "name": "query", "protocol": "udp", "port": 27015 },
            { "name": "rcon", "protocol": "tcp", "port": 27020 },
            { "name": "map-scorched-game", "protocol": "udp", "port": 7787 },
            { "name": "map-scorched-query", "protocol": "udp", "port": 27025 },
            { "name": "map-scorched-rcon", "protocol": "tcp", "port": 27030 },
            { "name": "map-third-game", "protocol": "udp", "port": 7797 },
            { "name": "map-third-query", "protocol": "udp", "port": 27035 },
            { "name": "map-third-rcon", "protocol": "tcp", "port": 27040 }
        ],
        "active_run": { "run_id": 1, "session_id": "session-a", "pid": 101,
            "log_path": null, "process_count": 3, "processes": [
                { "run_id": 1, "session_id": "session-a", "process_key": "main", "display_name": "Island",
                    "pid": 101, "status": "running", "started_at": null, "stopped_at": null,
                    "exit_code": null, "crash_flag": false, "log_path": null, "is_primary": true },
                { "run_id": 2, "session_id": "session-a", "process_key": "map-scorched", "display_name": "Scorched",
                    "pid": 102, "status": "running", "started_at": null, "stopped_at": null,
                    "exit_code": null, "crash_flag": false, "log_path": null, "is_primary": false },
                { "run_id": 3, "session_id": "session-a", "process_key": "map-third", "display_name": "Third map",
                    "pid": 103, "status": "running", "started_at": null, "stopped_at": null,
                    "exit_code": null, "crash_flag": false, "log_path": null, "is_primary": false }
            ] }
    })).unwrap()
}

fn request() -> WorldSaveRequest<'static> {
    WorldSaveRequest {
        command: "saveworld",
        port_name: Some("rcon"),
        password_setting_key: Some("admin_password"),
        enabled_setting_key: Some("rcon_enabled"),
        expected_run_id: Some(1),
        redact_audit_payload: false,
        submitted_at_unix_ms: 1,
    }
}

fn execute_success(endpoint: &str, password: &str, command: &str) -> Result<String, String> {
    assert!(["127.0.0.1:27020", "127.0.0.1:27030", "127.0.0.1:27040"].contains(&endpoint));
    assert_eq!(password, "fixture-password");
    assert_eq!(command, "saveworld");
    Ok(String::from("server response"))
}

#[tokio::test]
async fn instance_save_sends_once_to_every_enabled_map_for_both_ark_editions() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let details = instance(edition);
        let mut observed = Vec::new();
        let mut ownership_checks = 0;
        let sent = dispatch_using(
            &details,
            &request(),
            execute_success,
            || {
                ownership_checks += 1;
                Ok(())
            },
            |key, response| {
                assert_eq!(response, "server response");
                observed.push(key.to_owned());
            },
        )
        .await
        .unwrap();
        assert_eq!(sent, ["main", "map-scorched", "map-third"]);
        assert_eq!(observed, sent);
        assert_eq!(ownership_checks, 3);
    }
}

#[tokio::test]
async fn invalid_map_preflight_sends_nothing_including_to_preceding_valid_maps() {
    for invalid in [
        "stopped",
        "missing_pid",
        "missing_process",
        "missing_port",
        "wrong_protocol",
    ] {
        let mut details = instance("arksurvivalascended");
        match invalid {
            "stopped" => {
                details.active_run.as_mut().unwrap().processes[1].status = "stopped".into()
            }
            "missing_pid" => details.active_run.as_mut().unwrap().processes[1].pid = None,
            "missing_process" => {
                details.active_run.as_mut().unwrap().processes.remove(1);
            }
            "missing_port" => details
                .ports
                .retain(|port| port.name != "map-scorched-rcon"),
            "wrong_protocol" => {
                details
                    .ports
                    .iter_mut()
                    .find(|port| port.name == "map-scorched-rcon")
                    .unwrap()
                    .protocol = "udp".into()
            }
            _ => unreachable!(),
        }
        let mut attempts = 0;
        let error = dispatch_using(
            &details,
            &request(),
            execute_success,
            || {
                attempts += 1;
                Ok(())
            },
            |_, _| panic!("preflight failure must not send to any map"),
        )
        .await
        .unwrap_err();
        assert!(
            error.contains("preflight failed; no save commands were sent"),
            "{error}"
        );
        assert!(error.contains("map-scorched"), "{error}");
        assert_eq!(attempts, 0, "{invalid}");
    }
}

#[tokio::test]
async fn disabled_or_unconfigured_rcon_fails_before_sending() {
    for (key, invalid) in [
        ("rcon_enabled", json!(false)),
        ("admin_password", json!("")),
    ] {
        let mut details = instance("arksurvivalevolved");
        let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
        settings[key] = invalid;
        details.settings_json = settings.to_string();
        let error = dispatch_using(
            &details,
            &request(),
            execute_success,
            || panic!("invalid RCON settings must prevent all attempts"),
            |_, _| panic!("invalid RCON settings must prevent all sends"),
        )
        .await
        .unwrap_err();
        assert!(error.contains("no save commands were sent"));
        assert!(error.contains(key));
    }
}

fn execute_fail_second(endpoint: &str, password: &str, command: &str) -> Result<String, String> {
    if endpoint == "127.0.0.1:27030" {
        Err(String::from("fixture response timed out"))
    } else {
        execute_success(endpoint, password, command)
    }
}

#[tokio::test]
async fn transport_failure_reports_partial_delivery_without_retrying_or_sending_later_maps() {
    let mut attempts = 0;
    let mut sent = Vec::new();
    let error = dispatch_using(
        &instance("arksurvivalascended"),
        &request(),
        execute_fail_second,
        || {
            attempts += 1;
            Ok(())
        },
        |key, _| sent.push(key.to_owned()),
    )
    .await
    .unwrap_err();
    assert_eq!(attempts, 2);
    assert_eq!(sent, ["main"]);
    assert!(error.contains("stopped at `map-scorched`"), "{error}");
    assert!(error.contains("command delivery and save completion are unknown"));
    assert!(error.contains("Commands already sent: main."));
    assert!(error.contains("Maps not attempted after this target: map-third."));
    assert!(error.contains("Disk completion is not confirmed."));
}

#[tokio::test]
async fn ownership_change_stops_before_the_next_map_write() {
    let mut attempts = 0;
    let mut sent = Vec::new();
    let error = dispatch_using(
        &instance("arksurvivalevolved"),
        &request(),
        execute_success,
        || {
            attempts += 1;
            if attempts == 2 {
                Err("run changed".into())
            } else {
                Ok(())
            }
        },
        |key, _| sent.push(key.to_owned()),
    )
    .await
    .unwrap_err();
    assert_eq!(sent, ["main"]);
    assert!(error.contains("stopped at `map-scorched` (command was not sent)"));
    assert!(error.contains("Commands already sent: main."));
    assert!(error.contains("Maps not attempted after this target: map-third."));
}

#[tokio::test]
async fn single_map_runs_without_process_rows_keep_the_owned_primary_target() {
    let mut details = instance("arksurvivalevolved");
    details.active_run.as_mut().unwrap().processes.clear();
    let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    settings["additional_maps"] = json!([]);
    details.settings_json = settings.to_string();
    let sent = dispatch_using(&details, &request(), execute_success, || Ok(()), |_, _| {})
        .await
        .unwrap();
    assert_eq!(sent, ["main"]);
}

#[test]
fn explicit_console_map_projection_remains_a_single_target() {
    let details = instance("arksurvivalascended");
    for (key, expected_port) in [(None, 27020), (Some("map-scorched"), 27030)] {
        let selected =
            crate::commands::commands_runtime_ark::project_running_command(&details, key)
                .unwrap()
                .unwrap();
        assert_eq!(
            selected
                .ports
                .iter()
                .find(|port| port.name == "rcon")
                .unwrap()
                .port,
            expected_port
        );
    }
}
