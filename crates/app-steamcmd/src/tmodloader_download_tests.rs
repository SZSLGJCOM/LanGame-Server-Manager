use super::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

enum Reply {
    StallAfterHeaders,
    Complete(&'static [u8]),
}

struct Route {
    url: String,
    requests: Arc<AtomicU64>,
    service: tokio::task::JoinHandle<()>,
}

impl Drop for Route {
    fn drop(&mut self) {
        self.service.abort();
    }
}

async fn serve(replies: Vec<Reply>) -> Route {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/asset", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicU64::new(0));
    let count = requests.clone();
    let service = tokio::spawn(async move {
        let mut stalled = Vec::new();
        for reply in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            count.fetch_add(1, Ordering::Relaxed);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            match reply {
                Reply::StallAfterHeaders => stalled.push(socket),
                Reply::Complete(body) => socket.write_all(body).await.unwrap(),
            }
        }
        // Keep incomplete responses open until the test's Route is dropped.
        if !stalled.is_empty() {
            std::future::pending::<()>().await;
        }
    });
    Route {
        url,
        requests,
        service,
    }
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let p = crate::tests::unique_test_root();
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn integrity() -> DownloadIntegrity<'static> {
    DownloadIntegrity {
        sha256: Some("239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5"), // payload
        size: Some(7),
        ..Default::default()
    }
}

#[tokio::test]
async fn slow_preferred_source_remains_available_after_a_wrong_same_size_mirror() {
    let primary = serve(vec![Reply::StallAfterHeaders, Reply::Complete(b"payload")]).await;
    let alternate = serve(vec![Reply::Complete(b"garbage")]).await;
    let paths = Directory::new();
    let urls = [primary.url.clone(), alternate.url.clone()];
    let deadline = InstallDeadline::new("fixture", Duration::from_secs(10));
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let file = download_ordered(
        &client,
        &urls,
        &paths.0.join("release.zip"),
        integrity(),
        deadline,
        Duration::from_millis(50),
        &mut |_| {},
    )
    .await
    .unwrap();
    assert_eq!(fs::read(file.path()).unwrap(), b"payload");
    assert_eq!(primary.requests.load(Ordering::Relaxed), 2);
    assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
    drop(file);
    assert_eq!(fs::read_dir(&paths.0).unwrap().count(), 0);
}

#[tokio::test]
async fn cancelling_the_owner_does_not_switch_sources_or_leave_partial_files() {
    let primary = serve(vec![Reply::StallAfterHeaders]).await;
    let alternate = serve(vec![Reply::Complete(b"payload")]).await;
    let paths = Directory::new();
    let urls = [primary.url.clone(), alternate.url.clone()];
    let deadline = InstallDeadline::new("fixture", Duration::from_secs(5));
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let owner = InstallCancellation::new();
    let result = owner
        .scope(download_ordered(
            &client,
            &urls,
            &paths.0.join("release.zip"),
            integrity(),
            deadline,
            FIRST_WINDOW,
            &mut |update| {
                if update
                    .install_progress
                    .is_some_and(|progress| progress.downloaded_bytes.is_some())
                {
                    owner.cancel();
                }
            },
        ))
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(alternate.requests.load(Ordering::Relaxed), 0);
    assert_eq!(fs::read_dir(&paths.0).unwrap().count(), 0);
}

#[tokio::test]
async fn slow_response_headers_do_not_start_the_body_speed_window() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let primary = [format!("http://{}/asset", listener.local_addr().unwrap())];
    let paths = Directory::new();
    let destination = paths.0.join("release.zip");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let owner = InstallCancellation::new();
    let request = owner.scope(preferred_download(
        &client,
        &primary,
        &destination,
        integrity(),
        InstallDeadline::new("fixture", Duration::from_secs(5)),
        Duration::ZERO,
        |_, _| {},
    ));
    tokio::pin!(request);
    let _connected = tokio::select! {
        result = &mut request => panic!("no response was sent: {}", result.is_ok()),
        connection = listener.accept() => connection.unwrap(),
    };
    // The fixture intentionally holds the response headers. Even a zero
    // body-observation window must stay unarmed until they arrive.
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut request)
            .await
            .is_err()
    );
    owner.cancel();
    assert!(matches!(
        request.await,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(fs::read_dir(&paths.0).unwrap().count(), 0);
}
