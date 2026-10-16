use super::*;

#[test]
fn large_archive_metrics_do_not_impose_a_payload_byte_cap() {
    crate::commands::commands_mod_staging::validate_manual_mod_archive_metrics(
        2,
        5 * 1024 * 1024 * 1024,
        5 * 1024 * 1024 * 1024,
    )
    .unwrap();
}

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// These probes deliberately write real bytes. They are opt-in so routine unit
// runs do not allocate multi-GiB scratch space or transfer it over loopback.
#[tokio::test]
#[ignore = "Streams a real 346 MiB ZIP through download, extraction and publication"]
async fn thunderstore_archive_above_256_mib_downloads_extracts_and_publishes() {
    install_large_archive(346 * 1024 * 1024, false).await;
}

#[tokio::test]
#[ignore = "Streams a real ZIP64 larger than 4 GiB, without Content-Length"]
async fn thunderstore_zip64_above_4_gib_streams_without_content_length_and_publishes() {
    install_large_archive(4 * 1024 * 1024 * 1024 + 64 * 1024, true).await;
}

async fn install_large_archive(payload_bytes: u64, chunked: bool) {
    use crate::commands::commands_mod_capacity::{ModSpaceBudget, require_mod_capacity};

    let fixture = Fixture(download_test_root("large-archive"));
    let archive_path = fixture.0.join("source.zip");
    require_mod_capacity(
        &archive_path,
        payload_bytes.checked_add(1024 * 1024).unwrap(),
    )
    .unwrap();
    {
        let mut capacity = ModSpaceBudget::new(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(true);
        archive.start_file("Plugin.dll", options).unwrap();
        archive.write_all(b"MZplugin-fixture").unwrap();
        archive.start_file("ModManifest.json", options).unwrap();
        archive
            .write_all(br#"{"name":"Large archive fixture","version":"1.0.0"}"#)
            .unwrap();
        archive.start_file("assets.bundle", options).unwrap();
        let buffer = [0xa5; 64 * 1024];
        let mut remaining = payload_bytes;
        while remaining > 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            capacity.before_write(count).unwrap();
            archive.write_all(&buffer[..count]).unwrap();
            remaining -= count as u64;
        }
        archive.finish().unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/large.zip", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let deadline = Instant::now() + HTTP_TEST_TIMEOUT;
        let mut socket = loop {
            assert!(
                Instant::now() < deadline,
                "large archive fixture received no connection"
            );
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("large archive fixture failed to accept: {error}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(60)))
            .unwrap();
        read_request_headers(&mut socket);
        let mut file = fs::File::open(archive_path).unwrap();
        if chunked {
            if write!(
                socket,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
            )
            .is_err()
            {
                return;
            }
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let count = file.read(&mut buffer).unwrap();
                if count == 0 {
                    let _ = socket.write_all(b"0\r\n\r\n");
                    break;
                }
                if write!(socket, "{count:x}\r\n")
                    .and_then(|_| socket.write_all(&buffer[..count]))
                    .and_then(|_| socket.write_all(b"\r\n"))
                    .is_err()
                {
                    break;
                }
            }
        } else if write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            file.metadata().unwrap().len()
        )
        .is_ok()
        {
            let _ = std::io::copy(&mut file, &mut socket);
        }
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(HTTP_TEST_TIMEOUT)
        .build()
        .unwrap();
    let downloaded = download_mod_site_archive(&client, &url, "Fixture-Large", "1.0.0").await;
    server.join().unwrap();
    let downloaded = downloaded.unwrap();
    assert!(fs::metadata(&downloaded).unwrap().len() > payload_bytes);
    let extracted =
        prepare_downloaded_thunderstore_archive(downloaded, "Fixture-Large", "1.0.0", "corekeeper")
            .await
            .unwrap();
    let source = Fixture(extracted);
    let target = fixture
        .0
        .join("runtime/CoreKeeperServer_Data/StreamingAssets/Mods");
    let destination = target.join("thunderstore-Fixture-Large/assets.bundle");
    let source_path = source.0.clone();
    let result = tokio::task::spawn_blocking(move || {
        stage_online_mod_sources(
            ResolvedManualModTarget {
                instance_id: "large-chain".into(),
                module_id: "corekeeper".into(),
                source_label: "Thunderstore".into(),
                target_label: "Mods".into(),
                target_path: target,
                accepts: vec!["dll".into(), "folder".into(), "zip".into()],
                id_strategy: None,
            },
            &[DownloadedManualModSource {
                path: source_path,
                label: "Fixture-Large".into(),
                identity: OnlineModIdentity {
                    provider: "thunderstore".into(),
                    project: "Fixture-Large".into(),
                    historical_names: Vec::new(),
                    version: "1.0.0".into(),
                    dependencies: Vec::new(),
                },
            }],
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(fs::metadata(destination).unwrap().len(), payload_bytes);
    assert_eq!(result.copied_file_count, 3);
    assert!(result.copied_total_bytes > payload_bytes);
}
