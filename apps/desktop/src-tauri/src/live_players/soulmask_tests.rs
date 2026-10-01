use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;

use super::*;
use app_core::{RuntimeLivePlayerStatus, RuntimePlayerIdentityKind};

const NORMAL: &str = include_str!("../../test-data/live-players/soulmask/normal.txt");
const EMPTY: &str = include_str!("../../test-data/live-players/soulmask/empty.txt");

fn wire(text: &str) -> Vec<u8> {
    text.replace("\r\n", "\n")
        .replace('\n', "\r\n")
        .into_bytes()
}

fn page(body: &[u8], current: usize, total: usize) -> Vec<u8> {
    let mut result = body.to_vec();
    result.extend_from_slice(
        format!(
            "\r\n=========QUERY INTERACTIVE MODE========\r\n\
|  PAGE:   {current} of {total}                     |\r\n\
|  Enter any number to goto that page.|\r\n\
|  Enter n to show next page.         |\r\n\
|  Enter q to exit interactive mode.  |\r\n\
=======================================\r\n"
        )
        .as_bytes(),
    );
    result
}

#[test]
fn soulmask_real_empty_header_and_constructed_online_accounts_are_read_only() {
    let empty =
        response::parse("instance", &wire(EMPTY), "request", 42).expect("native empty header");
    assert_eq!(empty.public_snapshot.current_players, Some(0));
    assert!(empty.public_snapshot.complete);
    let result =
        response::parse("instance", &wire(NORMAL), "request", 42).expect("constructed rows");
    let snapshot = result.public_snapshot;
    assert_eq!(snapshot.status, RuntimeLivePlayerStatus::Ready);
    assert_eq!(snapshot.current_players, Some(2));
    assert_eq!(snapshot.entries[1].display_name, "探索者 | O'Brien");
    assert_eq!(
        snapshot.entries[0].identifiers[0].value,
        "76561198000000001"
    );
    assert!(
        snapshot
            .entries
            .iter()
            .all(
                |entry| entry.identifiers[0].kind == RuntimePlayerIdentityKind::SteamId
                    && entry.identifiers[0].stable
                    && entry.available_action_ids.is_empty()
            )
    );
    assert!(result.private_action_bindings.is_empty());
}

#[test]
fn soulmask_rejects_unknown_empty_partial_rows_duplicate_accounts_and_bad_utf8() {
    let normal = wire(NORMAL);
    for body in [
        Vec::new(),
        b"\r\n".to_vec(),
        b"No players\r\n".to_vec(),
        normal[..normal.len() - 2].to_vec(),
        wire(&NORMAL.replace("76561198000000002", "76561198000000001")),
        wire(&NORMAL.replace("76561198000000002", "account-name")),
        wire(&NORMAL.replace("'Explorer'", "Explorer")),
        wire(&NORMAL.replace("'Explorer'", "''")),
        wire(&NORMAL.replace("FixturePawnA", "")),
        wire(&NORMAL.replace("'Explorer'", "'Line\nbreak'")),
        [
            wire(EMPTY),
            b"| 76561198000000001 | 'Broken' |\r\n".to_vec(),
        ]
        .concat(),
        [wire(EMPTY), vec![0xff, b'\r', b'\n']].concat(),
    ] {
        let error = response::parse("instance", &body, "request", 42).expect_err("invalid table");
        assert!(!error.complete);
        assert!(error.entries.is_empty());
        assert_eq!(
            error.issue.expect("issue").code,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete
        );
    }
}

#[test]
fn soulmask_duplicate_names_are_distinct_accounts_and_limits_fail_closed() {
    let result = response::parse(
        "instance",
        &wire(&NORMAL.replace("探索者 | O'Brien", "Explorer")),
        "request",
        42,
    )
    .expect("same name, separate accounts");
    assert_ne!(
        result.public_snapshot.entries[0].player_key,
        result.public_snapshot.entries[1].player_key
    );
    let mut many = wire(EMPTY);
    for index in 0..1025 {
        many.extend_from_slice(
            format!(
                "| {} | 'Fixture' | Pawn | V(X=0) |\r\n",
                76_561_198_000_000_000_u64 + index
            )
            .as_bytes(),
        );
    }
    for body in [many, vec![b'x'; MAX_RESPONSE_BYTES + 1]] {
        let error =
            response::parse("instance", &body, "request", 42).expect_err("bounded response");
        assert!(error.truncated);
        assert!(error.entries.is_empty());
        assert_eq!(
            error.issue.expect("limit").code,
            RuntimeLivePlayerIssueCode::CaptureLimit
        );
    }
}

