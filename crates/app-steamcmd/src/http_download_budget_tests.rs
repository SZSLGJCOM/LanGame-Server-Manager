use super::*;

async fn stalled_body() -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/artifact", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicU64::new(0));
    let observed = requests.clone();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        observed.fetch_add(1, Ordering::Relaxed);
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\n\r\nx")
            .await
            .unwrap();
        // The body stays open, so only the caller's own budget can advance it.
        std::future::pending::<()>().await;
    });
    Fixture {
        url,
        task,
        requests,
    }
}

#[tokio::test]
async fn stalled_metadata_leaves_time_for_the_alternate() {
    let primary = stalled_body().await;
    let alternate = fixture(vec![reply(200, b"version=42")]).await;
    let result = fetch_text_from_candidates(
        &fixture_client(),
        &[primary.url.clone(), alternate.url.clone()],
        deadline(),
        1024,
        |text| {
            text.strip_prefix("version=")
                .ok_or_else(|| "invalid manifest".to_owned())
                .and_then(|value| value.parse::<u32>().map_err(|error| error.to_string()))
        },
    )
    .await;
    assert_eq!(
        result.expect("a stalled primary must leave time for an alternate"),
        ("version=42".into(), 42)
    );
    assert_eq!(primary.requests.load(Ordering::Relaxed), 1);
    assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn stalled_download_leaves_time_for_verified_alternate_without_mixing_bytes() {
    let primary = stalled_body().await;
    let alternate = fixture(vec![reply(200, b"verified archive")]).await;
    let path = destination();
    std::fs::write(&path, b"retained installation").unwrap();
    let checksum = sha1(b"verified archive");
    let result = download_file_from_candidates(
        &fixture_client(),
        &[primary.url.clone(), alternate.url.clone()],
        &path,
        DownloadIntegrity {
            sha1: Some(&checksum),
            size: Some(16),
            ..Default::default()
        },
        deadline(),
    )
    .await;
    // Keep the disposable fixture clean even when demonstrating the regression.
    let actual = result
        .as_ref()
        .ok()
        .map(|file| std::fs::read(file.path()).unwrap());
    let source = result.as_ref().ok().map(|file| file.source.clone());
    let retained = std::fs::read(&path).unwrap();
    drop(result);
    let entries = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    assert_eq!(actual.as_deref(), Some(b"verified archive".as_slice()));
    assert_eq!(source.as_deref(), Some(alternate.url.as_str()));
    assert_eq!(retained, b"retained installation");
    assert_eq!(
        entries, 1,
        "only the retained installation survives dropping the download"
    );
    assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
}
