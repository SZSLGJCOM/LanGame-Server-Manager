use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Instant;

use super::*;

pub(crate) fn capture_response(
    status: &str,
    headers: &str,
    body: &[u8],
    delay: Duration,
) -> (u16, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let length = if headers.contains("Content-Length:") {
        String::new()
    } else {
        format!("Content-Length: {}\r\n", body.len())
    };
    let response = [
        format!("HTTP/1.1 {status}\r\n{headers}{length}Connection: close\r\n\r\n").as_bytes(),
        body,
    ]
    .concat();
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error) => panic!("HTTP fixture accept: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0 && request.len() + count <= 16 * 1024);
            request.extend_from_slice(&chunk[..count]);
            if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&request[..end]);
                let length = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        std::thread::sleep(delay);
        if let Err(error) = stream.write_all(&response) {
            assert!(!delay.is_zero(), "fixture write: {error}");
        }
        String::from_utf8(request).unwrap()
    });
    (port, handle)
}

#[tokio::test]
async fn palworld_rest_requests_use_exact_official_endpoints_and_json_bodies() {
    use base64::Engine;
    for (command, endpoint, expected_body) in [
        (
            PalworldCommand::Announce {
                message: String::from("你好 \"builders\" \\ world"),
            },
            "announce",
            Some(json!({"message":"你好 \"builders\" \\ world"})),
        ),
        (
            PalworldCommand::Kick {
                userid: String::from("steam_76561190000000001"),
            },
            "kick",
            Some(json!({"userid":"steam_76561190000000001"})),
        ),
        (
            PalworldCommand::Ban {
                userid: String::from("steam_76561190000000001"),
            },
            "ban",
            Some(json!({"userid":"steam_76561190000000001"})),
        ),
        (
            PalworldCommand::Unban {
                userid: String::from("steam_76561190000000001"),
            },
            "unban",
            Some(json!({"userid":"steam_76561190000000001"})),
        ),
        (PalworldCommand::Save, "save", None),
        (
            PalworldCommand::Shutdown,
            "shutdown",
            Some(json!({"waittime":10,"message":"LanGame server shutdown"})),
        ),
    ] {
        let (port, fixture) = capture_response("200 OK", "", b"", Duration::ZERO);
        let details = super::super::palworld::tests::instance_details(port);
        let settings: Value = serde_json::from_str(&details.settings_json).unwrap();
        let result = execute(&details, &serde_json::to_string(&command).unwrap())
            .await
            .unwrap();
        assert!(result.contains("HTTP 200"));
        let request = fixture.join().unwrap();
        assert!(request.starts_with(&format!("POST /v1/api/{endpoint} HTTP/1.1\r\n")));
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        let authorization = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.eq_ignore_ascii_case("authorization"))
                    .map(|(_, value)| value.trim())
            })
            .unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(authorization.strip_prefix("Basic ").unwrap())
            .unwrap();
        assert_eq!(
            decoded,
            format!("admin:{}", settings["admin_password"].as_str().unwrap()).as_bytes()
        );
        if let Some(expected) = expected_body {
            assert_eq!(serde_json::from_str::<Value>(body).unwrap(), expected);
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("content-type: application/json")
            );
        } else {
            assert!(body.is_empty());
            assert!(headers.lines().any(|line| {
                line.split_once(':').is_some_and(|(key, value)| {
                    key.eq_ignore_ascii_case("content-length") && value.trim() == "0"
                })
            }));
        }
    }
}

#[tokio::test]
async fn palworld_rest_mutations_reject_non_200_and_bounded_response_failures() {
    for (status, headers, expected) in [
        ("401 Unauthorized", "", "credentials"),
        ("400 Bad Request", "", "HTTP 400"),
        ("500 Internal Server Error", "", "HTTP 500"),
        ("202 Accepted", "", "HTTP 202"),
        ("302 Found", "Location: http://127.0.0.1:1/\r\n", "HTTP 302"),
        ("200 OK", "Content-Length: 262145\r\n", "size limit"),
    ] {
        let (port, fixture) = capture_response(status, headers, b"", Duration::ZERO);
        let error = execute(
            &super::super::palworld::tests::instance_details(port),
            r#"{"operation":"save"}"#,
        )
        .await
        .unwrap_err();
        assert!(error.contains(expected), "{error}");
        fixture.join().unwrap();
    }
    let (port, fixture) = capture_response("200 OK", "", b"", Duration::from_millis(100));
    let result = request_json(
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        "/v1/api/save",
        Some("fixture"),
        Duration::from_millis(20),
        reqwest::Method::POST,
        None,
    )
    .await;
    assert_eq!(result, Err(HttpApiError::Timeout));
    fixture.join().unwrap();
}

#[tokio::test]
async fn palworld_rest_rejects_foreign_instances_and_unrecognized_operations() {
    let mut details = super::super::palworld::tests::instance_details(1);
    for command in [
        r#"{"operation":"kick","userid":""}"#,
        r#"{"operation":"save","url":"http://elsewhere"}"#,
        r#"{"operation":"shutdown","waittime":0}"#,
        r#"{"operation":"wipe"}"#,
    ] {
        assert!(execute(&details, command).await.is_err());
    }
    details.summary.module_id = String::from("minecraft");
    assert!(
        execute(&details, r#"{"operation":"save"}"#)
            .await
            .unwrap_err()
            .contains("context")
    );
}

#[tokio::test]
async fn palworld_rest_show_players_returns_only_the_public_player_projection() {
    let body = include_bytes!("../../test-data/live-players/palworld/normal.json");
    let (port, fixture) = capture_response("200 OK", "", body, Duration::ZERO);
    let result = execute(
        &super::super::palworld::tests::instance_details(port),
        r#"{"operation":"players"}"#,
    )
    .await
    .unwrap();
    let players: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(players.as_array().unwrap().len(), 2);
    assert!(!result.contains("192.0.2."));
    assert!(!result.contains("location_x"));
    assert!(
        fixture
            .join()
            .unwrap()
            .starts_with("GET /v1/api/players HTTP/1.1\r\n")
    );
    let (port, fixture) = capture_response("200 OK", "", b"{}", Duration::ZERO);
    assert!(
        execute(
            &super::super::palworld::tests::instance_details(port),
            r#"{"operation":"players"}"#
        )
        .await
        .unwrap_err()
        .contains("incomplete")
    );
    fixture.join().unwrap();
}

#[test]
fn palworld_rest_rendering_serializes_targets_without_interpreting_json_or_commands() {
    let action: ModulePlayerActionSpec = toml::from_str("id='broadcast'\nlabel='Broadcast'\ntransport='palworld_rest'\ncommand_template='announce {{target}}'\ntarget_required=true").unwrap();
    let target = r#"Welcome \"builders\"; {"operation":"shutdown"} \\ world"#;
    let rendered = render_command(&action, Some(target), None, None, false).unwrap();
    assert_eq!(
        serde_json::from_str::<PalworldCommand>(&rendered).unwrap(),
        PalworldCommand::Announce {
            message: target.to_owned()
        }
    );
    for target in ["", "line\nbreak", "nul\0byte"] {
        assert!(render_command(&action, Some(target), None, None, false).is_err());
    }
    assert!(render_command(&action, Some("message"), Some("admin"), None, false).is_err());
}
