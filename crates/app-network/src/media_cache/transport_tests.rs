use super::*;

#[tokio::test]
async fn incomplete_or_cancelled_bodies_are_not_published() {
    let root = Directory::new();
    let gate = Arc::new(tokio::sync::Notify::new());
    let normal = Arc::new(AtomicBool::new(false));
    let server = Server::new({
        let gate = Arc::clone(&gate);
        let normal = Arc::clone(&normal);
        move |request| {
            let mut reply = Wire::image();
            if !normal.load(Ordering::Relaxed) {
                if request.path == "/truncated" {
                    reply.declared = Some(reply.body.len() + 10);
                } else {
                    reply.gate = Some(Arc::clone(&gate));
                }
            }
            reply
        }
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    assert!(
        image(&cache, &format!("{}/truncated", server.base))
            .await
            .is_err()
    );
    assert!(root.blobs().is_empty());
    let pending = tokio::spawn({
        let cache = cache.clone();
        let url = format!("{}/cancel", server.base);
        async move { image(&cache, &url).await }
    });
    server.wait_for(2).await;
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert!(root.blobs().is_empty());
    normal.store(true, Ordering::Relaxed);
    gate.notify_one();
    assert_eq!(
        image(&cache, &format!("{}/cancel", server.base))
            .await
            .unwrap()
            .body,
        b"fixture-image"
    );
}

#[tokio::test]
async fn terminal_http_status_does_not_fallback_or_serve_stale() {
    for status in [401, 403, 429] {
        let root = Directory::new();
        let enabled = Arc::new(AtomicBool::new(false));
        let primary = Server::new({
            let enabled = Arc::clone(&enabled);
            move |_| {
                let mut reply = Wire::image();
                if enabled.load(Ordering::Relaxed) {
                    reply.status = status;
                    reply.headers.push(("Retry-After".into(), "60".into()));
                }
                reply
            }
        })
        .await;
        let alternative =
            Server::new(|_| panic!("terminal status must not try another origin")).await;
        let clock = Arc::new(AtomicU64::new(100));
        let cache = cache(
            &root,
            resolver(vec![primary.base.clone(), alternative.base.clone()]),
            &clock,
        );
        let url = format!("{}/image", primary.base);
        image(&cache, &url).await.unwrap();
        enabled.store(true, Ordering::Relaxed);
        clock.store(111, Ordering::Relaxed);
        let error = image(&cache, &url).await.unwrap_err();
        assert_eq!(error.status_code(), status);
        assert_eq!(error.retry_after(), Some("60"));
        assert_eq!(alternative.count(), 0);
    }
}

#[tokio::test]
async fn redirects_and_unmarked_roots_cannot_expand_cache_authority() {
    let root = Directory::new();
    let target = Server::new(|_| panic!("unlisted redirect must not be contacted")).await;
    let server = Server::new({
        let target = target.base.clone();
        move |_| {
            let mut reply = Wire::image();
            reply.status = 302;
            reply
                .headers
                .push(("Location".into(), format!("{target}/private")));
            reply
        }
    })
    .await;
    let clock = Arc::new(AtomicU64::new(100));
    let cache = cache(&root, resolver(vec![server.base.clone()]), &clock);
    assert_eq!(
        image(&cache, &format!("{}/image", server.base))
            .await
            .unwrap_err()
            .status_code(),
        403
    );
    assert_eq!(target.count(), 0);
    let unsafe_root = Directory::new();
    std::fs::create_dir_all(&unsafe_root.0).unwrap();
    std::fs::write(unsafe_root.0.join("user-file"), b"preserve").unwrap();
    assert!(store::Store::open(unsafe_root.0.clone(), 1024, 2).is_err());
    assert_eq!(
        std::fs::read(unsafe_root.0.join("user-file")).unwrap(),
        b"preserve"
    );
}
