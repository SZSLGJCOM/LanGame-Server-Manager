use super::*;

fn parse_manifest(text: &str) -> Result<u32, String> {
    text.strip_prefix("version=")
        .ok_or_else(|| String::from("missing manifest version"))?
        .parse()
        .map_err(|error: std::num::ParseIntError| error.to_string())
}

#[tokio::test]
async fn manifest_parse_or_request_failure_uses_the_next_candidate() {
    for responses in [
        vec![reply(200, b"not a manifest")],
        vec![reply(503, b"busy"), reply(503, b"busy")],
    ] {
        let expected_requests = responses.len() as u64;
        let primary = fixture(responses).await;
        let alternate = fixture(vec![reply(200, b"version=42")]).await;
        let (text, version) = fetch_text_from_candidates(
            &fixture_client(),
            &[primary.url.clone(), alternate.url.clone()],
            deadline(),
            1024,
            parse_manifest,
        )
        .await
        .unwrap();
        assert_eq!((text.as_str(), version), ("version=42", 42));
        assert_eq!(primary.requests.load(Ordering::Relaxed), expected_requests);
        assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn invalid_utf8_and_oversized_text_never_reach_the_parser() {
    for response in [
        reply(200, &[0xff, 0xfe]),
        reply(200, b"version=123456"),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nD\r\nversion=12345\r\n0\r\n\r\n".to_vec(),
    ] {
        let source = fixture(vec![response]).await;
        let result = fetch_text_validated::<u32>(
            &fixture_client(),
            &source.url,
            deadline(),
            10,
            |_| panic!("untrusted bytes must be checked before parsing"),
        )
        .await;
        assert!(matches!(result, Err(SteamCmdError::HttpDownload { .. })));
        assert_eq!(source.requests.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn interrupted_text_restarts_and_only_returns_a_complete_manifest() {
    let source = fixture(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nver".to_vec(),
        reply(200, b"version=42"),
    ])
    .await;
    let result = fetch_text_validated(
        &fixture_client(),
        &source.url,
        deadline(),
        1024,
        parse_manifest,
    )
    .await
    .unwrap();
    assert_eq!(result, (String::from("version=42"), 42));
    assert_eq!(source.requests.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn cancellation_after_manifest_validation_is_not_success_or_source_fallback() {
    let primary = fixture(vec![reply(200, b"version=42")]).await;
    let alternate = fixture(vec![reply(200, b"version=43")]).await;
    let cancellation = crate::InstallCancellation::new();
    let result = cancellation
        .scope(fetch_text_from_candidates(
            &fixture_client(),
            &[primary.url.clone(), alternate.url.clone()],
            deadline(),
            1024,
            |text| {
                let version = parse_manifest(text)?;
                cancellation.cancel();
                Ok(version)
            },
        ))
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(alternate.requests.load(Ordering::Relaxed), 0);
}
