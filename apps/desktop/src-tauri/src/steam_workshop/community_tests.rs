use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BROWSE: &str = "https://steamcommunity.com/workshop/browse/?appid=322330&section=readytouseitems&searchtext=%E6%98%BE%E7%A4%BA&l=schinese";

#[test]
fn cdn_preserves_the_query_and_selects_community_without_changing_tls_identity() {
    let client = Client::new();
    for preference in [
        SourcePreference::ChinaFirst,
        SourcePreference::InternationalFirst,
    ] {
        let requests = routes(client.get(BROWSE).build().unwrap(), preference).unwrap();
        assert_eq!(requests.len(), 2);
        let cdn_index = usize::from(preference == SourcePreference::InternationalFirst);
        let cdn = &requests[cdn_index].request;
        assert_eq!(cdn.url().scheme(), "https");
        assert_eq!(cdn.url().host_str(), Some(COMMUNITY_CDN));
        assert_eq!(
            cdn.url().query(),
            reqwest::Url::parse(BROWSE).unwrap().query()
        );
        assert_eq!(cdn.headers()[reqwest::header::HOST], COMMUNITY_HOST);
        let direct = &requests[1 - cdn_index].request;
        assert_eq!(direct.url().as_str(), BROWSE);
        assert!(!direct.headers().contains_key(reqwest::header::HOST));
    }
    assert!(supports(
        &client
            .get("https://steamcommunity.com/sharedfiles/filedetails/?id=2964299587&l=english")
            .build()
            .unwrap()
    ));
}

#[test]
fn authenticated_requests_and_unapproved_paths_never_change_destination() {
    let client = Client::new();
    for url in [
        "https://steamcommunity.com/login/home/",
        "https://steamcommunity.com/workshop/browse/?appid=322330&key=example",
        "https://steamcommunity.com/workshop/browse/?appid=322330&appid=440",
        "https://steamcommunity.com/workshop/browse/?appid=0",
        "https://steamcommunity.com/workshop/browse/?appid=invalid",
        "https://steamcommunity.com/workshop/browse/?appid=18446744073709551616",
        "https://steamcommunity.com/workshop/browse/?l=english",
        "https://steamcommunity.com/sharedfiles/filedetails/?id=123&searchtext=test",
        "https://steamcommunity.com/sharedfiles/filedetails/?id=123#fragment",
        "https://steamcommunity.com:444/sharedfiles/filedetails/?id=123",
        "http://steamcommunity.com/sharedfiles/filedetails/?id=123",
        "https://example.invalid/sharedfiles/filedetails/?id=123",
    ] {
        let requests = routes(
            client.get(url).build().unwrap(),
            SourcePreference::ChinaFirst,
        )
        .unwrap();
        assert_eq!(requests.len(), 1, "{url}");
        assert_eq!(requests[0].request.url().as_str(), url);
        assert!(requests[0].connection.is_none());
    }
    for header in ["cookie", "authorization", "host", "x-api-key"] {
        assert!(!supports(
            &client
                .get(BROWSE)
                .header(header, "example")
                .build()
                .unwrap()
        ));
    }
    assert!(!supports(&client.post(BROWSE).build().unwrap()));
    assert!(!supports(
        &client.get(BROWSE).body("payload").build().unwrap()
    ));
    assert!(!supports(
        &client
            .get(BROWSE)
            .basic_auth("example", Some("example"))
            .build()
            .unwrap()
    ));
}

#[test]
fn cooling_connection_loses_to_either_locale_then_recovers_after_sixty_seconds() {
    let client = Client::new();
    let now = Instant::now();
    for (preference, failed, alternate) in [
        (
            SourcePreference::ChinaFirst,
            Connection::Cdn,
            COMMUNITY_HOST,
        ),
        (
            SourcePreference::InternationalFirst,
            Connection::Origin,
            COMMUNITY_CDN,
        ),
    ] {
        let health = ConnectionHealth::default();
        health.record_failure(Some(failed), now);
        let mut requests = routes(client.get(BROWSE).build().unwrap(), preference).unwrap();
        let preferred = requests[0].request.url().host_str().unwrap().to_owned();
        health.order(&mut requests, now + Duration::from_secs(59));
        assert_eq!(requests[0].request.url().host_str(), Some(alternate));
        let mut requests = routes(client.get(BROWSE).build().unwrap(), preference).unwrap();
        health.order(&mut requests, now + Duration::from_secs(60));
        assert_eq!(
            requests[0].request.url().host_str(),
            Some(preferred.as_str())
        );
    }
}

