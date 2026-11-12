use super::*;

const SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const INTERNATIONAL: &str = "https://raw.example/maps/v1/island.jpg";
const DOMESTIC: &str = "https://cn.example/api/raw/abc123";

#[test]
fn mod_icons_are_image_only_and_keep_provider_size_and_version_identity() {
    let curseforge = "https://83374.media.forgecdn.net/avatars/thumbnails/1949/338/256/256/639208701457408935.png";
    let thunderstore =
        "https://ccdn.thunderstore.io/live/repository/icons/ValheimModding-Jotunn-2.30.2.png";
    let nexus =
        "https://staticdelivery.nexusmods.com/mods/1059/images/9559/9559-1770000000-123456789.png";
    for source in [curseforge, thunderstore, nexus] {
        assert!(media_identity(source, "image").is_some(), "{source}");
        for purpose in ["video", "hls", "download", "metadata"] {
            assert!(media_identity(source, purpose).is_none(), "{purpose}");
        }
        assert_eq!(
            official_url_candidates(source, SourcePreference::ChinaFirst),
            [source],
            "No unverified equivalent CDN is manufactured"
        );
    }
    let key = |url: &str| media_identity(url, "image").unwrap().key;
    assert_ne!(
        key(curseforge),
        key(&curseforge.replace("/256/256/", "/64/64/"))
    );
    assert_ne!(
        key(thunderstore),
        key(&thunderstore.replace("2.30.2", "2.30.1"))
    );
    assert_ne!(
        key(thunderstore),
        key(&thunderstore.replace("ccdn.", "gcdn."))
    );
    assert_ne!(
        key(thunderstore),
        key(&format!("{thunderstore}.128x128_q95.png"))
    );
    assert_ne!(
        key(nexus),
        key(&nexus.replace("/images/", "/images/thumbnails/"))
    );
}

#[test]
fn mod_icon_cache_rejects_downloads_unrelated_hosts_and_private_urls() {
    for source in [
        "https://83374.media.forgecdn.net/files/1949/338/server.zip",
        "https://83374.media.forgecdn.net/attachments/1949/338/screenshot.png",
        "https://83374.media.forgecdn.net/avatars/1949/338/payload.zip",
        "https://83374.media.forgecdn.net/avatars/1949/338/icon.svg",
        "https://83374.media.forgecdn.net/avatars/1949/338/icon.png?token=private",
        "https://83374.media.forgecdn.net/avatars/1949/338/icon.png#private",
        "https://83374.media.forgecdn.net.evil.invalid/avatars/1949/338/icon.png",
        "https://127.0.0.1/avatars/1949/338/icon.png",
        "http://83374.media.forgecdn.net/avatars/1949/338/icon.png",
        "https://user@83374.media.forgecdn.net/avatars/1949/338/icon.png",
        "https://83374.media.forgecdn.net:8443/avatars/1949/338/icon.png",
        "https://ccdn.thunderstore.io/live/repository/packages/Author-Mod-1.0.0.zip",
        "https://ccdn.thunderstore.io/live/repository/icons/Author-Mod-1.0.0.zip",
        "https://ccdn.thunderstore.io/live/repository/icons/Author-Mod-1.0.0.png?token=private",
        "https://ccdn.thunderstore.io/live/repository/icons/Author-Mod-1.0.0.png#private",
        "https://gcdn.thunderstore.io/account/avatar.png",
        "https://user@gcdn.thunderstore.io/live/repository/icons/Author-Mod-1.0.0.png",
        "https://gcdn.thunderstore.io:8443/live/repository/icons/Author-Mod-1.0.0.png",
        "https://cf-files.nexusmods.com/1059/9559/package.zip",
        "https://staticdelivery.nexusmods.com/mods/1059/files/package.zip",
        "https://staticdelivery.nexusmods.com/mods/1059/images/icon.svg",
        "https://staticdelivery.nexusmods.com/mods/1059/images/icon.png?key=private",
        "https://staticdelivery.nexusmods.com/mods/1059/images/icon.png#private",
        "https://user@staticdelivery.nexusmods.com/mods/1059/images/icon.png",
        "https://staticdelivery.nexusmods.com:8443/mods/1059/images/icon.png",
        "https://staticdelivery.nexusmods.com.evil.invalid/mods/1059/images/icon.png",
        "https://avatars.nexusmods.com/1234/100.png",
    ] {
        assert!(media_identity(source, "image").is_none(), "{source}");
    }
}