#[test]
fn soulmask_page_footer_requires_complete_known_control_lines_and_bounded_counts() {
    let body = wire(EMPTY);
    let paged = page(&body, 1, 3);
    let response::NativePage {
        payload,
        pagination: metadata,
    } = response::split_page(&paged).expect("native footer");
    assert_eq!(metadata, Some((1, 3)));
    assert_eq!(payload, [body.clone(), b"\r\n".to_vec()].concat());
    for bad in [
        page(&body, 0, 3),
        page(&body, 3, 2),
        paged[..paged.len() - 1].to_vec(),
        String::from_utf8(paged.clone())
            .expect("fixture")
            .replace("Enter n", "Enter x")
            .into_bytes(),
    ] {
        assert_eq!(response::split_page(&bad), Err(FetchError::Incomplete));
    }
    assert_eq!(
        response::split_page(&page(&body, 1, MAX_PAGES + 1)),
        Err(FetchError::TooLarge)
    );
}

async fn read_request(reader: &mut BufReader<TcpStream>, command: &str) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("command");
    assert_eq!(line, format!("{command}\r\n"));
    line.clear();
    reader.read_line(&mut line).await.expect("nonce");
    let nonce = line.strip_suffix("\r\n").expect("CRLF");
    let value = nonce
        .strip_prefix("LGM_PLAYER_QUERY_END_")
        .expect("native unknown command");
    assert_eq!(value.len(), 32);
    assert!(value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    nonce.to_owned()
}

#[tokio::test]
async fn soulmask_fragmented_pages_require_matching_nonces_in_order_and_native_eof() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local fixture");
    let endpoint = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("connection");
        let mut reader = BufReader::new(stream);
        let rows = NORMAL.lines().collect::<Vec<_>>();
        let bodies = [
            page(&wire(&format!("{}\n{}\n", rows[0], rows[1])), 1, 2),
            page(&wire(&format!("{}\n", rows[2])), 2, 2),
        ];
        let mut previous = None;
        for (index, body) in bodies.into_iter().enumerate() {
            let nonce = read_request(&mut reader, if index == 0 { "lp" } else { "n" }).await;
            assert_ne!(previous.as_ref(), Some(&nonce));
            let reply = [body, format!("{nonce} Not Found!\r\n").into_bytes()].concat();
            for fragment in reply.chunks(7) {
                reader
                    .get_mut()
                    .write_all(fragment)
                    .await
                    .expect("fragment");
                tokio::task::yield_now().await;
            }
            previous = Some(nonce);
        }
        let mut disconnect = String::new();
        reader.read_line(&mut disconnect).await.expect("disconnect");
        assert_eq!(disconnect, "dc\r\n");
    });
    let body = fetch_players(endpoint, Duration::from_secs(2))
        .await
        .expect("complete pages");
    server.await.expect("fixture completion");
    let result = response::parse("instance", &body, "request", 42).expect("complete table");
    assert_eq!(result.public_snapshot.current_players, Some(2));
}

#[tokio::test]
async fn soulmask_transport_rejects_early_eof_wrong_nonce_oversize_and_extra_tail() {
    for case in [
        "empty",
        "no_nonce",
        "wrong_nonce",
        "partial_marker",
        "extra_tail",
        "too_large",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local fixture");
        let endpoint = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("connection");
            let mut reader = BufReader::new(stream);
            let nonce = read_request(&mut reader, "lp").await;
            let body = match case {
                "empty" => Vec::new(),
                "no_nonce" => wire(EMPTY),
                "wrong_nonce" => [
                    wire(EMPTY),
                    b"LGM_PLAYER_QUERY_END_other Not Found!\r\n".to_vec(),
                ]
                .concat(),
                "partial_marker" => {
                    [wire(EMPTY), format!("{nonce} Not Found!").into_bytes()].concat()
                }
                "extra_tail" => [
                    wire(EMPTY),
                    format!("{nonce} Not Found!\r\nextra\r\n").into_bytes(),
                ]
                .concat(),
                "too_large" => vec![b'x'; MAX_RESPONSE_BYTES + 1],
                _ => unreachable!("fixture case"),
            };
            let _ = reader.get_mut().write_all(&body).await;
        });
        assert_eq!(
            fetch_players(endpoint, Duration::from_secs(2))
                .await
                .expect_err(case),
            if case == "too_large" {
                FetchError::TooLarge
            } else {
                FetchError::Incomplete
            }
        );
        server.await.expect("fixture completion");
    }
}