#[test]
fn both_cooling_connections_keep_each_requests_locale_and_do_not_remove_fallbacks() {
    let client = Client::new();
    let now = Instant::now();
    let health = ConnectionHealth::default();
    health.record_failure(Some(Connection::Origin), now);
    health.record_failure(Some(Connection::Cdn), now);
    for (preference, first) in [
        (SourcePreference::ChinaFirst, COMMUNITY_CDN),
        (SourcePreference::InternationalFirst, COMMUNITY_HOST),
    ] {
        let mut requests = routes(client.get(BROWSE).build().unwrap(), preference).unwrap();
        health.order(&mut requests, now);
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].request.url().host_str(), Some(first));
    }
}

async fn fixture(response: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/workshop/browse/?appid=322330",
        listener.local_addr().unwrap()
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("fixture must receive a request")
            .unwrap();
        let mut bytes = Vec::new();
        while !bytes.windows(4).any(|value| value == b"\r\n\r\n") {
            let mut buffer = [0; 1024];
            let size = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut buffer))
                .await
                .expect("fixture request must complete")
                .unwrap();
            assert!(size > 0 && bytes.len() + size < 16 * 1024);
            bytes.extend_from_slice(&buffer[..size]);
        }
        socket.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(bytes).unwrap()
    });
    (url, task)
}

