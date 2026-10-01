use std::io::{Read, Write};
use std::net::TcpListener;

use super::*;

const NORMAL: &[u8] = include_bytes!("../../test-data/live-players/palworld/normal.json");
const EMPTY: &[u8] = include_bytes!("../../test-data/live-players/palworld/empty.json");

#[test]
fn palworld_projects_verified_identities_without_private_ip_or_action_targets() {
    let result = parse_palworld("instance", NORMAL, "request", 42).expect("official schema case");
    assert_eq!(result.public_snapshot.current_players, Some(2));
    assert_eq!(result.public_snapshot.entries[0].display_name, "Builder");
    assert_eq!(result.public_snapshot.entries[1].display_name, "探索者");
    assert_eq!(result.public_snapshot.entries[0].ping_ms, Some(3));
    assert_eq!(result.public_snapshot.entries[1].ping_ms, Some(29));
    assert_eq!(
        result.public_snapshot.entries[0].identifiers[0].kind,
        RuntimePlayerIdentityKind::PalworldUserId
    );
    assert!(result.public_snapshot.complete);
    assert!(result.private_action_bindings.is_empty());
    let serialized = serde_json::to_string(&result.public_snapshot).expect("public projection");
    assert!(!serialized.contains("192.0.2."));
    assert!(!serialized.contains("location_x"));
    assert!(
        result
            .public_snapshot
            .entries
            .iter()
            .all(|entry| entry.available_action_ids.is_empty())
    );
}

#[test]
fn palworld_empty_is_authoritative_but_missing_malformed_or_duplicate_identities_fail() {
    let result = parse_palworld("instance", EMPTY, "request", 42).expect("empty schema");
    assert_eq!(result.public_snapshot.current_players, Some(0));
    assert!(result.public_snapshot.complete);
    for body in [
        "",
        "{}",
        "{\"players\": null}",
        "{\"players\":[{\"name\":\"Builder\"}]}",
        r#"{"players":[{"name":"Builder","userId":""}]}"#,
        r#"{"players":[{"name":"Builder","userId":"steam_1","ping":-1}]}"#,
        r#"{"players":[{"name":"Builder","userId":"steam_1\n"}]}"#,
        r#"{"players":[{"name":"A","userId":"steam_1"},{"name":"B","userId":"steam_1"}]}"#,
    ] {
        let error = parse_palworld("instance", body.as_bytes(), "request", 42).expect_err(body);
        assert!(!error.complete);
        assert!(error.entries.is_empty());
    }
}

#[test]
fn palworld_player_actions_bind_server_user_ids_to_the_current_snapshot_only() {
    use super::super::cache::{LivePlayerCacheKey, LivePlayerRegistry};
    let mut parsed = parse_palworld("instance", NORMAL, "snapshot", 42).unwrap();
    bind_player_actions(
        &mut parsed,
        &[
            "kick_player".into(),
            "ban_player".into(),
            "unban_player".into(),
            "arbitrary".into(),
        ],
    );
    let player_id = parsed.public_snapshot.entries[0].player_key.clone();
    for entry in &parsed.public_snapshot.entries {
        assert_eq!(entry.available_action_ids, ["kick_player", "ban_player"]);
        for action_id in &entry.available_action_ids {
            assert_eq!(
                parsed
                    .private_action_bindings
                    .get(&(entry.player_key.clone(), action_id.clone())),
                Some(&entry.player_key)
            );
        }
    }
    let key = LivePlayerCacheKey {
        instance_id: "instance".into(),
        run_id: "run-a".into(),
        security_contract_fingerprint: "rest-contract".into(),
        refresh_interval_ms: 30_000,
    };
    let registry = LivePlayerRegistry::default();
    registry.store_success(&key, parsed);
    assert_eq!(
        registry.resolve_action_binding(&key, "snapshot", &player_id, "kick_player"),
        Some(player_id.clone())
    );
    assert!(
        registry
            .resolve_action_binding(&key, "forged-snapshot", &player_id, "kick_player")
            .is_none()
    );
    assert!(
        registry
            .resolve_action_binding(&key, "snapshot", &player_id, "unban_player")
            .is_none()
    );
    let mut next_run = key.clone();
    next_run.run_id = "run-b".into();
    assert!(
        registry
            .resolve_action_binding(&next_run, "snapshot", &player_id, "kick_player")
            .is_none()
    );
    registry.invalidate_instance("instance");
    assert!(
        registry
            .resolve_action_binding(&key, "snapshot", &player_id, "kick_player")
            .is_none()
    );
}

