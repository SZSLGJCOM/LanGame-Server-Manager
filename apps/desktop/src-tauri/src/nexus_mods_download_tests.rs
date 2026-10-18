use super::*;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::thread;

#[test]
fn nexus_download_origins_are_official_https_mirrors_only() {
    for url in [
        "https://premium-files.nexus-cdn.com/12/88/Mod.zip?fixture=signature",
        "https://cf-files.nexusmods.com/12/88/Mod.zip?fixture=signature",
    ] {
        assert!(official_download_url(url));
        let request = download_client().unwrap().get(url).build().unwrap();
        assert!(!request.headers().contains_key("apikey"));
    }
    for value in [
        "http://files.nexus-cdn.com/Mod.zip",
        "https://files.nexus-cdn.com.evil.test/Mod.zip",
        "https://evil-nexus-cdn.com/Mod.zip",
        "https://user@files.nexus-cdn.com/Mod.zip",
        "https://127.0.0.1/Mod.zip",
        "https://api.nexusmods.com/v1/users/validate",
        "https://cf-files.nexusmods.com.evil.example/Mod.zip",
        "https://user@cf-files.nexusmods.com/Mod.zip",
        "https://cf-files.nexusmods.com:8443/Mod.zip",
        "http://cf-files.nexusmods.com/Mod.zip",
        "https://unapproved.nexusmods.com/Mod.zip",
    ] {
        assert!(!official_download_url(value));
    }
}

#[test]
fn nexus_download_raii_cleans_only_its_owned_directory() {
    let directory = TemporaryDirectory::new().unwrap();
    let root = directory.0.clone();
    fs::write(root.join("owned.zip"), b"owned").unwrap();
    let retained = directory.clone();
    drop(directory);
    assert!(root.exists());
    drop(retained);
    assert!(!root.exists());
}

#[tokio::test]
async fn nexus_selection_guards_run_before_any_authenticated_request() {
    let source: ModuleModSourceSpec = serde_json::from_value(json!({"provider": "nexus", "label": "Nexus", "url": "https://www.nexusmods.com/abioticfactor"})).unwrap();
    let mismatched = [NexusFileSelection {
        reference: "nxm://abioticfactor/mods/12/files/88?key=fixture-secret&expires=200&user_id=42"
            .into(),
        file_id: "89".into(),
    }];
    let error = download_selected_files(&source, &["zip".into()], &mismatched, "")
        .await
        .err()
        .unwrap();
    assert!(error.contains("nexus_reference_invalid"));
    assert!(!error.contains("fixture-secret"));
    let duplicates = [
        NexusFileSelection {
            reference: "12".into(),
            file_id: "88".into(),
        },
        NexusFileSelection {
            reference: "https://www.nexusmods.com/abioticfactor/mods/12".into(),
            file_id: "88".into(),
        },
    ];
    assert!(
        download_selected_files(&source, &["zip".into()], &duplicates, "")
            .await
            .err()
            .unwrap()
            .contains("same Nexus file")
    );
    let changed_website_selection = [NexusFileSelection {
        reference: "https://www.nexusmods.com/abioticfactor/mods/12?file_id=88".into(),
        file_id: "89".into(),
    }];
    // Empty credentials stop at authentication, proving website preselection does not pin the
    // user to that file. The same change for nxm is rejected earlier above.
    assert!(
        download_selected_files(&source, &["zip".into()], &changed_website_selection, "")
            .await
            .err()
            .unwrap()
            .contains("nexus_auth_invalid")
    );
}

struct FixtureServer {
    thread: Option<thread::JoinHandle<String>>,
    stop: Arc<AtomicBool>,
    stream: Arc<StdMutex<Option<TcpStream>>>,
}

impl FixtureServer {
    fn join(mut self) -> thread::Result<String> {
        self.thread.take().unwrap().join()
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(stream) = self.stream.lock()
            && let Some(stream) = stream.as_ref()
        {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct AbortCallerOnDrop(tokio::task::AbortHandle);

impl Drop for AbortCallerOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn serve_file(body: &'static [u8], stall: bool) -> (String, FixtureServer) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let stream_owner = Arc::new(StdMutex::new(None));
    let worker_stream_owner = stream_owner.clone();
    let task = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            if worker_stop.load(Ordering::Acquire) {
                return String::new();
            }
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "Nexus file test request timed out"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("Nexus file test accept failed: {error}"),
            }
        };
        // Winsock can carry the listener's nonblocking state onto accepted sockets.
        stream.set_nonblocking(false).unwrap();
        *worker_stream_owner.lock().unwrap() = Some(stream.try_clone().unwrap());
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            assert!(request.len() < 16384);
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).unwrap(), 1);
            request.push(byte[0]);
        }
        let length = if stall { 1048576 } else { body.len() };
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.write_all(body).unwrap();
        stream.flush().unwrap();
        if stall {
            let mut byte = [0];
            assert_eq!(
                stream.read(&mut byte).unwrap(),
                0,
                "cancelled Nexus client must close its stream"
            );
        }
        String::from_utf8(request).unwrap()
    });
    (
        format!("http://{address}/fixture?fixture-signature=secret"),
        FixtureServer {
            thread: Some(task),
            stop,
            stream: stream_owner,
        },
    )
}

