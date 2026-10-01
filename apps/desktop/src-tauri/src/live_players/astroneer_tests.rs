use tokio::net::TcpListener;

use super::*;
use app_core::{RuntimeLivePlayerStatus, RuntimePlayerIdentityKind};

const NORMAL: &[u8] = include_bytes!("../../test-data/live-players/astroneer/normal.json");
const EMPTY: &[u8] = include_bytes!("../../test-data/live-players/astroneer/empty.json");

#[test]
fn astroneer_filters_history_and_preserves_opaque_game_guids() {
    let result = response::parse("instance", NORMAL, "request", 42).expect("documented fields");
    let snapshot = result.public_snapshot;
    assert_eq!(snapshot.current_players, Some(2));
    assert!(snapshot.complete);
    assert_eq!(snapshot.entries[0].display_name, "Explorer");
    assert_eq!(snapshot.entries[1].display_name, "探索者 } \"Moon\"");
    assert_eq!(
        snapshot.entries[0].identifiers[0].value,
        "18446744073709551001"
    );
    assert!(
        snapshot
            .entries
            .iter()
            .all(
                |entry| entry.identifiers[0].kind == RuntimePlayerIdentityKind::AstroneerGuid
                    && entry.identifiers[0].stable
                    && entry.available_action_ids.is_empty()
            )
    );
    assert!(result.private_action_bindings.is_empty());
}

#[test]
fn astroneer_zero_online_does_not_promote_known_players_or_access_records() {
    for body in [EMPTY, br#"{"playerInfo":[{"playerGuid":"42","playerName":"Known","inGame":false,"playerCategory":"Owner"}]}"#] {
        let result = response::parse("instance", body, "request", 42).expect("zero online");
        assert_eq!(result.public_snapshot.current_players, Some(0));
        assert!(result.public_snapshot.entries.is_empty());
        assert!(result.public_snapshot.complete);
    }
}

#[test]
fn astroneer_rejects_incomplete_ambiguous_and_invalid_identities() {
    for body in [
        "",
        "{}",
        r#"{"playerInfo":[]"#,
        r#"{"playerInfo":[]} {"playerInfo":[]}"#,
        r#"{"playerInfo":[],"playerInfo":[]}"#,
        r#"{"playerInfo":[{"playerGuid":"42","playerName":"Known"}]}"#,
        r#"{"playerInfo":[{"playerGuid":42,"playerName":"Known","inGame":true}]}"#,
        r#"{"playerInfo":[{"playerGuid":"42","playerName":"Known","inGame":"true"}]}"#,
        r#"{"playerInfo":[{"playerGuid":"42","playerName":"","inGame":true}]}"#,
        r#"{"playerInfo":[{"playerGuid":"42","playerName":"Line\nbreak","inGame":true}]}"#,
        r#"{"playerInfo":[{"playerGuid":"","playerName":"Known","inGame":true}]}"#,
        r#"{"playerInfo":[{"playerGuid":"4 2","playerName":"Known","inGame":true}]}"#,
        r#"{"playerInfo":[{"playerGuid":"42","playerName":"One","inGame":true},{"playerGuid":"42","playerName":"Two","inGame":true}]}"#,
    ] {
        let error = response::parse("instance", body.as_bytes(), "request", 42).expect_err(body);
        assert!(!error.complete);
        assert!(error.entries.is_empty());
        assert_eq!(
            error.issue.expect("protocol error").code,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete
        );
    }
}

#[test]
fn astroneer_equal_names_are_distinct_accounts_and_limits_fail_closed() {
    let body = br#"{"playerInfo":[{"playerGuid":"41","playerName":"Same","inGame":true},{"playerGuid":"42","playerName":"Same","inGame":true}]}"#;
    let result = response::parse("instance", body, "request", 42).expect("equal names");
    assert_ne!(
        result.public_snapshot.entries[0].player_key,
        result.public_snapshot.entries[1].player_key
    );
    for body in [vec![b' '; MAX_RESPONSE_BYTES + 1], serde_json::to_vec(&serde_json::json!({"playerInfo":
        (0..257).map(|index| serde_json::json!({"playerGuid":index.to_string(),"playerName":"Fixture","inGame":true})).collect::<Vec<_>>()
    })).expect("bounded fixture")] {
        let error = response::parse("instance", &body, "request", 42).expect_err("limit");
        assert!(error.truncated);
        assert!(error.entries.is_empty());
    }
}