#[tokio::test]
async fn soulmask_changed_or_skipped_page_never_publishes_a_partial_roster() {
    for second_page in [(1, 2), (2, 3)] {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local fixture");
        let endpoint = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("connection");
            let mut reader = BufReader::new(stream);
            for (command, current, total) in [("lp", 1, 2), ("n", second_page.0, second_page.1)] {
                let nonce = read_request(&mut reader, command).await;
                let body = [
                    page(&wire(EMPTY), current, total),
                    format!("{nonce} Not Found!\r\n").into_bytes(),
                ]
                .concat();
                reader.get_mut().write_all(&body).await.expect("page");
            }
        });
        assert_eq!(
            fetch_players(endpoint, Duration::from_secs(2))
                .await
                .expect_err("page sequence"),
            FetchError::Incomplete
        );
        server.await.expect("fixture completion");
    }
}

#[tokio::test]
async fn soulmask_total_deadline_closes_silent_peers_and_requires_final_disconnect() {
    for reply in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local fixture");
        let endpoint = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("connection");
            let mut reader = BufReader::new(stream);
            let nonce = read_request(&mut reader, "lp").await;
            if reply {
                reader
                    .get_mut()
                    .write_all(
                        &[wire(EMPTY), format!("{nonce} Not Found!\r\n").into_bytes()].concat(),
                    )
                    .await
                    .expect("complete table");
            }
            let mut rest = Vec::new();
            tokio::time::timeout(Duration::from_secs(1), reader.read_to_end(&mut rest))
                .await
                .expect("cancellation closes socket")
                .expect("EOF");
            assert_eq!(
                rest,
                if reply {
                    b"dc\r\n".to_vec()
                } else {
                    Vec::new()
                }
            );
        });
        assert_eq!(
            fetch_players(endpoint, Duration::from_millis(100))
                .await
                .expect_err("deadline"),
            FetchError::Timeout
        );
        server.await.expect("fixture completion");
    }
}

#[tokio::test]
async fn soulmask_requires_matching_instance_and_one_valid_echo_tcp_port() {
    for change in [
        "module",
        "instance",
        "missing",
        "duplicate",
        "udp",
        "zero",
        "hostname",
    ] {
        let mut details = instance_details(1234);
        match change {
            "module" => details.summary.module_id = "other".to_owned(),
            "instance" => details.summary.id = "other".to_owned(),
            "missing" => details.ports.clear(),
            "duplicate" => details.ports.push(details.ports[0].clone()),
            "udp" => details.ports[0].protocol = "udp".to_owned(),
            "zero" => details.ports[0].port = 0,
            "hostname" => details.summary.bind_ip = "example.com".to_owned(),
            _ => unreachable!("fixture"),
        }
        let error = collect_soulmask(&details, "instance", "request", 42)
            .await
            .expect_err(change);
        assert_eq!(error.status, RuntimeLivePlayerStatus::Misconfigured);
    }
}

fn instance_details(port: u16) -> InstanceDetails {
    serde_json::from_value(serde_json::json!({
        "summary":{"id":"instance","name":"Soulmask fixture","module_id":"soulmask",
            "status":"Running","bind_ip":"0.0.0.0","port_count":1,"autostart":false},
        "config_file_path":"","saves_path":"","auto_backup_on_stop":false,
        "backup_retention_count":1,"settings_json":"{}",
        "ports":[{"name":"echo","protocol":"tcp","port":port}],"active_run":null,
    }))
    .expect("synthetic local instance")
}
