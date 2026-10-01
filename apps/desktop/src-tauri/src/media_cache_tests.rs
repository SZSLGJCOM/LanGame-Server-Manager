use super::*;

fn protocol(url: &str, kind: &str) -> String {
    let mut endpoint = reqwest::Url::parse("http://lgsm-media.localhost/").unwrap();
    endpoint
        .query_pairs_mut()
        .append_pair("url", url)
        .append_pair("kind", kind)
        .append_pair("locale", "zh-CN");
    endpoint.to_string()
}

#[test]
fn media_protocol_only_accepts_verified_public_resources_and_known_parameters() {
    let source =
        "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/322330/header.jpg?t=7";
    let endpoint = protocol(source, "image");
    let parsed = protocol_source(&endpoint).unwrap();
    assert_eq!(parsed.url, source);
    assert_eq!(parsed.preference, SourcePreference::ChinaFirst);
    assert_eq!(parsed.purpose, MediaPurpose::Image);
    for url in [
        "http://127.0.0.1/private",
        "file:///secret",
        "https://shared.akamai.steamstatic.com/private/account",
        "https://shared.akamai.steamstatic.com/steam/apps/1/header.jpg?token=private",
        "https://reader@shared.akamai.steamstatic.com/steam/apps/1/header.jpg",
        "https://shared.akamai.steamstatic.com:8443/steam/apps/1/header.jpg",
    ] {
        assert!(protocol_source(&protocol(url, "image")).is_none(), "{url}");
    }
    for suffix in [
        "&url=another",
        "&locale=en-US",
        "&extra=private",
        "&kind=video",
    ] {
        assert!(protocol_source(&format!("{endpoint}{suffix}")).is_none());
    }
    assert!(protocol_source(&protocol(source, "download")).is_none());
    assert!(protocol_source(&protocol(source, "video")).is_none());
}

#[test]
fn cache_responses_preserve_range_length_and_remote_hls_base_without_upstream_side_effects() {
    let result = MediaResponse {
        status: 206,
        final_url: "https://video.cdn.steamchina.queniuam.com/store_trailers/fixture/segment.m4s"
            .into(),
        headers: vec![
            ("Content-Type".into(), "video/mp4".into()),
            ("Content-Length".into(), "3".into()),
            ("Content-Range".into(), "bytes 4-6/20".into()),
            ("X-LanGame-Media-Cache".into(), "hit".into()),
            ("Set-Cookie".into(), "session=untrusted".into()),
            ("Location".into(), "http://private/".into()),
        ],
        body: vec![4, 5, 6],
    };
    let response = cached_response(result, true);
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()["content-length"], "3");
    assert_eq!(response.headers()["content-range"], "bytes 4-6/20");
    assert_eq!(response.headers()["x-langame-media-cache"], "hit");
    assert!(
        response.headers()["x-langame-media-origin"]
            .to_str()
            .unwrap()
            .starts_with("https://video.cdn.steamchina")
    );
    assert!(response.body().is_empty());
    assert!(!response.headers().contains_key("set-cookie"));
    assert!(!response.headers().contains_key("location"));
    assert_eq!(response.headers()["cache-control"], "no-store");
}

#[tokio::test]
async fn media_admission_is_bounded_and_shutdown_interrupts_waiting_requests() {
    let state = MediaCacheState::new(std::env::temp_dir().join("unused-media-admission-test"));
    let permits: Vec<_> = (0..MAX_MEDIA_OPERATIONS)
        .map(|_| state.admit().unwrap())
        .collect();
    assert!(state.admit().is_none());
    drop(permits);
    assert!(state.admit().is_some());
    tokio::time::timeout(
        Duration::from_millis(100),
        wait_for_shutdown(Arc::new(|| true)),
    )
    .await
    .unwrap();
}

#[test]
fn playback_lease_retains_generation_when_disk_metadata_is_replaced() {
    let leases = leases::MediaLeases::default();
    let source = MediaSource {
        url: "https://video.akamai.steamstatic.com/store_trailers/fixture/movie.mp4".into(),
        purpose: MediaPurpose::Video,
        preference: SourcePreference::InternationalFirst,
        playback: Some(PlaybackGeneration::default()),
    };
    let first = leases.register("fixture".into(), source).unwrap();
    let response = |etag: &str, total: u64| MediaResponse {
        status: 206,
        final_url: "https://video.akamai.steamstatic.com/store_trailers/fixture/movie.mp4".into(),
        headers: vec![
            ("ETag".into(), etag.into()),
            ("Content-Range".into(), format!("bytes 0-2/{total}")),
        ],
        body: vec![1, 2, 3],
    };
    let original = leases.lookup(&first).unwrap();
    assert!(
        original
            .playback
            .as_ref()
            .unwrap()
            .accepts(&response("\"v1\"", 100))
    );
    // A later request only carries the lease; evicted/recreated disk entries do
    // not overwrite the already accepted representation for this playback.
    let resumed = leases.lookup(&first).unwrap();
    let playback = resumed.playback.as_ref().unwrap();
    assert!(playback.accepts(&response("\"v1\"", 100)));
    assert!(!playback.accepts(&response("\"v2\"", 100)));
    assert!(!playback.accepts(&response("\"v1\"", 101)));
    let second = leases
        .register(
            "fixture".into(),
            MediaSource {
                playback: Some(PlaybackGeneration::default()),
                ..resumed
            },
        )
        .unwrap();
    assert_ne!(
        first, second,
        "independent playback must get its own generation"
    );
    assert!(
        leases
            .lookup(&second)
            .unwrap()
            .playback
            .unwrap()
            .accepts(&response("\"v2\"", 100))
    );
    let direct = protocol(
        "https://video.akamai.steamstatic.com/store_trailers/fixture/movie.mp4",
        "video",
    );
    assert!(
        protocol_source(&direct).is_none(),
        "native requests cannot bypass lease generation"
    );
}
