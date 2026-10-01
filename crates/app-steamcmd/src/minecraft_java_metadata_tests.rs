use super::*;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TAG: &str = "jdk-21.0.12.1+1";
const NAME: &str = "OpenJDK21U-jre_x64_windows_hotspot_21.0.12.1_1.zip";
const URL: &str = "https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12.1%2B1/OpenJDK21U-jre_x64_windows_hotspot_21.0.12.1_1.zip";
const DIGEST: &str = "d35f31e712f0fcf6ac5a093edc90204fbff22f720ba3950bd09d331d5e621636";
const SIZE: u64 = 48_999_141;

fn adoptium() -> Value {
    json!([{
        "release_name": TAG,
        "vendor": "eclipse",
        "version": {"major": 21},
        "binary": {
            "architecture": "x64", "image_type": "jre", "jvm_impl": "hotspot",
            "heap_size": "normal", "os": "windows",
            "package": {"name": NAME, "link": URL, "checksum": DIGEST, "size": SIZE}
        }
    }])
}

fn github() -> Value {
    json!({
        "tag_name": TAG,
        "html_url": "https://github.com/adoptium/temurin21-binaries/releases/tag/jdk-21.0.12.1+1",
        "draft": false, "prerelease": false,
        "assets": [{
            "name": NAME, "state": "uploaded", "browser_download_url": URL,
            "digest": format!("sha256:{DIGEST}"), "size": SIZE
        }]
    })
}

fn expected() -> JavaPackage {
    JavaPackage {
        link: URL.into(),
        checksum: DIGEST.into(),
        size: SIZE,
    }
}

#[test]
fn java_package_matches_both_official_metadata_shapes() {
    assert_eq!(
        parse_package(&adoptium().to_string(), 21).unwrap(),
        expected()
    );
    assert_eq!(
        parse_package(&github().to_string(), 21).unwrap(),
        expected()
    );
    assert_eq!(
        metadata_sources(21),
        vec![
            "https://api.adoptium.net/v3/assets/latest/21/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse",
            "https://api.github.com/repos/adoptium/temurin21-binaries/releases/latest",
        ]
    );
}

#[test]
fn java_package_ga_naming_preserves_supported_java_versions() {
    for (major, tag, expected_name) in [
        (
            8,
            "jdk8u462-b08",
            "OpenJDK8U-jre_x64_windows_hotspot_8u462b08.zip",
        ),
        (
            17,
            "jdk-17.0.16+8",
            "OpenJDK17U-jre_x64_windows_hotspot_17.0.16_8.zip",
        ),
        (21, TAG, NAME),
        (
            25,
            "jdk-25+36",
            "OpenJDK25U-jre_x64_windows_hotspot_25_36.zip",
        ),
    ] {
        assert_eq!(package_name(major, tag).unwrap(), expected_name);
    }
    for tag in [
        "jdk-17.0.16+8",
        "jdk-21.0.12-ea+1",
        "jdk-21.0.12+1-ea",
        "jdk-21..12+1",
        "jdk-21.0.12+",
        "jdk-21/other+1",
    ] {
        assert!(package_name(21, tag).is_err(), "accepted {tag}");
    }
    for tag in ["jdk8u462-ea-b08", "jdk8u-b08", "jdk8u462-b", "jdk-8+1"] {
        assert!(package_name(8, tag).is_err(), "accepted {tag}");
    }
}

#[test]
fn java_package_rejects_ambiguous_or_mismatched_adoptium_assets() {
    for (pointer, wrong) in [
        ("/0/vendor", json!("other")),
        ("/0/version/major", json!(17)),
        ("/0/release_name", json!("jdk-17.0.16+8")),
        ("/0/binary/architecture", json!("aarch64")),
        ("/0/binary/image_type", json!("jdk")),
        ("/0/binary/jvm_impl", json!("openj9")),
        ("/0/binary/heap_size", json!("large")),
        ("/0/binary/os", json!("linux")),
    ] {
        let mut body = adoptium();
        *body.pointer_mut(pointer).unwrap() = wrong;
        assert!(
            parse_package(&body.to_string(), 21).is_err(),
            "accepted {pointer}"
        );
    }
    let mut duplicate = adoptium();
    duplicate
        .as_array_mut()
        .unwrap()
        .push(adoptium()[0].clone());
    assert!(parse_package(&duplicate.to_string(), 21).is_err());
    assert!(parse_package("[]", 21).is_err());
}

