use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn fixture_client() -> reqwest::Client {
    // Loopback fixtures must not depend on the workstation's system proxy.
    reqwest::Client::builder().no_proxy().build().unwrap()
}

struct Fixture {
    url: String,
    task: tokio::task::JoinHandle<()>,
    requests: Arc<AtomicU64>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(responses: Vec<Vec<u8>>) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/artifact", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicU64::new(0));
    let observed = Arc::clone(&requests);
    let task = tokio::spawn(async move {
        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            observed.fetch_add(1, Ordering::Relaxed);
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(&response).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    });
    Fixture {
        url,
        task,
        requests,
    }
}

fn reply(status: u16, body: &[u8]) -> Vec<u8> {
    let mut wire = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    wire.extend_from_slice(body);
    wire
}

fn destination() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "langame-http-fixture-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    root.join("server.zip")
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new("HTTP fixture", Duration::from_secs(5))
}

fn sha1(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[tokio::test]
async fn streamed_download_reports_verified_bytes_and_advertised_total() {
    let source = fixture(vec![reply(200, b"installer bytes")]).await;
    let path = destination();
    let mut progress = Vec::new();
    let downloaded = download_file_from_candidates_with_progress(
        &fixture_client(),
        std::slice::from_ref(&source.url),
        &path,
        DownloadIntegrity::default(),
        deadline(),
        &mut |bytes, total| progress.push((bytes, total)),
    )
    .await
    .unwrap();
    assert_eq!(progress.first(), Some(&(0, Some(15))));
    assert_eq!(progress.last(), Some(&(15, Some(15))));
    assert_eq!(
        std::fs::read(downloaded.path()).unwrap(),
        b"installer bytes"
    );
    drop(downloaded);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn installer_download_retries_official_request_then_uses_next_candidate() {
    let primary = fixture(vec![reply(503, b"busy"), reply(503, b"busy")]).await;
    let alternate = fixture(vec![reply(200, b"verified archive")]).await;
    let path = destination();
    std::fs::write(&path, b"previous install").unwrap();
    let checksum = sha1(b"verified archive");
    let downloaded = download_file_from_candidates(
        &fixture_client(),
        &[primary.url.clone(), alternate.url.clone()],
        &path,
        DownloadIntegrity {
            sha1: Some(&checksum),
            size: Some(16),
            ..Default::default()
        },
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(downloaded.path()).unwrap(),
        b"verified archive"
    );
    assert_eq!(downloaded.path().extension().unwrap(), "zip");
    assert_eq!(std::fs::read(&path).unwrap(), b"previous install");
    assert_eq!(downloaded.source, alternate.url);
    assert_eq!(primary.requests.load(Ordering::Relaxed), 2);
    assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
    downloaded.persist(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"verified archive");
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[path = "http_text_tests.rs"]
mod text;

#[path = "http_download_budget_tests.rs"]
mod budget;

#[cfg(windows)]
#[tokio::test]
async fn persist_failure_keeps_the_existing_target_and_cleans_the_staging_file() {
    use std::os::windows::fs::OpenOptionsExt;

    let source = fixture(vec![reply(200, b"new verified artifact")]).await;
    let path = destination();
    std::fs::write(&path, b"old verified artifact").unwrap();
    let downloaded = download_file(
        &fixture_client(),
        &source.url,
        &path,
        DownloadIntegrity::default(),
        deadline(),
    )
    .await
    .unwrap();
    // An exclusive reader prevents replacement without changing the old data.
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let result = downloaded.persist(&path);
    assert!(matches!(
        result,
        Err(SteamCmdError::WriteMinecraftServerFile { .. })
    ));
    drop(locked);
    assert_eq!(std::fs::read(&path).unwrap(), b"old verified artifact");
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn interrupted_stream_restarts_once_without_appending_partial_content() {
    let truncated =
        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\npart".to_vec();
    let source = fixture(vec![truncated, reply(200, b"whole-body")]).await;
    let path = destination();
    let checksum = sha1(b"whole-body");
    let downloaded = download_file(
        &fixture_client(),
        &source.url,
        &path,
        DownloadIntegrity {
            sha1: Some(&checksum),
            size: Some(10),
            ..Default::default()
        },
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), b"whole-body");
    assert_eq!(source.requests.load(Ordering::Relaxed), 2);
    drop(downloaded);
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        0
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn interrupted_primary_stream_switches_to_the_next_candidate() {
    let primary = fixture(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nx".to_vec(),
    ])
    .await;
    let alternate = fixture(vec![reply(200, b"payload")]).await;
    let path = destination();
    let checksum = sha1(b"payload");
    let downloaded = download_file_from_candidates(
        &fixture_client(),
        &[primary.url.clone(), alternate.url.clone()],
        &path,
        DownloadIntegrity {
            sha1: Some(&checksum),
            size: Some(7),
            ..Default::default()
        },
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), b"payload");
    drop(downloaded);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn wrong_hash_and_size_never_publish_or_leave_partial_files() {
    for integrity in [
        DownloadIntegrity {
            sha256: Some("incorrect"),
            size: Some(7),
            ..Default::default()
        },
        DownloadIntegrity {
            size: Some(6),
            ..Default::default()
        },
    ] {
        let source = fixture(vec![reply(200, b"payload")]).await;
        let path = destination();
        std::fs::write(&path, b"old verified artifact").unwrap();
        let result =
            download_file(&fixture_client(), &source.url, &path, integrity, deadline()).await;
        assert!(matches!(result, Err(SteamCmdError::HttpDownload { .. })));
        assert_eq!(std::fs::read(&path).unwrap(), b"old verified artifact");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[tokio::test]
async fn terminal_access_and_retry_after_responses_never_switch_sources() {
    for wire in [
        reply(401, b"unauthorized"), reply(403, b"forbidden"), reply(429, b"rate limited"),
        b"HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nRetry-After: 86400\r\nConnection: close\r\n\r\n".to_vec(),
        b"HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nRetry-After: Wed, 21 Oct 2099 07:28:00 GMT\r\nConnection: close\r\n\r\n".to_vec(),
    ] {
        let primary = fixture(vec![wire]).await;
        let alternate = fixture(vec![reply(200, b"must not be requested")]).await;
        let path = destination();
        let result = download_file_from_candidates(
            &fixture_client(), &[primary.url.clone(), alternate.url.clone()], &path,
            DownloadIntegrity::default(), deadline(),
        ).await;
        assert!(matches!(result, Err(SteamCmdError::HttpDownload { permits_source_fallback: false, .. })));
        assert_eq!(alternate.requests.load(Ordering::Relaxed), 0);
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 0);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[tokio::test]
async fn json_metadata_rejects_oversized_bodies_and_restarts_interrupted_streams() {
    let source = fixture(vec![
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            METADATA_LIMIT + 1
        )
        .into_bytes(),
    ])
    .await;
    let result: Result<serde_json::Value, _> =
        fetch_json(&fixture_client(), &source.url, deadline()).await;
    assert!(result.unwrap_err().to_string().contains("4 MiB"));
    let source = fixture(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{".to_vec(),
        reply(200, br#"{"valid":true}"#),
    ])
    .await;
    let value: serde_json::Value = fetch_json(&fixture_client(), &source.url, deadline())
        .await
        .unwrap();
    assert_eq!(value["valid"], true);
}

#[tokio::test]
async fn overall_deadline_and_caller_cancellation_remove_download_staging() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/stall", listener.local_addr().unwrap());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\npartial")
            .await
            .unwrap();
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    let fixture = Fixture {
        url,
        task,
        requests: Arc::new(AtomicU64::new(0)),
    };
    let path = destination();
    let client = fixture_client();
    let cancellation = crate::InstallCancellation::new();
    let mut download = Box::pin(cancellation.scope(download_file(
        &client,
        &fixture.url,
        &path,
        DownloadIntegrity::default(),
        deadline(),
    )));
    tokio::select! {
        _ = started_rx => {},
        _ = &mut download => panic!("stalled download must stay pending"),
    }
    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), &mut download)
        .await
        .expect("explicit cancellation closes the file before returning");
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    drop(download);
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        0
    );
    drop(fixture);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();

    let path = destination();
    let expired = InstallDeadline::new("expired download", Duration::ZERO);
    let result = download_file(
        &client,
        "http://127.0.0.1:1/unreachable",
        &path,
        DownloadIntegrity::default(),
        expired,
    )
    .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::OperationTimedOut { .. })
    ));
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        0
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn cancelling_after_body_writes_closes_and_removes_only_the_staging_file() {
    let expected_body = vec![42; 256 * 1024];
    let response_body = expected_body.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/stream", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let mut received = 0;
        while !request[..received]
            .windows(4)
            .any(|bytes| bytes == b"\r\n\r\n")
        {
            assert!(
                received < request.len(),
                "fixture request headers are bounded"
            );
            let length = stream.read(&mut request[received..]).await.unwrap();
            assert!(length > 0, "request must finish before responding");
            received += length;
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1048576\r\n\r\n")
            .await
            .unwrap();
        stream.write_all(&response_body).await.unwrap();
        // Keep the incomplete response open until the caller cancels.
        std::future::pending::<()>().await;
    });
    let fixture = Fixture {
        url,
        task,
        requests: Arc::new(AtomicU64::new(0)),
    };
    let path = destination();
    std::fs::write(&path, b"retained installation").unwrap();
    let client = fixture_client();
    let cancellation = crate::InstallCancellation::new();
    let mut download = Box::pin(cancellation.scope(download_file(
        &client,
        &fixture.url,
        &path,
        DownloadIntegrity::default(),
        deadline(),
    )));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            tokio::select! {
                _ = &mut download => panic!("partial body must remain pending"),
                _ = tokio::time::sleep(Duration::from_millis(5)) => {}
            }
            let body_written = std::fs::read_dir(path.parent().unwrap())
                .unwrap()
                .any(|entry| {
                    let entry = entry.unwrap();
                    // Windows directory-entry metadata can retain the old
                    // length while a writer is open. Read through a fresh file
                    // handle to prove the complete fixture body was written.
                    entry.path() != path && std::fs::read(entry.path()).unwrap() == expected_body
                });
            if body_written {
                break;
            }
        }
    })
    .await
    .expect("partial body reaches the staging file");
    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), &mut download)
        .await
        .expect("cancellation drains the file writer");
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    drop(download);
    assert_eq!(std::fs::read(&path).unwrap(), b"retained installation");
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
    drop(fixture);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
