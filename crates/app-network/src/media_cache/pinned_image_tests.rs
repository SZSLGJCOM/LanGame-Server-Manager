use super::*;

fn pinned_cache(root: &Directory, sources: Vec<String>, digest: Option<String>) -> MediaCache {
    image_cache(root, sources, digest, false)
}

fn image_cache(
    root: &Directory,
    sources: Vec<String>,
    digest: Option<String>,
    immutable: bool,
) -> MediaCache {
    test_cache(
        root.0.clone(),
        Arc::new(move |input, _, _| {
            if !sources.iter().any(|source| source == input) {
                return None;
            }
            Some(ResolvedMedia {
                identity: format!(
                    "fixture-image:{}:{immutable}",
                    digest.as_deref().unwrap_or("unpinned")
                ),
                sources: sources.clone(),
                image_sha256: digest.clone(),
                cache_immutable: immutable,
            })
        }),
        Arc::new(|| 100),
        32 * 1024 * 1024,
        100,
    )
}

#[tokio::test]
async fn immutable_image_download_reopens_without_http_revalidation() {
    // GitCode's public frozen-commit API returns these headers even for the
    // project-owned artwork. Persistence is an explicit resource-policy choice.
    let root = Directory::new();
    let origin = Server::new(|_| {
        let mut reply = Wire::image();
        reply
            .headers
            .retain(|(key, _)| !key.eq_ignore_ascii_case("cache-control"));
        reply.headers.push((
            "Cache-Control".into(),
            "no-store, must-revalidate, no-cache".into(),
        ));
        reply
    })
    .await;
    let url = format!("{}/frozen-commit/image", origin.base);
    let digest = Some(store::digest(b"fixture-image"));
    let cache = image_cache(&root, vec![url.clone()], digest.clone(), true);
    let first = image(&cache, &url).await.unwrap();
    assert_eq!(first.body, b"fixture-image");
    assert_eq!(root.blobs().len(), 1);
    assert_eq!(
        header(&first, "Cache-Control"),
        Some("no-store, must-revalidate, no-cache")
    );
    drop(cache);
    let reopened = image_cache(&root, vec![url.clone()], digest, true);
    let hit = image(&reopened, &url).await.unwrap();
    assert_eq!(header(&hit, "X-LanGame-Media-Cache"), Some("hit"));
    assert_eq!(hit.body, first.body);
    assert_eq!(
        origin.count(),
        1,
        "an immutable resource needs no HTTP revalidation"
    );
}

#[tokio::test]
async fn pinned_http_images_still_obey_no_store_without_immutable_opt_in() {
    let root = Directory::new();
    let origin = Server::new(|_| {
        let mut reply = Wire::image();
        reply
            .headers
            .push(("Cache-Control".into(), "no-store".into()));
        reply
    })
    .await;
    let url = format!("{}/image", origin.base);
    let cache = pinned_cache(
        &root,
        vec![url.clone()],
        Some(store::digest(b"fixture-image")),
    );
    for _ in 0..2 {
        assert_eq!(image(&cache, &url).await.unwrap().body, b"fixture-image");
    }
    assert!(root.blobs().is_empty());
    assert_eq!(origin.count(), 2);
}

#[tokio::test]
async fn immutable_image_head_stores_verified_bytes_for_a_reopened_get() {
    let root = Directory::new();
    let origin = Server::new(|request| {
        assert_eq!(request.method, "GET");
        Wire::image()
    })
    .await;
    let url = format!("{}/image", origin.base);
    let digest = Some(store::digest(b"fixture-image"));
    let cache = image_cache(&root, vec![url.clone()], digest.clone(), true);
    let head = cache
        .respond(
            &url,
            MediaKind::Image,
            SourcePreference::ChinaFirst,
            None,
            true,
        )
        .await
        .unwrap();
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_eq!(root.blobs().len(), 1);
    drop(cache);
    let reopened = image_cache(&root, vec![url.clone()], digest, true);
    let get = image(&reopened, &url).await.unwrap();
    assert_eq!(get.body, b"fixture-image");
    assert_eq!(header(&get, "X-LanGame-Media-Cache"), Some("hit"));
    assert_eq!(origin.count(), 1);
}