#[tokio::test]
async fn unavailable_route_falls_back_and_delivers_the_actual_body() {
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let (missing, first) =
        fixture("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    let (working, second) =
        fixture("HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\ncatalog").await;
    let health = ConnectionHealth::default();
    let requests = vec![
        Route {
            request: client.get(missing).build().unwrap(),
            connection: Some(Connection::Origin),
        },
        Route {
            request: client
                .get(&working)
                .header("Host", COMMUNITY_HOST)
                .build()
                .unwrap(),
            connection: Some(Connection::Cdn),
        },
    ];
    let response = read_routes(
        &client,
        requests,
        Duration::from_secs(4),
        1024,
        SourcePreference::ChinaFirst,
        &health,
    )
    .await
    .unwrap();
    assert_eq!(response.bytes, b"catalog");
    assert_eq!(response.url.as_str(), working);
    first.await.unwrap();
    assert!(
        second
            .await
            .unwrap()
            .to_ascii_lowercase()
            .contains("host: steamcommunity.com\r\n")
    );
    let mut next = routes(
        client.get(BROWSE).build().unwrap(),
        SourcePreference::InternationalFirst,
    )
    .unwrap();
    health.order(&mut next, Instant::now());
    assert_eq!(next[0].request.url().host_str(), Some(COMMUNITY_CDN));

    // A second real read must avoid opening the cooled connection, even when
    // its request locale still prefers that route. No elapsed-time assertion.
    let cooled = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    cooled.set_nonblocking(true).unwrap();
    let (working, second_read) =
        fixture("HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\ncatalog").await;
    let response = read_routes(
        &client,
        vec![
            Route {
                request: client
                    .get(format!("http://{}/", cooled.local_addr().unwrap()))
                    .build()
                    .unwrap(),
                connection: Some(Connection::Origin),
            },
            Route {
                request: client.get(&working).build().unwrap(),
                connection: Some(Connection::Cdn),
            },
        ],
        Duration::from_secs(4),
        1024,
        SourcePreference::InternationalFirst,
        &health,
    )
    .await
    .unwrap();
    assert_eq!(response.bytes, b"catalog");
    assert_eq!(
        cooled.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    second_read.await.unwrap();
}

#[tokio::test]
async fn http_success_does_not_erase_cooldown_before_workshop_content_validation() {
    let client = Client::builder().no_proxy().build().unwrap();
    let health = ConnectionHealth::default();
    let failed_at = Instant::now();
    health.record_failure(Some(Connection::Cdn), failed_at);
    let (url, server) =
        fixture("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nlogin").await;
    let response = read_routes(
        &client,
        vec![Route {
            request: client.get(url).build().unwrap(),
            connection: Some(Connection::Cdn),
        }],
        Duration::from_secs(3),
        1024,
        SourcePreference::ChinaFirst,
        &health,
    )
    .await
    .unwrap();
    assert_eq!(response.bytes, b"login");
    let mut next = routes(
        client.get(BROWSE).build().unwrap(),
        SourcePreference::ChinaFirst,
    )
    .unwrap();
    health.order(&mut next, failed_at);
    assert_eq!(next[0].request.url().host_str(), Some(COMMUNITY_HOST));
    server.await.unwrap();
}

#[tokio::test]
async fn redirects_throttling_and_credentials_failures_do_not_reach_another_route() {
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    for response in [
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 503 Unavailable\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ] {
        let health = ConnectionHealth::default();
        let (url, server) = fixture(response).await;
        let unused = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let requests = vec![
            Route {
                request: client
                    .get(url)
                    .header("Host", COMMUNITY_HOST)
                    .build()
                    .unwrap(),
                connection: Some(Connection::Cdn),
            },
            Route {
                request: client
                    .get(format!("http://{}/", unused.local_addr().unwrap()))
                    .build()
                    .unwrap(),
                connection: Some(Connection::Origin),
            },
        ];
        let error = read_routes(
            &client,
            requests,
            Duration::from_secs(3),
            1024,
            SourcePreference::ChinaFirst,
            &health,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            NetworkError::Status { .. } | NetworkError::RetryDeferred { .. }
        ));
        assert!(!error.permits_source_fallback());
        let mut next = routes(
            client.get(BROWSE).build().unwrap(),
            SourcePreference::ChinaFirst,
        )
        .unwrap();
        health.order(&mut next, Instant::now());
        assert_eq!(next[0].request.url().host_str(), Some(COMMUNITY_CDN));
        server.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), unused.accept())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn oversized_response_is_rejected_before_fallback() {
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let (url, server) =
        fixture("HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n").await;
    let error = read_routes(
        &client,
        vec![Route {
            request: client.get(url).build().unwrap(),
            connection: None,
        }],
        Duration::from_secs(2),
        8,
        SourcePreference::ChinaFirst,
        &ConnectionHealth::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        NetworkError::BodyTooLarge { max_bytes: 8, .. }
    ));
    server.await.unwrap();
}

#[tokio::test]
#[ignore = "requires live public Steam endpoints; run explicitly on the affected network"]
async fn live_dst_workshop_search_and_uncached_details() {
    // This known server Mod must also pass a cold item-type lookup independently
    // of whichever items the live search happened to return.
    let details = super::super::lookup_public_workshop_items(
        vec!["378160973".to_owned()],
        SourcePreference::ChinaFirst,
    )
    .await
    .expect("live Workshop details");
    assert_eq!(details.len(), 1);
    assert_eq!(details[0].consumer_app_id, Some(322330));
    assert_eq!(details[0].item_kind, "item");
    assert!(
        details[0]
            .title
            .as_ref()
            .is_some_and(|title| !title.is_empty())
    );
    println!("DST details: app 322330, item type verified");
    let search = super::super::search_public_workshop_items(
        322330,
        Some("Display".to_owned()),
        None,
        Some(1),
        Some("zh-CN".to_owned()),
        None,
    )
    .await
    .expect("live Workshop search");
    assert!(!search.items.is_empty());
    assert!(
        search
            .items
            .iter()
            .all(|item| item.consumer_app_id == Some(322330))
    );
    println!(
        "DST search: {} items, total {:?}",
        search.items.len(),
        search.total_count
    );
}
