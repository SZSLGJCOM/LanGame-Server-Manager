use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::*;

const FIXTURE_TIMEOUT: Duration = Duration::from_secs(2);
const NORMAL: &[u8] = include_bytes!("../../test-data/live-players/satisfactory/normal.json");

// Exercise the production request and response handling over a local HTTP peer.
// Only this test seam changes the scheme; it does not claim TLS handshake coverage.
fn local_request(port: u16) -> (reqwest::Client, reqwest::Request) {
    let (client, mut request) =
        player_request(PlayerEndpoint::GameApi(port)).expect("production request");
    assert_eq!(request.url().scheme(), "https");
    assert_eq!(request.url().host_str(), Some("127.0.0.1"));
    assert_eq!(request.url().port(), Some(port));
    assert_eq!(request.url().path(), "/api/v1");
    assert_eq!(request.url().query(), None);
    assert_eq!(request.timeout(), Some(&REQUEST_TIMEOUT));
    request
        .url_mut()
        .set_scheme("http")
        .expect("fixture scheme");
    (client, request)
}

async fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    tokio::time::timeout(FIXTURE_TIMEOUT, async {
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let count = stream.read(&mut chunk).await.expect("request bytes");
            assert_ne!(count, 0, "request must finish before peer closes");
            request.extend_from_slice(&chunk[..count]);
            assert!(request.len() <= 8192, "fixture request limit");
            if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&request[..header_end]).expect("HTTP headers");
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().expect("content length"))
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + length {
                    assert_eq!(request.len(), header_end + 4 + length);
                    return request;
                }
            }
        }
    })
    .await
    .expect("request deadline")
}

async fn spawn_response(response: Vec<u8>) -> (u16, JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local fixture");
    let port = listener.local_addr().expect("fixture address").port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = tokio::time::timeout(FIXTURE_TIMEOUT, listener.accept())
            .await
            .expect("connection deadline")
            .expect("fixture connection");
        let request = read_request(&mut stream).await;
        // Small writes cover header/body fragmentation independently of TCP packet boundaries.
        tokio::time::timeout(FIXTURE_TIMEOUT, async {
            for fragment in response.chunks(4093) {
                if stream.write_all(fragment).await.is_err() {
                    break; // An over-limit response may be rejected before its last fragment.
                }
            }
        })
        .await
        .expect("response write deadline");
        request
    });
    (port, server)
}

fn response(status: u16, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ).into_bytes();
    response.extend_from_slice(body);
    response
}

#[tokio::test]
async fn frm_sends_the_official_read_only_post_and_parses_its_response() {
    let (port, server) = spawn_response(response(200, NORMAL)).await;
    let (client, request) = local_request(port);
    let body = execute_players(client, request)
        .await
        .expect("FRM response");
    let snapshot = parse_players("instance", "request", 42, &body)
        .expect("complete roster")
        .public_snapshot;
    assert_eq!(snapshot.current_players, Some(2));
    let wire = server.await.expect("fixture completion");
    let header_end = wire
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .expect("headers");
    let headers = std::str::from_utf8(&wire[..header_end])
        .expect("headers")
        .to_ascii_lowercase();
    assert!(headers.starts_with("post /api/v1 http/1.1\r\n"));
    assert!(
        headers
            .lines()
            .any(|line| line == "accept: application/json")
    );
    assert!(
        headers
            .lines()
            .any(|line| line == "content-type: application/json")
    );
    assert!(!headers.contains("authorization:"));
    assert!(!headers.contains("cookie:"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&wire[header_end + 4..]).expect("request JSON"),
        serde_json::json!({"function":"frm", "endpoint":"getPlayer"})
    );
}

#[tokio::test]
async fn frm_native_http_transport_uses_the_same_bounded_roster_contract() {
    let (port, server) = spawn_response(response(200, NORMAL)).await;
    let (client, request) = player_request(PlayerEndpoint::WebApi(port)).expect("HTTP request");
    assert_eq!(request.url().host_str(), Some("127.0.0.1"));
    assert_eq!(request.url().scheme(), "http");
    assert_eq!(request.url().query(), None);
    assert_eq!(request.timeout(), Some(&REQUEST_TIMEOUT));
    let body = execute_players(client, request).await.expect("FRM roster");
    let snapshot = parse_players("instance", "request", 42, &body).expect("complete roster");
    assert_eq!(snapshot.public_snapshot.current_players, Some(2));
    let wire = server.await.expect("HTTP peer completion");
    let wire = std::str::from_utf8(&wire)
        .expect("HTTP request")
        .to_ascii_lowercase();
    assert!(wire.starts_with("get /api/getplayer http/1.1\r\n"));
    assert!(!wire.contains("authorization:"));
    assert!(!wire.contains("cookie:"));
    assert!(wire.ends_with("\r\n\r\n"));
}

