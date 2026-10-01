use std::io::{Read, Write};
use std::net::TcpListener;

use super::*;

const NORMAL: &[u8] = include_bytes!("../../test-data/live-players/nightingale/normal.json");
const EMPTY: &[u8] = include_bytes!("../../test-data/live-players/nightingale/empty.json");

#[test]
fn nightingale_returns_observed_names_without_claiming_stable_account_identity() {
    let result = parse_nightingale("instance", NORMAL, "request", 42).expect("official contract");
    assert_eq!(result.public_snapshot.current_players, Some(2));
    assert_eq!(result.public_snapshot.entries[0].display_name, "Builder");
    assert_eq!(result.public_snapshot.entries[1].display_name, "探索者");
    assert!(result.public_snapshot.complete);
    assert!(result.public_snapshot.entries.iter().all(|entry| {
        entry.available_action_ids.is_empty()
            && entry.identifiers[0].kind == RuntimePlayerIdentityKind::PlayerName
            && !entry.identifiers[0].stable
    }));
    assert!(result.private_action_bindings.is_empty());
}

#[test]
fn nightingale_keeps_equal_names_as_distinct_snapshot_rows() {
    let response = br#"{"status":"ready","player_count":2,"player_names":["Builder","Builder"]}"#;
    let result =
        parse_nightingale("instance", response, "request", 42).expect("duplicate names allowed");
    assert_ne!(
        result.public_snapshot.entries[0].player_key,
        result.public_snapshot.entries[1].player_key
    );
    assert_eq!(result.public_snapshot.current_players, Some(2));
}

#[test]
fn nightingale_zero_players_requires_complete_ready_status() {
    let result = parse_nightingale("instance", EMPTY, "request", 42).expect("empty contract");
    assert_eq!(result.public_snapshot.current_players, Some(0));
    assert!(result.public_snapshot.complete);
    for response in [
        "",
        "{}",
        r#"{"status":"loading","player_count":0,"player_names":[]}"#,
        r#"{"status":"ready","player_count":1,"player_names":[]}"#,
        r#"{"status":"ready","player_count":1,"player_names":[""]}"#,
        r#"{"status":"ready","player_count":1,"player_names":["line\nbreak"]}"#,
        r#"{"status":"ready","player_count":-1,"player_names":[]}"#,
    ] {
        let error =
            parse_nightingale("instance", response.as_bytes(), "request", 42).expect_err(response);
        assert!(!error.complete);
        assert!(error.entries.is_empty());
    }
}

#[test]
fn nightingale_limits_responses_and_rejects_nonlocal_endpoints() {
    let error = parse_nightingale(
        "instance",
        &vec![b' '; MAX_HTTP_RESPONSE_BYTES + 1],
        "request",
        42,
    )
    .expect_err("bounded");
    assert!(error.truncated);
    assert!(local_address("localhost", 8080).is_none());
    assert!(local_address("192.0.2.10", 8080).is_none());
    assert_eq!(
        local_address("0.0.0.0", 8080),
        Some(SocketAddr::from(([127, 0, 0, 1], 8080)))
    );
}

#[tokio::test]
async fn nightingale_disabled_status_returns_actionable_configuration_state() {
    let mut details = instance_details(8080);
    details.settings_json = r#"{"status_endpoint_enabled":false}"#.to_owned();
    let result = collect_nightingale(&details, "instance", "request", 42)
        .await
        .expect_err("disabled");
    assert_eq!(result.status, RuntimeLivePlayerStatus::Misconfigured);
    assert_eq!(
        result.issue.expect("setting hint").setting_keys,
        ["status_endpoint_enabled"]
    );
}

#[tokio::test]
async fn nightingale_reads_local_status_and_keeps_loading_non_authoritative() {
    for (status, body, ready) in [
        ("200 OK", NORMAL, true),
        (
            "503 Service Unavailable",
            br#"{"status":"loading","player_count":0,"player_names":[]}"#.as_slice(),
            false,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let port = listener.local_addr().expect("fixture address").port();
        listener
            .set_nonblocking(true)
            .expect("bounded fixture accept");
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            std::str::from_utf8(body).expect("UTF-8 fixture")
        );
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            };
            // Winsock inherits the listener mode; request reads below use deadlines.
            stream
                .set_nonblocking(false)
                .expect("blocking fixture connection");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("request deadline");
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .expect("response deadline");
            let mut request = [0_u8; 2048];
            let size = stream.read(&mut request).expect("HTTP request");
            assert!(
                String::from_utf8_lossy(&request[..size]).starts_with("GET /status HTTP/1.1\r\n")
            );
            stream
                .write_all(response.as_bytes())
                .expect("HTTP response");
        });
        let result = collect_nightingale(&instance_details(port), "instance", "request", 42).await;
        server.join().expect("fixture server");
        if ready {
            assert_eq!(
                result.expect("ready list").public_snapshot.current_players,
                Some(2)
            );
        } else {
            let error = result.expect_err("loading");
            assert_eq!(error.status, RuntimeLivePlayerStatus::Failed);
            assert_eq!(
                error.issue.expect("preparing server").code,
                RuntimeLivePlayerIssueCode::ProcessUnavailable
            );
            assert!(!error.complete);
            assert!(error.entries.is_empty());
        }
    }
}

fn instance_details(port: u16) -> InstanceDetails {
    serde_json::from_value(serde_json::json!({
        "summary": { "id": "instance", "name": "Nightingale fixture", "module_id": "nightingale",
            "status": "Running", "bind_ip": "0.0.0.0", "port_count": 1, "autostart": false },
        "config_file_path": "", "saves_path": "", "auto_backup_on_stop": false,
        "backup_retention_count": 1, "settings_json": "{\"status_endpoint_enabled\":true}",
        "ports": [{"name": "status", "protocol": "tcp", "port": port}], "active_run": null,
    }))
    .expect("synthetic local instance")
}
