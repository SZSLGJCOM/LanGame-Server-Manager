use super::*;
use crate::InstallCancellation;
use std::time::Duration;

async fn read_request_headers(connection: &mut tokio::net::TcpStream) {
    use tokio::io::AsyncReadExt;
    let mut headers = Vec::new();
    let mut buffer = [0; 1024];
    while headers.len() < 8192 {
        let count = connection.read(&mut buffer).await.unwrap();
        assert!(count > 0, "Fixture request ended before its headers");
        headers.extend_from_slice(&buffer[..count]);
        if headers.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            return;
        }
    }
    panic!("Fixture request headers exceeded 8 KiB");
}

fn transport_error(retryable: bool) -> SteamCmdError {
    SteamCmdError::HttpDownload {
        url: "https://aka.ms/runtime".into(),
        detail: "transport fixture".into(),
        permits_source_fallback: true,
        source_unavailable: true,
        permits_ip_family_retry: retryable,
    }
}

#[test]
fn only_typed_transport_failure_recovers_the_observed_dynamic_target() {
    let initial = Url::parse("https://aka.ms/vc14/vc_redist.x64.exe").unwrap();
    let target = Url::parse(
        "https://download.visualstudio.microsoft.com/current/runtime.exe?ticket=private",
    )
    .unwrap();
    let mut trace = RedirectTrace {
        target: initial,
        depth: 0,
        rejected: false,
    };
    assert!(trace.observe(&target, 2));
    assert!(trace.observe(
        &Url::parse("https://aka.ms/vs/18/release/vc_redist.x64.exe").unwrap(),
        1
    ));
    assert_eq!(trace.recovery_target(&transport_error(true)), Some(target));
    assert!(trace.recovery_target(&transport_error(false)).is_none());
    // A partial stream, validation failure or operation timeout is not a new
    // transport handshake. Their existing restart/cancellation owner decides.
    assert!(
        trace
            .recovery_target(&SteamCmdError::HttpDownloadInterrupted {
                detail: "body truncated".into()
            })
            .is_none()
    );
    assert!(
        trace
            .recovery_target(&crate::operation_timeout(InstallDeadline::new(
                "fixture",
                Duration::ZERO
            )))
            .is_none()
    );
}

#[test]
fn unsafe_redirects_and_excess_hops_permanently_prevent_recovery() {
    for invalid in [
        "http://download.microsoft.com/runtime.exe",
        "https://download.microsoft.com.evil.invalid/runtime.exe",
        "https://user:password@download.microsoft.com/runtime.exe",
        "https://download.microsoft.com:8443/runtime.exe",
        "https://127.0.0.1/runtime.exe",
    ] {
        let trusted = Url::parse("https://download.microsoft.com/runtime.exe").unwrap();
        let mut trace = RedirectTrace {
            target: trusted.clone(),
            depth: 0,
            rejected: false,
        };
        assert!(
            !trace.observe(&Url::parse(invalid).unwrap(), 1),
            "{invalid}"
        );
        assert!(!trace.observe(&trusted, 2));
        assert!(trace.recovery_target(&transport_error(true)).is_none());
    }
    let trusted = Url::parse("https://download.microsoft.com/runtime.exe").unwrap();
    let mut trace = RedirectTrace {
        target: trusted.clone(),
        depth: 0,
        rejected: false,
    };
    assert!(trace.observe(&trusted, 10));
    assert!(!trace.observe(&trusted, 11));
    assert!(trace.recovery_target(&transport_error(true)).is_none());
}

#[tokio::test]
async fn cancellation_and_untrusted_initial_url_cannot_start_either_route() {
    let cancellation = InstallCancellation::new();
    cancellation.cancel();
    let mut events = Vec::new();
    let result = cancellation
        .scope(download_native_package_with_progress(
            "https://aka.ms/vc14/vc_redist.x64.exe",
            Path::new("never-created-runtime.exe"),
            DownloadIntegrity::default(),
            InstallDeadline::new("cancelled native download", Duration::from_secs(30)),
            |event| events.push(event),
        ))
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert!(events.is_empty());
    let result = download_native_package_with_progress(
        "http://127.0.0.1/runtime.exe",
        Path::new("never-created-runtime.exe"),
        DownloadIntegrity::default(),
        InstallDeadline::new("untrusted native URL", Duration::from_secs(30)),
        |event| events.push(event),
    )
    .await;
    assert!(result.is_err());
    assert!(events.is_empty());
}

#[tokio::test]
async fn real_connection_failure_is_retryable_but_server_deferral_is_not() {
    use tokio::io::AsyncWriteExt;
    let client = http_client_builder().no_proxy().build().unwrap();
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = closed.local_addr().unwrap();
    drop(closed);
    let error = super::super::response(
        &client,
        &format!("http://{address}/"),
        InstallDeadline::new("connection failure fixture", Duration::from_secs(5)),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        SteamCmdError::HttpDownload {
            permits_ip_family_retry: true,
            ..
        }
    ));

    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let serve = tokio::spawn(async move {
        let (mut connection, _) = tokio::time::timeout(Duration::from_secs(5), server.accept())
            .await
            .unwrap()
            .unwrap();
        read_request_headers(&mut connection).await;
        connection.write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 3600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
    });
    let error = super::super::response(
        &client,
        &format!("http://{address}/"),
        InstallDeadline::new("server deferral fixture", Duration::from_secs(5)),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        SteamCmdError::HttpDownload {
            permits_ip_family_retry: false,
            ..
        }
    ));
    serve.await.unwrap();
}

#[tokio::test]
async fn short_header_budget_keeps_transport_classification_and_owner_time() {
    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let hanging = tokio::spawn(async move {
        let (_connection, _) = server.accept().await.unwrap();
        std::future::pending::<()>().await;
    });
    let client = http_client_builder().no_proxy().build().unwrap();
    let owner = InstallDeadline::new("short header fixture", Duration::from_secs(5));
    let error = super::super::response_with_budget(
        &client,
        &format!("http://{address}/"),
        owner,
        Duration::from_millis(100),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        SteamCmdError::HttpDownload {
            permits_ip_family_retry: true,
            ..
        }
    ));
    assert!(owner.check_cancelled().is_ok());
    hanging.abort();
    assert!(hanging.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn healthy_body_can_outlive_the_header_recovery_reservation() {
    use tokio::io::AsyncWriteExt;
    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let serve = tokio::spawn(async move {
        let (mut connection, _) = server.accept().await.unwrap();
        read_request_headers(&mut connection).await;
        connection
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        connection.write_all(b"abc").await.unwrap();
    });
    let directory = std::env::temp_dir().join(format!(
        "lgsm-native-body-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let client = http_client_builder().no_proxy().build().unwrap();
    let downloaded = super::super::download_file_with_header_budget(
        &client,
        &format!("http://{address}/"),
        &directory.join("package.exe"),
        DownloadIntegrity {
            size: Some(3),
            ..Default::default()
        },
        InstallDeadline::new("healthy body fixture", Duration::from_secs(10)),
        Duration::from_secs(1),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), b"abc");
    drop(downloaded);
    std::fs::remove_dir(directory).unwrap();
    serve.await.unwrap();
}
