use super::*;

#[tokio::test]
async fn restart_and_opposite_locale_alias_hit_without_network_and_versions_do_not_collide() {
    let root = Directory::new();
    let origin = Server::new(|_| Wire::image()).await;
    let alias =
        Server::new(|_| panic!("equivalent alias must not be fetched on a cache hit")).await;
    let resolve = resolver(vec![origin.base.clone(), alias.base.clone()]);
    let clock = Arc::new(AtomicU64::new(100));
    let first = cache(&root, Arc::clone(&resolve), &clock);
    let url = format!("{}/image?t=1", origin.base);
    assert_eq!(image(&first, &url).await.unwrap().body, b"fixture-image");
    drop(first);
    let restarted = cache(&root, resolve, &clock);
    let response = restarted
        .respond(
            &format!("{}/image?t=1", alias.base),
            MediaKind::Image,
            SourcePreference::ChinaFirst,
            None,
            false,
        )
        .await
        .unwrap();
    assert_eq!(header(&response, "X-LanGame-Media-Cache"), Some("hit"));
    assert_eq!(response.final_url, url);
    assert_eq!(origin.count(), 1);
    assert_eq!(alias.count(), 0);
    image(&restarted, &format!("{}/image?t=2", origin.base))
        .await
        .unwrap();
    assert_eq!(origin.count(), 2);
    assert_eq!(root.blobs().len(), 2);
}

