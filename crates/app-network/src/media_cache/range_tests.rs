use super::*;

#[tokio::test]
async fn native_ranges_cache_aligned_blocks_seek_repeat_suffix_and_head() {
    let root = Directory::new();
    let data = Arc::new(
        (0..3 * BLOCK_SIZE as usize + 37)
            .map(|offset| (offset % 251) as u8)
            .collect::<Vec<_>>(),
    );
    let server = Server::new({
        let data = Arc::clone(&data);
        move |request| Wire::video(request, &data, 1)
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let resolve = resolver(vec![server.base.clone()]);
    let cache = cache(&root, Arc::clone(&resolve), &clock);
    let url = format!("{}/video", server.base);
    let first = video(&cache, &url, "bytes=0-").await.unwrap();
    assert_eq!(first.status, 206);
    assert_eq!(first.body, data[..BLOCK_SIZE as usize]);
    assert_eq!(
        header(&first, "Content-Range"),
        Some(format!("bytes 0-{}/{}", BLOCK_SIZE - 1, data.len()).as_str())
    );
    assert_eq!(server.count(), 1);
    let seek = video(&cache, &url, &format!("bytes={}-", BLOCK_SIZE * 2 + 5))
        .await
        .unwrap();
    assert_eq!(
        seek.body,
        data[(BLOCK_SIZE * 2 + 5) as usize..(BLOCK_SIZE * 3 + 5) as usize]
    );
    assert_eq!(server.count(), 3);
    for request in server.requests.lock().unwrap().iter().skip(1) {
        assert_eq!(
            request.headers.get("if-range").map(String::as_str),
            Some("\"version-1\"")
        );
    }
    video(&cache, &url, "bytes=0-10").await.unwrap();
    assert_eq!(
        video(&cache, &url, "bytes=-16").await.unwrap().body,
        data[data.len() - 16..]
    );
    assert_eq!(server.count(), 3);
    let head = cache
        .respond(
            &url,
            MediaKind::Video,
            SourcePreference::ChinaFirst,
            Some("bytes=0-"),
            true,
        )
        .await
        .unwrap();
    assert!(head.body.is_empty());
    assert_eq!(
        header(&head, "Content-Length"),
        Some(BLOCK_SIZE.to_string().as_str())
    );
    let invalid = video(&cache, &url, &format!("bytes={}-", data.len()))
        .await
        .unwrap();
    assert_eq!(invalid.status, 416);
    assert_eq!(
        header(&invalid, "Content-Range"),
        Some(format!("bytes */{}", data.len()).as_str())
    );
    assert_eq!(server.count(), 3);
    drop(cache);
    let restarted = fixtures::cache(&root, resolve, &clock);
    video(&restarted, &url, "bytes=0-").await.unwrap();
    assert_eq!(server.count(), 3);
}

#[tokio::test]
async fn hls_explicit_range_is_complete_and_whole_segments_share_cache() {
    let root = Directory::new();
    let data = vec![7; BLOCK_SIZE as usize * 3];
    let server = Server::new(move |request| Wire::video(request, &data, 1)).await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let url = format!("{}/segment.m4s", server.base);
    let response = cache
        .respond(
            &url,
            MediaKind::Hls,
            SourcePreference::InternationalFirst,
            Some(&format!("bytes=10-{}", BLOCK_SIZE * 2 + 9)),
            false,
        )
        .await
        .unwrap();
    assert_eq!(response.body.len(), BLOCK_SIZE as usize * 2);
    assert_eq!(server.count(), 3);
    let whole = cache
        .respond(
            &format!("{}/whole.m4s", server.base),
            MediaKind::Hls,
            SourcePreference::InternationalFirst,
            None,
            false,
        )
        .await
        .unwrap();
    assert_eq!(whole.body.len(), BLOCK_SIZE as usize * 3);
    let range = video(&cache, &format!("{}/whole.m4s", server.base), "bytes=12-99")
        .await
        .unwrap();
    assert_eq!(range.body, vec![7; 88]);
    assert_eq!(server.count(), 4);
}

#[tokio::test]
async fn changed_if_range_invalidates_all_prior_blocks_instead_of_joining_generations() {
    let root = Directory::new();
    let version = Arc::new(AtomicU64::new(1));
    let server = Server::new({
        let version = Arc::clone(&version);
        move |request| {
            let value = version.load(Ordering::Relaxed);
            Wire::video(request, &vec![value as u8; BLOCK_SIZE as usize * 2], value)
        }
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let url = format!("{}/movie", server.base);
    video(&cache, &url, "bytes=0-").await.unwrap();
    version.store(2, Ordering::Relaxed);
    let error = video(&cache, &url, &format!("bytes={}-", BLOCK_SIZE))
        .await
        .unwrap_err();
    assert_eq!(error.status_code(), 409);
    assert!(root.blobs().is_empty());
    assert_eq!(
        video(&cache, &url, "bytes=0-").await.unwrap().body,
        vec![2; BLOCK_SIZE as usize]
    );
}

#[tokio::test]
async fn range_health_tracks_upstream_failures_without_reviving_sources_on_disk_hits() {
    // All HTTPS traffic ends at this loopback proxy. Rejecting CONNECT produces
    // a real transport failure for an official URL without reaching the internet.
    let proxy = Server::new(|request| {
        assert_eq!(request.method, "CONNECT");
        let mut reply = Wire::image();
        reply.status = 502;
        reply.body.clear();
        reply
    })
    .await;
    let alternative =
        Server::new(|request| Wire::video(request, &vec![7; BLOCK_SIZE as usize * 2], 1)).await;
    let root = Directory::new();
    let clock = Arc::new(AtomicU64::new(100));
    let official_origin = "https://video.akamai.steamstatic.com";
    let path = "/store_trailers/range-health-test/movie.mp4";
    let official = format!("{official_origin}{path}");
    let local_origin = alternative.base.clone();
    let resolve: Arc<Resolver> = Arc::new(move |input, _, _| {
        let url = reqwest::Url::parse(input).ok()?;
        let origin = url.origin().ascii_serialization();
        if origin != official_origin && origin != local_origin {
            return None;
        }
        Some((
            url.path().to_string(),
            vec![
                format!("{official_origin}{}", url.path()),
                format!("{local_origin}{}", url.path()),
            ],
        ))
    });
    let mut cache = cache(&root, resolve, &clock);
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::https(&proxy.base).unwrap())
        .build()
        .unwrap();
    let inner = Arc::get_mut(&mut cache.inner).unwrap();
    inner.client = OnceCell::new();
    inner.client.set(client).unwrap();
    let cooled = || {
        official_url_candidates(&official, SourcePreference::InternationalFirst).last()
            == Some(&official)
    };

    crate::record_success(&official);
    assert!(!cooled());
    let response = video(&cache, &official, "bytes=0-").await.unwrap();
    assert_eq!(response.body, vec![7; BLOCK_SIZE as usize]);
    assert_eq!(proxy.count(), 1);
    assert_eq!(alternative.count(), 1);
    assert!(cooled(), "the failed official source must enter cooldown");

    let entry = meta::Entry::new(
        official.clone(),
        meta::Headers {
            content_type: "video/mp4".into(),
            etag: Some("\"version-1\"".into()),
            cache_control: Some("max-age=10".into()),
            ..Default::default()
        },
        BLOCK_SIZE * 2,
        100,
    );
    let key = store::digest(path.as_bytes());
    let store = cache.inner.store.get().unwrap();
    store
        .put(
            &key,
            entry.clone(),
            vec![(Some(0), vec![7; BLOCK_SIZE as usize])],
        )
        .await
        .unwrap();
    let hit = video(&cache, &official, "bytes=0-10").await.unwrap();
    assert_eq!(hit.body, vec![7; 11]);
    assert_eq!(header(&hit, "X-LanGame-Media-Cache"), Some("hit"));
    assert_eq!(proxy.count(), 1);
    assert_eq!(alternative.count(), 1);
    assert!(cooled(), "a disk hit must not clear a source's cooldown");

    crate::record_success(&official);
    let missing = video(&cache, &official, &format!("bytes={}-", BLOCK_SIZE))
        .await
        .unwrap_err();
    assert!(matches!(missing, MediaCacheError::Network(_)));
    assert!(cooled(), "a failed pinned block must enter cooldown");
    assert_eq!(proxy.count(), 2);
    assert_eq!(
        alternative.count(),
        1,
        "pinned blocks cannot switch origins"
    );
    assert!(root.blobs().is_empty());

    store
        .put(&key, entry, vec![(Some(0), vec![7; BLOCK_SIZE as usize])])
        .await
        .unwrap();
    crate::record_success(&official);
    clock.store(111, Ordering::Relaxed);
    let stale = video(&cache, &official, "bytes=0-10").await.unwrap();
    assert_eq!(stale.body, vec![7; 11]);
    assert_eq!(header(&stale, "X-LanGame-Media-Cache"), Some("stale"));
    assert!(
        cooled(),
        "offline cached content does not prove source health"
    );
    assert_eq!(proxy.count(), 3);
    assert_eq!(alternative.count(), 1);
}
