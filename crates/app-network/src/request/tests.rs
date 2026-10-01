use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn read_request(stream: &mut TcpStream) {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 1024];
        let size = stream.read(&mut buffer).await.unwrap();
        if size == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..size]);
        assert!(
            bytes.len() <= 32 * 1024,
            "fixture request exceeded its bound"
        );
        if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..end]).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                return;
            }
        }
    }
}

async fn fixture(responses: Vec<&'static str>) -> (String, tokio::task::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/resource?test=1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut count = 0;
        for response in responses {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            read_request(&mut stream).await;
            stream.write_all(response.as_bytes()).await.unwrap();
            count += 1;
        }
        count
    });
    (url, server)
}

fn client() -> Client {
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap()
}
const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
const UNAVAILABLE: &str =
    "HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nRetry-After: 0\r\nConnection: close\r\n\r\n";

#[tokio::test]
async fn transient_get_retries_once_and_returns_the_real_response() {
    let (url, server) = fixture(vec![UNAVAILABLE, OK]).await;
    let client = client();
    let response = send_read_only(
        &client,
        client.get(url).build().unwrap(),
        Duration::from_secs(2),
        SourcePreference::InternationalFirst,
    )
    .await
    .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    assert_eq!(server.await.unwrap(), 2);
}

#[tokio::test]
async fn repeated_transient_failure_terminates_after_two_attempts() {
    let (url, server) = fixture(vec![UNAVAILABLE, UNAVAILABLE]).await;
    let client = client();
    let error = send_with_retry(
        &client,
        client.get(url).build().unwrap(),
        Duration::from_secs(2),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        NetworkError::Status {
            status: StatusCode::SERVICE_UNAVAILABLE,
            ..
        }
    ));
    assert_eq!(server.await.unwrap(), 2);
}

