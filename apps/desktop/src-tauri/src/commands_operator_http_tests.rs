use super::build_operator_http_client;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn http_fixture(response: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local operator listener");
    let endpoint = format!(
        "http://{}/",
        listener.local_addr().expect("fixture address")
    );
    let server = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(3), async move {
            let (mut stream, _) = listener.accept().await.expect("operator request");
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).await.expect("request headers");
                assert_ne!(count, 0, "request must contain complete headers");
                request.extend_from_slice(&buffer[..count]);
                assert!(request.len() <= 8192, "request headers are bounded");
            }
            stream
                .write_all(response.as_bytes())
                .await
                .expect("operator response");
            stream.shutdown().await.expect("close fixture connection");
            String::from_utf8(request).expect("ASCII HTTP request")
        })
        .await
        .expect("operator request must finish within its deadline")
    });
    (endpoint, server)
}

#[tokio::test]
async fn operator_http_probe_bypasses_configured_proxy_to_reach_instance_listener() {
    let (endpoint, server) =
        http_fixture("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    let builder = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all("http://127.0.0.1:9").expect("fixture proxy"));
    let client = build_operator_http_client(builder).expect("direct operator client");
    let response = client
        .get(endpoint)
        .send()
        .await
        .expect("instance response");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let request = server.await.expect("fixture completion");
    assert!(request.starts_with("GET / HTTP/1.1\r\n"));
    assert!(!request.to_ascii_lowercase().contains("authorization:"));
}

#[tokio::test]
async fn operator_http_probe_does_not_follow_redirects_away_from_instance() {
    let (endpoint, server) = http_fixture(
        "HTTP/1.1 302 Found\r\nLocation: http://example.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    let client =
        build_operator_http_client(reqwest::Client::builder()).expect("direct operator client");
    let response = client
        .get(endpoint)
        .send()
        .await
        .expect("instance response");
    assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    server.await.expect("fixture completion");
}