fn file_metadata(filename: &str, version: &str, exact_size: Option<u64>) -> api::NexusFile {
    serde_json::from_value(json!({
        "file_id": 88, "name": "Fixture Mod", "file_name": filename, "version": version,
        "size_in_bytes": exact_size, "size_kb": 1, "category_id": 1, "category_name": "MAIN"
    }))
    .unwrap()
}

fn local_download_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[tokio::test]
async fn nexus_download_preserves_original_filename_and_cleans_after_staging_owner_drops() {
    let directory = TemporaryDirectory::new().unwrap();
    let root = directory.0.clone();
    let (url, server) = serve_file(b"payload", false);
    let path = download_one(
        local_download_client(),
        directory.clone(),
        0,
        url,
        file_metadata("Example_P.pak", "1.0", Some(7)),
    )
    .await
    .unwrap();
    assert_eq!(path.file_name().unwrap(), "Example_P.pak");
    assert_eq!(fs::read(&path).unwrap(), b"payload");
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(!request.contains("apikey:"));
    let downloads = NexusDownloads {
        directory,
        paths: vec![path.clone()],
    };
    assert_eq!(downloads.paths(), &[path]);
    drop(downloads);
    assert!(!root.exists());
}

#[tokio::test]
async fn nexus_exact_size_mismatch_removes_the_owned_download_before_any_install() {
    let directory = TemporaryDirectory::new().unwrap();
    let root = directory.0.clone();
    let (url, server) = serve_file(b"payload", false);
    let error = download_one(
        local_download_client(),
        directory.clone(),
        0,
        url,
        file_metadata("Example.zip", "1.0", Some(8)),
    )
    .await
    .err()
    .unwrap();
    assert!(error.contains("size does not match"));
    assert!(!error.contains("fixture-signature"));
    server.join().unwrap();
    drop(directory);
    assert!(!root.exists());
}

#[tokio::test]
async fn nexus_dropped_caller_cancels_stream_closes_writer_and_removes_temp_directory() {
    let directory = TemporaryDirectory::new().unwrap();
    let root = directory.0.clone();
    let (url, server) = serve_file(&[b'z'; 4096], true);
    let caller = tokio::spawn(download_one(
        local_download_client(),
        directory,
        0,
        url,
        file_metadata("Example.zip", "1.0", None),
    ));
    let _abort_on_failure = AbortCallerOnDrop(caller.abort_handle());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let wrote_partial = fs::read_dir(root.join("0")).ok().is_some_and(|entries| {
            entries
                .flatten()
                .any(|entry| fs::read(entry.path()).is_ok_and(|bytes| !bytes.is_empty()))
        });
        if wrote_partial {
            break;
        }
        if caller.is_finished() {
            let result = caller.await;
            panic!("Nexus caller ended before writing the fixture bytes: {result:?}");
        }
        assert!(
            Instant::now() < deadline,
            "Nexus stream never wrote the fixture bytes"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let cleanup_deadline = Instant::now() + Duration::from_secs(5);
    while root.exists() {
        assert!(
            Instant::now() < cleanup_deadline,
            "Nexus cancellation retained its owned temp directory"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let request = tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
    assert!(!request.to_ascii_lowercase().contains("apikey:"));
}

#[tokio::test]
async fn nexus_long_official_filename_and_version_use_a_short_intermediate_name() {
    let filename = format!("{}.pak", "m".repeat(216));
    assert_eq!(filename.len(), 220);
    let file = file_metadata(&filename, &"v".repeat(512), Some(7));
    let reference = parse_reference("12", "abioticfactor").unwrap();
    let choice = file_choice(&file, &["pak".into()], &reference).unwrap();
    assert!(choice.downloadable);
    assert_eq!(choice.version.len(), 512);
    let directory = TemporaryDirectory::new().unwrap();
    let root = directory.0.clone();
    let (url, server) = serve_file(b"payload", false);
    let path = download_one(local_download_client(), directory.clone(), 0, url, file)
        .await
        .unwrap();
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), filename);
    assert_eq!(fs::read(&path).unwrap(), b"payload");
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    server.join().unwrap();
    drop(directory);
    assert!(!root.exists());
}
