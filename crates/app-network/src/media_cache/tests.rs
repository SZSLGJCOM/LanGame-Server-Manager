use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[path = "fixtures.rs"]
mod fixtures;
use fixtures::*;

#[path = "cache_tests.rs"]
mod cache_tests;
#[path = "edge_tests.rs"]
mod edge_tests;
#[path = "range_tests.rs"]
mod range_tests;
#[path = "transport_tests.rs"]
mod transport_tests;

#[test]
fn identity_preserves_version_query_and_rejects_non_media_or_signed_requests() {
    let international =
        "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/570/header.jpg?t=1";
    let domestic = "https://shared.cdn.steamchina.queniuam.com/store_item_assets/steam/apps/570/header.jpg?t=1";
    assert_eq!(
        media_cache_identity(international, MediaKind::Image),
        media_cache_identity(domestic, MediaKind::Image)
    );
    assert!(media_cache_identity(international, MediaKind::Image).is_some());
    assert_ne!(
        media_cache_identity(international, MediaKind::Image),
        media_cache_identity(&international.replace("t=1", "t=2"), MediaKind::Image)
    );
    for url in [
        "http://127.0.0.1/image",
        concat!(
            "https://user:",
            "password@",
            "shared.fastly.steamstatic.com/store_item_assets/a.png"
        ),
        "https://shared.fastly.steamstatic.com:8443/store_item_assets/a.png",
        "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/",
        "https://shared.fastly.steamstatic.com/store_item_assets/a.png?token=secret",
        "https://shared.fastly.steamstatic.com/store_item_assets/a.png?t=1&t=2",
    ] {
        assert!(
            media_cache_identity(url, MediaKind::Image).is_none(),
            "accepted {url}"
        );
    }
}

#[test]
fn inline_store_videos_share_verified_cdn_identity_without_admitting_images_as_video() {
    let origins = [
        "https://shared.akamai.steamstatic.com",
        "https://shared.fastly.steamstatic.com",
        "https://shared.cdn.steamchina.queniuam.com",
    ];
    let base = "/store_item_assets/steam/apps/322330/extras/ab42e93c5610ff5c234e67ae9f3a8c4a";
    let mut encodings = Vec::new();
    for extension in ["webm", "mp4"] {
        let sources = origins.map(|origin| format!("{origin}{base}.{extension}?t=1790287771"));
        let identity = media_cache_identity(&sources[0], MediaKind::Video)
            .expect("verified inline video must enter the native media cache");
        for source in &sources {
            assert_eq!(
                media_cache_identity(source, MediaKind::Video),
                Some(identity.clone())
            );
        }
        assert_ne!(
            media_cache_identity(
                &sources[0].replace("1790287771", "1790287772"),
                MediaKind::Video
            ),
            Some(identity.clone())
        );
        assert_eq!(
            official_url_candidates(&sources[0], SourcePreference::ChinaFirst)[0],
            sources[2]
        );
        assert_eq!(
            official_url_candidates(&sources[2], SourcePreference::InternationalFirst)[0],
            sources[0]
        );
        encodings.push(identity);
    }
    assert_ne!(encodings[0], encodings[1]);
    let poster = format!("{}{base}.poster.avif?t=1790287771", origins[0]);
    assert!(media_cache_identity(&poster, MediaKind::Image).is_some());
    for source in [
        poster,
        format!("{}{base}.jpg", origins[0]),
        format!("{}/private/movie.webm", origins[0]),
        format!("{}/steam/apps/322330/extras/movie.webm", origins[0]),
        format!(
            "{}/store_item_assets/Steam/apps/322330/extras/movie.webm",
            origins[0]
        ),
        format!("{}{base}.WEBM", origins[0]),
        format!("{}{base}.w%65bm", origins[0]),
        format!("{}{base}.webm?token=private", origins[0]),
        format!("{}{base}.mp4#private", origins[0]),
        format!("{}{base}.webm?t=1&t=2", origins[0]),
        format!("https://unknown.invalid{base}.webm"),
    ] {
        assert!(
            media_cache_identity(&source, MediaKind::Video).is_none(),
            "{source}"
        );
    }
}

#[test]
fn freshness_obeys_http_limits_and_single_range_validation() {
    let mut headers = meta::Headers {
        expires: Some("Thu, 01 Jan 1970 00:02:00 GMT".into()),
        ..Default::default()
    };
    assert_eq!(headers.freshness(100).expires_at, 120);
    headers.cache_control = Some("max-age=999999999".into());
    assert_eq!(headers.freshness(100).expires_at, 100 + 30 * 86400);
    headers.cache_control = Some("max-age=30, must-revalidate".into());
    headers.age = 20;
    assert_eq!(headers.freshness(100).expires_at, 110);
    assert!(!headers.freshness(100).stale_allowed);
    headers.cache_control = Some("no-store".into());
    assert!(headers.freshness(100).no_store);
    for value in [
        "bytes=1-0",
        "bytes=-0",
        "bytes=0-1,2-3",
        "items=0-1",
        "bytes=0-x",
    ] {
        assert!(ranges::RequestedRange::parse(value).is_err());
    }
}
