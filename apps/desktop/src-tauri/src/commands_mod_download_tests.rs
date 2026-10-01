use super::*;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Instant;

const HTTP_TEST_TIMEOUT: Duration = Duration::from_secs(5);

fn download_test_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(HTTP_TEST_TIMEOUT)
        .build()
        .expect("build direct local download test client")
}

fn download_test_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "langame-mod-download-test-{}-{}",
        label,
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn serve_single_response(response_parts: Vec<Vec<u8>>) -> (String, thread::JoinHandle<()>) {
    serve_responses(vec![response_parts])
}

fn serve_repeated_status(
    status: u16,
) -> (
    String,
    std::sync::mpsc::Sender<()>,
    thread::JoinHandle<usize>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/package", listener.local_addr().unwrap());
    let (stop, stopped) = std::sync::mpsc::channel();
    let task = thread::spawn(move || {
        let deadline = Instant::now() + HTTP_TEST_TIMEOUT;
        let mut count = 0;
        while stopped.try_recv().is_err() {
            assert!(Instant::now() < deadline, "status fixture was not stopped");
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream.set_write_timeout(Some(HTTP_TEST_TIMEOUT)).unwrap();
                    read_request_headers(&mut stream);
                    count += 1;
                    write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("status fixture failed: {error}"),
            }
        }
        count
    });
    (url, stop, task)
}

#[tokio::test]
async fn header_failures_do_not_multiply_the_shared_retry_limit() {
    for (status, expected_requests) in [(404, 1), (503, 2)] {
        let client = download_test_client();
        let root = download_test_root("header-retry-limit");
        let (url, stop, server) = serve_repeated_status(status);
        let result = download_mod_site_file_with_limit(
            &client,
            ModDownloadRequest {
                download_url: &url,
                package_name: "package",
                version: "1.0",
                preferred_filename: None,
                fallback_extension: "zip",
                integrity: None,
                max_bytes: 20,
                download_root: &root,
            },
        )
        .await;
        stop.send(()).unwrap();
        assert_eq!(server.join().unwrap(), expected_requests, "HTTP {status}");
        assert!(result.unwrap_err().contains(&status.to_string()));
        assert_download_root_empty(&root);
        fs::remove_dir_all(root).unwrap();
    }
}

fn serve_responses(responses: Vec<Vec<Vec<u8>>>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener
        .set_nonblocking(true)
        .expect("make download test listener nonblocking");
    let handle = thread::spawn(move || {
        for response_parts in responses {
            let deadline = Instant::now() + HTTP_TEST_TIMEOUT;
            let mut stream = loop {
                assert!(
                    Instant::now() < deadline,
                    "download test server at {address} received no connection within {HTTP_TEST_TIMEOUT:?}"
                );
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => {
                        panic!("download test server at {address} failed to accept: {error}")
                    }
                }
            };
            stream
                .set_nonblocking(false)
                .expect("make download test connection blocking");
            stream
                .set_write_timeout(Some(HTTP_TEST_TIMEOUT))
                .expect("bound download test response writes");
            read_request_headers(&mut stream);
            for part in response_parts {
                if stream.write_all(&part).is_err() || stream.flush().is_err() {
                    break;
                }
            }
        }
    });
    (format!("http://{address}/package"), handle)
}

#[tokio::test]
async fn interrupted_download_restarts_with_a_clean_file() {
    let client = download_test_client();
    let root = download_test_root("restart-stream");
    let (url, server) = serve_responses(vec![
        vec![b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nab".to_vec()],
        vec![b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngood".to_vec()],
    ]);
    let path = download_mod_site_file_with_limit(
        &client,
        ModDownloadRequest {
            download_url: &url,
            package_name: "package",
            version: "1.0",
            preferred_filename: None,
            fallback_extension: "zip",
            integrity: None,
            max_bytes: 20,
            download_root: &root,
        },
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(fs::read(path).unwrap(), b"good");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_rate_limited_download_does_not_restart_or_publish() {
    let client = download_test_client();
    let root = download_test_root("rate-limit");
    let (url, server) = serve_single_response(vec![
        b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
    ]);
    let error = download_mod_site_file_with_limit(
        &client,
        ModDownloadRequest {
            download_url: &url,
            package_name: "package",
            version: "1.0",
            preferred_filename: None,
            fallback_extension: "zip",
            integrity: None,
            max_bytes: 20,
            download_root: &root,
        },
    )
    .await
    .unwrap_err();
    server.join().unwrap();
    assert!(error.contains("429"), "{error}");
    assert_download_root_empty(&root);
    fs::remove_dir_all(root).unwrap();
}

fn read_request_headers(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
    }
}

fn assert_download_root_empty(root: &Path) {
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}

#[tokio::test]
async fn chunked_download_is_stopped_at_the_streaming_limit() {
    let client = download_test_client();
    let root = download_test_root("chunk-limit");
    let (url, server) = serve_single_response(vec![
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec(),
        b"4\r\nABCD\r\n".to_vec(),
        b"5\r\nEFGHI\r\n0\r\n\r\n".to_vec(),
    ]);
    let result = download_mod_site_file_with_limit(
        &client,
        ModDownloadRequest {
            download_url: &url,
            package_name: "package",
            version: "1.0",
            preferred_filename: None,
            fallback_extension: "zip",
            integrity: None,
            max_bytes: 8,
            download_root: &root,
        },
    )
    .await;

    server.join().expect("local download test server failed");
    let error = result.expect_err("chunked body must be bounded while it is read");
    assert!(error.contains("too large"), "{error}");
    assert_download_root_empty(&root);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn false_oversize_content_length_is_rejected_without_a_partial_file() {
    let client = download_test_client();
    let root = download_test_root("content-length-limit");
    let (url, server) = serve_single_response(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nx".to_vec(),
    ]);
    let result = download_mod_site_file_with_limit(
        &client,
        ModDownloadRequest {
            download_url: &url,
            package_name: "package",
            version: "1.0",
            preferred_filename: None,
            fallback_extension: "zip",
            integrity: None,
            max_bytes: 8,
            download_root: &root,
        },
    )
    .await;

    server.join().expect("local download test server failed");
    let error = result.expect_err("declared length must be rejected before creating a file");
    assert!(error.contains("too large"), "{error}");
    assert_download_root_empty(&root);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn cancelled_download_writer_removes_its_unique_file() {
    let root = download_test_root("writer-cancel");
    let path = root.join("package.zip");
    let (messages, receiver) = tokio::sync::mpsc::channel(1);
    let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
    let writer_path = path.clone();
    let writer = tokio::task::spawn_blocking(move || {
        write_downloaded_mod_file(writer_path, receiver, ready_sender, None)
    });
    ready_receiver.await.unwrap().unwrap();
    messages
        .send(ModDownloadWriteMessage::Chunk(b"partial".to_vec()))
        .await
        .unwrap();

    drop(messages);
    let error = wait_for_mod_download_writer(writer)
        .await
        .expect_err("closed channel represents cancellation");

    assert!(error.contains("cancelled"));
    assert!(!path.exists());
    assert_download_root_empty(&root);
    let _ = fs::remove_dir_all(root);
}

#[path = "commands_mod_integrity_tests.rs"]
mod integrity;