#[tokio::test]
async fn pinned_image_rejects_disk_bytes_that_only_match_their_own_metadata() {
    let root = Directory::new();
    let origin = Server::new(|_| Wire::image()).await;
    let url = format!("{}/image", origin.base);
    let digest = Some(store::digest(b"fixture-image"));
    let cache = image_cache(&root, vec![url.clone()], digest.clone(), true);
    image(&cache, &url).await.unwrap();
    drop(cache);
    let path = root.0.join("index.json");
    let mut index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let entry = index["entries"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    let wrong = b"fixture-wrong";
    std::fs::write(root.0.join(entry["full"]["file"].as_str().unwrap()), wrong).unwrap();
    entry["full"]["sha256"] = store::digest(wrong).into();
    entry["full"]["size"] = wrong.len().into();
    std::fs::write(path, serde_json::to_vec(&index).unwrap()).unwrap();
    let reopened = image_cache(&root, vec![url.clone()], digest, true);
    let response = image(&reopened, &url).await.unwrap();
    assert_eq!(response.body, b"fixture-image");
    assert_eq!(header(&response, "X-LanGame-Media-Cache"), Some("miss"));
    assert_eq!(origin.count(), 2);
}

#[tokio::test]
async fn withdrawing_immutable_storage_cannot_reuse_its_prior_disk_entry() {
    let root = Directory::new();
    let origin = Server::new(|_| {
        let mut reply = Wire::image();
        reply
            .headers
            .push(("Cache-Control".into(), "no-store".into()));
        reply
    })
    .await;
    let url = format!("{}/image", origin.base);
    let digest = Some(store::digest(b"fixture-image"));
    let asset = image_cache(&root, vec![url.clone()], digest.clone(), true);
    image(&asset, &url).await.unwrap();
    drop(asset);
    let http = pinned_cache(&root, vec![url.clone()], digest);
    for _ in 0..2 {
        assert_eq!(
            header(&image(&http, &url).await.unwrap(), "X-LanGame-Media-Cache"),
            Some("miss")
        );
    }
    assert_eq!(origin.count(), 3);
    assert_eq!(
        root.blobs().len(),
        1,
        "the old asset stays isolated until ordinary LRU eviction"
    );
}

#[tokio::test]
async fn pinned_image_wrong_digest_is_rejected_before_cache_publication() {
    let root = Directory::new();
    let origin = Server::new(|_| Wire::image()).await;
    let url = format!("{}/image", origin.base);
    let cache = pinned_cache(
        &root,
        vec![url.clone()],
        Some(store::digest(b"different bytes")),
    );
    assert!(matches!(
        image(&cache, &url).await,
        Err(MediaCacheError::InvalidResponse(_))
    ));
    assert!(
        root.blobs().is_empty(),
        "mismatched bytes must never be published"
    );
}

#[tokio::test]
async fn pinned_image_bad_source_falls_back_and_reopen_hits_verified_bytes() {
    let root = Directory::new();
    let primary = Server::new(|_| {
        let mut reply = Wire::image();
        reply.body = b"corrupt".to_vec();
        reply
    })
    .await;
    let alternate = Server::new(|_| Wire::image()).await;
    let sources = vec![
        format!("{}/version/image.jpg", primary.base),
        format!("{}/api/raw/123", alternate.base),
    ];
    let digest = Some(store::digest(b"fixture-image"));
    let cache = pinned_cache(&root, sources.clone(), digest.clone());
    let response = image(&cache, &sources[0]).await.unwrap();
    assert_eq!(response.body, b"fixture-image");
    assert_eq!(response.final_url, sources[1]);
    assert_eq!(primary.count(), 1);
    assert_eq!(alternate.count(), 1);
    assert_eq!(root.blobs().len(), 1);
    drop(cache);
    let reopened = pinned_cache(&root, sources.clone(), digest);
    let hit = image(&reopened, &sources[1]).await.unwrap();
    assert_eq!(hit.body, b"fixture-image");
    assert_eq!(header(&hit, "X-LanGame-Media-Cache"), Some("hit"));
    assert_eq!(primary.count(), 1);
    assert_eq!(alternate.count(), 1);
}

#[tokio::test]
async fn pinned_image_403_uses_hash_verified_octet_stream_alternative() {
    let root = Directory::new();
    let primary = Server::new(|_| {
        let mut reply = Wire::image();
        reply.status = 403;
        reply
    })
    .await;
    let alternate = Server::new(|_| {
        let mut reply = Wire::image();
        reply.headers[0].1 = "application/octet-stream".into();
        reply
    })
    .await;
    let sources = vec![
        format!("{}/image", primary.base),
        format!("{}/raw", alternate.base),
    ];
    let cache = pinned_cache(
        &root,
        sources.clone(),
        Some(store::digest(b"fixture-image")),
    );
    let response = image(&cache, &sources[0]).await.unwrap();
    assert_eq!(response.body, b"fixture-image");
    assert_eq!(response.final_url, sources[1]);
    assert_eq!(primary.count(), 1);
    assert_eq!(alternate.count(), 1);
}

#[tokio::test]
async fn pinned_image_head_checks_bytes_before_reporting_success() {
    let root = Directory::new();
    let origin = Server::new(|request| {
        assert_eq!(request.method, "GET");
        Wire::image()
    })
    .await;
    let url = format!("{}/image", origin.base);
    let cache = pinned_cache(
        &root,
        vec![url.clone()],
        Some(store::digest(b"different bytes")),
    );
    assert!(matches!(
        cache
            .respond(
                &url,
                MediaKind::Image,
                SourcePreference::InternationalFirst,
                None,
                true
            )
            .await,
        Err(MediaCacheError::InvalidResponse(_))
    ));
    assert!(root.blobs().is_empty());
}

#[tokio::test]
async fn pinned_image_exception_preserves_unpinned_and_video_http_boundaries() {
    for (kind, pinned, status, retry_after) in [
        (MediaKind::Image, true, 401, None),
        (MediaKind::Image, true, 429, None),
        (MediaKind::Image, true, 403, Some("60")),
        (MediaKind::Image, true, 404, Some("60")),
        (MediaKind::Image, false, 403, None),
        (MediaKind::Video, true, 403, None),
        (MediaKind::Hls, true, 403, None),
        (MediaKind::Video, true, 429, None),
    ] {
        let root = Directory::new();
        let primary = Server::new(move |_| {
            let mut reply = Wire::image();
            reply.status = status;
            if let Some(value) = retry_after {
                reply.headers.push(("Retry-After".into(), value.into()));
            }
            reply
        })
        .await;
        let alternate =
            Server::new(|_| panic!("terminal failure must not contact another source")).await;
        let sources = vec![
            format!("{}/image", primary.base),
            format!("{}/alternate", alternate.base),
        ];
        let cache = pinned_cache(
            &root,
            sources.clone(),
            pinned.then(|| store::digest(b"fixture-image")),
        );
        let error = cache
            .respond(
                &sources[0],
                kind,
                SourcePreference::InternationalFirst,
                None,
                false,
            )
            .await
            .unwrap_err();
        assert_eq!(error.status_code(), status);
        assert_eq!(error.retry_after(), retry_after);
        assert_eq!(alternate.count(), 0);
        assert!(root.blobs().is_empty());
    }
}

#[tokio::test]
async fn pinned_image_octet_stream_permission_never_applies_to_unpinned_images() {
    let root = Directory::new();
    let origin = Server::new(|_| {
        let mut reply = Wire::image();
        reply.headers[0].1 = "application/octet-stream".into();
        reply
    })
    .await;
    let url = format!("{}/image", origin.base);
    let cache = pinned_cache(&root, vec![url.clone()], None);
    assert!(matches!(
        image(&cache, &url).await,
        Err(MediaCacheError::InvalidResponse(_))
    ));
    assert!(root.blobs().is_empty());
}

#[tokio::test]
async fn pinned_image_forbidden_with_unusable_retry_after_remains_terminal() {
    let root = Directory::new();
    let primary = Server::new(|_| {
        let mut reply = Wire::image();
        reply.status = 403;
        reply.headers.push(("Retry-After".into(), "0".repeat(129)));
        reply
    })
    .await;
    let alternate =
        Server::new(|_| panic!("a present Retry-After forbids the image exception")).await;
    let sources = vec![
        format!("{}/image", primary.base),
        format!("{}/image", alternate.base),
    ];
    let cache = pinned_cache(
        &root,
        sources.clone(),
        Some(store::digest(b"fixture-image")),
    );
    let error = image(&cache, &sources[0]).await.unwrap_err();
    assert_eq!(error.status_code(), 403);
    assert_eq!(error.retry_after(), None);
    assert_eq!(alternate.count(), 0);
    assert!(root.blobs().is_empty());
}