#[tokio::test]
async fn astroneer_missing_password_and_invalid_ports_are_configuration_failures() {
    for password in [
        "",
        "  ",
        "fixture\nDSServerShutdown",
        "fixture\0secret",
        &"x".repeat(129),
    ] {
        let mut details = instance_details(1234);
        details.settings_json = serde_json::json!({"console_password":password}).to_string();
        let error = collect_astroneer(&details, "instance", "request", 42)
            .await
            .expect_err("password validation");
        assert_eq!(error.status, RuntimeLivePlayerStatus::Misconfigured);
        assert_eq!(
            error.issue.expect("setting hint").setting_keys,
            ["console_password"]
        );
    }
    let mut details = instance_details(0);
    assert!(
        collect_astroneer(&details, "instance", "request", 42)
            .await
            .is_err()
    );
    details.ports[0].port = 1234;
    details.ports.push(details.ports[0].clone());
    assert!(
        collect_astroneer(&details, "instance", "request", 42)
            .await
            .is_err()
    );
    details.ports.pop();
    details.ports[0].protocol = "udp".to_owned();
    assert!(
        collect_astroneer(&details, "instance", "request", 42)
            .await
            .is_err()
    );
    assert!(
        collect_astroneer(&details, "another", "request", 42)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn astroneer_native_tcp_accepts_fragmented_json_and_ignores_public_ip() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local fixture");
    let port = listener.local_addr().expect("fixture address").port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("fixture connection");
        let expected = b"fixture-password\nDSListPlayers\n";
        let mut request = vec![0; expected.len()];
        tokio::time::timeout(Duration::from_secs(1), stream.read_exact(&mut request))
            .await
            .expect("request deadline")
            .expect("request");
        assert_eq!(request, expected);
        let mut response = NORMAL.to_vec();
        response.extend_from_slice(b"\r\n");
        for fragment in response.chunks(7) {
            stream.write_all(fragment).await.expect("response fragment");
            tokio::task::yield_now().await;
        }
    });
    let mut details = instance_details(port);
    details.summary.bind_ip = "203.0.113.11".to_owned();
    details.settings_json =
        r#"{"console_password":"fixture-password","public_ip":"203.0.113.10"}"#.to_owned();
    let result = collect_astroneer(&details, "instance", "request", 42)
        .await
        .expect("local response");
    server.await.expect("fixture completion");
    assert_eq!(result.public_snapshot.current_players, Some(2));
}

#[tokio::test]
async fn astroneer_transport_rejects_early_eof_trailing_data_and_oversize() {
    for (body, expected) in [
        (Vec::new(), FetchError::ClosedBeforeResponse),
        (b"{\"playerInfo\":[]}".to_vec(), FetchError::Incomplete),
        (
            b"{\"playerInfo\":[]}{\"playerInfo\":[]}\r\n".to_vec(),
            FetchError::Incomplete,
        ),
        (vec![b'x'; MAX_RESPONSE_BYTES + 1], FetchError::TooLarge),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local fixture");
        let port = listener.local_addr().expect("fixture address").port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("fixture connection");
            let mut request = [0_u8; 31];
            tokio::time::timeout(Duration::from_secs(1), stream.read_exact(&mut request))
                .await
                .expect("request deadline")
                .expect("request");
            let _ = stream.write_all(&body).await;
        });
        assert_eq!(
            fetch_players(port, "fixture-password", Duration::from_secs(1))
                .await
                .expect_err("invalid response"),
            expected
        );
        server.await.expect("fixture completion");
    }
}

#[tokio::test]
async fn astroneer_total_deadline_closes_a_silent_console_connection() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local fixture");
    let port = listener.local_addr().expect("fixture address").port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("fixture connection");
        let mut request = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut request))
            .await
            .expect("cancel releases socket")
            .expect("peer closure");
        assert_eq!(request, b"fixture-password\nDSListPlayers\n");
    });
    assert_eq!(
        fetch_players(port, "fixture-password", Duration::from_millis(50))
            .await
            .expect_err("silent peer"),
        FetchError::Timeout
    );
    server.await.expect("fixture completion");
}

fn instance_details(port: u16) -> InstanceDetails {
    serde_json::from_value(serde_json::json!({
        "summary": {"id":"instance","name":"ASTRONEER fixture","module_id":"astroneer",
            "status":"Running","bind_ip":"0.0.0.0","port_count":1,"autostart":false},
        "config_file_path":"","saves_path":"","auto_backup_on_stop":false,
        "backup_retention_count":1,"settings_json":"{\"console_password\":\"fixture-password\"}",
        "ports":[{"name":"console","protocol":"tcp","port":port}],"active_run":null,
    }))
    .expect("synthetic local instance")
}