#[test]
fn java_package_rejects_non_ga_or_ambiguous_github_release() {
    for (pointer, wrong) in [
        ("/draft", json!(true)),
        ("/prerelease", json!(true)),
        ("/tag_name", json!("jdk-21.0.12-ea+1")),
        ("/tag_name", json!("jdk-17.0.16+8")),
        (
            "/html_url",
            json!("https://github.com/other/temurin21-binaries/releases/tag/jdk-21.0.12.1+1"),
        ),
        (
            "/html_url",
            json!("https://github.com/adoptium/temurin17-binaries/releases/tag/jdk-21.0.12.1+1"),
        ),
        (
            "/html_url",
            json!("https://github.com/adoptium/temurin21-binaries/releases/tag/jdk-21.0.11+1"),
        ),
        ("/assets/0/state", json!("new")),
        (
            "/assets/0/name",
            json!("OpenJDK21U-jdk_x64_windows_hotspot_21.0.12.1_1.zip"),
        ),
    ] {
        let mut body = github();
        *body.pointer_mut(pointer).unwrap() = wrong;
        assert!(
            parse_package(&body.to_string(), 21).is_err(),
            "accepted {pointer}"
        );
    }
    let mut duplicate = github();
    duplicate["assets"]
        .as_array_mut()
        .unwrap()
        .push(github()["assets"][0].clone());
    assert!(parse_package(&duplicate.to_string(), 21).is_err());
    let mut empty = github();
    empty["assets"] = json!([]);
    assert!(parse_package(&empty.to_string(), 21).is_err());
}