#[tokio::test]
async fn frm_http_failures_keep_their_actionable_issue_codes() {
    for (status, expected) in [
        (400, RuntimeLivePlayerIssueCode::ExtensionUnavailable),
        (401, RuntimeLivePlayerIssueCode::AuthenticationFailed),
        (403, RuntimeLivePlayerIssueCode::AuthenticationFailed),
        (404, RuntimeLivePlayerIssueCode::ExtensionUnavailable),
        (422, RuntimeLivePlayerIssueCode::ExtensionUnavailable),
        (503, RuntimeLivePlayerIssueCode::ProcessUnavailable),
        (500, RuntimeLivePlayerIssueCode::ProtocolIncomplete),
    ] {
        let (port, server) = spawn_response(response(status, b"{}")).await;
        let (client, request) = local_request(port);
        let error = execute_players(client, request)
            .await
            .expect_err("HTTP failure");
        let snapshot = fetch_failure("instance", "request", error);
        assert_eq!(
            snapshot.issue.expect("issue").code,
            expected,
            "status {status}"
        );
        assert!(!snapshot.complete);
        assert!(snapshot.entries.is_empty());
        assert_eq!(snapshot.current_players, None);
        server.await.expect("fixture completion");
    }
}

#[tokio::test]
async fn frm_refuses_redirects_without_contacting_the_destination() {
    let destination = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("redirect fixture");
    let destination_port = destination.local_addr().expect("redirect address").port();
    let redirect = format!(
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{destination_port}/other\r\nContent-Length: 0\r\n\r\n"
    );
    let (port, server) = spawn_response(redirect.into_bytes()).await;
    let (client, request) = local_request(port);
    assert_eq!(
        execute_players(client, request)
            .await
            .expect_err("no redirects"),
        HttpApiError::HttpStatus(302)
    );
    server.await.expect("fixture completion");
    assert!(
        tokio::time::timeout(Duration::from_millis(20), destination.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn frm_bounds_chunked_responses_without_a_content_length() {
    for size in [MAX_HTTP_RESPONSE_BYTES, MAX_HTTP_RESPONSE_BYTES + 1] {
        let mut response =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        // Several HTTP chunks ensure the cumulative limit is tested.
        for chunk in vec![b' '; size].chunks(16384) {
            response.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
            response.extend_from_slice(chunk);
            response.extend_from_slice(b"\r\n");
        }
        response.extend_from_slice(b"0\r\n\r\n");
        let (port, server) = spawn_response(response).await;
        let (client, request) = local_request(port);
        let result = execute_players(client, request).await;
        if size == MAX_HTTP_RESPONSE_BYTES {
            assert_eq!(result.expect("inclusive limit").len(), size);
        } else {
            assert_eq!(
                result.expect_err("cumulative limit"),
                HttpApiError::TooLarge
            );
        }
        server.await.expect("fixture completion");
    }
}

#[tokio::test]
async fn frm_rejects_advertised_oversize_and_truncated_bodies() {
    for (response, expected) in [
        (
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                MAX_HTTP_RESPONSE_BYTES + 1
            )
            .into_bytes(),
            HttpApiError::TooLarge,
        ),
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n[".to_vec(),
            HttpApiError::Transport,
        ),
    ] {
        let (port, server) = spawn_response(response).await;
        let (client, request) = local_request(port);
        assert_eq!(
            execute_players(client, request)
                .await
                .expect_err("incomplete body"),
            expected
        );
        server.await.expect("fixture completion");
    }
}

async fn assert_peer_closed(stream: &mut TcpStream) {
    let mut byte = [0; 1];
    let result = tokio::time::timeout(FIXTURE_TIMEOUT, stream.read(&mut byte))
        .await
        .expect("cancel must close the socket before the normal request timeout");
    match result {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ) => {}
        result => panic!("expected peer closure, got {result:?}"),
    }
}

#[tokio::test]
async fn frm_deadline_covers_silent_headers_and_an_unfinished_body() {
    for send_headers in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("silent fixture");
        let port = listener.local_addr().expect("fixture address").port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = tokio::time::timeout(FIXTURE_TIMEOUT, listener.accept())
                .await
                .expect("connection deadline")
                .expect("fixture connection");
            read_request(&mut stream).await;
            if send_headers {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n[")
                    .await
                    .expect("unfinished response");
            }
            assert_peer_closed(&mut stream).await;
        });
        let (client, mut request) = local_request(port);
        *request.timeout_mut() = Some(Duration::from_millis(100));
        let error = execute_players(client, request)
            .await
            .expect_err("total deadline");
        assert_eq!(error, HttpApiError::Timeout);
        assert_eq!(
            fetch_failure("instance", "request", error)
                .issue
                .expect("issue")
                .code,
            RuntimeLivePlayerIssueCode::CollectionTimeout
        );
        server.await.expect("fixture completion");
    }
}

#[tokio::test]
async fn frm_cancelling_collection_releases_the_connection() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("cancel fixture");
    let port = listener.local_addr().expect("fixture address").port();
    let (started, received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = tokio::time::timeout(FIXTURE_TIMEOUT, listener.accept())
            .await
            .expect("connection deadline")
            .expect("fixture connection");
        read_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n[")
            .await
            .expect("unfinished response");
        started.send(()).expect("request observer");
        assert_peer_closed(&mut stream).await;
    });
    let (client, request) = local_request(port);
    let collection = tokio::spawn(execute_players(client, request));
    tokio::time::timeout(FIXTURE_TIMEOUT, received)
        .await
        .expect("request started")
        .expect("request observer");
    collection.abort();
    assert!(
        collection
            .await
            .expect_err("cancelled collection")
            .is_cancelled()
    );
    server.await.expect("fixture completion");
}