#[test]
fn palworld_enforces_body_and_player_limits() {
    let oversized_body = vec![b' '; MAX_RESPONSE_BYTES + 1];
    let rows = (0..=MAX_PLAYERS)
        .map(|index| {
            serde_json::json!({
                "name": "Builder", "userId": format!("steam_{index}"),
            })
        })
        .collect::<Vec<_>>();
    let oversized_rows = serde_json::to_vec(&serde_json::json!({"players": rows})).expect("rows");
    for body in [oversized_body, oversized_rows] {
        let error = parse_palworld("instance", &body, "request", 42).expect_err("bounded");
        assert!(error.truncated);
        assert_eq!(
            error.issue.expect("limit issue").code,
            RuntimeLivePlayerIssueCode::CaptureLimit
        );
    }
}

#[tokio::test]
async fn palworld_http_authenticates_only_the_local_players_endpoint() {
    use base64::Engine;

    let credential = fixture_credential();
    let (port, server) = http_response("200 OK", "", NORMAL, Duration::ZERO);
    let body = fetch_players(port, &credential, REQUEST_TIMEOUT)
        .await
        .expect("HTTP body");
    assert_eq!(body, NORMAL);
    let request = server.join().expect("HTTP fixture thread");
    assert!(request.starts_with("GET /v1/api/players HTTP/1.1\r\n"));
    let authorization = request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization")
                .then_some(value.trim())
        })
        .expect("authorization header");
    let encoded = authorization.strip_prefix("Basic ").expect("Basic scheme");
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .expect("Basic credentials");
    assert_eq!(decoded, format!("admin:{credential}").as_bytes());
    assert!(!request.contains(&credential));
}

#[tokio::test]
async fn palworld_http_rejects_auth_failures_redirects_and_oversized_responses() {
    for (status, headers, body, expected) in [
        (
            "401 Unauthorized",
            "",
            b"private response".as_slice(),
            FetchError::Authentication,
        ),
        (
            "302 Found",
            "Location: http://127.0.0.1:1/\r\n",
            b"".as_slice(),
            FetchError::HttpStatus(302),
        ),
        (
            "200 OK",
            "Content-Length: 262145\r\n",
            b"".as_slice(),
            FetchError::TooLarge,
        ),
    ] {
        let (port, server) = http_response(status, headers, body, Duration::ZERO);
        assert_eq!(
            fetch_players(port, &fixture_credential(), REQUEST_TIMEOUT).await,
            Err(expected)
        );
        server.join().expect("HTTP fixture thread");
    }
}

#[tokio::test]
async fn palworld_http_applies_total_request_deadline() {
    let (port, server) = http_response("200 OK", "", EMPTY, Duration::from_millis(150));
    assert_eq!(
        fetch_players(port, &fixture_credential(), Duration::from_millis(30)).await,
        Err(FetchError::Timeout)
    );
    server.join().expect("HTTP fixture thread");
}