#[test]
fn java_package_requires_trusted_archive_url_digest_and_size_from_either_source() {
    let invalid_urls = [
        URL.replace("https:", "http:"),
        URL.replace("github.com/", "github.com.evil.invalid/"),
        URL.replace("github.com/", "github.com:8443/"),
        URL.replace("github.com/", "token@github.com/"),
        URL.replace("/adoptium/", "/other/"),
        URL.replace("temurin21-binaries", "temurin17-binaries"),
        URL.replace("jdk-21.0.12.1%2B1", "jdk-21.0.11%2B1"),
        URL.replace("-jre_", "-jdk_"),
        format!("{URL}?token=unexpected"),
        format!("{URL}#fragment"),
    ];
    for source in [false, true] {
        let (body, package_pointer, url_key, digest_key) = if source {
            (github(), "/assets/0", "browser_download_url", "digest")
        } else {
            (adoptium(), "/0/binary/package", "link", "checksum")
        };
        for url in &invalid_urls {
            let mut changed = body.clone();
            changed.pointer_mut(package_pointer).unwrap()[url_key] = json!(url);
            assert!(
                parse_package(&changed.to_string(), 21).is_err(),
                "accepted {url}"
            );
        }
        for digest in [
            json!(null),
            json!(""),
            json!("sha256:abc"),
            json!("g".repeat(64)),
            json!(format!("sha1:{DIGEST}")),
        ] {
            let mut changed = body.clone();
            changed.pointer_mut(package_pointer).unwrap()[digest_key] = digest;
            assert!(parse_package(&changed.to_string(), 21).is_err());
        }
        for size in [json!(null), json!(0), json!(-1), json!(ARCHIVE_LIMIT + 1)] {
            let mut changed = body.clone();
            changed.pointer_mut(package_pointer).unwrap()["size"] = size;
            assert!(parse_package(&changed.to_string(), 21).is_err());
        }
        for key in [digest_key, "size"] {
            let mut changed = body.clone();
            changed
                .pointer_mut(package_pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(parse_package(&changed.to_string(), 21).is_err());
        }
    }
}

struct Source {
    url: String,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Source {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn source(status: u16, headers: &str, body: &str) -> Source {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/metadata.json", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&requests);
    let response = format!(
        "HTTP/1.1 {status} Fixture\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            observed.fetch_add(1, Ordering::Relaxed);
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).await;
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    Source {
        url,
        requests,
        task,
    }
}

async fn resolve(primary: &Source, alternate: &Source) -> Result<JavaPackage, SteamCmdError> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    fetch_java_package_from_candidates(
        &client,
        &[primary.url.clone(), alternate.url.clone()],
        21,
        InstallDeadline::new("Temurin metadata fixture", Duration::from_secs(5)),
    )
    .await
}

#[tokio::test]
async fn java_package_uses_primary_without_requesting_alternate_when_valid() {
    let primary = source(200, "", &adoptium().to_string()).await;
    let alternate = source(200, "", &github().to_string()).await;
    assert_eq!(resolve(&primary, &alternate).await.unwrap(), expected());
    assert_eq!(primary.requests.load(Ordering::Relaxed), 1);
    assert_eq!(alternate.requests.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn java_package_recovers_from_unavailable_or_invalid_adoptium_metadata() {
    let mut mismatched = adoptium();
    mismatched[0]["version"]["major"] = json!(17);
    for (status, body) in [
        (404, String::new()),
        (200, "<html>gateway</html>".into()),
        (200, mismatched.to_string()),
    ] {
        let primary = source(status, "", &body).await;
        let alternate = source(200, "", &github().to_string()).await;
        assert_eq!(resolve(&primary, &alternate).await.unwrap(), expected());
        assert_eq!(primary.requests.load(Ordering::Relaxed), 1);
        assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn java_package_does_not_bypass_terminal_or_retry_after_responses() {
    for (status, headers) in [
        (401, ""),
        (403, ""),
        (429, ""),
        (503, "Retry-After: 120\r\n"),
    ] {
        let primary = source(status, headers, "blocked").await;
        let alternate = source(200, "", &github().to_string()).await;
        assert!(resolve(&primary, &alternate).await.is_err());
        assert_eq!(primary.requests.load(Ordering::Relaxed), 1);
        assert_eq!(alternate.requests.load(Ordering::Relaxed), 0);
    }
}

#[tokio::test]
async fn java_package_never_uses_unverified_github_fallback() {
    let mut invalid = github();
    invalid["assets"][0]["digest"] = Value::Null;
    let primary = source(404, "", "unavailable").await;
    let alternate = source(200, "", &invalid.to_string()).await;
    let error = resolve(&primary, &alternate).await.unwrap_err();
    assert!(error.to_string().contains("SHA-256"));
    assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn java_package_fallback_can_select_an_independently_published_ga_patch() {
    let primary = source(404, "", "unavailable").await;
    let fallback_body = github().to_string().replace("21.0.12.1", "21.0.11");
    let alternate = source(200, "", &fallback_body).await;
    let selected = resolve(&primary, &alternate).await.unwrap();
    assert_eq!(selected.link, URL.replace("21.0.12.1", "21.0.11"));
    assert_eq!(selected.checksum, DIGEST);
    assert_eq!(selected.size, SIZE);
}

#[tokio::test]
#[ignore = "read-only live check of official Adoptium and GitHub metadata; no JRE download or execution"]
async fn java_package_live_official_metadata_supported_majors() {
    let client = crate::http_download::http_client().unwrap();
    for major in [17, 21] {
        for candidate in metadata_sources(major) {
            let package = fetch_java_package_from_candidates(
                &client,
                std::slice::from_ref(&candidate),
                major,
                InstallDeadline::new("Official Temurin metadata probe", Duration::from_secs(45)),
            )
            .await
            .unwrap_or_else(|error| panic!("{candidate}: {error}"));
            assert!(package.link.starts_with(&format!(
                "https://github.com/adoptium/temurin{major}-binaries/releases/download/"
            )));
            assert!(
                package
                    .link
                    .contains(&format!("/OpenJDK{major}U-jre_x64_windows_hotspot_"))
            );
            assert_eq!(package.checksum.len(), 64);
            assert!((1..=ARCHIVE_LIMIT).contains(&package.size));
            println!(
                "Java {major} source={candidate} archive={} bytes={} sha256={}",
                package.link, package.size, package.checksum
            );
        }
    }
}