#[tokio::test]
async fn authorization_failure_is_not_retried() {
    let (url, server) = fixture(vec![
        "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ])
    .await;
    let client = client();
    assert!(matches!(
        send_read_only(
            &client,
            client.get(url).build().unwrap(),
            Duration::from_secs(2),
            SourcePreference::ChinaFirst,
        )
        .await,
        Err(NetworkError::Status {
            status: StatusCode::FORBIDDEN,
            ..
        })
    ));
    assert_eq!(server.await.unwrap(), 1);
}

#[tokio::test]
async fn arbitrary_post_is_never_replayed() {
    let (url, server) = fixture(vec![UNAVAILABLE]).await;
    let client = client();
    assert!(
        send_with_retry(
            &client,
            client.post(url).body("value=1").build().unwrap(),
            Duration::from_secs(2)
        )
        .await
        .is_err()
    );
    assert_eq!(server.await.unwrap(), 1);
}

#[tokio::test]
async fn retry_after_beyond_budget_does_not_issue_an_early_request() {
    let (url, server) = fixture(vec!["HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"]).await;
    let client = client();
    assert!(matches!(
        send_with_retry(
            &client,
            client.get(url).build().unwrap(),
            Duration::from_millis(100)
        )
        .await,
        Err(NetworkError::RetryDeferred {
            status: StatusCode::TOO_MANY_REQUESTS,
            retry_after: Some(delay),
            ..
        }) if delay == Duration::from_secs(120)
    ));
    assert_eq!(server.await.unwrap(), 1);
}

#[tokio::test]
async fn http_date_retry_window_is_preserved_relative_to_server_clock() {
    let (url, server) = fixture(vec!["HTTP/1.1 429 Too Many Requests\r\nDate: Wed, 21 Oct 2037 07:26:00 GMT\r\nRetry-After: Wed, 21 Oct 2037 07:28:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"]).await;
    let client = client();
    let error = send_with_retry(
        &client,
        client.get(url).build().unwrap(),
        Duration::from_secs(2),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, NetworkError::RetryDeferred { retry_after: Some(delay), .. }
        if delay == Duration::from_secs(120))
    );
    assert_eq!(server.await.unwrap(), 1);
}

#[test]
fn throttling_and_server_retry_windows_terminate_all_source_attempts() {
    for error in [
        NetworkError::Status {
            status: StatusCode::TOO_MANY_REQUESTS,
            origin: String::from("https://official.example"),
        },
        NetworkError::RetryDeferred {
            status: StatusCode::SERVICE_UNAVAILABLE,
            origin: String::from("https://official.example"),
            retry_after: Some(Duration::from_secs(120)),
        },
        NetworkError::Status {
            status: StatusCode::FORBIDDEN,
            origin: String::from("https://official.example"),
        },
    ] {
        assert!(!error.permits_source_fallback());
    }
}

#[test]
fn retry_classification_excludes_mutations_and_covers_only_public_lookup_posts() {
    let client = client();
    for host in ["api.steampowered.com", "api.steamchina.com"] {
        assert!(may_retry(
            &client
                .post(format!(
                    "https://{host}/ISteamRemoteStorage/GetPublishedFileDetails/v1/"
                ))
                .form(&[("itemcount", "1"), ("publishedfileids[0]", "2964299587")])
                .build()
                .unwrap()
        ));
        assert!(!may_retry(
            &client
                .post(format!(
                    "https://{host}/ISteamRemoteStorage/SetUGCUsedByGC/v1/"
                ))
                .build()
                .unwrap()
        ));
    }
}

async fn unused_source() -> (String, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    (
        format!("http://{}/unused", listener.local_addr().unwrap()),
        listener,
    )
}

fn assert_unused(listener: TcpListener) {
    let listener = listener.into_std().unwrap();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn throttling_never_retries_or_contacts_an_alternate_source() {
    for response in [
        "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ] {
        let (url, server) = fixture(vec![response]).await;
        let (alternate, unused) = unused_source().await;
        let client = client();
        let request = client.get(&url).build().unwrap();
        let error = body::read_from_sources(&client, request, Duration::from_secs(2), 64, |_| {
            vec![url, alternate]
        })
        .await
        .unwrap_err();
        assert!(!error.permits_source_fallback());
        assert_eq!(server.await.unwrap(), 1);
        assert_unused(unused);
    }
}

#[tokio::test]
async fn retry_after_on_final_response_stops_all_sources() {
    for response in [
        "HTTP/1.1 503 Unavailable\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 503 Unavailable\r\nRetry-After: Wed, 21 Oct 2037 07:28:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 503 Unavailable\r\nRetry-After: invalid\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ] {
        let (url, server) = fixture(vec![UNAVAILABLE, response]).await;
        let (alternate, unused) = unused_source().await;
        let client = client();
        let request = client.get(&url).build().unwrap();
        let error = send_from_sources(&client, request, Duration::from_secs(2), |_| {
            vec![url, alternate]
        })
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            NetworkError::RetryDeferred {
                status: StatusCode::SERVICE_UNAVAILABLE,
                ..
            }
        ));
        assert_eq!(server.await.unwrap(), 2);
        assert_unused(unused);
    }
}

#[tokio::test]
async fn credentials_and_unknown_headers_are_only_sent_to_the_original_source() {
    for name in [
        "authorization",
        "cookie",
        "x-api-key",
        "x-custom-session",
        "referer",
    ] {
        let (url, server) = fixture(vec![UNAVAILABLE, UNAVAILABLE]).await;
        let (alternate, unused) = unused_source().await;
        let client = client();
        let request = client
            .get(&url)
            .header(name, "fixture-private-value")
            .build()
            .unwrap();
        assert!(
            send_from_sources(&client, request, Duration::from_secs(2), |_| vec![
                url, alternate
            ])
            .await
            .is_err()
        );
        assert_eq!(server.await.unwrap(), 2);
        assert_unused(unused);
    }
}

async fn lookup_fixture(
    responses: Vec<&'static str>,
) -> (Client, String, tokio::task::JoinHandle<usize>) {
    let (url, server) = fixture(responses).await;
    let url = reqwest::Url::parse(&url).unwrap();
    let address = format!("127.0.0.1:{}", url.port().unwrap())
        .parse()
        .unwrap();
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .resolve("api.steampowered.com", address)
        .build()
        .unwrap();
    let url = format!(
        "http://api.steampowered.com:{}/ISteamRemoteStorage/GetPublishedFileDetails/v1/",
        url.port().unwrap()
    );
    (client, url, server)
}

