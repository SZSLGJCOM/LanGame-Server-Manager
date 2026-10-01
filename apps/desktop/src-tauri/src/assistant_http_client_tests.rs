use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn http_fixture(responses: Vec<String>) -> (Url, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(3), async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).await.unwrap();
                    assert_ne!(count, 0, "request must include complete headers");
                    request.extend_from_slice(&buffer[..count]);
                    assert!(request.len() <= 8192, "fixture request headers are bounded");
                }
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
                requests.push(String::from_utf8(request).unwrap());
            }
            requests
        })
        .await
        .expect("fixture request must finish within its deadline")
    });
    (endpoint, task)
}

fn ok_response() -> String {
    String::from("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
}

#[test]
fn assistant_http_client_direct_scope_is_only_loopback() {
    for endpoint in [
        "http://127.0.0.1:11434/v1",
        "https://127.255.255.254/v1",
        "http://[::1]:11434/v1",
        "http://LOCALHOST:11434/v1",
    ] {
        assert!(
            is_loopback_url(&Url::parse(endpoint).unwrap()),
            "{endpoint}"
        );
    }
    for endpoint in [
        "https://example.invalid/v1",
        "http://localhost.example.invalid/v1",
        "http://127.0.0.1.example.invalid/v1",
        "http://192.168.1.5:11434/v1",
        "http://10.0.0.1:11434/v1",
        "http://172.16.0.1:11434/v1",
        "http://0.0.0.0:11434/v1",
        "http://128.0.0.1:11434/v1",
        "http://[::ffff:127.0.0.1]:11434/v1",
        "http://[fe80::1]:11434/v1",
        "file://localhost/model",
    ] {
        assert!(
            !is_loopback_url(&Url::parse(endpoint).unwrap()),
            "{endpoint}"
        );
    }
}

#[test]
fn assistant_http_client_keeps_service_url_validation() {
    for endpoint in [
        "http://user:fixture@127.0.0.1:11434/v1",
        "http://127.0.0.1:11434/v1?token=fixture",
        "file://localhost/model",
        "relative/path",
    ] {
        assert!(build_assistant_http_client(endpoint, Duration::from_secs(2)).is_err());
    }
}

#[tokio::test]
async fn assistant_http_client_model_queries_and_generation_bypass_proxy_for_loopback() {
    for (method, path) in [
        (reqwest::Method::GET, "/api/tags"),
        (reqwest::Method::POST, "/v1/chat/completions"),
    ] {
        let (mut endpoint, server) = http_fixture(vec![ok_response()]).await;
        endpoint.set_path(path);
        let builder = Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .timeout(Duration::from_secs(2));
        let client = apply_endpoint_policy(builder, &endpoint).build().unwrap();
        let response = client
            .request(method.clone(), endpoint)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "OK");
        assert!(server.await.unwrap()[0].starts_with(&format!("{method} {path} HTTP/1.1")));
    }
}

#[tokio::test]
async fn assistant_http_client_remote_and_private_endpoints_keep_configured_proxy() {
    for endpoint in [
        "http://example.invalid/v1/models",
        "http://192.168.1.5/v1/models",
    ] {
        let endpoint = Url::parse(endpoint).unwrap();
        let (proxy, server) = http_fixture(vec![ok_response()]).await;
        let builder = Client::builder()
            .proxy(reqwest::Proxy::all(proxy.as_str()).unwrap())
            .timeout(Duration::from_secs(2));
        let client = apply_endpoint_policy(builder, &endpoint).build().unwrap();
        let response = client.get(endpoint.as_str()).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(server.await.unwrap()[0].starts_with(&format!("GET {endpoint} HTTP/1.1")));
    }
}

#[tokio::test]
async fn assistant_http_client_loopback_redirect_cannot_bypass_remote_proxy_policy() {
    let response = String::from(
        "HTTP/1.1 302 Found\r\nLocation: http://example.invalid/v1/models\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let (endpoint, server) = http_fixture(vec![response]).await;
    let client = build_assistant_http_client(endpoint.as_str(), Duration::from_secs(2)).unwrap();
    let response = client.get(endpoint).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    server.await.unwrap();
}

#[tokio::test]
async fn assistant_http_client_does_not_forward_private_requests_to_another_origin() {
    let fixture_key = ["fixture", "provider", "key"].join("-");
    let (destination, destination_server) = http_fixture(vec![ok_response()]).await;
    let response = format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let (endpoint, server) = http_fixture(vec![response]).await;
    let client = build_assistant_http_client(endpoint.as_str(), Duration::from_secs(2)).unwrap();
    let response = client
        .post(endpoint)
        .header("x-api-key", &fixture_key)
        .json(&serde_json::json!({"messages": ["private diagnostic evidence"]}))
        .send()
        .await
        .unwrap();
    server.await.unwrap();
    destination_server.abort();
    let destination_result = destination_server.await;
    assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
    assert!(destination_result.is_err_and(|error| error.is_cancelled()));
}

#[tokio::test]
async fn assistant_http_client_allows_provider_redirects_within_the_same_origin() {
    let fixture_key = ["fixture", "provider", "key"].join("-");
    let redirect = String::from(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: /canonical/messages\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let (endpoint, server) = http_fixture(vec![redirect, ok_response()]).await;
    let client = build_assistant_http_client(endpoint.as_str(), Duration::from_secs(2)).unwrap();
    let response = client
        .post(endpoint)
        .header("x-api-key", &fixture_key)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let requests = server.await.unwrap();
    assert!(requests[1].starts_with("POST /canonical/messages HTTP/1.1"));
    assert!(requests[1].contains(&format!("x-api-key: {fixture_key}")));
}

#[tokio::test]
async fn assistant_http_client_remote_redirect_cannot_change_the_provider_origin() {
    let redirect = String::from(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://other.example.invalid/messages\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let (proxy, server) = http_fixture(vec![redirect]).await;
    let endpoint = Url::parse("http://provider.example.invalid/messages").unwrap();
    let client = apply_endpoint_policy(
        Client::builder()
            .proxy(reqwest::Proxy::all(proxy.as_str()).unwrap())
            .timeout(Duration::from_secs(2)),
        &endpoint,
    )
    .build()
    .unwrap();
    let response = client.post(endpoint).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
    server.await.unwrap();
}