fn fixture() -> Policy {
    serde_json::from_value(serde_json::json!({
        "schemaVersion": 1, "chinaOrigins": ["https://cn.example"], "groups": [],
        "exactResources": [{ "id": "map-island", "urls": [INTERNATIONAL, DOMESTIC],
            "purposes": ["image"], "sha256": SHA256 }]
    }))
    .unwrap()
}

#[test]
fn pinned_exact_images_share_identity_across_paths_and_partition_by_digest() {
    let mut policy = fixture();
    let original = media_identity_for(&policy, INTERNATIONAL, "image").expect("reviewed image");
    let domestic = media_identity_for(&policy, DOMESTIC, "image").expect("reviewed mirror");
    assert_eq!(original.key, domestic.key);
    assert_eq!(original.image_sha256.as_deref(), Some(SHA256));
    assert_eq!(domestic.image_sha256, original.image_sha256);
    policy.exact_resources[0].sha256 = Some("f".repeat(64));
    assert_ne!(
        media_identity_for(&policy, INTERNATIONAL, "image")
            .unwrap()
            .key,
        original.key
    );
}

#[test]
fn pinned_exact_images_reject_other_purposes_and_unreviewed_urls() {
    let policy = fixture();
    for purpose in ["video", "download", "metadata"] {
        assert!(media_identity_for(&policy, INTERNATIONAL, purpose).is_none());
    }
    for url in [
        format!("{DOMESTIC}?token=private"),
        format!("{DOMESTIC}#fragment"),
        DOMESTIC.replace("https:", "http:"),
        DOMESTIC.replace("cn.example", "cn.example:8443"),
        DOMESTIC.replace("cn.example", "user@cn.example"),
        DOMESTIC.replace("abc123", "unreviewed"),
    ] {
        assert!(
            media_identity_for(&policy, &url, "image").is_none(),
            "{url}"
        );
    }
}

#[test]
fn pinned_exact_images_fail_closed_on_invalid_or_nonimage_hash_contracts() {
    for hash in [
        "".into(),
        "0".repeat(63),
        "g".repeat(64),
        SHA256.to_uppercase(),
    ] {
        let mut policy = fixture();
        policy.exact_resources[0].sha256 = Some(hash);
        assert!(media_identity_for(&policy, INTERNATIONAL, "image").is_none());
    }
    let mut policy = fixture();
    policy.exact_resources[0].purposes = vec!["video".into()];
    assert!(media_identity_for(&policy, INTERNATIONAL, "video").is_none());
    policy.exact_resources[0].sha256 = None;
    let video = media_identity_for(&policy, INTERNATIONAL, "video").expect("unpinned exact video");
    assert!(video.image_sha256.is_none());
}

#[test]
fn pinned_exact_images_follow_request_locale_without_a_broad_group() {
    let policy = fixture();
    let now = Instant::now();
    assert_eq!(
        ordered_candidates(
            &policy,
            INTERNATIONAL,
            SourcePreference::ChinaFirst,
            &[],
            now
        ),
        [DOMESTIC, INTERNATIONAL]
    );
    assert_eq!(
        ordered_candidates(
            &policy,
            DOMESTIC,
            SourcePreference::InternationalFirst,
            &[],
            now
        ),
        [INTERNATIONAL, DOMESTIC]
    );
}

#[test]
fn immutable_image_storage_requires_a_digest_and_uses_a_distinct_identity() {
    let mut policy = fixture();
    let http = media_identity_for(&policy, INTERNATIONAL, "image").unwrap();
    assert!(!http.cache_immutable);
    policy.exact_resources[0].cache_immutable = true;
    let artifact = media_identity_for(&policy, INTERNATIONAL, "image").unwrap();
    assert!(artifact.cache_immutable);
    assert_ne!(
        artifact.key, http.key,
        "HTTP and download persistence must not share entries"
    );
    policy.exact_resources[0].sha256 = None;
    assert!(media_identity_for(&policy, INTERNATIONAL, "image").is_none());
    policy.exact_resources[0].purposes = vec!["video".into()];
    assert!(media_identity_for(&policy, INTERNATIONAL, "video").is_none());
}
