//! Matching symbol preparation belongs to the explicit, stopped-instance
//! installation command. Ordinary status inspection and startup never download.
use super::*;
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

fn allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some("cdn.pelayori.com" | "cdn.shadowhunter.co.za" | "cdn.shadowhunter-systems.co.za")
        )
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .user_agent("LanGame-Server-Manager/ARK-Symbols")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 || !allowed(attempt.url()) {
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
    if total.is_some_and(|size| size == 0 || size > cache::MAX_ARCHIVE) {
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
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| error.without_url().to_string())?
    {
        if chunk.len() as u64 > cache::MAX_ARCHIVE.saturating_sub(received) {
            return Err("ASA symbol archive exceeds its bounded download size".into());
        }
        output
            .write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
        received += chunk.len() as u64;
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
    output.sync_all().await.map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) async fn prepare(
    root: PathBuf,
    report: InstallReporter,
) -> Result<cache::Prepared, String> {
    let mut prepared = tokio::task::spawn_blocking(move || cache::Prepared::inspect(&root))
        .await
        .map_err(|error| format!("ASA symbol inspection task failed: {error}"))??;
    if prepared.ready() {
        return Ok(prepared);
    }
    let client = client()?;
    let deadline = Instant::now() + BUDGET;
    let mut failures = Vec::new();
    for (index, source) in SOURCES.iter().enumerate() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let source_deadline = Instant::now() + remaining / (SOURCES.len() - index) as u32;
        let path = prepared.archive_path()?;
        let url = format!("{source}{}.zip", prepared.executable_hash);
        let fetched = timeout_at(
            source_deadline,
            download(&client, &url, &path, source_deadline, &report),
        )
        .await
        .unwrap_or_else(|_| Err("ASA symbol download timed out".into()));
        let result = match fetched {
            Ok(()) => {
                report(InstallUpdate {
                    phase: InstallPhase::Verifying,
                    source: Some((*source).into()),
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
            assert!(allowed(&reqwest::Url::parse(source).unwrap()));
        }
        for source in [
            "http://cdn.pelayori.com/cache/",
            "https://cdn.pelayori.com:8443/cache/",
            "https://user@cdn.pelayori.com/cache/",
            "https://cdn.pelayori.com.example.org/cache/",
        ] {
            assert!(!allowed(&reqwest::Url::parse(source).unwrap()));
        }
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
