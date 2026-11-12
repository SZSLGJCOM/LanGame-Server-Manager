use super::*;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::thread;

#[path = "nexus_mods_archive_download_tests.rs"]
mod archive_download_tests;

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

fn serve_file(body: &[u8], stall: bool) -> (String, FixtureServer) {
    let length = if stall { 1048576 } else { body.len() as u64 };
    serve_file_with_length(body, length, stall)
}

fn serve_file_with_length(body: &[u8], length: u64, stall: bool) -> (String, FixtureServer) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let stream_owner = Arc::new(StdMutex::new(None));
    let worker_stream_owner = stream_owner.clone();
    let body = body.to_vec();
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
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.write_all(&body).unwrap();
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

#[test]
fn nexus_download_failures_keep_existing_categories_without_exporting_private_details() {
    for (cause, code) in [
        (
            ModDownloadFailure::Network("https://private.invalid/?token=private".into()),
            "nexus_network_error",
        ),
        (
            ModDownloadFailure::Interrupted("private transfer detail".into()),
            "nexus_network_error",
        ),
        (
            ModDownloadFailure::Http("private HTTP detail".into()),
            "nexus_file_unavailable",
        ),
        (
            ModDownloadFailure::Terminal("private disk path".into()),
            "nexus_installation_failed",
        ),
        (
            ModDownloadFailure::SizeLimit {
                received_bytes: 257 * 1024 * 1024,
                max_bytes: 256 * 1024 * 1024,
            },
            "nexus_installation_failed",
        ),
        (ModDownloadFailure::TimeLimit, "nexus_installation_failed"),
        (ModDownloadFailure::Cancelled, "nexus_installation_failed"),
    ] {
        let error = payload_download_failure(cause);
        let parsed: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(parsed["code"], code);
        assert!(!error.contains("private"));
    }
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

#[test]
fn nexus_website_download_validates_origin_and_file_type_without_exposing_signed_values() {
    let url = "https://cf-files.nexusmods.com/12/88/Fixture.zip?token=synthetic-test-only";
    let prepared =
        prepare_website_download(url.into(), "Fixture.zip".into(), &["zip".into()]).unwrap();
    assert_eq!(prepared.url, url);
    assert_eq!(prepared.file_name, "Fixture.zip");
    for (url, filename, accepts, code) in [
        (
            "https://untrusted.example/Fixture.zip?token=synthetic-test-only",
            "Fixture.zip",
            "zip",
            "nexus_file_unavailable",
        ),
        (
            "https://cf-files.nexusmods.com/12/88/Fixture.zip#fragment",
            "Fixture.zip",
            "zip",
            "nexus_file_unavailable",
        ),
        (
            url,
            "../synthetic-test-only.zip",
            "zip",
            "nexus_file_unavailable",
        ),
        (url, "Fixture.rar", "rar", "nexus_format_unsupported"),
        (url, "Fixture.7z", "7z", "nexus_format_unsupported"),
        (url, "Camera Tweaks", "dll", "nexus_format_unsupported"),
        (url, "Fixture.dll", "pak", "nexus_format_unsupported"),
    ] {
        let error = prepare_website_download(url.into(), filename.into(), &[accepts.into()])
            .err()
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(parsed["code"], code);
        assert!(!error.contains("synthetic-test-only"));
        assert!(!error.contains("token="));
        assert!(!error.contains(url));
    }
}

fn website_zip_payload(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, contents) in entries {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(contents).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

#[tokio::test]
async fn nexus_website_display_name_downloads_are_identified_and_staged_with_presentation() {
    use crate::commands::commands_mod_staging::{
        ManualModImportPresentation, stage_website_mod_source,
    };
    use crate::commands::commands_mods::mod_inventory::read_inventory;

    let body = website_zip_payload(&[("CameraTweaks/CameraTweaks.dll", b"mod payload")]);
    for suggestion in [
        "Camera Tweaks".to_string(),
        "Camera Tweaks v1.0".into(),
        "m".repeat(240),
    ] {
        let (url, server) = serve_file(&body, false);
        let mut prepared = prepare_website_download(
            "https://cf-files.nexusmods.com/5811/fixture?token=synthetic-test-only".into(),
            suggestion,
            &["zip".into(), "dll".into()],
        )
        .unwrap();
        assert!(prepared.needs_archive_identification);
        prepared.url = url;
        let updates = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = updates.clone();
        let prepared = prepared.observe(Arc::new(move |progress| {
            observed.lock().unwrap().push(progress)
        }));
        let downloads = download_website_file_with_client(prepared, local_download_client())
            .await
            .unwrap();
        let updates = updates.lock().unwrap();
        assert!(matches!(
            updates.last(),
            Some(WebsiteDownloadProgress::Verifying)
        ));
        assert!(updates.iter().any(|update| matches!(update,
            WebsiteDownloadProgress::Downloading(progress) if progress.received_bytes == body.len() as u64)));
        drop(updates);
        assert_eq!(downloads.paths()[0].file_name().unwrap(), "payload.zip");
        let owned_download = downloads.directory.0.clone();
        let installed = TemporaryDirectory::new().unwrap();
        let target = || ResolvedManualModTarget {
            instance_id: "synthetic-nexus-display-name".into(),
            module_id: "enshrouded".into(),
            source_label: "Nexus Mods".into(),
            target_label: "mods".into(),
            target_path: installed.0.join("mods"),
            accepts: vec!["folder".into(), "zip".into(), "dll".into()],
            id_strategy: None,
        };
        let icon = "https://staticdelivery.nexusmods.com/mods/5811/images/thumbnails/315/315-1790000000-123456789.png";
        let result = stage_website_mod_source(
            target(),
            downloads.paths()[0].clone(),
            ManualModImportPresentation {
                display_name: Some("Camera Tweaks".into()),
                icon_url: Some(icon.into()),
            },
            None,
        )
        .unwrap();
        assert_eq!(result.copied_file_count, 1);
        assert_eq!(
            fs::read(target().target_path.join("CameraTweaks/CameraTweaks.dll")).unwrap(),
            b"mod payload"
        );
        let inventory = read_inventory(target()).unwrap();
        assert_eq!(inventory.items.len(), 1);
        assert_eq!(
            inventory.items[0].display_name.as_deref(),
            Some("Camera Tweaks")
        );
        assert_eq!(inventory.items[0].icon_url.as_deref(), Some(icon));
        drop(downloads);
        assert!(!owned_download.exists());
        let request = server.join().unwrap().to_ascii_lowercase();
        for credential in ["apikey:", "cookie:", "authorization:"] {
            assert!(!request.contains(credential));
        }
    }
}

#[test]
fn nexus_website_content_identification_accepts_real_empty_zip_and_zip64() {
    let mut zip64 = Vec::new();
    zip64.extend_from_slice(b"PK\x06\x06");
    zip64.extend_from_slice(&44u64.to_le_bytes());
    zip64.extend_from_slice(&45u16.to_le_bytes());
    zip64.extend_from_slice(&45u16.to_le_bytes());
    zip64.extend_from_slice(&[0; 40]);
    zip64.extend_from_slice(b"PK\x06\x07");
    zip64.extend_from_slice(&0u32.to_le_bytes());
    zip64.extend_from_slice(&0u64.to_le_bytes());
    zip64.extend_from_slice(&1u32.to_le_bytes());
    zip64.extend_from_slice(b"PK\x05\x06\0\0\0\0");
    zip64.extend_from_slice(&u16::MAX.to_le_bytes());
    zip64.extend_from_slice(&u16::MAX.to_le_bytes());
    zip64.extend_from_slice(&u32::MAX.to_le_bytes());
    zip64.extend_from_slice(&u32::MAX.to_le_bytes());
    zip64.extend_from_slice(&0u16.to_le_bytes());
    for body in [website_zip_payload(&[]), zip64] {
        let directory = TemporaryDirectory::new().unwrap();
        let source = directory.0.join("Empty website archive");
        fs::write(&source, body).unwrap();
        let archive_path = identify_website_archive(source).unwrap();
        assert_eq!(archive_path.file_name().unwrap(), "payload.zip");
        assert!(
            zip::ZipArchive::new(fs::File::open(archive_path).unwrap())
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn nexus_website_display_names_do_not_turn_unrecognized_payloads_into_zip_files() {
    for (body, expected_code) in [
        (b"".as_slice(), "nexus_format_unsupported"),
        (b"PK\x03\x04fake".as_slice(), "nexus_format_unsupported"),
        (
            b"<!doctype html><body>sign in</body>".as_slice(),
            "nexus_format_unsupported",
        ),
        (b"MZ executable".as_slice(), "nexus_format_unsupported"),
        (b"Rar!\x1a\x07\0".as_slice(), "nexus_installation_failed"),
        (
            b"7z\xbc\xaf\x27\x1c".as_slice(),
            "nexus_installation_failed",
        ),
    ] {
        let directory = TemporaryDirectory::new().unwrap();
        let root = directory.0.clone();
        let (url, server) = serve_file(body, false);
        let path = download_payload(
            local_download_client(),
            directory.clone(),
            0,
            url,
            "Camera Tweaks".into(),
            None,
            None,
        )
        .await
        .unwrap();
        let error = identify_website_archive(path.clone()).unwrap_err();
        assert!(error.contains(expected_code), "{error}");
        assert!(!error.contains("fixture-signature"));
        assert!(!error.contains("secret"));
        assert!(path.exists());
        assert!(!path.with_file_name("payload.zip").exists());
        server.join().unwrap();
        drop(directory);
        assert!(!root.exists());
    }
}

#[test]
fn nexus_download_io_failures_preserve_only_fixed_steps_and_os_categories() {
    let private =
        std::io::Error::other("https://cf-files.nexusmods.com/private?token=fixture-client-secret");
    let error = download_io_error("create_file_directory", &private);
    let parsed: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(parsed["code"], "nexus_installation_failed");
    assert_eq!(parsed["local_file_step"], "create_file_directory");
    assert!(!error.contains("fixture-client-secret"));
    assert!(!error.contains("https:"));
    let error = download_io_error(
        "restore_suggested_name",
        &std::io::Error::from_raw_os_error(5),
    );
    let parsed: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(parsed["os_error"], 5);
}

#[test]
fn nexus_website_identification_retains_staging_entry_validation_and_old_files() {
    use crate::commands::commands_mod_staging::{
        ManualModImportPresentation, stage_website_mod_source,
    };
    let directory = TemporaryDirectory::new().unwrap();
    let source = directory.0.join("Untrusted suggested title");
    fs::write(
        &source,
        website_zip_payload(&[
            ("SafeMod/plugin.dll", b"new"),
            ("../outside.dll", b"outside"),
        ]),
    )
    .unwrap();
    let archive = identify_website_archive(source).unwrap();
    let target_path = directory.0.join("mods");
    fs::create_dir(&target_path).unwrap();
    fs::write(
        target_path.join("existing.cfg"),
        b"keep existing configuration",
    )
    .unwrap();
    let target = ResolvedManualModTarget {
        instance_id: "synthetic-nexus-untrusted-zip".into(),
        module_id: "enshrouded".into(),
        source_label: "Nexus Mods".into(),
        target_label: "mods".into(),
        target_path: target_path.clone(),
        accepts: vec!["folder".into(), "zip".into(), "dll".into()],
        id_strategy: None,
    };
    let error = stage_website_mod_source(
        target,
        archive,
        ManualModImportPresentation {
            display_name: None,
            icon_url: None,
        },
        None,
    )
    .unwrap_err();
    assert!(error.contains("unsafe archive entry"), "{error}");
    assert_eq!(
        fs::read(target_path.join("existing.cfg")).unwrap(),
        b"keep existing configuration"
    );
    assert!(!target_path.join("SafeMod").exists());
    assert!(!directory.0.join("outside.dll").exists());
}

#[tokio::test]
async fn nexus_website_download_needs_no_api_metadata_or_exported_credentials() {
    let directory = TemporaryDirectory::new().unwrap();
    let root = directory.0.clone();
    let (url, server) = serve_file(b"website payload", false);
    let path = download_payload(
        local_download_client(),
        directory.clone(),
        0,
        url,
        "OfficialFilename.dll".into(),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(path.file_name().unwrap(), "OfficialFilename.dll");
    assert_eq!(fs::read(&path).unwrap(), b"website payload");
    let request = server.join().unwrap().to_ascii_lowercase();
    for header in ["apikey:", "cookie:", "authorization:"] {
        assert!(!request.contains(header), "{header}");
    }
    drop(directory);
    assert!(!root.exists());
}

#[tokio::test]
async fn nexus_website_oversized_response_is_rejected_without_publication_or_signed_url_echo() {
    let directory = TemporaryDirectory::new().unwrap();
    let root = directory.0.clone();
    let (url, server) = serve_file_with_length(b"", MANUAL_MOD_SITE_DOWNLOAD_MAX_BYTES + 1, false);
    let error = download_payload(
        local_download_client(),
        directory.clone(),
        0,
        url,
        "TooLarge.zip".into(),
        None,
        None,
    )
    .await
    .err()
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(parsed["code"], "nexus_installation_failed");
    assert!(parsed["message"].as_str().unwrap().contains("256 MiB"));
    assert!(!error.contains("fixture-signature"));
    assert!(!error.contains("secret"));
    assert!(!root.join("0/TooLarge.zip").exists());
    assert_eq!(fs::read_dir(root.join("0")).unwrap().count(), 0);
    server.join().unwrap();
    drop(directory);
    assert!(!root.exists());
}