#[tokio::test]
async fn public_form_can_fall_back_but_unknown_form_fields_cannot() {
    let (client, url, server) = lookup_fixture(vec![UNAVAILABLE, UNAVAILABLE]).await;
    let (alternate, fallback) = fixture(vec![OK]).await;
    let request = client
        .post(&url)
        .form(&[("itemcount", "1"), ("publishedfileids[0]", "2964299587")])
        .build()
        .unwrap();
    let response = send_from_sources(&client, request, Duration::from_secs(2), |_| {
        vec![url, alternate]
    })
    .await
    .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    assert_eq!(server.await.unwrap(), 2);
    assert_eq!(fallback.await.unwrap(), 1);

    for form in [
        "itemcount=1&publishedfileids%5B0%5D=2964299587&key=fixture-private-value",
        "itemcount=1&publishedfileids%5B0%5D=2964299587&%61pi_key=fixture-private-value",
        "itemcount=2&publishedfileids%5B0%5D=2964299587",
        "itemcount=1&publishedfileids%5B0%5D=not-an-id",
        "itemcount=1&publishedfileids%5B0%5D=2964299587&itemcount=1",
    ] {
        let (client, url, server) = lookup_fixture(vec![UNAVAILABLE]).await;
        let (alternate, unused) = unused_source().await;
        let request = client
            .post(&url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
            .build()
            .unwrap();
        assert!(
            send_from_sources(&client, request, Duration::from_secs(2), |_| vec![
                url, alternate
            ])
            .await
            .is_err()
        );
        assert_eq!(server.await.unwrap(), 1);
        assert_unused(unused);
    }
}

#[tokio::test]
async fn incomplete_body_is_discarded_before_reading_the_alternate() {
    let (url, server) = fixture(vec![
        "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\npartial",
    ])
    .await;
    let (alternate, fallback) = fixture(vec![OK]).await;
    let expected_url = alternate.clone();
    let client = client();
    let request = client.get(&url).build().unwrap();
    let result = body::read_from_sources(&client, request, Duration::from_secs(2), 64, |_| {
        vec![url, alternate]
    })
    .await
    .unwrap();
    assert_eq!(result.bytes, b"ok");
    assert_eq!(result.url.as_str(), expected_url);
    assert_eq!(server.await.unwrap(), 1);
    assert_eq!(fallback.await.unwrap(), 1);
}

#[tokio::test]
async fn advertised_and_streamed_size_limits_stop_without_another_source() {
    for response in [
        "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nlarge",
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nbig\r\n0\r\n\r\n",
    ] {
        let (url, server) = fixture(vec![response]).await;
        let (alternate, unused) = unused_source().await;
        let client = client();
        let request = client.get(&url).build().unwrap();
        let error = body::read_from_sources(&client, request, Duration::from_secs(2), 2, |_| {
            vec![url, alternate]
        })
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            NetworkError::BodyTooLarge { max_bytes: 2, .. }
        ));
        assert_eq!(server.await.unwrap(), 1);
        assert_unused(unused);
    }
}

async fn held_body() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/held-body", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\no")
            .await
            .unwrap();
        std::future::pending::<()>().await;
    });
    (url, server)
}

#[tokio::test]
async fn body_wait_uses_the_same_total_budget_as_headers() {
    let (url, server) = held_body().await;
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let request = client.get(url).build().unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        read_public_bytes(
            &client,
            request,
            Duration::from_millis(500),
            64,
            SourcePreference::InternationalFirst,
        ),
    )
    .await;
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    assert!(matches!(
        result.unwrap(),
        Err(NetworkError::Deadline { .. })
    ));
}

#[tokio::test]
async fn stalled_body_reserves_time_for_the_remaining_official_source() {
    let (url, server) = held_body().await;
    let (alternate, fallback) = fixture(vec![OK]).await;
    let client = client();
    let request = client.get(&url).build().unwrap();
    let result = body::read_from_sources(&client, request, Duration::from_secs(1), 64, |_| {
        vec![url, alternate]
    })
    .await;
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    assert_eq!(result.unwrap().bytes, b"ok");
    assert_eq!(fallback.await.unwrap(), 1);
}
