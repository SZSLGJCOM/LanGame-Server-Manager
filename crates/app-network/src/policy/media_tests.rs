use super::*;

const SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const INTERNATIONAL: &str = "https://raw.example/maps/v1/island.jpg";
const DOMESTIC: &str = "https://cn.example/api/raw/abc123";

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
