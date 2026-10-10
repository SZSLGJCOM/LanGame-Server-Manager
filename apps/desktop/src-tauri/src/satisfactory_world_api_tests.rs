use super::*;

#[test]
fn world_io_uses_remaining_operation_budget_while_reads_remain_short() {
    for function in ["CreateNewGame", "LoadGame", "SaveGame"] {
        assert_eq!(
            request_budget(function, Duration::from_secs(35)),
            Duration::from_secs(35)
        );
        assert_eq!(
            request_budget(function, Duration::from_secs(3)),
            Duration::from_secs(3)
        );
    }
    assert_eq!(
        request_budget("GetServerOptions", Duration::from_secs(35)),
        Duration::from_secs(6)
    );
    assert_eq!(
        request_budget("GetServerOptions", Duration::from_secs(3)),
        Duration::from_secs(3)
    );
}

#[test]
fn native_error_envelopes_are_errors_even_when_http_status_is_success() {
    let bytes = br#"{"errorCode":"missing_params","errorMessage":"synthetic-password-and-private-path","errorData":{"secret":"private"}}"#;
    let error = parse_reply(200, bytes).err().unwrap();
    assert!(matches!(error, ApiError::Rejected(_)));
    assert!(!error.to_string().contains("synthetic-password"));
    assert!(!error.to_string().contains("private-path"));
    assert!(matches!(
        parse_reply(403, b"private"),
        Err(ApiError::Authentication)
    ));
    assert_eq!(parse_reply(202, b"").unwrap().status, 202);
    assert_eq!(parse_reply(204, b"").unwrap().status, 204);
}

#[test]
fn malformed_success_and_invalid_machine_codes_are_rejected() {
    assert!(parse_reply(200, b"not-json").is_err());
    assert!(parse_reply(200, br#"{"data":[]}"#).is_err());
    assert!(
        parse_reply(
            200,
            br#"{"errorCode":"secret value","errorMessage":"private"}"#
        )
        .is_err()
    );
    for bytes in [b"null".as_slice(), b"[]", b"true", b"42"] {
        assert!(parse_reply(200, bytes).is_err());
        assert!(parse_reply(202, bytes).is_err());
    }
}

#[test]
fn native_udp_name_query_uses_cookie_length_and_terminator() {
    let packet = [
        0xd5, 0xf6, 1, 1, 8, 0x56, 0x52, 0x0a, 0x8f, 0x3e, 0x4d, 0xca, 1, 0x4e, 0xa9, 7, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 4, 0, 2, 0, 1, 1, 0, 3, 1, 0, 2, 1, 0, 14, 0, b'L', b'G', b'S', b'M',
        b' ', b'A', b'P', b'I', b' ', b'P', b'r', b'o', b'b', b'e', 1,
    ];
    assert_eq!(
        parse_server_name(&packet, &packet[4..12]).unwrap(),
        "LGSM API Probe"
    );
    assert!(parse_server_name(&packet, &[0; 8]).is_err());
    assert!(parse_server_name(&packet[..packet.len() - 1], &packet[4..12]).is_err());
    let mut malformed = packet;
    malformed[38] = 80;
    assert!(parse_server_name(&malformed, &packet[4..12]).is_err());
}

#[tokio::test]
async fn transport_does_not_follow_redirects_or_expose_secret_error_bodies() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        let size = socket.read(&mut request).await.unwrap();
        assert!(
            std::str::from_utf8(&request[..size])
                .unwrap()
                .starts_with("POST /api/v1")
        );
        socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://other.invalid/credentials\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let request = client
        .post(format!("http://{address}/api/v1"))
        .json(&json!({"function":"QueryServerState","data":{}}))
        .build()
        .unwrap();
    assert!(matches!(
        execute(&client, request, false).await,
        Err(ApiError::Unavailable)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn a_disconnected_write_has_unknown_outcome_and_is_not_retried() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        drop(socket);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let request = client
        .post(format!("http://{address}/api/v1"))
        .json(&json!({"function":"CreateNewGame","data":{}}))
        .build()
        .unwrap();
    assert!(matches!(
        execute(&client, request, true).await,
        Err(ApiError::OutcomeUnknown)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn oversized_sent_writes_have_unknown_outcome_for_headers_and_streamed_bodies() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for declared_length in [true, false] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            if declared_length {
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    MAX_RESPONSE_BYTES + 1
                );
                socket.write_all(header.as_bytes()).await.unwrap();
            } else {
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
                socket
                    .write_all(&vec![b'x'; MAX_RESPONSE_BYTES + 1])
                    .await
                    .unwrap();
            }
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let request = client
            .post(format!("http://{address}/api/v1"))
            .json(&json!({"function":"ApplyAdvancedGameSettings","data":{}}))
            .build()
            .unwrap();
        assert!(matches!(
            execute(&client, request, true).await,
            Err(ApiError::OutcomeUnknown)
        ));
        server.await.unwrap();
    }
}
