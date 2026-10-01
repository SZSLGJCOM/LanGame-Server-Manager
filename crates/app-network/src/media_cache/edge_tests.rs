use super::*;

#[tokio::test]
async fn four_pending_network_downloads_do_not_delay_a_warm_cache_hit() {
    let root = Directory::new();
    let gate = Arc::new(tokio::sync::Notify::new());
    let server = Server::new({
        let gate = Arc::clone(&gate);
        move |request| {
            let mut reply = Wire::image();
            if request.path != "/warm" {
                reply.gate = Some(Arc::clone(&gate));
            }
            reply
        }
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let warm = format!("{}/warm", server.base);
    image(&cache, &warm).await.unwrap();
    let mut pending = Vec::new();
    for index in 0..4 {
        let cache = cache.clone();
        let url = format!("{}/blocked-{index}", server.base);
        pending.push(tokio::spawn(async move { image(&cache, &url).await }));
    }
    server.wait_for(5).await;
    let hit = tokio::time::timeout(Duration::from_secs(2), image(&cache, &warm))
        .await
        .expect("disk hits must not wait for upstream permits")
        .unwrap();
    assert_eq!(header(&hit, "X-LanGame-Media-Cache"), Some("hit"));
    assert_eq!(server.count(), 5);
    for request in pending {
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn weak_or_missing_etag_cannot_use_last_modified_to_join_range_blocks() {
    for tag in [None, Some("W/\"same-second\"")] {
        let root = Directory::new();
        let server = Server::new(move |request| {
            let mut reply = Wire::video(request, &vec![1; BLOCK_SIZE as usize * 2], 1);
            reply.headers.retain(|(name, _)| name != "ETag");
            if let Some(tag) = tag {
                reply.headers.push(("ETag".into(), tag.into()));
            }
            reply.headers.push((
                "Last-Modified".into(),
                "Thu, 01 Jan 1970 00:00:00 GMT".into(),
            ));
            reply
        })
        .await;
        let clock = Arc::new(AtomicU64::new(100));
        let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
        assert!(matches!(
            video(&cache, &format!("{}/video", server.base), "bytes=0-").await,
            Err(MediaCacheError::InvalidResponse(_))
        ));
        assert_eq!(server.count(), 1);
        assert!(root.blobs().is_empty());
    }
}

#[tokio::test]
async fn cold_suffix_fetches_only_metadata_probe_and_the_required_complete_block() {
    let root = Directory::new();
    let data = vec![4; BLOCK_SIZE as usize * 3 + 17];
    let server = Server::new(move |request| Wire::video(request, &data, 1)).await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let reply = video(&cache, &format!("{}/video", server.base), "bytes=-10")
        .await
        .unwrap();
    assert_eq!(reply.body, vec![4; 10]);
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].headers["range"], "bytes=0-0");
    assert_eq!(
        requests[1].headers["range"],
        format!("bytes={}-{}", BLOCK_SIZE * 3, BLOCK_SIZE * 3 + 16)
    );
    assert_eq!(root.blobs().len(), 1);
}

#[tokio::test]
async fn oversized_whole_video_is_an_error_and_does_not_publish_a_short_200() {
    let root = Directory::new();
    let server = Server::new(|_| {
        let mut wire = Wire::image();
        wire.headers[0].1 = "video/mp4".into();
        wire.declared = Some(OBJECT_LIMIT + 1);
        wire
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let response = cache
        .respond(
            &format!("{}/video", server.base),
            MediaKind::Video,
            SourcePreference::InternationalFirst,
            None,
            false,
        )
        .await;
    assert_eq!(response.unwrap_err().status_code(), 413);
    assert!(root.blobs().is_empty());
}

#[tokio::test]
async fn range_expiry_uses_304_then_complete_old_blocks_can_be_served_offline() {
    let root = Directory::new();
    let server =
        Server::new(|request| Wire::video(request, &vec![9; BLOCK_SIZE as usize * 2], 1)).await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let url = format!("{}/video", server.base);
    video(&cache, &url, "bytes=0-").await.unwrap();
    clock.store(111, Ordering::Relaxed);
    assert_eq!(
        video(&cache, &url, "bytes=0-").await.unwrap().body,
        vec![9; BLOCK_SIZE as usize]
    );
    assert_eq!(server.count(), 2);
    assert_eq!(
        server.requests.lock().unwrap()[1].headers["if-none-match"],
        "\"version-1\""
    );
    clock.store(122, Ordering::Relaxed);
    drop(server);
    tokio::task::yield_now().await;
    let stale = video(&cache, &url, "bytes=0-").await.unwrap();
    assert_eq!(header(&stale, "X-LanGame-Media-Cache"), Some("stale"));
    assert!(
        video(&cache, &url, &format!("bytes={}-", BLOCK_SIZE))
            .await
            .is_err()
    );
    assert!(root.blobs().is_empty());
}

#[tokio::test]
async fn truncated_block_drops_generation_and_a_new_request_can_restart() {
    let root = Directory::new();
    let broken = Arc::new(AtomicBool::new(true));
    let server = Server::new({
        let broken = Arc::clone(&broken);
        move |request| {
            let mut wire = Wire::video(request, &vec![1; BLOCK_SIZE as usize * 2], 1);
            if broken.load(Ordering::Relaxed) && request.headers.contains_key("if-range") {
                wire.declared = Some(wire.body.len());
                wire.body.truncate(10);
            }
            wire
        }
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let url = format!("{}/video", server.base);
    video(&cache, &url, "bytes=0-").await.unwrap();
    assert!(
        video(&cache, &url, &format!("bytes={}-", BLOCK_SIZE))
            .await
            .is_err()
    );
    assert!(root.blobs().is_empty());
    broken.store(false, Ordering::Relaxed);
    video(&cache, &url, "bytes=0-").await.unwrap();
    assert_eq!(root.blobs().len(), 1);
}

#[tokio::test]
async fn body_failure_can_retry_an_equivalent_source_before_a_generation_is_pinned() {
    let root = Directory::new();
    let failed = Server::new(|_| {
        let mut wire = Wire::image();
        wire.declared = Some(99);
        wire
    })
    .await;
    let alternative = Server::new(|_| Wire::image()).await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(
        &root,
        resolver(vec![failed.base.clone(), alternative.base.clone()]),
        &clock,
    );
    let result = image(&cache, &format!("{}/image", failed.base))
        .await
        .unwrap();
    assert_eq!(result.final_url, format!("{}/image", alternative.base));
    assert_eq!(result.body, b"fixture-image");
    assert_eq!(failed.count(), 1);
    assert_eq!(alternative.count(), 1);
}

#[tokio::test]
async fn frequent_hits_update_memory_lru_without_rewriting_the_durable_index() {
    let root = Directory::new();
    let server = Server::new(|_| Wire::image()).await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let url = format!("{}/image", server.base);
    image(&cache, &url).await.unwrap();
    let before = std::fs::read(root.0.join("index.json")).unwrap();
    clock.store(105, Ordering::Relaxed);
    image(&cache, &url).await.unwrap();
    assert_eq!(std::fs::read(root.0.join("index.json")).unwrap(), before);
    assert_eq!(server.count(), 1);
}

#[tokio::test]
async fn no_store_native_range_rejects_initial_and_changed_cache_policy() {
    for initially_no_store in [true, false] {
        let root = Directory::new();
        let no_store = Arc::new(AtomicBool::new(initially_no_store));
        let server = Server::new({
            let no_store = Arc::clone(&no_store);
            move |request| {
                let mut wire = Wire::video(request, &vec![3; BLOCK_SIZE as usize * 2], 1);
                if no_store.load(Ordering::Relaxed) {
                    wire.headers[1].1 = "no-store".into();
                }
                wire
            }
        })
        .await;
        let clock = Arc::new(AtomicU64::new(100));
        let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
        let url = format!("{}/video", server.base);
        if !initially_no_store {
            video(&cache, &url, "bytes=0-").await.unwrap();
            no_store.store(true, Ordering::Relaxed);
        }
        let range = if initially_no_store {
            "bytes=0-".into()
        } else {
            format!("bytes={}-", BLOCK_SIZE)
        };
        let error = video(&cache, &url, &range).await.unwrap_err();
        assert!(matches!(error, MediaCacheError::InvalidResponse(_)));
        assert!(root.blobs().is_empty());
    }
}

#[tokio::test]
async fn repeated_cache_control_fields_cannot_hide_no_store() {
    let root = Directory::new();
    let server = Server::new(|_| {
        let mut wire = Wire::image();
        wire.headers
            .push(("Cache-Control".into(), "no-store".into()));
        wire
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let url = format!("{}/image", server.base);
    image(&cache, &url).await.unwrap();
    image(&cache, &url).await.unwrap();
    assert_eq!(server.count(), 2);
    assert!(root.blobs().is_empty());
}
