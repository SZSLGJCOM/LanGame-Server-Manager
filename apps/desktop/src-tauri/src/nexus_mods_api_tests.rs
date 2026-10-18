use super::*;
use std::net::TcpListener;
use std::thread;

#[test]
fn nexus_api_key_is_sensitive_and_attached_only_to_fixed_origin_requests() {
    let api = NexusApi::new("fixture-api-secret").unwrap();
    let request = api
        .request(reqwest::Url::parse("https://api.nexusmods.com/v1/users/validate").unwrap())
        .unwrap();
    assert!(request.headers()["apikey"].is_sensitive());
    assert_eq!(
        request.headers()["Application-Name"],
        "LanGameServerManager"
    );
    let unrelated = api
        .client
        .get("https://files.nexus-cdn.com/mod.zip")
        .build()
        .unwrap();
    assert!(!unrelated.headers().contains_key("apikey"));
    for url in [
        "https://evil.example/v1/users/validate",
        "http://api.nexusmods.com/v1/users/validate",
        "https://api.nexusmods.com/v3/users/validate",
    ] {
        assert!(api.request(reqwest::Url::parse(url).unwrap()).is_err());
    }
    let debug = format!("{request:?}");
    assert!(!debug.contains("fixture-api-secret"));
}

#[test]
fn nexus_v1_download_authorization_encodes_key_and_expires_but_not_user_id() {
    let reference = parse_reference(
        "nxm://abioticfactor/mods/12/files/88?key=fixture%2Bsecret%26value&expires=200&user_id=42",
        "abioticfactor",
    )
    .unwrap();
    let path = download_path(&reference, "88").unwrap();
    let url = reqwest::Url::parse(&format!("{API_BASE}{path}")).unwrap();
    let query = url.query_pairs().collect::<HashMap<_, _>>();
    assert_eq!(query.get("key").unwrap(), "fixture+secret&value");
    assert_eq!(query.get("expires").unwrap(), "200");
    assert!(!query.contains_key("user_id"));
}

fn serve_response(response: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let task = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "Nexus test request timed out");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("Nexus test accept failed: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut received = Vec::new();
        let mut chunk = [0; 1024];
        while !received.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0, "Nexus test request ended before its headers");
            received.extend_from_slice(&chunk[..count]);
            assert!(
                received.len() <= 16 * 1024,
                "Nexus test headers exceed the fixture limit"
            );
        }
        let _ = stream.write_all(&response);
    });
    (format!("http://{address}/fixture"), task)
}

#[tokio::test]
async fn nexus_metadata_responses_are_bounded_and_server_errors_never_echo_secrets() {
    for (status, body, expected_code) in [
        (401, "fixture-server-secret", "nexus_auth_invalid"),
        (429, "fixture-server-secret", "nexus_rate_limited"),
        (200, "fixture-server-secret", "nexus_file_unavailable"),
    ] {
        let (url, task) = serve_response(format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes());
        let response = reqwest::Client::new().get(url).send().await.unwrap();
        let error = read_json::<Value>(response, 4096, false)
            .await
            .err()
            .unwrap();
        assert!(error.contains(expected_code));
        assert!(!error.contains("fixture-server-secret"));
        task.join().unwrap();
    }
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 9999\r\nConnection: close\r\n\r\n".to_vec(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n10\r\n0123456789ABCDEF\r\n0\r\n\r\n".to_vec(),
    ] {
        let (url, task) = serve_response(response);
        let response = reqwest::Client::new().get(url).send().await.unwrap();
        assert!(read_json::<Value>(response, 8, false).await.err().unwrap().contains("size limit"));
        task.join().unwrap();
    }
}

#[tokio::test]
async fn nexus_authenticated_client_does_not_follow_redirects() {
    let (url, task) = serve_response(b"HTTP/1.1 302 Found\r\nLocation: https://evil.invalid/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec());
    let api = NexusApi::new("fixture-api-secret").unwrap();
    let response = api.client.get(url).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    task.join().unwrap();
}

#[tokio::test]
async fn nexus_free_direct_download_permission_is_decided_by_the_official_api() {
    for (status, free_keyless, body, expected_error) in [
        (
            200,
            true,
            r#"[{"URI":"https://files.nexus-cdn.com/Mod.zip"}]"#,
            None,
        ),
        (
            403,
            true,
            "fixture-private-error",
            Some("nexus_premium_required"),
        ),
        (
            403,
            false,
            "fixture-private-error",
            Some("nexus_file_unavailable"),
        ),
    ] {
        let (url, task) = serve_response(format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes());
        let response = reqwest::Client::new().get(url).send().await.unwrap();
        let result = read_json::<Vec<DownloadLink>>(response, 4096, free_keyless).await;
        if let Some(expected) = expected_error {
            let error = result.err().unwrap();
            assert!(error.contains(expected));
            assert!(!error.contains("fixture-private-error"));
        } else {
            assert_eq!(
                result.ok().unwrap()[0].uri,
                "https://files.nexus-cdn.com/Mod.zip"
            );
        }
        task.join().unwrap();
    }
}

#[test]
fn nexus_kilobyte_metadata_is_not_used_as_an_exact_download_size() {
    let file: NexusFile = serde_json::from_value(
        json!({"file_id":88,"name":"Mod","file_name":"Mod.zip","version":"1","size_kb":2}),
    )
    .unwrap();
    assert_eq!(file.size_bytes().unwrap(), 2048);
    assert_eq!(file.exact_size_bytes(), None);
}
