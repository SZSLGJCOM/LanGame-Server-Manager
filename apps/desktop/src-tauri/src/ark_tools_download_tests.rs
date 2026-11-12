use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::*;

fn test_package(bytes: &[u8]) -> Package {
    let mut package = package("arksurvivalevolved").unwrap().clone();
    package.size = bytes.len();
    package.sha256 = digest(bytes);
    package
}

fn test_report() -> (InstallReporter, Arc<Mutex<Vec<InstallUpdate>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&events);
    (
        Arc::new(move |event| recorded.lock().unwrap().push(event)),
        events,
    )
}

fn test_client() -> Client {
    Client::builder().no_proxy().build().unwrap()
}

async fn request(stream: &mut tokio::net::TcpStream) {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(stream.read_u8().await.unwrap());
        assert!(bytes.len() < 16 * 1024);
    }
}

async fn source(response: &'static [u8]) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/archive.zip", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        request(&mut stream).await;
        stream.write_all(response).await.unwrap();
    });
    (url, server)
}

#[test]
fn pinned_packages_keep_original_upstream_identities_and_locale_order() {
    for (module, size, hash) in [
        (
            "arksurvivalevolved",
            11_003_472,
            "169533afd6529dcdb6c58e5b0bfd13f5d90f593f0e1188e4c156104db53e84f6",
        ),
        (
            "arksurvivalascended",
            32_408_318,
            "64f5f1165ef73f3439bba3794e77e379489167cc385c0ac5c4717ca60ad3e88a",
        ),
    ] {
        let package = package(module).unwrap();
        assert_eq!(package.size, size);
        assert_eq!(package.sha256, hash);
        assert_eq!(
            sources(package, SourcePreference::ChinaFirst),
            [package.mirror_url.as_str(), package.upstream_url.as_str()]
        );
        assert_eq!(
            sources(package, SourcePreference::InternationalFirst),
            [package.upstream_url.as_str(), package.mirror_url.as_str()]
        );
        assert!(allowed_delivery(&package.upstream_url.parse().unwrap()));
        assert!(allowed_delivery(&package.mirror_url.parse().unwrap()));
    }
    assert!(package("other-game").is_err());
}

#[test]
fn delivery_redirects_keep_https_and_approved_provider_hosts() {
    for url in [
        "https://release-assets.githubusercontent.com/assets/archive?signature=temporary",
        "https://download.example.myhuaweicloud.com/archive?signature=temporary",
    ] {
        assert!(allowed_delivery(&url.parse().unwrap()));
    }
    for url in [
        "http://api.gitcode.com/archive",
        "https://127.0.0.1/archive",
        "https://api.gitcode.com.attacker.invalid/archive",
        "https://attacker.invalid/archive",
        "https://user:fixture@api.gitcode.com/archive",
        "https://api.gitcode.com:8443/archive",
    ] {
        assert!(!allowed_delivery(&url.parse().unwrap()));
    }
}

#[tokio::test]
async fn real_partial_bytes_are_reported_before_the_archive_completes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/archive.zip", listener.local_addr().unwrap());
    let (release, released) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npa")
            .await
            .unwrap();
        released.await.unwrap();
        stream.write_all(b"ck").await.unwrap();
    });
    let gate = Mutex::new(Some(release));
    let (record, events) = test_report();
    let report: InstallReporter = Arc::new(move |event| {
        if matches!(event.phase, InstallPhase::Downloading) && event.downloaded_bytes == Some(2) {
            gate.lock().unwrap().take().unwrap().send(()).unwrap();
        }
        record(event);
    });
    let bytes = tokio::time::timeout(
        Duration::from_secs(3),
        fetch_from(
            &test_client(),
            &test_package(b"pack"),
            &[&url],
            Duration::from_secs(2),
            &report,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(bytes, b"pack");
    server.await.unwrap();
    let events = events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.downloaded_bytes == Some(2) && event.total_bytes == Some(4))
    );
    assert!(matches!(
        events.last().unwrap().phase,
        InstallPhase::Verifying
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.phase, InstallPhase::Ready))
    );
}

#[tokio::test]
async fn corrupt_truncated_and_oversized_mirrors_fall_back_without_reusing_bytes() {
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbad!".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npa",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nlarge\r\n0\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\nConnection: close\r\n\r\n",
    ] {
        let (bad, failed) = source(response).await;
        let (good, succeeded) = source(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npack").await;
        let (report, events) = test_report();
        let bytes = fetch_from(&test_client(), &test_package(b"pack"), &[&bad, &good],
            Duration::from_secs(3), &report).await.unwrap();
        assert_eq!(bytes, b"pack");
        failed.await.unwrap();
        succeeded.await.unwrap();
        let events = events.lock().unwrap();
        let starts: Vec<_> = events.iter().filter(|event| matches!(event.phase, InstallPhase::Connecting)).collect();
        assert_eq!(starts.len(), 2);
        assert!(starts.iter().all(|event| event.downloaded_bytes == Some(0)));
    }
}

#[tokio::test]
async fn stalled_body_preserves_a_bounded_opportunity_for_the_other_source() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let stalled = format!("http://{}/archive.zip", listener.local_addr().unwrap());
    let (release, released) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npa")
            .await
            .unwrap();
        let _ = released.await;
    });
    let (good, succeeded) =
        source(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npack").await;
    let (report, _) = test_report();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        fetch_from(
            &test_client(),
            &test_package(b"pack"),
            &[&stalled, &good],
            Duration::from_secs(1),
            &report,
        ),
    )
    .await;
    drop(release);
    server.await.unwrap();
    assert_eq!(result.unwrap().unwrap(), b"pack");
    succeeded.await.unwrap();
}

#[tokio::test]
async fn denial_and_retry_windows_do_not_trigger_another_source() {
    for response in [
        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".as_slice(),
        b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ] {
        let (denied, server) = source(response).await;
        let unused = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let alternate = format!("http://{}/archive.zip", unused.local_addr().unwrap());
        let (report, _) = test_report();
        assert!(fetch_from(&test_client(), &test_package(b"pack"), &[&denied, &alternate],
            Duration::from_secs(2), &report).await.is_err());
        server.await.unwrap();
        assert_eq!(unused.into_std().unwrap().accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    }
}