#[tokio::test]
async fn expiry_304_refreshes_metadata_without_replacing_body() {
    let root = Directory::new();
    let server = Server::new(|request| {
        let mut reply = Wire::image();
        if request.headers.contains_key("if-none-match") {
            assert_eq!(request.headers["if-none-match"], "\"one\"");
            reply.status = 304;
            reply.body.clear();
        }
        reply
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let url = format!("{}/image", server.base);
    image(&cache, &url).await.unwrap();
    let blob = root.blobs().pop().unwrap();
    clock.store(111, Ordering::Relaxed);
    let response = image(&cache, &url).await.unwrap();
    assert_eq!(response.body, b"fixture-image");
    assert_eq!(server.count(), 2);
    assert_eq!(root.blobs(), vec![blob]);
    image(&cache, &url).await.unwrap();
    assert_eq!(server.count(), 2);
}

#[tokio::test]
async fn expired_complete_media_uses_current_locale_before_the_cached_origin() {
    let root = Directory::new();
    let international = Server::new(|_| Wire::image()).await;
    let domestic = Server::new(|request| {
        assert!(!request.headers.contains_key("if-none-match"));
        assert!(!request.headers.contains_key("if-modified-since"));
        let mut reply = Wire::image();
        reply.body = b"domestic-image".to_vec();
        reply.headers[2].1 = "\"domestic\"".into();
        reply
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(
        &root,
        resolver(vec![international.base.clone(), domestic.base.clone()]),
        &clock,
    );
    let url = format!("{}/image", international.base);
    image(&cache, &url).await.unwrap();
    clock.store(111, Ordering::Relaxed);

    let response = cache
        .respond(
            &url,
            MediaKind::Image,
            SourcePreference::ChinaFirst,
            None,
            false,
        )
        .await
        .unwrap();
    assert_eq!(response.body, b"domestic-image");
    assert_eq!(response.final_url, format!("{}/image", domestic.base));
    assert_eq!(international.count(), 1);
    assert_eq!(domestic.count(), 1);
}

#[tokio::test]
async fn expired_complete_media_revalidates_old_origin_only_after_preferred_source_fails() {
    let root = Directory::new();
    let international = Server::new(|request| {
        let mut reply = Wire::image();
        if let Some(tag) = request.headers.get("if-none-match") {
            assert_eq!(tag, "\"one\"");
            reply.status = 304;
            reply.body.clear();
        }
        reply
    })
    .await;
    let domestic = Server::new(|request| {
        assert!(!request.headers.contains_key("if-none-match"));
        let mut reply = Wire::image();
        reply.status = 503;
        reply
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(
        &root,
        resolver(vec![international.base.clone(), domestic.base.clone()]),
        &clock,
    );
    let url = format!("{}/image", international.base);
    image(&cache, &url).await.unwrap();
    clock.store(111, Ordering::Relaxed);

    let response = cache
        .respond(
            &url,
            MediaKind::Image,
            SourcePreference::ChinaFirst,
            None,
            false,
        )
        .await
        .unwrap();
    assert_eq!(response.body, b"fixture-image");
    assert_eq!(header(&response, "X-LanGame-Media-Cache"), Some("hit"));
    assert_eq!(domestic.count(), 1);
    assert_eq!(international.count(), 2);
}

#[tokio::test]
async fn expired_complete_content_is_available_offline_but_no_cache_requires_validation() {
    for no_cache in [false, true] {
        let root = Directory::new();
        let server = Server::new(move |_| {
            let mut reply = Wire::image();
            if no_cache {
                reply.headers[1].1 = "no-cache".into();
            }
            reply
        })
        .await;
        let clock = Arc::new(AtomicU64::new(100));
        let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
        let url = format!("{}/image", server.base);
        image(&cache, &url).await.unwrap();
        clock.store(111, Ordering::Relaxed);
        drop(server);
        tokio::task::yield_now().await;
        let response = image(&cache, &url).await;
        if no_cache {
            assert!(response.is_err());
        } else {
            let response = response.unwrap();
            assert_eq!(header(&response, "X-LanGame-Media-Cache"), Some("stale"));
            assert_eq!(response.body, b"fixture-image");
        }
    }
}

#[tokio::test]
async fn simultaneous_same_key_requests_share_one_fetch() {
    let root = Directory::new();
    let gate = Arc::new(tokio::sync::Notify::new());
    let server = Server::new({
        let gate = Arc::clone(&gate);
        move |_| {
            let mut reply = Wire::image();
            reply.gate = Some(Arc::clone(&gate));
            reply
        }
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    let mut requests = Vec::new();
    for _ in 0..8 {
        let cache = cache.clone();
        let url = format!("{}/image", server.base);
        requests.push(tokio::spawn(
            async move { image(&cache, &url).await.unwrap() },
        ));
    }
    server.wait_for(1).await;
    gate.notify_one();
    for request in requests {
        assert_eq!(request.await.unwrap().body, b"fixture-image");
    }
    assert_eq!(server.count(), 1);
}

#[tokio::test]
async fn no_store_never_persists_and_corrupt_blobs_are_refetched() {
    for no_store in [true, false] {
        let root = Directory::new();
        let server = Server::new(move |_| {
            let mut reply = Wire::image();
            if no_store {
                reply.headers[1].1 = "no-store".into();
            }
            reply
        })
        .await;
        let clock = Arc::new(AtomicU64::new(100));
        let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
        let url = format!("{}/image", server.base);
        image(&cache, &url).await.unwrap();
        if no_store {
            assert!(root.blobs().is_empty());
        } else {
            std::fs::write(root.blobs().pop().unwrap(), b"corrupt-image").unwrap();
        }
        assert_eq!(image(&cache, &url).await.unwrap().body, b"fixture-image");
        assert_eq!(server.count(), 2);
    }
}

#[tokio::test]
async fn lru_and_entry_limits_evict_only_owned_cache_files() {
    for (disk_limit, entry_limit) in [(26, 100), (1024, 2)] {
        let root = Directory::new();
        let server = Server::new(|_| Wire::image()).await;
        let clock = Arc::new(AtomicU64::new(100));
        let cache = test_cache(
            root.0.clone(),
            resolver(vec![server.base.clone()]),
            {
                let clock = Arc::clone(&clock);
                Arc::new(move || clock.load(Ordering::Relaxed))
            },
            disk_limit,
            entry_limit,
        );
        for path in ["a", "b"] {
            image(&cache, &format!("{}/{path}", server.base))
                .await
                .unwrap();
            clock.fetch_add(1, Ordering::Relaxed);
        }
        image(&cache, &format!("{}/a", server.base)).await.unwrap();
        clock.fetch_add(1, Ordering::Relaxed);
        std::fs::write(root.0.join("user-note.txt"), b"preserve").unwrap();
        image(&cache, &format!("{}/c", server.base)).await.unwrap();
        assert_eq!(root.blobs().len(), 2);
        image(&cache, &format!("{}/a", server.base)).await.unwrap();
        assert_eq!(server.count(), 3);
        image(&cache, &format!("{}/b", server.base)).await.unwrap();
        assert_eq!(server.count(), 4);
        assert_eq!(
            std::fs::read(root.0.join("user-note.txt")).unwrap(),
            b"preserve"
        );
    }
}
