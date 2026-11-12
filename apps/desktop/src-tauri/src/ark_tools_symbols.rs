//! Matching symbol preparation belongs to the explicit, stopped-instance
//! installation command. Ordinary status inspection and startup never download.
use super::*;
use app_network::SourcePreference;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::time::{Instant, timeout_at};

#[path = "ark_tools_symbol_cache.rs"]
mod cache;

const SOURCES: [&str; 3] = [
    "https://cdn.pelayori.com/cache/",
    "https://cdn.shadowhunter.co.za/cache/",
    "https://cdn.shadowhunter-systems.co.za/cache/",
];
const BUDGET: Duration = Duration::from_secs(180);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    mirror_repository: String,
    packages: Vec<ReviewedCache>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewedCache {
    steam_build_id: String,
    executable_sha256: String,
    file_name: String,
    upstream_url: String,
    mirror_tag: String,
    mirror_url: String,
    size: u64,
    sha256: String,
}

fn reviewed_caches() -> Result<&'static [ReviewedCache], String> {
    static MANIFEST: OnceLock<Result<Manifest, String>> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let manifest: Manifest = serde_json::from_str(include_str!(
            "../../../../modules/ark-tools/symbol-caches.json"
        )).map_err(|error| format!("Invalid bundled ASA symbol manifest: {error}"))?;
        let hash = |value: &str| value.len() == 64
            && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        let mut executables = std::collections::HashSet::new();
        if manifest.schema_version != 1
            || manifest.mirror_repository != "SZSLGJCOM/LanGame-Server-Manager-Releases"
            || manifest.packages.len() > 128 {
            return Err("Invalid bundled ASA symbol publication identity".into());
        }
        for package in &manifest.packages {
            if !hash(&package.executable_sha256) || !hash(&package.sha256)
                || !executables.insert(&package.executable_sha256)
                || package.steam_build_id.is_empty()
                || !package.steam_build_id.bytes().all(|b| b.is_ascii_digit())
                || package.file_name != format!("{}.zip", package.executable_sha256)
                || package.upstream_url != format!("{}{}", SOURCES[0], package.file_name)
                || package.size == 0 || package.size > cache::MAX_ARCHIVE {
                return Err("Invalid bundled ASA symbol archive identity".into());
            }
            if package.mirror_tag != format!("ark-tools-symbols-asa-{}", &package.sha256[..16])
                || package.mirror_url != format!("https://api.gitcode.com/api/v5/repos/{}/releases/{}/attach_files/{}/download", manifest.mirror_repository, package.mirror_tag, package.file_name) {
                return Err("Invalid bundled ASA symbol mirror identity".into());
            }
        }
        Ok(manifest)
    }).as_ref().map(|manifest| manifest.packages.as_slice()).map_err(Clone::clone)
}

fn sources(
    executable: &str,
    preference: SourcePreference,
) -> Result<(Vec<String>, Option<&'static ReviewedCache>), String> {
    let reviewed = reviewed_caches()?
        .iter()
        .find(|package| package.executable_sha256 == executable);
    let mut urls: Vec<_> = SOURCES
        .iter()
        .map(|source| format!("{source}{executable}.zip"))
        .collect();
    if let Some(package) = reviewed {
        match preference {
            SourcePreference::ChinaFirst => urls.insert(0, package.mirror_url.clone()),
            SourcePreference::InternationalFirst => urls.push(package.mirror_url.clone()),
        }
    }
    Ok((urls, reviewed))
}

fn allowed(url: &reqwest::Url, reviewed: bool) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.host_str().is_some_and(|host| {
            matches!(
                host,
                "cdn.pelayori.com" | "cdn.shadowhunter.co.za" | "cdn.shadowhunter-systems.co.za"
            ) || reviewed
                && (matches!(host, "gitcode.com" | "api.gitcode.com")
                    || host.ends_with(".gitcode.com")
                    || host.ends_with(".myhuaweicloud.com"))
        })
}