#[tokio::test]
async fn palworld_collection_requires_matching_instance_and_explicit_api_configuration() {
    let mut details = instance_details(8212);
    let rejected = collect_palworld(&details, "different-instance", "request", 42, &[])
        .await
        .expect_err("instance isolation");
    assert_eq!(rejected.status, RuntimeLivePlayerStatus::Misconfigured);
    details.settings_json = r#"{"rest_api_enabled":false}"#.to_owned();
    let rejected = collect_palworld(&details, "instance", "request", 42, &[])
        .await
        .expect_err("API disabled");
    assert_eq!(
        rejected.issue.expect("configuration hint").setting_keys,
        ["rest_api_enabled"]
    );
    details.settings_json = r#"{"rest_api_enabled":true}"#.to_owned();
    let rejected = collect_palworld(&details, "instance", "request", 42, &[])
        .await
        .expect_err("credentials absent");
    assert_eq!(
        rejected.issue.expect("configuration hint").setting_keys,
        ["admin_password"]
    );
    details.settings_json = fixture_settings();
    details.ports.push(details.ports[0].clone());
    let rejected = collect_palworld(&details, "instance", "request", 42, &[])
        .await
        .expect_err("ambiguous endpoint");
    assert_eq!(rejected.status, RuntimeLivePlayerStatus::Misconfigured);
}

#[tokio::test]
async fn palworld_collection_uses_instance_port_and_ignores_advertised_remote_addresses() {
    let (port, server) = http_response("200 OK", "", NORMAL, Duration::ZERO);
    let details = instance_details(port);
    let result = collect_palworld(&details, "instance", "request", 42, &[])
        .await
        .expect("local REST list");
    server.join().expect("HTTP fixture thread");
    assert_eq!(result.public_snapshot.current_players, Some(2));
    assert_eq!(
        result.public_snapshot.source,
        Some(ModulePlayerListSource::HttpApi)
    );
}

pub(crate) fn instance_details(port: u16) -> InstanceDetails {
    InstanceDetails {
        summary: app_core::InstanceSummary {
            id: String::from("instance"),
            name: String::from("Palworld fixture"),
            module_id: String::from("palworld"),
            status: app_core::InstanceStatus::Running,
            active_process_count: 1,
            bind_ip: String::from("192.0.2.10"),
            port_count: 1,
            autostart: false,
        },
        config_file_path: String::new(),
        saves_path: String::new(),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 1,
        settings_json: fixture_settings(),
        ports: vec![app_core::PortBinding {
            name: String::from("rest_api"),
            protocol: String::from("tcp"),
            port,
        }],
        active_run: None,
    }
}

fn fixture_credential() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn fixture_settings() -> String {
    serde_json::json!({
        "rest_api_enabled": true,
        "admin_password": fixture_credential(),
        "public_ip": "192.0.2.11",
    })
    .to_string()
}

fn http_response(
    status: &str,
    headers: &str,
    body: &[u8],
    delay: Duration,
) -> (u16, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("local fixture listener");
    listener.set_nonblocking(true).expect("nonblocking accept");
    let port = listener.local_addr().expect("fixture address").port();
    let length = if headers.contains("Content-Length:") {
        String::new()
    } else {
        format!("Content-Length: {}\r\n", body.len())
    };
    let mut response =
        format!("HTTP/1.1 {status}\r\n{headers}{length}Connection: close\r\n\r\n").into_bytes();
    response.extend_from_slice(body);
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
                Err(error) => panic!("HTTP fixture accept failed: {error}"),
            }
        };
        // Winsock accepted sockets inherit the listener's nonblocking mode.
        // The fixture uses blocking reads with a deadline after bounded accept.
        stream
            .set_nonblocking(false)
            .expect("blocking fixture connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read deadline");
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .expect("write deadline");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).expect("fixture request");
            assert!(read > 0 && request.len() < 8192, "bounded request headers");
            request.extend_from_slice(&buffer[..read]);
        }
        std::thread::sleep(delay);
        if let Err(error) = stream.write_all(&response) {
            assert!(!delay.is_zero(), "HTTP response write failed: {error}");
        }
        String::from_utf8(request).expect("ASCII HTTP headers")
    });
    (port, server)
}