fn client(reviewed: bool) -> Result<reqwest::Client, String> {
    let redirect_limit = if reviewed { 5 } else { 3 };
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .user_agent("LanGame-Server-Manager/ARK-Symbols")
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= redirect_limit || !allowed(attempt.url(), reviewed) {
                attempt.error("ASA symbols redirected outside the approved providers")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|error| error.without_url().to_string())
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    deadline: Instant,
    report: &InstallReporter,
    reviewed: Option<&ReviewedCache>,
) -> Result<(), String> {
    let request = client
        .get(url)
        .build()
        .map_err(|error| error.without_url().to_string())?;
    let source = request.url().host_str().unwrap_or("ASA symbols").to_owned();
    report(InstallUpdate {
        phase: InstallPhase::Connecting,
        source: Some(source.clone()),
        downloaded_bytes: Some(0),
        total_bytes: None,
    });
    let mut response = app_network::send_with_retry(
        client,
        request,
        deadline.saturating_duration_since(Instant::now()),
    )
    .await
    .map_err(|error| error.to_string())?;
    let total = response.content_length();
    let limit = reviewed.map_or(cache::MAX_ARCHIVE, |package| package.size);
    if total.is_some_and(|size| size == 0 || size > limit || reviewed.is_some() && size != limit) {
        return Err("ASA symbol archive exceeds its bounded download size".into());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x1).custom_flags(0x0020_0000);
    }
    let mut output =
        tokio::fs::File::from_std(options.open(path).map_err(|error| error.to_string())?);
    let mut received = 0_u64;
    let mut digest = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| error.without_url().to_string())?
    {
        if chunk.len() as u64 > limit.saturating_sub(received) {
            return Err("ASA symbol archive exceeds its bounded download size".into());
        }
        output
            .write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
        received += chunk.len() as u64;
        digest.update(&chunk);
        report(InstallUpdate {
            phase: InstallPhase::Downloading,
            source: Some(source.clone()),
            downloaded_bytes: Some(received),
            total_bytes: total,
        });
    }
    if received == 0 || total.is_some_and(|size| size != received) {
        return Err("ASA symbol download did not contain the complete archive".into());
    }
    if let Some(package) = reviewed {
        let actual: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if received != package.size || actual != package.sha256 {
            return Err("ASA symbol archive differs from its reviewed size or SHA-256".into());
        }
    }
    output.sync_all().await.map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) async fn prepare(
    root: PathBuf,
    preference: SourcePreference,
    report: InstallReporter,
) -> Result<cache::Prepared, String> {
    let mut prepared = tokio::task::spawn_blocking(move || cache::Prepared::inspect(&root))
        .await
        .map_err(|error| format!("ASA symbol inspection task failed: {error}"))??;
    if prepared.ready() {
        return Ok(prepared);
    }
    let (sources, reviewed) = sources(&prepared.executable_hash, preference)?;
    let client = client(reviewed.is_some())?;
    let deadline = Instant::now() + BUDGET;
    let mut failures = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let source_deadline = Instant::now() + remaining / (sources.len() - index) as u32;
        let path = prepared.archive_path()?;
        let fetched = timeout_at(
            source_deadline,
            download(&client, source, &path, source_deadline, &report, reviewed),
        )
        .await
        .unwrap_or_else(|_| Err("ASA symbol download timed out".into()));
        let result = match fetched {
            Ok(()) => {
                report(InstallUpdate {
                    phase: InstallPhase::Verifying,
                    source: reqwest::Url::parse(source)
                        .ok()
                        .and_then(|url| url.host_str().map(str::to_owned)),
                    downloaded_bytes: None,
                    total_bytes: None,
                });
                let (next, result) = tokio::task::spawn_blocking(move || {
                    let result = prepared.extract();
                    (prepared, result)
                })
                .await
                .map_err(|error| format!("ASA symbol validation task failed: {error}"))?;
                prepared = next;
                result
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => return Ok(prepared),
            Err(error) => {
                failures.push(format!("{source}: {error}"));
                prepared.discard_download();
            }
        }
    }
    Err(format!(
        "Matching ASA symbols could not be prepared. Check the network and use Check and update while the instance is stopped. {}",
        failures.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[test]
    fn asa_symbol_delivery_is_restricted_to_the_disclosed_https_providers() {
        for source in SOURCES {
            assert!(allowed(&reqwest::Url::parse(source).unwrap(), true));
        }
        for source in [
            "https://api.gitcode.com/",
            "https://file.gitcode.com/",
            "https://release.obs.cn-east-3.myhuaweicloud.com/",
        ] {
            assert!(allowed(&reqwest::Url::parse(source).unwrap(), true));
        }
        for source in [
            "http://cdn.pelayori.com/cache/",
            "https://cdn.pelayori.com:8443/cache/",
            "https://user@cdn.pelayori.com/cache/",
            "https://cdn.pelayori.com.example.org/cache/",
            "https://api.gitcode.com.example.org/",
            "https://myhuaweicloud.com.example.org/",
            "http://api.gitcode.com/",
        ] {
            assert!(!allowed(&reqwest::Url::parse(source).unwrap(), true));
        }
    }

    #[test]
    fn asa_unknown_executable_redirects_cannot_expand_to_mirror_hosts() {
        for source in SOURCES {
            assert!(allowed(&reqwest::Url::parse(source).unwrap(), false));
        }
        for source in [
            "https://api.gitcode.com/",
            "https://file.gitcode.com/",
            "https://release.obs.cn-east-3.myhuaweicloud.com/",
        ] {
            assert!(!allowed(&reqwest::Url::parse(source).unwrap(), false));
        }
    }

    #[test]
    fn asa_symbols_use_reviewed_mirrors_in_preference_order_and_keep_unknown_versions_upstream() {
        let package = &reviewed_caches().unwrap()[0];
        for preference in [
            SourcePreference::ChinaFirst,
            SourcePreference::InternationalFirst,
        ] {
            let (urls, pin) = sources(&package.executable_sha256, preference).unwrap();
            assert_eq!(pin.unwrap().sha256, package.sha256);
            assert_eq!(urls.len(), 4);
            assert_eq!(
                if preference == SourcePreference::ChinaFirst {
                    &urls[0]
                } else {
                    &urls[3]
                },
                &package.mirror_url
            );
            let unknown = "a".repeat(64);
            let (urls, pin) = sources(&unknown, preference).unwrap();
            assert!(pin.is_none());
            assert_eq!(urls, SOURCES.map(|base| format!("{base}{unknown}.zip")));
        }
    }

    #[tokio::test]
    #[ignore = "opt-in: copies the supplied native executable to a disposable fixture and downloads the reviewed GitCode cache; never starts a game or edits the source"]
    async fn asa_reviewed_symbol_mirror_prepares_owned_copy() {
        let source_path = PathBuf::from(
            std::env::var_os("LANGAME_ARK_SYMBOLS_NATIVE_EXE").expect("native ASA executable path"),
        );
        assert!(source_path.is_absolute());
        assert_eq!(source_path.file_name().unwrap(), ASA.executable);
        let fixture = super::super::tests::Fixture::new();
        let mut source = File::open(&source_path).expect("read-only native executable handle");
        let before = source.metadata().unwrap();
        let mut target = File::create(fixture.bin().join(ASA.executable)).unwrap();
        assert_eq!(
            std::io::copy(&mut source, &mut target).unwrap(),
            before.len()
        );
        target.sync_all().unwrap();
        drop(target);
        let inspection = cache::Prepared::inspect(&fixture.0).unwrap();
        let expected_hash = inspection.executable_hash.clone();
        assert!(
            reviewed_caches()
                .unwrap()
                .iter()
                .any(|package| package.executable_sha256 == expected_hash)
        );
        assert!(!inspection.ready(), "fixture must exercise real download");
        drop(inspection);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = events.clone();
        let report: InstallReporter = std::sync::Arc::new(move |update| {
            if update.phase == InstallPhase::Connecting {
                captured.lock().unwrap().push(update.source);
            }
        });
        let prepared = prepare(fixture.0.clone(), SourcePreference::ChinaFirst, report)
            .await
            .unwrap();
        assert!(prepared.ready());
        assert_eq!(prepared.executable_hash, expected_hash);
        assert_eq!(
            *events.lock().unwrap(),
            vec![Some("api.gitcode.com".to_owned())],
            "a fallback is not a mirror verification"
        );
        prepared.publish().unwrap();
        assert!(cache::Prepared::inspect(&fixture.0).unwrap().ready());
        let after = source.metadata().unwrap();
        assert_eq!(before.len(), after.len());
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        drop(source);
        println!(
            "ASA_SYMBOL_MIRROR_VERIFIED executable_sha256={expected_hash} source=api.gitcode.com ready=true source_access=read_only copied_fixture=true game_started=false"
        );
        fixture.finish();
    }

    #[tokio::test]
    async fn asa_reviewed_symbol_download_rejects_changed_bytes_before_extraction() {
        let fixture = super::super::tests::Fixture::new();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let report: InstallReporter = std::sync::Arc::new(|_| {});
        for valid in [true, false] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/cache.zip", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                assert!(stream.read(&mut request).await.unwrap() > 0);
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\ncomplete").await.unwrap();
            });
            let mut pin = reviewed_caches().unwrap()[0].clone();
            pin.size = 8;
            pin.sha256 = if valid {
                digest(b"complete")
            } else {
                "0".repeat(64)
            };
            let output = fixture
                .bin()
                .join(if valid { "verified.zip" } else { "changed.zip" });
            let result = download(
                &client,
                &url,
                &output,
                Instant::now() + Duration::from_secs(5),
                &report,
                Some(&pin),
            )
            .await;
            server.await.unwrap();
            if valid {
                result.unwrap();
                assert_eq!(fs::read(output).unwrap(), b"complete");
            } else {
                assert!(result.unwrap_err().contains("reviewed size or SHA-256"));
            }
        }
        fixture.finish();
    }

    #[tokio::test]
    async fn asa_symbol_download_streams_complete_body_and_rejects_oversized_header() {
        let fixture = super::super::tests::Fixture::new();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let report: InstallReporter = std::sync::Arc::new(|_| {});
        for oversized in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/fixture.zip", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                let received = stream.read(&mut request).await.unwrap();
                assert!(received > 0, "client closed before sending its request");
                let size = if oversized { cache::MAX_ARCHIVE + 1 } else { 8 };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n"
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                if !oversized {
                    stream.write_all(b"complete").await.unwrap();
                }
            });
            let path = fixture.bin().join(if oversized {
                "oversized.zip"
            } else {
                "complete.zip"
            });
            let result = download(
                &client,
                &url,
                &path,
                Instant::now() + Duration::from_secs(5),
                &report,
                None,
            )
            .await;
            server.await.unwrap();
            if oversized {
                assert!(result.unwrap_err().contains("bounded download size"));
                assert!(!path.exists());
            } else {
                result.unwrap();
                assert_eq!(fs::read(path).unwrap(), b"complete");
            }
        }
        fixture.finish();
    }
}
